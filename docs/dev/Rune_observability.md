# Rune Observability

`Rune` の開発観測オプションに関する補助仕様。
`surtr run` / `surtr dump` の観測系 UX、出力先、実装境界をまとめる。

正本との関係:

- CLI surface の正本は `doc/要件定義v9.md`
- VM 実行意味と観測の非介入原則は `docs/dev/EldrVM_spec.md`
- 本書は `Rune` と `Eldr` の観測系オプション設計メモ兼運用ガイド

---

## 1. 対象オプション

### 1.1 `surtr run`

- `--entry <name>`
- `--vm-dump <path>`
- `--vm-dump-on error|always`
- `--vm-stats`
- `--vm-stats-json`
- `--trace-call`
- `--trace-opcode`
- `--trace-limit <n>`
- `--trace-filter <csv>`
- `--phase-times`
- `--error-context verbose`

### 1.2 `surtr dump`

- `--format json`
- `--format viewer-json`
- `--entry <name>`
- `--opcode-histogram`
- `--peephole-candidates`

---

## 2. 出力方針

- ユーザプログラムの通常出力は従来どおり `stdout`
- 開発観測のための統計・トレース・時間計測は `stderr`
- `--vm-dump` は指定パスへ JSON ファイルを書き出す
- `dump --format json` / `dump --format viewer-json` の本体 JSON は `stdout`
- `dump --opcode-histogram` は `dump --format json` の JSON 本体に内包する

この方針により、`spec` テストやパイプ処理で `stdout` 契約を壊さない。

---

## 3. 各オプションの意味

### 3.1 `--vm-stats`

実行完了後に以下を出力する。

- `executed_opcodes`
- `builtin_calls`
- `function_calls`
- `closure_calls`
- `return_count`
- `tail_calls_optimized`
- `max_stack_depth`
- `max_frame_depth`
- conditional branch outcome counters
- opcode 別実行回数

`tail_calls_optimized` は current frame を再利用した user-function tail call 回数を表す。
TCO が効いた実行では `return_count` や `max_frame_depth` が非最適化時より小さくなりうる。

### 3.1.1 `--vm-stats-json`

実行完了後、compact JSON を `stderr` に 1 行出力する。`stdout` はユーザプログラム用のまま保持する。
`--vm-stats` と同時指定した場合は human-readable stats を先に出し、JSON を最後に出す。

```json
{
  "schema_version": 1,
  "stats": {
    "executed_opcodes": 12,
    "builtin_calls": 2,
    "function_calls": 1,
    "closure_calls": 0,
    "return_count": 1,
    "tail_calls_optimized": 0,
    "max_stack_depth": 3,
    "max_frame_depth": 2,
    "per_opcode": {
      "LoadConst": 3,
      "JumpIfFalse": 1
    },
    "branch": {
      "jump_if_true_taken": 0,
      "jump_if_true_not_taken": 0,
      "jump_if_false_taken": 1,
      "jump_if_false_not_taken": 0
    }
  },
  "trace": {
    "dropped_events": 0,
    "lines": []
  }
}
```

### 3.1.2 `--vm-dump <path>`

実行終了時に VM dump JSON を指定パスへ保存する。

- `stdout` / `stderr` とは分離し、既存の run 契約を壊さない
- dump には終了状態、exit code、最終 `pc` / opcode、stack / frame 深さ、VM observation、process runtime snapshot を含む
- `stats.branch` に conditional branch outcome counters を含む
- compile error で VM 実行に到達しなかった場合は dump を生成しない

process runtime snapshot の `worker_sets` は次の JSON 形状を持つ。

```json
{
  "id": 0,
  "worker_process": "ImageWorker",
  "supervisor": "ImageWorkerSupervisor",
  "target": 2,
  "min": 2,
  "max": 2,
  "member_pids": [3, 4],
  "live_count": 2
}
```

### 3.1.3 `--vm-dump-on error|always`

`--vm-dump` の保存条件を指定する。

- `error`: runtime error、`Err(...)` 終了、非0 exit code のときだけ保存
- `always`: 成功終了を含めて常に保存

### 3.2 `--trace-call`

関数呼び出し・builtin 呼び出し・closure 呼び出し・`Return` をトレースする。
opcode 単位より低ノイズな call-flow 確認を主目的とする。

### 3.3 `--trace-opcode`

各 opcode 実行時の以下をトレースする。

- `pc`
- `opcode`
- `stack_depth`
- `frame_depth`

`JumpIfFalse` / `JumpIfTrue` は opcode trace に加えて、条件評価後の分岐 outcome を
`branch pc=<pc> opcode=<kind> target=<target> taken=<bool>` として記録する。

### 3.4 `--trace-limit <n>`

trace 行の最大件数。上限超過分は捨て、末尾で `dropped_trace_events` を報告する。

