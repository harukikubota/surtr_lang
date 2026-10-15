# Scar リファクタリングのテスト計画

調査対象は `a0d430a609da73f7f35bbba7a01f41f9b28dfc09`、調査日は 2026-10-15。本書は追加・拡張するテストの案であり、テストの追加や実行結果ではない。製品コードと実行可能テストは変更していない。

Scar には、型推論の成功・拒否に加え、候補検査の rollback、共有状態の分離、構造化診断、特殊化後の型を検査する回帰テストがある。大規模な変更に備えるには、この既存ケースを維持したうえで、**状態を分割する境界と、処理を省略・共有する最適化の境界**を補うのが先になる。単純な型不一致を各層へ重複追加することは避ける。

## 1. 調査方法と件数の読み方

[開発ガイド](../../AGENTS.md)、[テスト方針](../../docs/dev/テスト方針.md)、[nextest 設定](../../.config/nextest.toml)、[診断仕様](../../docs/dev/diagnostics.md)、[Xldr 仕様](../../docs/dev/Xldr_spec.md) と、Scar のソース・テスト、Rune の fixture runner を読み合わせた。`rg` でテスト名・登録配列・状態復元・診断 reason を検索し、主要ケースの本文を確認した。実行時 coverage や mutation testing は実施していないため、以下の「追加候補」は未検査経路の断定ではない。

静的集計では、`crates/scar/tests/*.rs` に 38 ファイルがある。source 内の `#[test]` の数、nextest が起動するテストの数、登録されたケース関数の数、各関数内の入力数は一致しない。

| 対象 | 登録されたケース関数 | Rust 側の実行単位 |
|---|---:|---|
| [typecheck_surface.rs](../../crates/scar/tests/typecheck_surface.rs) | 430 | 12 bucket ＋ inventory 1 |
| [return_type_arguments.rs](../../crates/scar/tests/return_type_arguments.rs) | 41 | 8 bucket ＋ inventory 1 |
| [type_constructor_carriers.rs](../../crates/scar/tests/type_constructor_carriers.rs) | 42 | 8 bucket ＋ inventory 1 |
| [trait_method_type_lists.rs](../../crates/scar/tests/trait_method_type_lists.rs) | 22 | 8 bucket ＋ inventory 1 |
| [nominal_constructor_parameters.rs](../../crates/scar/tests/nominal_constructor_parameters.rs) | 21 | 8 bucket ＋ inventory 1 |
| [generic_extractor_specialization.rs](../../crates/scar/tests/generic_extractor_specialization.rs) | 11 | 8 bucket ＋ inventory 1 |
| [generic_trait_forwarding.rs](../../crates/scar/tests/generic_trait_forwarding.rs) | 8 | 8 bucket ＋ inventory 1 |
| [default_trait_methods.rs](../../crates/scar/tests/default_trait_methods.rs) | 22 ＋ standalone 4 | 8 bucket ＋ inventory 1 ＋ standalone 4 |
| [structured_diagnostics.rs](../../crates/scar/tests/structured_diagnostics.rs) | 16 | suite 1 ＋ inventory 1 |
| [closed_diagnostic_schema.rs](../../crates/scar/tests/closed_diagnostic_schema.rs) | 5 | suite 1 ＋ inventory 1 |

件数は登録配列／登録 macro の静的集計であり、現在の実行成功数ではない。複数入力を loop で検査するケースもあるため、登録関数数を言語ケースの総数としない。たとえば enum の未解決型引数テストは 1 関数で 5 入力を検査している。

[run_srt.rs](../../tests/integration/run_srt.rs) は成功・失敗 fixture を同じ 8 bucket で実行する。ファイル走査上、script は成功 152 組、失敗 352 組。module は成功 63 組、失敗 125 組で、[module_import_fixtures.rs](../../tests/integration/module_import_fixtures.rs) に成功 4 bucket、失敗 4 bucket がある。module 内の `.srt` ファイル数には補助 module が含まれるので、ここでは `.expected` / `.error` の数を組数とした。これらは全フェーズの fixture 数であり、Scar 専用の数ではない。

