# Surtr VM 改修対象仕様書 v0

> 本書は、Surtr のアクターモデル系プロセス基盤を導入する際の **VM 改修対象のみ** を対象にしたスコープ文書である。  
> language surface、型メタ記法、`defagent` / `defmessage` の user-facing 構文は本書の主対象ではない。  
> 本書は、VM / scheduler / runtime state が何を保持し、何を実行し、どこまで責務を持つかを定義する。

---

## 1. 目的

Surtr の初期フェーズでは、次のプロセス基盤を導入する。

- `Supervisor`
- `Task`
- `ReadOnlyAgent`
- `StateAgent`

この導入にあたり、VM は次を満たす必要がある。

1. **raw な型メタを解釈しない**  
   VM は compile 済みの runtime spec だけを見る。
2. **`Pending` をユーザ値にしない**  
   `Pending` は VM step の制御状態である。
3. **timeout を例外にしない**  
   timeout / cancel / process down は最終的に `Result::Err` として値化する。
4. **strict evaluation を崩さない**  
   非同期完了状態によって評価順序を変えない。
5. **プロセス監視・復旧・待機管理を runtime 側へ押し上げる**  
   ユーザロジックが process lifecycle の細部を背負わないようにする。

---

## 2. 本書のスコープ

### 2.1 含むもの

- VM が受け取る runtime process spec の形
- process instance / mailbox / waiting table / deadline queue の runtime 構造
- process call / reply / timeout / target down の実行フロー
- singleton registry slot / boot slot / root supervisor boot の runtime 責務
- `Pending` / `resume` / `Continuation` の VM step モデル
- Task 実行の最小ランタイムモデル
- debug / dump / stats の最小計測対象

### 2.2 含まないもの

- `defagent` / `defmessage` の最終 surface 構文
- 型メタの user-facing annotation syntax
- compiler 側の Scar / Forge の詳細実装
- 高機能 `TaskRef` API の最終仕様
- user-defined Supervisor
- dynamic registry / dynamic monitor / dynamic link
- distributed runtime

---

## 3. VM への入力契約

VM は source-level metadata を読まない。  
VM が受け取るのは、bytecode と一緒に渡される **`RuntimeProcessSpecTable`** のみである。

概念上:

```text
Typed program
-> ProcessMeta checked
-> RuntimeProcessSpec normalized
-> Bytecode + RuntimeProcessSpecTable
-> VM Execute
```

VM に必要なのは次の 2 つだけである。

- `BytecodeProgram`
- `RuntimeProcessSpecTable`

これにより、VM は language-level な意味論解釈器ではなく、
**runtime spec に従って process / waiting / resume を管理する実行器** に留まる。

---

## 4. 追加する runtime 中核データ構造

## 4.1 ProcessId / FutureId / CorrelationId

VM は少なくとも次の内部 ID を持つ。

```text
Pid
FutureId
CorrelationId
SpecId
```

- `Pid`: process instance 識別子
- `FutureId`: 未確定 reply / async call / task 完了待ち識別子
- `CorrelationId`: request-reply 対応づけ
- `SpecId`: process spec 参照

---

## 4.2 RuntimeProcessSpec

初期フェーズでは、VM は少なくとも次の spec 群を理解する。

```text
RuntimeProcessSpec =
  SupervisorSpec
  | TaskSpec
  | ReadOnlyAgentSpec
  | StateAgentSpec
```

### SupervisorSpec

- root かどうか
- boot children 一覧
- restart policy（最小）
- child 起動順

### TaskSpec

- call / async / cast / launch mode
- owner policy
- default call timeout
- task entry callable 形状

### ReadOnlyAgentSpec

- singleton 必須
- boot 対象か
- lazy 初期化か
- direct access 可否
- `init_fn` / `get_fn`
- registry slot（必要なら内部のみ）

### StateAgentSpec

- singleton / multi
- boot 対象か
- registry exposed か
- `init_fn` / `get_fn` / `set_fn`
- default call timeout
- owner policy

