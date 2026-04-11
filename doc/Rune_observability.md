# Rune Observability

`Rune` の開発観測オプションに関する補助仕様。
`surtr run` / `surtr dump` の観測系 UX、出力先、実装境界をまとめる。

正本との関係:

- CLI surface の正本は `doc/要件定義v9.md`
- VM 実行意味と観測の非介入原則は `doc/EldrVM_spec.md`
- 本書は `Rune` と `Eldr` の観測系オプション設計メモ兼運用ガイド

---

## 1. 対象オプション

### 1.1 `surtr run`

- `--entry <name>`
- `--vm-stats`
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

---

## 2. 出力方針

- ユーザプログラムの通常出力は従来どおり `stdout`
- 開発観測のための統計・トレース・時間計測は `stderr`
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
- opcode 別実行回数

`tail_calls_optimized` は current frame を再利用した user-function tail call 回数を表す。
TCO が効いた実行では `return_count` や `max_frame_depth` が非最適化時より小さくなりうる。

### 3.2 `--trace-call`

関数呼び出し・builtin 呼び出し・closure 呼び出し・`Return` をトレースする。
opcode 単位より低ノイズな call-flow 確認を主目的とする。

### 3.3 `--trace-opcode`

各 opcode 実行時の以下をトレースする。

- `pc`
- `opcode`
- `stack_depth`
- `frame_depth`

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
- `execute`
- `total`

`.eldr` 入力では compile phase は `n/a` を許容する。

### 3.7 `--error-context verbose`

runtime error 表示に以下を追加する。

- `pc`
- `opcode`
- `function`
- `call_site`
- stack / frame / locals 関連 detail

### 3.8 `--opcode-histogram`

`dump --format json` の出力に static opcode histogram を追加する。
これは実行回数ではなく、bytecode 上の命令内訳である。

---

## 4. 実装境界

### 4.1 `Rune`

- CLI option parse
- compile phase timing 計測
- 観測結果の整形と `stderr` 出力
- `dump` JSON の histogram 追加

### 4.2 `Eldr`

- 実行中の opcode / call 統計収集
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
- `dump --opcode-histogram` の JSON 形状
- VM stats の opcode 集計
- call trace の件数と kind
- runtime error verbose の出力形
- `stdout` 契約を壊さないこと

---

*Surtr — 開発観測も、意味論を壊さずに積み上げる。*
