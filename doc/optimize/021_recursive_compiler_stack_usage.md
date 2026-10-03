# 多段ブロックの compiler stack 使用量修正

## 対象と原因

`doc/list_runtime_followups.md` の旧 C-2 を起点に、Test DSL と List に限定せず通常の多段 Block / Closure を調査した。
言語仕様を変えない compiler 内部の修正。Sigil と Scar の二フェーズを扱うため level 3 とし、両 crate、CLI の process 境界、標準テストと workspace CI を検証する。

基線は `df09dc51`、通常の debug build。core dump を無効化した別 process で再現し、LLDB の停止箇所を確認した。

- `lib/tests/do.srt` の三つの `<-` を helper 呼出しから `it` 本文へ戻すと、`surtr test --quiet do` が SIGABRT で終了した。
- Test なしの List do 六 binds は Scar の `concretize_pending_trait_calls` のフレーム確保で停止した。この関数は一回あたり356,368 Bを確保し、有限のtyped木を20段辿る間に外側の型検査フレームと合算してstackを使い切った。
- do なしの12段Closureと12段matchブロックはSigilの`resolve_node`で停止した。
- parser の nesting 境界内にある31段の通常 Closure と binding 付き Block では、最初の修正後も Scar の `resolve_typed_node`、続いて `rewrite_specializations_in_node` のフレーム確保で停止した。共通の正規化と特殊化も対象に含めた。
- 小さいTest単独例の三 bindsは成功した。bind数だけを固定閾値とせず、周囲の式と生成後の木の深さを区別する。
- `.cargo/config.toml`の`RUST_MIN_STACK=33554432`が既定のRustテストthreadで失敗を隠す。局所回帰はCLI相当の8 MiBを明示して検証する。

## 修正方針と受入条件

Sigilの巨大なnode matchを責務ごとに分割し、Blockとliteral Closureの再帰を小さい入口から既存のscope処理へ渡す。
ScarのTrait call具体化は成功・エラーの再帰結果をBox化する。式検査もBlock / Closure / call / do / matchの再帰から、宣言等の大きな一時領域を分離する。
Scar の型付き木の正規化も再帰結果を Box 化し、特殊化は App / Block / Match / Closure の走査を小さい入口と専用 helper へ分離する。
型規則、Trait選択、capture、scope、source facts、走査順序と診断を維持する。旧巨大match経路は置換する。

正常な通常ブロック、closure、List / Option / Result doを受理し、誤型は原文の診断を返す。
parserの既存nesting拒否を維持する。List / Test専用処理、stack増量、再試行や曖昧なfallbackは追加しない。

回帰テストを先に追加して基線のabortを確認し、共通経路を修正する。
局所試験、元のTest内直接記述、別processのCLI試験と段数を変えた比較で確認する。
結果と未検証範囲は会話へ記録し、完了したC-2は残課題とdo仕様の制限から削除する。

## 検証結果

macOS arm64 の同じ debug build で、関数の prologue が確保する領域を比較した。
これらは各関数の値であり、呼出し先を含む合計値ではない。

| 関数 | 修正前 | 修正後 |
|---|---:|---:|
| Sigil `resolve_node` | 343,936 B | 2,672 B |
| Scar `check_node` | 145,152 B | 352 B |
| Scar `check_node_with_expected_relation` | 59,920 B | 6,288 B |
| Scar `concretize_pending_trait_calls` | 356,368 B | 65,200 B |
| Scar `resolve_typed_node` | 92,400 B | 58,768 B |
| Scar `rewrite_specializations_in_node` | 111,856 B | 3,360 B |

- 段数・bind 数・capture・binding を変えた115ケースは、基線の45件の abort が修正後は0件になった。正常な111件は成功し、parser の nesting 上限を超える4件は従来の診断で拒否した。
- binding を含む31段の通常 closure と16段の match ブロックは、別 process の `check`、`build`、`run` で成功した。
- 8 MiB を明示した Sigil の名前解決回帰と Scar の型検査回帰、CLI の別 process 回帰を workspace CI で確認した。誤型の拒否理由と原文の span を維持した。
- `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace`: 2,141 passed、53 binaries、終了コード0。
- `cargo run -- test --quiet --all`: 600件、終了コード0。新規 worktree では File テストに必要な `tmp/sandbox` がなく初回は5件失敗したため、既存 CLI integration test と同じディレクトリを用意して再実行した。
- 変更した Rust ファイルの `rustfmt --check` と `git diff --check` は成功した。workspace 全体の format check は、基線にもある `crates/sigil/src/resolver/pattern_consumers.rs` の整形差分で失敗する。このファイルは変更していない。
- サブエージェントによる最終差分の独立レビューに指摘事項はなかった。調査用の出力や stack 増量を製品コードに追加していない。