---

## 4.3 ProcessInstance

VM が保持する process 実体は少なくとも次を持つ。

```text
ProcessInstance {
  pid: Pid,
  spec_id: SpecId,
  status: ProcessStatus,
  mailbox: Mailbox,
  locals_and_stack: ExecutionContext,
  state_value: Value,
  owner: Option<Pid>,
  links: LinkSet,
  monitors: MonitorSet,
}
```

### `status`

```text
Runnable
Waiting(WaitReason)
Completed
Failed(RuntimeError)
Restarting
Stopped
```

### `WaitReason`

初期フェーズでは最小で次を持つ。

```text
WaitingFuture(FutureId)
WaitingReply(CorrelationId)
WaitingBoot
```

---

## 4.4 FutureState

`Future<A>` は公開型ではなく、VM 内部状態である。

```text
FutureState =
  Running
  | Ready(Value)
  | Cancelled(Value)
```

補足:

- timeout / process down / domain error は `Ready(Err(...))` として確定してよい
- `Failed(RuntimeError)` は `FutureState` ではなく VM 異常系として分ける

### FutureRecord

```text
FutureRecord {
  id: FutureId,
  owner: Pid,
  state: FutureState,
  deadline: Option<Instant>,
  waiters: SmallVec<Pid>,
  cancel_on_timeout: bool,
}
```

---

## 4.5 Mailbox / ReplyTable / WaitingTable

### Mailbox

- process 宛 message queue
- hidden message enum または runtime message packet を保持

### ReplyTable

```text
CorrelationId -> FutureId
```

- request 送信時に登録
- reply 受信時に resolve

### WaitingTable

```text
FutureId -> Vec<Pid>
```

または

```text
Pid -> WaitReason
```

- waiting process を逆引きできればよい
- resume 対象の復帰に使う

---

## 4.6 DeadlineQueue

timeout は起動時に固定し、VM 内では `deadline` として扱う。

```text
DeadlineQueue:
  (Instant, FutureId)
```

責務:

- tick ごと、または scheduler cycle ごとに期限切れ future を確定する
- timeout 発生時は `Ready(Err(Timeout))` にする
- waiter process を Runnable へ戻す

---

## 4.7 SingletonSlot / RegistrySlot

初期フェーズの registry は一般 registry ではなく、
**compiler-managed な singleton slot table** として扱う。

### SingletonSlotTable

```text
ConcreteProcessTypeId -> Pid
```

責務:

- boot 対象 singleton の pid 登録
- direct access singleton の内部 lookup
- registry exposed singleton の pid lookup

制約:

- multi process は登録しない
- runtime 任意キー登録はしない

---

## 5. VM step モデルの改修

## 5.1 StepOutcome の拡張

既存の `Continue / Halt / RuntimeError` に加え、少なくとも次を持つ。

```text
StepOutcome =
  Continue
  | Halt(Value)
  | Pending {
      future_id: FutureId,
      resume: Continuation,
    }
  | RuntimeError(RuntimeError)
```

`Pending` はユーザ値ではなく VM 制御結果である。

---

## 5.2 Continuation の保持

process が未確定値を demand した時、VM は continuation を保存して停止する。

Continuation は少なくとも次を復元できる必要がある。

- current frame / pc
- locals
- operand stack
- process status
- wait reason

初期実装では、既存 `ExecutionContext` を process instance ごと保持し、
`Pending` 時にそれをそのまま freeze する方式でよい。

---

## 5.3 Demand 処理

未確定な process call / task result をユーザ値として扱う直前で、VM は demand を行う。

概念上:

```text
DemandLocal(slot)
```

動作:

- local が `Ready(value)` ならそのまま積む
- local が `Pending(future_id)` なら `StepOutcome::Pending` を返す
- local が `Ready(Err(...))` なら `Result::Err` として積む

初期実装では新 opcode を増やさず、`LoadLocal` 前の VM 内部分岐として実装してよい。

