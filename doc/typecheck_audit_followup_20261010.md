# 型検査調査の修正方針・継続調査

2026-10-10の利用者判断に基づき、バグ修正・文書修正・継続調査の残作業を記録する。本書の判断は、前回調査書の「未選択」および併記した選択肢に優先する。今回は方針の文書化であり、処理系・標準定義・正本文書の修正は実施していない。

前回調査は `7458fe8d8d16aa795d038345d7f86b9d6209ccee`。今回の読み取り照合は main の `db5e4feaafebfec05d43bed6e4150c9f5cfde9c0` を対象とした。以下の実装・正本の行番号は後者に対応する。保存先の調査用worktreeは前者のまま保持しており、実装時には最新の作業基準へ照合する。

## 判断一覧

| ID | 決定 | 残作業 |
|---|---|---|
| TC-03 | バグとして修正 | WorkerLeaseからPIDへの一般的な暗黙適合を除去 |
| TC-04 | バグとして修正 | PID markerの名前解決・generic同一性を通常の型規則へ揃える |
| TC-05 | バグとして修正 | unary callableのHole入力だけを特別に適合させる経路を除去 |
| TC-06 | PIDは標準の`instance_of`実装、Enumは明示実装 | compilerによる暗黙の能力付与を整理し、正本も更新 |
| TC-10 | 正本修正 | 戻り値の具象エラー名列挙はドキュメント目的と明記 |
| TC-11 | 調査タスクとして保持 | 推論probeのエラー破棄と最終検査・診断の関係を確認 |
| INV-01〜07 | 調査タスクとして保持 | 前回の「誤検知を避けた箇所と未確認範囲」を引き継ぐ |

## TC-03: WorkerLeaseからPIDへの暗黙適合

現行 `crates/scar/src/checker/types.rs:2716-2728` は、通常の型比較で `WorkerLease<P>` を期待型 `PID<P>` に適合させる。これをバグとして修正する。

```surtr
# Counterが正しく宣言されている条件で検査する。
def leak(lease: WorkerLease<Counter>) -> PID<Counter> { lease }
```

期待値は型不一致による拒否。未知のCounterを拒否するだけでこの修正を完了としない。通常引数、binding、return、container、比較・adoptなどの利用先で同じ型規則を適用する。生成worker helperの取り扱いも確認し、必要な受渡しは明示的な契約として表し、一般の暗黙適合へ戻さない。

## TC-04: PID markerとgenericの同一性

現行 `types.rs:604-616` は名前を文字列で保持し、`2711-2714` は片側が`$`始まりなら適合とする。名前の解決と型変数の同一性を通常規則へ揃える。

```surtr
def fake(value: PID<NotAProcess>) -> PID<NotAProcess> { value }
def forget(value: PID<$P>) -> PID<Other> { value }
```

`fake`は未知markerとして拒否する。`forget`はOtherが未定義なら名前解決で拒否し、Otherが有効な別processでも任意の`$P`からの返却を拒否する。同一genericの再出現は同じ変数として扱う。実PID、複数module、handler/worker/singletonの許可範囲も確認し、宣言受理だけの検証で終えない。

## TC-05: Hole入力に限ったcallable適合

現行 `types.rs:2654-2677` は実入力が`[Ty::Hole]`なら入力を照合せず返り型だけを検査する。文書化されていても、この例外をバグとして修正する判断で確定する。

```surtr
mapper: (Int -> Int) = always(10)
mappers: List<(Int -> Int)> = [always(10)]
```

上の組を通常の型規則で検査する。実型が`(_ -> Int)`のままなら、HoleをIntとみなす救済をしない。通常の型推論で入力型を確定できる設計にする場合は、その推論根拠と標準APIの署名を明示する。従来の成功を保つために別の特例は追加しない。

`docs/dev/Trait_system_spec.md:82`、`docs/site/type-annotations.md`のHole節、`lib/function.srt`の宣言・説明、`grouped_contextual_callables`などの成功・拒否期待値を同じ規則に揃える。

## TC-06: PIDとEnumの比較能力

決定は「PIDは`instance_of`として標準が実装する」「Enumへの暗黙実装はバグなので、明示実装を通す」。PIDとEnumを一括したcompiler-owned Eqの継続案は採用しない。