## 2. 既存テストで確認している契約

| 契約 | 既存の根拠 | リファクタリング時の使い方 |
|---|---|---|
| relation probe は成功・失敗ともに一時的な代入を戻す。型の assertion は成功だけ確定する | [relations.rs](../../crates/scar/src/checker/relations.rs) の `speculative_type_relation_restores_success_and_failure_state_without_candidate_snapshot`、`failed_type_assertion_rolls_back_partial_substitution` | rollback の削除や対象状態の縮小に先立ち再実行する。単純 tuple mismatch の新規テストは不要 |
| 通常呼び出しが候補全体の checkpoint を作らない | [expr.rs](../../crates/scar/src/checker/expr.rs) の `ordinary_successful_applications_do_not_create_candidate_probe_checkpoints` | 「高速化できた」という wall time だけでなく、不要な候補検査へ戻っていないことを既存 counter で確認する |
| 候補の失敗で特殊化の identity が変化しない | [trait_selection.rs](../../crates/scar/src/checker/trait_selection.rs) の `failed_method_probe_does_not_change_complete_instantiation_identity` | 候補選択と特殊化を分割する際の直接的な回帰にする |
| 型の循環証明が reason を失わず、成功 checkpoint へ混ざらない | 同ファイルの `required_constructor_cycle_restores_relation_state_and_cannot_be_serialized`、[default_trait_methods.rs](../../crates/scar/tests/default_trait_methods.rs) の cycle 系ケース | `CyclicTraitObligation` を通常の不一致や「候補なし」へ変換しない。既存の不関連な cycle を無視できる成功例も維持する |
| impl の選択は宣言順で変わらない | [trait_impl_applicability.rs](../../crates/scar/tests/trait_impl_applicability.rs) の `nested_disjoint_heads_are_order_independent`、[return_type_arguments.rs](../../crates/scar/tests/return_type_arguments.rs) の `expected_constructor_selection_is_independent_of_impl_order` | index 導入・探索順変更時に再利用する |
| 特殊化した呼び出し・引数・返り型・body が対応する | [trait_method_instantiation.rs](../../crates/scar/tests/trait_method_instantiation.rs) の `box_field_return_and_body_are_instantiated_together` | call の成功だけで終わらず、typed IR の dispatch target と定義を照合する |
| signature と定義の共有が別セッションへ漏れない | [predeclare.rs](../../crates/scar/src/checker/predeclare.rs) の `shared_callable_signatures_keep_instantiations_and_failed_sessions_independent`、[checker/mod.rs](../../crates/scar/src/checker/mod.rs) の `retained_definitions_isolate_reconciled_sessions_and_checkpoints` | COW・共有化を変更する際、既存の親／子／兄弟／checkpoint 分離の検査を維持する |
| checkpoint の保存復元後も constructor capability の強化を拒否する | [constructor_capability_session.rs](../../crates/scar/tests/constructor_capability_session.rs) | 直列化の成功だけでなく、復元後の拒否境界を確認する |
| 未確定の enum slot が executable な式へ残らない | [enum_constructor_unresolved_type_arguments.rs](../../crates/scar/tests/enum_constructor_unresolved_type_arguments.rs) | 定義内で保持すべき多相性と、実行する式の未解決を区別する |
| 構造化診断が表示用 fact の加工に依存しない | [closed_diagnostic_schema.rs](../../crates/scar/tests/closed_diagnostic_schema.rs)、[structured_diagnostics.rs](../../crates/scar/tests/structured_diagnostics.rs) | reason、型、source ordinal、span を維持する。表示 message 全文の snapshot は追加しない |
| 深い依存関係を扱える | [type_dependency_stack.rs](../../crates/scar/tests/type_dependency_stack.rs) は 40,000 record、[trait_method_instantiation.rs](../../crates/scar/tests/trait_method_instantiation.rs) は 16 段の特殊化 | 両方とも 8 MiB stack を明示した検査。通常の全環境で無制限な深さを保証するものではない |
| canonical unifier の境界と割当の増え方 | [canonical_unifier_tests.rs](../../crates/scar/src/checker/trait_selection/canonical_unifier_tests.rs) | occurs check、rigid variable、Hole、constructor identity は既存ケースがある。割当測定と srt 全体の速度を区別する |