将来最適化として独立 opcode `DemandLocal` を追加してよい。

---

## 5.4 strict evaluation の維持

VM は非同期 ready 状態によって評価順序を変えてはならない。

必要な性質:

- 関数引数は左から右へ評価
- 途中で `Pending` になれば後続引数は評価しない
- resume 後に残りの評価を再開する
- let 束縛も lowering 順を維持する

この原則により、process call が内部的に async でも、surface の評価順序は一定になる。

---

## 6. process call 実行フロー

## 6.1 同期的に見える process call

公開型:

```text
Process.call(pid, msg, timeout) -> Result<Reply, ProcessCallError>
```

ただし内部的には pending し得る。

### 開始時

1. request packet を生成
2. `CorrelationId` を発行
3. `FutureId` を発行
4. `ReplyTable[CorrelationId] = FutureId` を登録
5. target mailbox へ request を送る
6. current process 側 local には pending handle を置く

### demand 時

1. future が `Ready(Ok(reply))` なら reply を返す
2. `Ready(Err(err))` なら `Err(err)` を返す
3. `Running` なら current process を `Waiting(FutureId)` にして `Pending`

---

## 6.2 reply 到着時

1. target process が reply packet を送る
2. runtime が `CorrelationId` から `FutureId` を解決
3. `FutureState = Ready(Ok(reply))` に更新
4. waiter process を Runnable に戻す

---

## 6.3 target process down

待機中に target process が終了した場合:

1. reply 待ち future を `Ready(Err(ProcessDown))` にする
2. waiter process を Runnable に戻す
3. `ReplyTable` から対応づけを削除する

---

## 6.4 timeout

期限切れ時:

1. deadline queue から期限切れ future を検出
2. `FutureState = Ready(Err(Timeout))` に更新
3. waiter process を Runnable に戻す
4. 対応 correlation を破棄する

---

## 7. Task 実行モデルの最小改修

初期フェーズの `Task` は、呼び出し側完結の explicit async 実行を担う。

## 7.1 Task mode

- `call`
- `async`
- `cast`
- `launch`

### `call`
- caller が完了まで待つ
- timeout 指定可
- 最終的に `Result<A, TaskError>` へ収束

### `async`
- future / handle を返す
- 後で await / poll 可能

### `cast`
- fire-and-forget
- reply を待たない

### `launch`
- detached 実行
- owner policy の最小管理だけ行う

---

## 7.2 Task 用 runtime 要素

追加対象:

- task worker entry 起動
- task completion future 登録
- task owner 記録
- timeout deadline 記録
- await / poll 時の demand

初期フェーズでは、thread pool 実装詳細を固定しない。  
VM から見える契約は、**task completion が future completion として観測できること**だけでよい。

---

## 8. singleton / boot / supervisor の runtime 責務

## 8.1 RootSupervisor

初期フェーズでは user-defined Supervisor は導入せず、RootSupervisor を runtime が持つ。

責務:

- boot 対象 singleton の起動
- boot 順序の実行
- singleton slot への登録
- permanent restart の最小実装

---

## 8.2 boot 対象 singleton

boot される singleton について、runtime は次を保証する。

- boot 完了前には公開しない
- direct access / registry access 到達時点では起動済みとみなせる
- init failure は通常の業務ロジックへ流さず、boot failure として扱う

---

## 8.3 lazy ReadOnlyAgent

lazy read-only は process 自体を遅延生成しない。  
**process は boot 時に生成し、内部 state のみ未初期化** とする。

runtime 追加責務:

- internal state marker: `Uninitialized | Ready(Value) | Failed(Error)`
- 初回 `get` で `init_fn` を実行
- 成功時 `Ready(state)` に更新
- 失敗時は `Err` を返すか、policy に従って `Failed(Error)` を保持

---

## 9. Agent 実行モデルの最小改修

## 9.1 ReadOnlyAgent

VM が扱う handler:

- `init_fn`
- `get_fn`

runtime 契約:

