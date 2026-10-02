# Lazy 事前評価式とキャプチャのスコープ

## 採用する契約

Lazy 引数位置の固定式 `(EXPR)` は事前評価の境界である。名前はキャプチャのプレースホルダ引数と Pattern binding が導入される前のローカルコンテキストから読む。この式で今回のキャプチャの `&N` を参照した場合は、Sigil がスコープ違反として拒否する。型不一致へ持ち越したり、通常値を thunk 化して救済したりしない。

Lazy 位置全体の `&N` と `(&N)` は直接置換であり、事前評価式ではない。通常の Lazy 位置では正規化済みの0引数関数を要求する。binding を作る Pattern の成功 branch は DirectExpression を要求し、直接置換も通常データとして扱う。

```surtr
x = 10
good = &if_let(Ok(1), Ok(x), x + &1, 0)  # &1 は Int、x は成功 binding
bad = &if_let(Ok(1), Ok(x), (x + &1), 0) # &1 を事前評価式から参照できない
outer = &if_let(&1, Ok(x), (x + 1), 0)   # x は外側の10
direct = &if(True, (&1), 0)            # &1 は (-> Int)
```

対象は canonical な標準 Lazy call とその成功・失敗 branch。外側のキャプチャ内に入れ子で書いた call にも同じ規則を適用する。通常の引数・算術式・match arm 内の grouping、Pattern の事前 Expr はこの Lazy 境界とは区別する。

これは参照スコープの変更であり、固定式の評価をキャプチャ生成時へ移動しない。実行時は生成関数を呼ぶたびに、既存の順序で分岐選択前に一回評価する。裸 call の遅延、選択された branch の一回の consume、既存の nested capture 制限は維持する。

## 現状との差分と実装計画

現行 Sigil は `&N` を生成引数へ置換した後に事前評価式を名前解決するため、この式から生成引数を参照できる。生成引数の解決済み ID を追跡し、Lazy の事前評価式に含まれる自由参照を拒否する。名前の綴りや型からプレースホルダを推測しない。Pattern 成功 binding が見えない既存の名前解決順序を保つ。

スコープ検査では、入れ子の Pattern consumer にある未確定の Extractor 引数の式候補も走査する。通常の closure capture は型検査後の確定を維持し、型が未確定であることを参照スコープの許可に使わない。事前評価境界の外にある Pattern の事前引数は、従来どおりプレースホルダを参照できる。

level4。Sigil の生成引数 identity と Lazy 引数の解決を修正し、Scar・Forge・Eldr には新しい型・評価経路を追加しない。

1. 正本の `docs/dev/Lazy_spec.md`・`Pattern_spec.md`・`diagnostics.md`、`docs/site/lazy-evaluation.md`・`capture-operator.md`、`lib/kernel.srt` の `@doc` を整合させる。
2. Sigil に拒否ケースと直接置換・通常 grouping の許可ケースを追加し、現行実装で拒否ケースが失敗することを確認する。
3. 生成引数 ID を用いた検査を共通の Lazy 引数解決へ追加する。既存の成功 scope と直接置換の例外を維持する。
4. runtime fixture で外側の値、通常データのプレースホルダ、評価順・回数を確認する。
5. 対象テストから workspace CI と標準 SRT テストへ広げ、別エージェントレビュー後に今回の差分だけをコミットする。

受入条件は、binding の有無・branch の位置・修飾名・入れ子 call によらず Lazy 事前評価式のプレースホルダを resolve error とし、その参照 span を示すこと。直接置換、事前評価外の通常プレースホルダ、外側の同名 binding の参照を維持すること。

検証は `rtk cargo nextest run -p sigil`、関連 Scar テスト、script fixture、`rtk cargo nextest run --profile ci --workspace`、`cargo run -- test --quiet --all` で行う。