`Pid` の marker identity、SafeBind / MatchBlock、`do`、lazy capture、Facet、process handler、Extractor、RTA も専用ファイルまたは surface に多数ある。ファイル分割だけを理由に同じ入力を新しいファイルへ複製せず、対応する既存ケースの移動・拡張を優先する。

## 3. 最優先の追加案

P0 は、対象の状態やアルゴリズムを変更する前に固定する契約。単純なファイル移動だけの場合は、すべてを先に追加する必要はない。各行はケース群であり、Rust の `#[test]` を同じ数だけ増やす指示ではない。

| ID | 入力・操作 | 成功／拒否の期待 | 追加先と既存の拡張先 |
|---|---|---|---|
| T-01 | 外側 relation probe で `$A` と `$B` を関連づけ、内側 probe で `$B = Boolean` とする成功と、tuple 後半の `Int` / `String` 不一致を組み合わせる。内側を戻した後に外側を戻す | 内側の終了後は外側開始後の状態、外側の終了後は開始前の状態。次の確定 assertion で `$A = String` を選べる。成功・拒否のどちらも同じ復元境界を守る | [relations.rs](../../crates/scar/src/checker/relations.rs) の既存 3 テストを拡張。crate-local で代入と obligations を直接確認する |
| T-02 | 代入の先を通してのみ到達する型変数へ pending obligation と process marker を付け、probe の途中で別変数に結合する。その後の要素で失敗する | 代入だけでなく bounds、pending obligation、process marker が復元される。後続の通常型／process marker の判定が、汚染されていない checker と一致する | 同じく relation テスト。公開 `Pid` 入力の単純な拒否は [pid_marker_identity.rs](../../crates/scar/tests/pid_marker_identity.rs) に既存のため重複しない |
| T-03 | 候補検査 A で lazy capture の署名・pattern alias・warning を生成して失敗し、候補 B では成功する。候補の探索順を反転する | B の型・capture 診断の由来が同じ。A だけの warning や alias が B へ残らず、採用した処理の warning が重複しない | [expr.rs](../../crates/scar/src/checker/expr.rs) の `candidate_rollback_restores_only_committed_operator_provenance` を起点に、実際に各状態を作る経路で拡張。warning 自体の契約は [warnings.rs](../../crates/scar/tests/warnings.rs) に既存 |
| T-04 | 1 つの標準 prefix から 2 セッションを作る。一方で新しい型・generic 定義を含む suffix の末尾を型不一致にし、他方で `identity(1)` / `identity("ok")` を成功させる。失敗側も明示 rollback 後に再試行する | transactional API の失敗では元の公開状態を保持。in-place API は失敗後の部分状態を許すが、保存した checkpoint から rollback すれば再試行結果が clean session と一致する。generic 結果型はそれぞれ `Int` / `String` | [checker/mod.rs](../../crates/scar/src/checker/mod.rs) の session テストに API ごとの表を追加。共有 signature の既存テストを拡張する |
| T-05 | 同じ resolved 入力を、通常 API と in-place API に同じ prefix から渡す。generic 呼び出し、enum、process metadata を含む成功入力と、既存診断ケースの代表を使う | 成功時の型・dispatch target・nominal/enum 定義・process_specs / boot_plan が一致。失敗時は reason、origin、data、元 source span が一致。処理後のセッション状態は T-04 の API 契約に従う | session テスト。staged 側は [typecheck_surface.rs](../../crates/scar/tests/typecheck_surface.rs) の `typecheck_staged_program_keeps_process_specs` を拡張 |
| T-06 | stdlib を最初から検査する経路と、同じ stdlib の checkpoint / resolve resume state から user suffix だけを検査する経路を比較する。代表入力に default method、constructor capability、process handler を含める | 同じユーザ定義・呼び出しの型と canonical dispatch が得られる。process metadata の欠落・重複がない。誤った入力の reason とユーザ source の span も同じ | [support/core.rs](../../crates/scar/tests/support/core.rs) のキャッシュ利用だけでは比較にならないため、full compile 側を明示的に組み立てる小さな crate-local または integration ケース。全 surface を二重実行しない |
| T-07 | 同じ generic 関数を `Int`、`String`、再び `Int` で呼ぶ。直接呼び出しと capture 経由を組み合わせ、先に不適合な候補を検査させる | 各呼び出しの型、採用した body、dispatch が対応する。同じ具体化の再利用と異なる具体化の分離を確認し、失敗候補の dispatch・cache entry・定義を成功した具体化へ流用しない。確定した identity と生成定義を一致させ、未解決の実行用 dispatch を残さない。rollback で取り消した番号の後続再割当は禁止しない | [trait_method_instantiation.rs](../../crates/scar/tests/trait_method_instantiation.rs)、既存 identity テスト、[checker/mod.rs](../../crates/scar/src/checker/mod.rs) の reconciliation テストを拡張 |

