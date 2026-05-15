# Apply / Compose Lowering

## 目的

- Apply / pipe / compose 周辺の surface behavior を変えずに、tail position の closure call bytecode を圧縮する。
- 広い `Apply` / `Compose` opcode は導入せず、Forge の狭い lowering / peephole と VM opcode で観測可能な命令削減を行う。
- 既存の zero-capture closure lowering は維持し、主要 dump で `capture_closure_zero = 0` を完了条件として固定する。

## 作成日

- 2026-05-12 (Asia/Tokyo)

## 方針

- 新規 opcode は `TailCallClosure { arity, span_start, span_end }` のみ追加する。
- `TailCallClosure` は `CallClosure; Return` の compressed opcode として扱う。
- 初期版では TCO の観測値 `tail_calls_optimized` を完了条件にしなかった。後続の TCO 条件整理で user function target は観測値へ含める契約に更新した。
- `.eldr` bytecode version は変更せず、opcode enum 末尾追加で既存 bincode tag 互換を守る。

## 実施内容

- `sindr::Opcode` に `TailCallClosure { arity, span_start, span_end }` を末尾追加した。
- opcode kind name / span extraction / viewer / serde roundtrip test を追加した。
- Forge の emit / finalize 経路で、ラベル境界を跨がない `CallClosure; Return` を `TailCallClosure` に融合する peephole を追加した。
- finalize 時の PC remap は current IR 内の function entry だけを対象にし、preloaded / 既存 function entry を壊さないようにした。
- Eldr では `TailCallClosure` を `CallClosure` と同じ callable / arity / capture ordering で評価するようにした。
- lexical captures は `lexical_captures -> args` の順で callee locals に入れる契約を維持した。
- builtin callable の tail call は call-site span と副作用を既存 `CallClosure` と同等に扱い、返り値を現在 frame の caller へ返すようにした。
- user function callable の tail call は現在 frame を callee locals / call-site へ差し替える。初期実装では `tail_calls_optimized` を増やさない暫定実装だったが、後続の TCO 条件整理で user-function TCO 観測値へ含めるよう更新した。
- verifier では top-level `Return` と同じく top-level `TailCallClosure` を拒否するようにした。
- `rune dump` の histogram / optimization summary / function summary / operand summary を `TailCallClosure` に対応させた。
- `rune dump --peephole-candidates` で残存 `CallClosure; Return` を `tail_call_closure` candidate として表示するようにした。
- `docs/dev/EldrVM_spec.md` に `TailCallClosure` の VM 契約を追記した。

## Corner Cases

- 非 callable target は既存と同等に `CallClosure expects a callable value` の runtime error を返す。
- arity mismatch は既存 closure call と同じ runtime error を返す。
- non-tail の `CallClosure` は融合しない。
- `CallClosure` 後続が `Pop`、算術、`StructNew`、branch、field access などの場合は融合しない。
- `if` / `match` branch 内では、各 branch 内の `CallClosure; Return` だけを個別に融合し、branch target PC を壊さない。
- label boundary 直後の `Return` とは融合しない。
- top-level には `Return` がないため融合対象にせず、top-level safety contract も維持する。
- TCO 条件整理後は、user function target の `TailCallClosure` も frame reuse / `tail_calls_optimized` 観測対象とする。builtin / template target は圧縮実行として caller へ返るが、user-function TCO 観測値には含めない。

## 観測

- custom tail-apply source の dump で `opcode_histogram.TailCallClosure > 0` と `optimization_summary.apply_compose.tail_call_closure > 0` を確認した。
- standard-heavy sample の `result_helpers.srt` では `capture_closure_zero = 0` を維持している。
- `result_helpers.srt` では `branch_fusion` / `tail_call_closure` ともに残存候補 0 を確認し、lowering 済み箇所は histogram / optimization summary 側で観測できることを確認した。

## 検証結果

- 2026-05-12 実施:
  - `cargo nextest run -p sindr -p eldr -p forge`
  - `cargo nextest run -p rune --test integration run_eldr`
  - `cargo nextest run -p rune --test integration run_srt`
  - `cargo nextest run -p rune --test integration build_roundtrip`
  - `cargo nextest run --workspace`
- 結果:
  - `cargo nextest run -p sindr -p eldr -p forge`: 298 passed
  - `cargo nextest run -p rune --test integration run_eldr`: 31 passed
  - `cargo nextest run -p rune --test integration run_srt`: 12 passed
  - `cargo nextest run -p rune --test integration build_roundtrip`: 13 passed
  - `cargo nextest run --workspace`: 1224 passed

## 今後の課題

- `TailCallClosure` の観測契約は user function target に限って `tail_calls_optimized` へ含める形で整理済み。今後は必要に応じて dump / viewer 側で target 種別をより見やすくする。
- 残存 `tail_call_closure` candidate のうち、prelude / generated wrapper 由来で安全に融合できるものを追加で調査する。
- Apply / compose 専用の広い VM opcode は、call semantics と capture semantics がさらに安定するまで導入しない。