### 3.5 `--trace-filter <csv>`

trace 対象の kind 名を CSV で指定する。

例:

- `CallBuiltin,Return`
- `JumpIfFalse,CallClosure`

比較は kind 名ベースで行い、大文字小文字は区別しない。

### 3.6 `--phase-times`

以下の elapsed time を ms 単位で出す。

- `parse`
- `resolve`
- `typecheck`
- `codegen`
- `compile` (`.srt` 入力時)
- `decode` (`.eldr` 入力時)
- `execute`
- `total`

現在の Rune compile helper は parse / resolve / typecheck / codegen の内訳を外部に公開していないため、
それらは `n/a` を許容する。`.srt` は `compile`、`.eldr` は `decode` を実測する。

### 3.7 `--error-context verbose`

runtime error または `run` entrypoint が返した `Err(...)` の表示に以下を追加する。

- `pc`
- `opcode`
- `function`
- stack / frame / locals 関連 detail

### 3.8 `--opcode-histogram`

`dump --format json` の出力に static opcode histogram を追加する。
これは実行回数ではなく、bytecode 上の命令内訳である。
`--opcode-histogram` または `--peephole-candidates` の指定時は、top-level に
`function_summary` も追加する。

`function_summary` は関数ごとの opcode histogram と call counts を持つ。

```json
{
  "summary": {
    "generated_function_count": 4,
    "partial_apply_wrapper_count": 1,
    "functions_with_call_closure": 2
  },
  "functions": [
    {
      "fun_idx": 3,
      "name": "compose#0",
      "arity": 1,
      "entry_pc": 200,
      "end_pc": 218,
      "flags": {
        "generated": true,
        "partial_apply_wrapper": false,
        "closure": false
      },
      "opcode_count": 18,
      "opcode_histogram": {
        "Call": 2,
        "Return": 1
      },
      "call_counts": {
        "call": 2,
        "call_builtin": 0,
        "call_closure": 0,
        "capture_closure": 0,
        "capture_closure_zero": 0
      }
    }
  ]
}
```

### 3.9 `--peephole-candidates`

`dump --format json` の出力に、現在の lowering 後にも残っている peephole 最適化候補に一致する
opcode window を追加する。各候補は `pc` / `function` / `source` /
`opcode_window` / `operands` を持つ。主用途は VM 命令圧縮の次手を選ぶための静的レポートであり、
実行意味には影響しない。

`operands` は window 内 opcode の機械可読な operand summary である。
すでに専用 opcode へ畳み込み済みの箇所は候補としては現れない。たとえば
`EqLocalTag + JumpIfFalse/JumpIfTrue` が `JumpIfLocalTagEq/JumpIfLocalTagNe` へ
lowering 済みの場合、`branch_fusion` は報告されず、専用 opcode の出現数は
`--opcode-histogram` / `optimization_summary` 側で観測する。

```json
{
  "kind": "branch_fusion",
  "pc": 123,
  "opcode_window": ["EqLocalTag", "JumpIfFalse"],
  "operands": [
    {
      "opcode": "EqLocalTag",
      "local_idx": 4,
      "tag_const_idx": 0,
      "tag": 0
    },
    {
      "opcode": "JumpIfFalse",
      "target": 140
    }
  ]
}
```

---

## 4. 実装境界

### 4.1 `Rune`

- CLI option parse
- VM dump の保存条件判定と JSON 書き出し
- compile phase timing 計測
- 観測結果の整形と `stderr` 出力
- `dump` JSON の histogram / peephole operand / function summary 追加

### 4.2 `Eldr`

- 実行中の opcode / call 統計収集
- conditional branch outcome 統計収集
- trace event 収集
- runtime error detail の verbose 表示

### 4.3 `Sindr`

- `Opcode::kind_name()` のような観測用の安定した opcode kind 名提供

---

## 5. 非目標

- 実行意味を変える profiling
- 最適化 pass の導入
- 永続 profile 設定ファイル
- flamegraph や sampling profiler
- viewer と trace の統合 UI

必要になった場合は、個別オプション運用を踏まえて `--profile dev|perf|trace` のような preset を後から導入する。

---

## 6. テスト観点

- `run` option parser の正常系 / 異常系
- `--vm-dump` の success / failure 保存条件
- `dump --opcode-histogram` の JSON 形状
- `dump --peephole-candidates` の JSON 形状と operand detail
- `--vm-stats-json` の JSON 形状
- branch outcome counters と branch trace
- `--phase-times` の stderr 出力
- VM stats の opcode 集計
- call trace の件数と kind
- runtime error verbose の出力形
- `stdout` 契約を壊さないこと

---

*Surtr — 開発観測も、意味論を壊さずに積み上げる。*
