# Tail Call Optimization

## 目的

- Surtr VM に末尾呼び出し最適化を入れ、tail-recursive な関数で frame depth が入力に比例して増えないようにする
- `fib(50)` と `reduce` ワークロードを before/after で比較し、非 tail 関数では挙動が変わらないことを確認する
- 今回の計測・実装・制約を次回以降の最適化作業に引き継げる形で残す

## 実装方針

- `forge`:
  - 関数本体と closure 本体は tail-position 専用の生成経路で終端を作る
  - `Block` の最後、`if` の各 branch、`match` の各 arm では tail call が `Call` / `CallClosure` の直後に `Return` へ並ぶようにする
- `eldr`:
  - 非 top-level frame 上で、`Call` / `CallClosure` の次 opcode が `Return` のとき current frame を再利用する
  - builtin target の `CallClosure` は再利用しない
  - 観測用に `VmStats.tail_calls_optimized` を追加する
- 計測:
  - `crates/xldr/benches/tco.rs` に `criterion` ベースの benchmark を追加する
  - benchmark 実行時に wall-clock と `VmObservation` の両方を採取する

## 変更点の要約

- `crates/forge/src/codegen.rs`
  - `emit_tail_node(...)` を追加
  - 関数本体・closure 本体で tail-position 専用生成を使うように変更
- `crates/eldr/src/vm.rs`
  - current frame 再利用による TCO を追加
  - `VmStats.tail_calls_optimized` を追加
- `crates/xldr/Cargo.toml`
  - `criterion` と `tco` bench を追加
- `crates/xldr/benches/tco.rs`
  - `fib_tail_50`
  - `reduce_with_fib_tail_inputs`
  - `sum_non_tail_10000`
- `tests/integration/language_features.rs`
  - tail recursion / mutual recursion / match arm / non-tail recursion の観測テストを追加
- `docs/dev/EldrVM_spec.md`
  - tail-position call の frame reuse 契約を追記
- `docs/dev/Rune_observability.md`
  - `tail_calls_optimized` を追記

## 計測ケース

- 実行日: 2026-04-11 (Asia/Tokyo)
- コマンド:
  - `cargo bench -p xldr --bench tco -- --noplot`
- ケース:
  - `fib_tail_50`
  - `reduce_with_fib_tail_inputs`
  - `sum_non_tail_10000`

## Before / After

### 観測値

| case | before max_frame_depth | after max_frame_depth | before return_count | after return_count | before tail_calls_optimized | after tail_calls_optimized |
|---|---:|---:|---:|---:|---:|---:|
| fib_tail_50 | 52 | 2 | 51 | 1 | 0 | 50 |
| reduce_with_fib_tail_inputs | 61 | 4 | 358 | 15 | 0 | 343 |
| sum_non_tail_10000 | 10002 | 10002 | 10001 | 10001 | 0 | 0 |

### Benchmark time

| case | before | after |
|---|---|---|
| fib_tail_50 | 6.2775 ms - 6.3846 ms | 6.2158 ms - 6.2437 ms |
| reduce_with_fib_tail_inputs | 6.6222 ms - 10.073 ms | 6.5532 ms - 6.6078 ms |
| sum_non_tail_10000 | 11.523 ms - 12.112 ms | 11.693 ms - 11.996 ms |

## 気づいた制約

- top-level call 自体は再利用対象にしないため、tail-recursive 関数でも `max_frame_depth` は 1 ではなく 2 になる
- `CallClosure` の TCO は target が user function の場合だけに限定した
- TCO 判定は bytecode 上で「次 opcode が `Return`」に依存するため、branch/arm の終端は `forge` 側で明示的に揃える必要があった
- non-tail 再帰では frame depth と return count は従来どおり増え、今回の変更で短絡されないことを確認した

## 次の改善候補

- `surtr run` の観測出力に `tail_calls_optimized` を露出する
- viewer / dump で tail-position call を視覚的に分かるようにする
- 必要になったら bytecode レベルの明示的な tail-call marker 追加を検討する
- top-level trampoline や loop lowering を検討して、さらに大きい再帰ワークロードを安定実行できるようにする
