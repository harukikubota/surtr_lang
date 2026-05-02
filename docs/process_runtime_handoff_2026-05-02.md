# Surtr Process Runtime Handoff 2026-05-02

## 対象コミット

- 実装コミット: `6d2cc301ce5384aa2c35cd2cdcde612e1643f205`
- コミット要約: `feat: add initial VM-backed process runtime`
- 検証: `cargo fmt && cargo nextest run --workspace`
- 検証結果: `900 tests run: 900 passed, 0 skipped`

この文書は、上記コミットで入った初期実装の引継ぎ用メモです。文書自体は後続のドキュメントコミットに含めます。

## 実装内容の要約

初期の VM 管理プロセス基盤として、`defagent` surface 構文、VM 内部の PID 値、プロセステーブル、隠し builtin、Task 標準モジュール、サンプル、仕様テストを追加しました。

主な到達点は次のとおりです。

- `@@agent(...) defagent` を parser で受理し、既存パイプラインに流せる `defmod` 相当へ lowering する。
- `ReadOnlyAgent` の直接 `get` と `StateAgent` の `pid/get/set` surface を動作させる。
- `Value::Pid` と VM 内部の `ProcessRuntime` を追加し、singleton process と state を VM 所有データとして保持する。
- `__process_pid`, `__process_spawn`, `__process_state`, `__process_store` を VM 特権 builtin として追加する。
- `Task.call/async/launch/cast` の surface を `lib/process.srt` に追加し、現時点では同期 builtin 経由で実行できるようにする。
- 仕様補足として `doc/surtr_actor_model_process_spec_v0.md` と `doc/surtr_vm_process_modification_scope_v0.md` を追加し、`doc/EldrVM_spec.md` に PID と隠し builtin の位置づけを追記する。
- `examples/process/**` と `tests/spec/**`, `tests/compile_errors/**` に動作サンプルと回帰テストを追加する。

## 主要ファイル

- `crates/spire/src/parser/decl.rs`: `defagent` の validation と lowering の中心。
- `crates/spire/src/lexer.rs`, `crates/spire/src/token.rs`: `defagent` token/keyword の追加。
- `crates/sindr/src/runtime.rs`: `Value::Pid` と `PidHandle`。
- `crates/sindr/src/builtin.rs`: 隠し process/task builtin metadata。
- `crates/eldr/src/vm.rs`: `ProcessRuntime`, process table, checkpoint rollback 連携、callable 同期呼び出し。
- `crates/eldr/src/builtin.rs`: process/task builtin の VM 実装。
- `lib/kernel.srt`: 隠し builtin 宣言。
- `lib/process.srt`: `Task` surface API。
- `crates/xldr/src/loader.rs`: `process.srt` を標準モジュールとして loader に登録。
- `tests/spec/modules/process_*`: agent surface の成功系仕様テスト。
- `tests/compile_errors/modules/process_readonly_agent_rejects_set`: read-only agent の不正 set を拒否する compile error test。

## 現在の動作モデル

`defagent` は独立した型・IR としてはまだパイプラインに載せず、Spire の段階で通常 module へ lowering しています。VM 側では、lowering 済みコードから隠し builtin を呼び、VM 内の `ProcessRuntime` が PID と state を管理します。

`ReadOnlyAgent` の `get` は agent 名から singleton PID を取得し、VM 内 state を読んで user 定義の getter を呼びます。`StateAgent` の `set` は現在 state を読み出し、user 定義 setter が `Ok(next_state)` を返した場合だけ `__process_store` で commit します。setter が `Err` を返した場合、既存 state は維持されます。

`Task` API は surface と builtin 経路を先に確保した段階です。`Task.call` は callable を同期実行して `Result` に包みますが、`async/launch/cast` も現時点では真の scheduler や mailbox を使わず、同期的な足場実装です。

## 既知の制限

この実装は「大量プロセスを安全に捌く完成基盤」ではなく、VM 管理プロセスへ移行するための最初の足場です。

未実装の大きな領域は次のとおりです。

- `RuntimeProcessSpecTable` を compiler から bytecode/runtime へ運ぶ仕組み。
- `PID<T>` の静的型。現状は runtime value として `Pid` を持つが、surface 型は暫定的に弱い。
- `ProcessInstance` の status, mailbox, execution context, owner, supervisor link。
- `FutureId`, `CorrelationId`, `FutureRecord`, `WaitingTable`, `DeadlineQueue`。
- `StepOutcome::Pending` と continuation freeze/resume。
- timeout, wake, cancel, backpressure, mailbox limit, process reaping。
- RootSupervisor boot と restart policy。
- VM dump/stats による process table 観測。
- `Task.async/launch/cast` の非同期 semantics。

また、`Multi` agent の `spawn` lowering は VM では PID を返せる形に寄せていますが、surface の静的型はまだ `PID<T>` を表現できていません。次フェーズで型システムと runtime metadata を合わせて整理してください。

## 次の作業方針

次フェーズでは `doc/surtr_vm_process_modification_scope_v0.md` を正として、Spire lowering だけに閉じた実装から VM の process object model へ段階的に寄せるのがよいです。

推奨順序は次のとおりです。

1. `defagent` の agent metadata を AST/Sigil/Scar/Forge/Sindr IR に明示的に運ぶ。
2. `Bytecode` に `RuntimeProcessSpecTable` を追加し、VM 起動時に agent spec を登録する。
3. `PID<T>` の型表現を Scar/Sindr に追加し、surface の `pid/spawn` 戻り値を正しく型付けする。
4. `ProcessRuntime` を `ProcessInstance` 中心の構造へ拡張し、status と mailbox を持たせる。
5. `FutureRecord`, `WaitingTable`, `DeadlineQueue` を追加し、timeout を `Result` の `Err` として返せるようにする。
6. VM step 結果に `Pending` を追加し、Task call/async が block ではなく suspend/resume できるようにする。
7. `Task.async/launch/cast` を本物の scheduler 経路に接続する。
8. RootSupervisor と ReadOnly singleton の lazy/eager boot policy を実装する。
9. process table の dump/stats と stress test を追加し、大量 process 時の安全性を検証する。

## 検証済みサンプル

次のサンプルと仕様テストが実装コミットに含まれています。

- `examples/process/read_only_agent`: `ReadOnlyAgent` の直接 `get`。
- `examples/process/state_agent_singleton`: `StateAgent` の singleton `pid/get/set` と setter error 時の rollback。
- `examples/process/task_call`: `Task.call` の同期 callable 実行。
- `tests/spec/modules/process_readonly_agent_direct_access`
- `tests/spec/modules/process_state_agent_singleton_surface`
- `tests/spec/process_task_call.srt`
- `tests/compile_errors/modules/process_readonly_agent_rejects_set`

## 注意点

`cargo fmt` により、今回の主目的ではない `crates/forge/src/codegen.rs`, `crates/scar/src/checker/types.rs`, `crates/xldr/tests/repl_core.rs` に formatting 差分が入っています。挙動変更の中心ではありませんが、実装コミットには含まれています。

今の `ProcessRuntime` は `BTreeMap` ベースの VM 所有テーブルで、checkpoint rollback には対応しています。ただし、これは scheduler/backpressure/reaping の代替ではありません。大量プロセス対応を判断する場合は、必ず mailbox と waiting/deadline 管理を入れてから stress test してください。
