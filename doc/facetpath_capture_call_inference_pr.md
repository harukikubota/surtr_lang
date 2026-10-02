# FacetPath capture の呼び出し内型推論：問題報告・修正提案

状態: 実装・検証完了（2026-10-01）。

## 目的と範囲

`Function::apply(&List.[0], [1])` を受理する。同じ呼び出しの後続引数が確定する source 型を、先行する FacetPath capture の型検査に使えるようにする。`&Type.path` と `_.path` の単項 capture を対象にし、List 専用の例外は設けない。

Facet 値の関数間受け渡し、`~source.path` の使用可能位置、Facet API ごとの適格性、式の評価順は変更しない。source 型が確定しない場合や path と source が合わない場合はエラーにする。

## 修正前の挙動と原因

`Function::apply(f: ($A -> $B), value: $A) -> $B` は `lib/function.srt` に定義されている。Scar の通常関数呼び出しは、宣言順に各引数を期待型で検査し、その場で型関係を確定する（`crates/scar/src/checker/expr.rs` の `typecheck_user_function_args`）。第1引数を検査する時点では `$A` が未解決である。

`&Type.path` は期待される単項関数型から `Facet::view(path, source)` の closure に展開される。`_.path` も期待される単項関数型から closure に展開される。いずれも path の検査には source の具体型が必要で、未解決の型変数に対して root・segment の検査が直ちに失敗する。第2引数 `[1]` による `$A = List<Int>` の確定には到達しない。診断の `the inferred argument type` は、この未解決の型変数の表示である。

REPL で確認した例:

| 入力 | 現状 |
|---|---|
| `Function::apply(&List.[0], [1])` | `List root Facet path requires List<T>, got the inferred argument type` |
| `Function::apply(&Tuple._0, (1, 2))` | tuple source context が未確定として拒否 |
| `Function::apply(&HashMap.["a"], HashMap::map_from_entries([("a", 1)]))` | HashMap source が未確定として拒否 |
| `Function::apply(&Option.Some, Option::Some(1))` | `Option` root と未確定 source の owner mismatch |
| `Function::apply(&User.name, first_user)` | `User` root と未確定 source の owner mismatch |
| `Function::apply(_._0, (1, 2))` / `Function::apply(_.[0], [1])` / `Function::apply(_.name, first_user)` | 未確定 source への segment access として拒否 |

`User` と `first_user` は既存の `tests/fixtures/script/pass/functions/inferred_facet_capture.srt` の定義を使用した。`&List.[0..1]` と `_.[0..1]` でも同じ境界を確認した。

対照例は成功する。`List::map([[1]], &List.[0])` と `List::map([[1]], _.[0])` はどちらも `[Ok(1)]` を返す。`get_first: (List<Int> -> Result<Int>) = &List.[0]` と型注釈を付けてから `Function::apply(get_first, [1])` を呼ぶと `Ok(1)` を返す。`Facet::view(List.[0], [1])` と `Facet::view(~[1].[0])` も `Ok(1)` を返す。Facet API の専用経路は source を先に検査し、その型で path を確定している。

## 変更後の契約

1. 通常の関数呼び出しで FacetPath capture の期待単項関数型が得られ、その source 型が未解決の場合、同一呼び出しの他の引数と期待返り型から得られる型制約を確定してから path を検査する。
2. source 型が確定したら既存の root・segment・可視性・readonly・Result 化の規則で capture を検査する。引数の位置や検査の段階によって受理規則を変えない。
3. 必要な source 型が最後まで確定しなければ、未解決の source を示す診断で拒否する。異なる source 型、存在しない field、範囲外の tuple index、誤った bracket 型なども既存の契約どおり拒否する。List index の範囲外は従来どおり実行時の `Result` で表す。エラーを捕捉して別経路に落とす方式は採らない。
4. 型検査上の依存解決だけを調整し、実行時の引数評価順と関数適用順は維持する。解決待ちの capture を Forge 以降へ渡さない。

成功条件の例:

```surtr
Function::apply(&List.[0], [1])               // Ok(1)
Function::apply(&Tuple._0, (1, 2))            // 1
Function::apply(&Option.Some, Option::Some(1)) // Ok(1)
Function::apply(_.[0], [1])                    // Ok(1)
```

拒否条件の例: `Function::apply(&List.[0], "text")` は source 型が `List<T>` でないため拒否する。文脈なしの `&List.[0]` や `_.name` に暗黙の source 型を与えない。

## 実装・検証案

影響 level は **4**。通常関数の引数間の型制約と FacetPath capture の解決時点を変えるため、型推論の契約変更として扱う。

- Scar の通常関数引数検査で、source 型待ちの FacetPath capture を明示的な未解決項目として保持し、他の引数・期待返り型による制約確定後に検査する。診断文字列による再試行や List などの root 名による特例分岐は使わない。
- 依存が確定しない場合の診断、複数の依存する引数がある場合の停止条件、元の引数順・span・診断優先順位を確認する。通常の関数引数全体に波及するため、既存の capture・closure・generic 呼び出しの検査も確認する。
- Scar の `crates/scar/tests/typecheck_surface.rs` に List / Tuple / HashMap / 名目型 root と `_.path` の成功・拒否境界を追加する。実行結果は `tests/fixtures/script/pass/stdmod/facet_path_case_api.srt` または専用 fixture で固定する。既存の型注釈付き capture、`Facet::view`、`~source.path` の成功と、誤った source・segment の拒否を維持する。
- 実装ターンでは失敗する回帰テストを先に確認し、対象検証後に `rtk cargo nextest run --profile ci --workspace` と `cargo run -- test --quiet --all` を実行する。最終差分を別エージェントにレビューさせる。

正本文書の更新先は `docs/site/facet.md` の capture 説明と、型推論の開発者向け記述が必要なら `docs/dev/` の該当箇所とする。この問題報告を仕様入力として、正本文書・実装・テストを更新する。

## 実装計画

1. `docs/site/facet.md` の呼び出し内推論の説明を整合させ、Scar の surface case に成功・拒否・依存解決・期待返り型の回帰例を追加する。Scar の対象テストで Red を確認する。
2. Scar の引数検査で source 型待ちの capture を引数位置ごとに保持する。他の引数を検査し、source が確定した項目を元の位置へ格納する。保留項目が減らなくなったら最初の未解決 capture の span で拒否する。名前付き引数と関数値にも同じ規則を適用する。
3. script fixture で実行結果と引数評価順を固定し、Scar・fixture の対象検証を行う。
4. CI profile の workspace 全件と標準 SRT テストを確認し、最終差分を別エージェントでレビューする。指摘対応後に必要な再検証を行う。

## 実装結果と検証

Scar の通常呼び出し引数検査に、source の型構造を待つ capture の保留処理を追加した。
位置引数・名前付き引数・関数値で共通の依存解決を使い、型が決まった引数は元の位置へ保持する。
参照しない型変数と最終 focus は待たず、必要な receiver が未確定なら保留する。
既知の不正 root・private field は通常の検査で診断し、保留項目が減らなければ
最初の未解決 capture の span と source 型で拒否する。

`typecheck_surface.rs` に成功・拒否・期待返り型・複数依存・括弧・診断順の回帰例を追加した。
`facet_capture_call_inference.srt` で実行結果と引数評価順を固定し、source 未確定を拒否する
既存 script fixture と REPL テストの期待診断を現行契約に合わせた。
利用者向け説明は `docs/site/facet.md`、開発者向け契約は `docs/dev/Trait_system_spec.md` に反映した。

最終コードに対する検証結果:

- `rtk cargo nextest run --profile ci --workspace`: 2,001 件成功、失敗なし。
- `cargo run -- test --quiet --all`: 終了コード 0。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。
- 別エージェントによる最終レビュー: 指摘対応済み、追加指摘なし。

残件なし。コミット・マージは実施していない。