T-01〜T-07 は、該当ファイル内の `probe` / `rollback` / `checkpoint` / `in_place` / `staged` / `specialization` と対応テスト本文の調査から選んだ。既存ケースは局所的な成功・拒否、共有分離、process metadata の保持を確認している。一方、表にある組み合わせをそのまま検査するケースは今回の検索・本文確認では特定できなかった。実装時には最終的な変更箇所に合わせて、さらに重複を確認する。

T-04 の区別は特に重要。現在の in-place API は one-shot compile 用であり、失敗後に部分状態を残すことがコード上の契約になっている。全 API に自動 rollback を期待するテストを追加すると、単なるリファクタリングが仕様変更になる。

## 4. 変更箇所に応じて追加する案

| ID | 追加する境界 | 期待する検査と置き場 |
|---|---|---|
| T-08 / P1 | finalization の走査対象を整理する場合、`Option::None` が tuple、list、closure body、match arm 内にある形を、期待型あり／なしで対にする | 既存の enum finalization suite を拡張する。期待型ありは slot が確定し、なしは `UnresolvedEnumConstructorTypeArgument` と該当 constructor の span。generic 定義の正当な型変数や `Err` の許可される成功 slot を一律拒否しない |
| T-09 / P1 | 候補 index / memo 化を導入する場合、3 つの互いに非重複な impl の全 6 宣言順と、nested argument の一部だけが一致する不適合候補を組み合わせる | 既存 `nested_disjoint_heads_are_order_independent` を拡張。成功時は同じ canonical impl、失敗時は `NoApplicableTraitImplementation` の同じ trait/argument。順序が変わることで生の UID / tag が変わりうる比較では、raw ID 全一致を条件にしない |
| T-10 / P1 | 診断生成や候補失敗の保持を分割する場合、日本語文字列を先行させ、候補内の型注釈に未知型または型引数 arity 不一致を置く | [contextual_type_final_remediation.rs](../../crates/scar/tests/contextual_type_final_remediation.rs) 等の既存候補診断を拡張。元の reason と source span が型構文を指す。文字位置・byte位置の意味を既存 producer 契約に合わせ、先頭候補の別エラーへ置換しない。複数ファイルの source ID は module fixture または既存 module 診断テストに置く |
| T-11 / P1 | dependency graph を変更する場合、長い直線だけでなく diamond、共有終端、末尾付近だけが循環する graph を生成する | [type_dependency_stack.rs](../../crates/scar/tests/type_dependency_stack.rs) と surface の既存 cycle ケースを拡張。非循環 record は成功、循環 record は型循環を拒否。再帰 enum は既存の許可／拒否規則に沿って別入力にする。単純にすべての循環を拒否しない |
| T-12 / P1 | 分割した traversal へ TypedNode variant を移す場合、Extractor、Facet、safe operator、capture 内の generic 呼び出しを代表として選ぶ | 既存 [generic_extractor_specialization.rs](../../crates/scar/tests/generic_extractor_specialization.rs)、surface の `facet_pending_dispatch_collects_path_type_inputs`、safe operator suite を拡張。具体入力で static dispatch と型が解決し、欠けた制約は Scar の診断になる。Forge に失敗を先送りしない |