- external set は存在しない
- singleton direct access は runtime が内部 pid lookup を行う
- `get` 呼び出しは hidden message dispatch と同等に扱ってよい

## 9.2 StateAgent

VM が扱う handler:

- `init_fn`
- `get_fn`
- `set_fn`

runtime 契約:

- `set_fn` は `Result<State>` を返す
- `Ok(next_state)` なら commit
- `Err(error)` なら state を変更しない
- surface 戻り値は `Result<()>` に lower してよい

---

## 10. timeout と error の VM 責務

## 10.1 timeout は例外にしない

VM は timeout を exception として投げない。

- task timeout → `Err(TaskError::Timeout)`
- process call timeout → `Err(ProcessCallError::Timeout)`

`Pending` は VM 制御状態、timeout は値レベル失敗、という境界を守る。

---

## 10.2 RuntimeError の境界

次は通常の `Result` に載せず `RuntimeError` でよい。

- bytecode 不正
- spec table 不整合
- scheduler 内部 invariant 破壊
- continuation 復元不能
- mailbox / reply table の内部矛盾

つまり:

- **ドメイン失敗** → `Err(...)`
- **timeout / process down** → `Err(...)`
- **VM 継続不能** → `RuntimeError`

---

## 11. debug / dump / stats 改修

VM 改修対象として、少なくとも次の可視化が欲しい。

## 11.1 vm dump への追加

既存 dump 系に以下を追加する。

- process spec table
- singleton slot table
- process instances
- waiting table
- deadline queue
- reply correlation table

---

## 11.2 stats

最低限ほしい計測値:

### Future / Pending
- created
- completed_ok
- completed_err
- timed_out
- resumed_count
- max_pending_futures
- max_waiting_processes

### Process call
- calls_started
- replies_received
- calls_timed_out
- target_down
- correlation_registered
- correlation_resolved

### Evaluation order
- demand_count
- demand_ready
- demand_pending
- skipped_due_to_pending

---

## 12. 初期フェーズの実装段階

## Stage 1: runtime 基盤

- `Pid`, `FutureId`, `CorrelationId`
- `ProcessInstance`
- `FutureRecord`
- `WaitingTable`
- `DeadlineQueue`
- `SingletonSlotTable`
- `StepOutcome::Pending`

## Stage 2: synchronous-looking process call

- request / reply correlation
- demand / pending / resume
- timeout → `Err(Timeout)`
- target down → `Err(ProcessDown)`

## Stage 3: RootSupervisor + singleton boot

- boot order
- singleton registration
- eager init
- lazy read-only internal state

## Stage 4: Agent 実行

- ReadOnlyAgent dispatch
- StateAgent dispatch
- `set_fn` commit / rollback

## Stage 5: Task 最小実装

- task start
- task completion future
- call / async / cast / launch の最小分岐

## Stage 6: dump / stats

- process tables dump
- pending / deadline 可視化
- correlation / waiting 統計

---

## 13. 避けること

初期フェーズでは次を避ける。

- `Future<A>` を通常型として公開する
- timeout を消費時指定の mutable 状態にする
- `PID.set_timeout()` のように shared PID を可変設定化する
- future readiness に応じて評価順序を変える
- dynamic registry を先に導入する
- user-defined supervisor を先に導入する
- runtime で raw metadata を解釈する

---

## 14. 最小結論

初期フェーズで VM が担うべき改修対象は、次に要約される。

1. **process instance を持つ runtime state への拡張**
2. **future / pending / resume を扱う scheduler 状態の追加**
3. **request-reply correlation と timeout deadline 管理**
4. **singleton boot / registry slot / root supervisor の最小導入**
5. **ReadOnlyAgent / StateAgent / Task の dispatch 実行**
6. **strict evaluation を崩さない demand モデルの導入**
7. **dump / stats による可視化の追加**

この範囲に留めることで、language surface を過剰に広げず、
安定した actor process 基盤を VM 側へ段階導入できる。