現行 `crates/scar/src/checker/predeclare.rs` の `compiler_trait_impl_exists` / `compiler_trait_dispatch_target`（2482行以降）と、`docs/dev/Trait_system_spec.md:713-719`、`docs/site/trait-impls.md:33-38`が変更対象となる。`instance_of`の公開署名やEqとの接続方法は今回の指示で具体化されていないため、本書でAPIを追加決定しない。利用者指定の名称と標準側が実装主体であることを実装入力として保持する。

```surtr
defenum Choice { One, Two }
Choice::One == Choice::One
# 修正後: 明示的なEqの宣言がないため拒否。
```

明示`impl Eq`、または明示`@derive Eq`から生成した実装がある場合は、その実装を通常のproof/dispatchで使う。payloadの有無だけで能力を付与しない。標準enumも必要な実装を明示し、暗黙付与への依存を除く。PIDの標準実装も、コンパイラが名前だけで比較能力を補う経路へ戻さない。

## TC-10: 具象エラー名の列挙はドキュメント目的

関数の直接戻り値位置に書く`Result<T, E>`の具象エラー名はドキュメント目的である。返すErrorのkindを静的に制限する意味は持たない。正本の「エラー契約」が返却kindの保証に読める記述を修正する。

```surtr
def other_error() -> Result<Int, NoneError> { Err(ZeroDivisionError) }
# 維持する期待値: 受理。NoneErrorと異なるkindだけを理由に拒否しない。
```

`docs/site/type-annotations.md:88-96`、`docs/dev/Trait_system_spec.md:843`、`docs/site/trait-impls.md:237`などを、説明用metadataと通常の値型`Result<T>`の関係として揃える。名前の存在、使用可能な記述位置、成功型・引数型の通常検査を弱める変更は含めない。新たなエラー列挙構文や網羅検査も追加しない。

## TC-11: 推論probeと診断情報の継続調査

`crates/scar/src/checker/expr.rs:7327-7405`には予備検査の`.ok()`、rollback、候補ごとの本検査、根拠がない場合のambiguity拒否が残る。成功への不正fallbackとは断定せず、調査タスクとして残す。

確認するのは、最初の型・注釈エラーが最終診断でも保持されるか、候補の順序・数で受理や診断が変わらないか、rollbackが必要な状態と診断を失わないか。`Applicative::pure(1)`のようにcarrierの根拠がない入力は、登録候補が一つでも推測で成功させない。調査結果には最小入力・現行診断・期待する診断・影響経路をセットで残す。

## 前回の未確認範囲を引き継ぐ調査タスク

以下は今回のバグ確定ではなく、条件と到達範囲を確認する作業である。

| ID | 対象 | 調査内容・完了条件 |
|---|---|---|
| INV-01 | `SignatureTyMode::allows_direct_constructor_name_fallback` | raw/resolved signature、alias再帰、全callerを追い、canonical identity欠損を名前で補える条件を確定する |
| INV-02 | nominal field情報が空の場合の照合 | owner・型引数の検査とrecursive/deferred表現の不変条件を確認し、不正な型まで受理するかを区別する |
| INV-03 | constructor projectionのRejected理由 | `Canonicalization`への変換で失う元エラーを調べ、成功化と診断劣化を区別する |
| INV-04 | Pattern注釈の予備hintでの`.ok()` | 本検査が必ず行われるかを追い、未知注釈・不正型の拒否と診断を確認する |
| INV-05 | Facet intrinsicのidentity | qualified名分岐の前にある予約owner・resolverガードを確認し、通常定義が専用処理へ入らないことを確定する |
| INV-06 | derive生成実装の複合where subject | 生成由来情報の許可境界と通常利用者宣言の拒否を確認し、field全体の能力要求を迂回しないことを確定する |
| INV-07 | trait/impl registryと複合経路 | candidate rollback、Trait helperのRTA/capture、複数moduleのPID generic、実WorkerLeaseのEq/adopt投入を確認する。TC-03/04/06と重複する修正は統合する |

各タスクは最新ソースで入口・ガード・最終検査を追い、可達例または非可達の根拠を残す。前回報告にあった「全組合せを未実測」を、バグが存在するとの断定へ置き換えない。

## 今回の確認範囲

現行ソースと正本文書を読み取り、利用者の決定を記録した。前回の実測値を今回のHEADで再実測したとは扱わない。製品コードを変えるターンではないため、ビルド・テストは実施していない。今回の成果はこの作業文書のみである。