小さい生成入力は、固定 seed または全列挙で再現できるものにする。件数だけを増やすランダム生成を先に導入する必要はない。expected result を同じ内部アルゴリズムで算出せず、成功・拒否の意味と公開される型・診断を比較する。

## 5. ケースを増やしても実行を不必要に重くしない

[共通 case registry](../../crates/scar/tests/support/case_registry.rs) と既存 suite の登録方式を使う。`typecheck_surface` は現時点で 12 bucket であり、各 bucket の worker は 1。以前の構成を前提に戻さない。ケースを分割した後も、名前の一意性、関数の重複、全ケースがちょうど 1 partition に属することを確認する。

Scar の統合テストは別 binary ごとに共通 prelude を準備する。意味上まとまったケースを既存 suite に追加し、1 ケースごとの binary や `#[test]` を無条件に増やさない。ただし bucket 化にも実行並列性との交換条件があるため、bucket 数の変更はテスト配置変更とは分離して測る。

full compile と prefix 復元の比較は、prefix そのものを検査する少数の T-06 に限定する。通常の型テストまで毎回 stdlib 全体を再コンパイルしない。テスト基盤の時間短縮と、利用者の srt コンパイル時間短縮は別の結果として報告する。

## 6. 実装ターンの検証順

まず変更する契約を直接検査するテストを選ぶ。以下は実装時のコマンド案であり、今回実行したコマンドではない。

```sh
# crate-local の relation / candidate / session を変更したとき
rtk cargo nextest run -p scar --lib

# 専用 integration target の例
rtk cargo nextest run -p scar --test trait_method_instantiation
rtk cargo nextest run -p scar --test constructor_capability_session

# surface は bucket が実行単位。内部のケース関数名を nextest filter にしない
rtk cargo nextest run -p scar --test typecheck_surface

# Scar の整理を一段完了した時点
rtk cargo nextest run -p scar

# 診断・script 境界が変わる段階
rtk cargo nextest run -p rune --test integration run_srt

# stdlib stage・module・source ID に影響する段階
rtk cargo nextest run -p rune --test integration module_import_fixtures

# incremental session / checkpoint / callable index に影響する段階
rtk cargo nextest run -p xldr
```

複数フェーズに及ぶ変更や統合前は `rtk cargo nextest run --profile ci --workspace` を実行する。default profile は cold CLI を除外するため、全件実行の代わりにはならない。標準ソースに影響する変更では `cargo run -- test --quiet --all` も実行する。仕様・フェーズ間契約の変更まで含む場合は level4 とし、両方の全件成功と別エージェントレビューを必須にする。文書だけの本調査にコンパイラ全体のテストは不要。

cache 条件を変えて検証する場合は、setup と本体に同じ `CARGO_TARGET_DIR` を使い、同じ target を複数の変更中 checkout で共有しない。通常の検証のために cache 設定を変更する必要はない。測定が必要な段階だけ `SURTR_TEST_TIMING=1` と `--no-capture` を使用し、準備時間、fixture 数、cache の有無、実行部分の時間を分けて残す。

## 7. 受入条件と今回の未検証範囲

テスト追加・分割の受入条件は、登録漏れがないこと、変更する状態・処理の成功と拒否を最も直接的な層で固定すること、失敗ケースが目的の reason で失敗すること、既存の公開 API / 型規則 / 診断を維持すること。実装を変えないテスト追加では、成功している動作を人工的に壊して Red を作らない。

今回確認したのはソース、登録構成、診断・状態契約である。Cargo / nextest、標準 SRT テスト、coverage、性能測定はこの担当調査では未実行。提案ケースの入力詳細は、実装ターンで最小入力に落として構文・型規則を確認する。未実行の追加案を回帰再現済み、または既知の不具合として扱わない。
