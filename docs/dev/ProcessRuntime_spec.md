# Surtr Process Runtime 仕様書

> Surtr の process 定義、BootPlan、Supervisor、handler dependency、標準 I/O handler、
> および VM に渡す正規化済み process runtime 契約の正本仕様。

対象: Process Runtime Architecture 改修  
除外: PubSub / distributed process / generic receive / user-facing generic send / yield / boundary layer 本実装

## 0. この文書の位置づけ

この文書は、Process Runtime 改修に伴う **正式仕様** である。

実装が本書に追いついていない箇所は、現行実装を正とせず、本書を目標契約として扱う。
未実装仕様をテストに固定する段階では skipped / ignored test ではなく、
実装可能な単位に分けて `spec` / `compile_errors` / `integration` へ通常テストとして追加する。

目的は次の通り。

- 入力ファイル範囲内で、現行実装から仕様が変わる箇所を整理する
- process 定義、Boot 設定、呼び出し側コードの最終形を整理する
- VM が最終的に受け取る型・概念を説明する
- I/O handler の定義、差し替え、標準 I/O の扱いを整理する
- diagnostics の発生パターンと簡易メッセージを表に落とす

本改修では、Agent 特化の暫定実装を拡張するのではなく、Agent / GenServer / Supervisor / Worker / Task を `RuntimeProcessSpec` と `BootPlan` によって共通化する。

---

## 1. 非対象

| 項目 | 扱い |
|---|---|
| PubSub | 完全に除外 |
| distributed process / node / cluster | 除外 |
| generic `receive` | 導入しない |
| user-facing generic `send(pid, msg)` | surface に出さない |
| `yield` | 当面実装しない |
| boundary layer 本実装 | process 基盤安定後の課題 |
| Task.Supervisor | 初期フェーズでは対象外 |
| Task と DynamicSupervisor の link | 初期フェーズでは対象外 |
| Worker lazy init | 後回し。非同期 call API で吸収予定 |

---

## 2. 入力ファイル範囲内で現行実装が変わるところ

### 2.1 全体方針

現行実装は `@agent(...) defagent` ベースの Agent 特化実装が先行している。仕様変更後は、各 process 定義を compiler が吸収し、VM 契約である `RuntimeProcessSpec` へ正規化する。

| 領域 | 現行 | 変更後 |
|---|---|---|
| process metadata | `@agent(kind, instance, boot, registry, lazy)` | `meta { instance, init_policy, state }` |
| process kind | `ReadOnlyAgent / StateAgent` 中心 | `Agent / GenServer / Supervisor / Task / DynamicSupervisor` を共通 spec 化 |
| instance 名 | `Multi` が残る | `Worker` に統一 |
| Agent kind | metadata の `kind` 明示 | `@set` の有無から導出 |
| boot / registry / lazy | process 定義 metadata に混在 | `init_policy` は定義側、起動対象・timeout・override は Boot / supervisor 側。`boot: Required` などの policy 指定は使わない |
| VM 入力 | Agent lowering 由来の metadata | immutable `RuntimeProcessSpec` + `RuntimeBootPlan` |
| Lazy | 初回 state access で materialize する実装がある | VM boot 時に process instance を起動し、Ready まで scheduler 管理 |
| Process state | hidden builtin `__process_state` がある | `meta { state: StateTy }` で process state を宣言する |
| GenServer | 未実装 | `defgenserver` / `@call` / `@cast` を追加 |
| Supervisor | Root 足場中心 | Root / Runtime / Dynamic の概念を spec / runtime に反映 |
| Worker owner | allocation 時 `owner: None` の経路がある | default owner = current process |
| `Process::sleep` | host thread blocking 寄り | scheduler timer。呼び出した process のみ suspend |
| Task | body 実行が同期寄り | 使い捨て process として scheduler 管理 |
| timeout | owner ごとの hidden helper に寄る | runtime-managed call 後方 modifier `@timeout` として整理 |
| singleton 利用検査 | singleton direct call / PID lookup で実施 | compile unit 単位で `required_singletons ⊆ available_singletons` を検査 |
| 標準 I/O | VM 内部の標準入力・標準出力・標準エラー用リストに直接寄る経路がある | `StdIn` / `StdOut` / `StdErr` builtin handler への message call として扱う |
| I/O handler 差し替え | 実行モードや VM 内部処理に寄る | process `meta.handlers` で default を宣言し、`supervisor_init` で override する |

### 2.2 想定される実装入力の範囲

この表は、実装作業のタスク一覧ではなく、仕様変更が影響する入力範囲を示す。

| 入力範囲 | 変わる仕様 |
|---|---|
| parser / AST | `meta {}`、`meta.state`、`meta.handlers`、`ctx.<slot>`、`supervisor_init`、`defgenserver`、`ProcessInit<T>` の出現制限を扱う |
| semantic check | Agent kind 導出、process state 契約一致、Lazy 許可 kind 検査、Boot timeout 範囲検査、handler capability / override 検査を行う |
| IR / runtime metadata | `RuntimeProcessSpec`、`RuntimeHandlerSpec`、`RuntimeInitSpec`、`RuntimeBootPlan` へ正規化する |
| codegen | surface syntax ではなく、正規化済み spec と dispatch 情報を VM に渡す |
| VM / scheduler | `Initializing`、`Ready`、`Waiting`、`deadline_queue`、`waiting_table`、`init_waiters`、process context の handler slot を扱う |
| standard library | `ProcessInit<T>`、`TimeOutError`、`Process::sleep`、`Task::async`、singleton PID API、`InHandler` / `OutHandler` capability を整理する |
| diagnostics | 定義、Boot、呼び出し、VM spec 境界の各エラーを分ける |

---

## 3. 各プロセス定義、Boot 設定、呼び出し側のコード

### 3.1 process kind と instance 軸

process kind と instance は別軸として扱う。

| 軸 | 値 |
|---|---|
| process kind | `Agent`, `GenServer`, `Supervisor`, `RuntimeSupervisor`, `DynamicSupervisor`, `Task` |
| instance | `Singleton`, `Worker` |

`Agent / GenServer / Supervisor` は振る舞い種別であり、`Singleton / Worker` は生成・取得方針である。

### 3.2 process metadata

process 定義に残す metadata は `meta { ... }` に置く。

```surtr
defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  ...
}
```

定義側に置くもの:

| key | 意味 |
|---|---|
| `instance` | `Singleton` または `Worker` |
| `init_policy` | `Eager` または `Lazy` |
| `state` | process handler が扱う state 型。primitive / container / user-defined のいずれも明記必須 |
| `handlers` | process-local readonly handler dependency と default target |

定義側に置かないもの:

| 項目 | 移動先 |
|---|---|
| 起動対象に含めるか | BootPlan / `supervisor_init` |
| init route | BootPlan / `supervisor_init` |
| init timeout | BootPlan / `supervisor_init` |
| standard singleton override | BootPlan / `supervisor_init` |
| handler override | BootPlan / `supervisor_init` |
| registry | runtime / singleton slot / BootPlan |

### 3.3 `init_policy`

`init_policy` は process 定義側の性質である。

| policy | `@init` 戻り値 | 意味 |
|---|---|---|
| `Eager` | `Result<State>` | 1 回の init 実行で state を確定する |
| `Lazy` | `Result<ProcessInit<State>>` | Ready まで scheduler 管理で init を再実行する |

`Lazy` は Rust の lazy loading のような初回参照時 materialize ではない。VM boot 時に process instance / PID / singleton slot は確保される。state value は `ProcessInit::Ready(state)` が返るまで未確定である。

### 3.4 `ProcessInit<T>`

`Lazy` の `@init` だけが runtime protocol として `ProcessInit<T>` を返せる。

```surtr
defenum ProcessInit<T> {
  Pending,
  PendingAfter(Duration),
  Ready(T),
}
```

| variant | 意味 |
|---|---|
| `Pending` | runtime default retry policy に従って再実行 |
| `PendingAfter(Duration)` | 指定 duration 後に同じ init route を再実行 |
| `Ready(T)` | 初期化完了。`T` を live state として設定 |

`ProcessInit<T>` は Lazy `@init` の戻り値以外に出現してはならない。

### 3.5 Lazy retry policy

`Pending` の runtime default retry policy は次の暫定値とする。

| 項目 | 値 |
|---|---:|
| 初回 retry | `10ms` |
| backoff | exponential |
| backoff 係数 | `2.0` |
| jitter | なし |
| 最大 retry interval | `1s` |
| 最小 scheduler tick | `1ms` |

`Pending` が続く場合の概形:

```text
10ms -> 20ms -> 40ms -> 80ms -> 160ms -> 320ms -> 640ms -> 1000ms -> 1000ms -> ...
```

`PendingAfter(Duration)` は retry hint であり、timeout を延長しない。Boot timeout が最優先である。

### 3.6 Boot timeout

BootPlan の init timeout は、process 起動から `Ready(state)` 到達までの deadline である。

| 項目 | 値 |
|---|---:|
| default init timeout | `5s` |
| min init timeout | `1ms` |
| max init timeout | `60s` |
| 未指定時 | `5s` |
| `PendingAfter(0ms)` | runtime が `1ms` に丸めてよい |
| `PendingAfter` が deadline を超える | deadline で timeout |

Boot timeout 超過は `RuntimeError::ProcessInitTimeout` とする。

Lazy `@init` が `Err(error)` を返した場合は、ユーザによる回復対象にしない。`RuntimeError::ProcessInitFailed` として扱う。これは VM 実装バグではなく、process init failure を表す runtime error である。

### 3.7 Lazy 許可範囲

`init_policy: Lazy` を許可する範囲は次に限定する。

| process | Lazy |
|---|---|
| Singleton Agent | 許可 |
| Singleton GenServer | 許可 |
| Worker Agent | 禁止 |
| Worker GenServer | 禁止 |
| Supervisor | 禁止 |
| RuntimeSupervisor | 禁止 |
| DynamicSupervisor | 禁止 |
| Task | 禁止 |

Supervisor は state を持たないため Lazy init の概念を持たない。Worker の Lazy は将来課題とし、当面は同期 API 実装を優先する。

### 3.8 process state declaration

process state は process 定義側の `meta.state` を唯一の宣言場所とする。

```surtr
defstruct CounterState {
  value: Int,
}

impl CounterState {
  def new(value: Int) -> Self {
    CounterState { value: value }
  }
}

defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: CounterState
  }
}
```

`meta.state` は primitive / builtin container / user-defined type のいずれでも省略不可とする。
user-defined state 型は通常の type と同じ規則に従い、public signature・pattern match・field access・外側スコープでの構築に追加制約を持たない。
struct literal / `new` 契約 / private field は process state でも一般 struct と同一ルールを適用する。

### 3.8.1 compiler-managed lower surface

process runtime の lower surface は [lib/process.srt](/Users/haruca/work/rust/surtr/lib/process.srt) に置く。

- `Process` は通常 user code からそのまま呼べる runtime utility module とし、`Process::self` / `Process::sleep` など public API だけを置く
- `Process::self()` は process handler / process-owned helper の内部だけで使える public API とし、通常 top-level code や一般関数からは使えない
- user-facing 正規系は `Process::*`, `Task::*`, `Workers::*`, generated owner helper (`MySupervisor::spawn` など) とする
- `Agent` / `GenServer` / `Supervisor` は compiler-managed lower module であり、generated owner helper がここへ lower される
- `Workers` は public API を `Workers::submit` / `Workers::broadcast` / `Workers::reserve` / `Workers::size` に一本化し、`__workers_*` hidden 宣言は `process.srt` には置かない
- canonical な runtime builtin 名は `__process_*`, `__workers_*`, `__supervisor_*`, `__dynamic_supervisor_*`, `__out_handler_write` とするが、`__*` は VM/runtime 内部名であり user-facing stdlib surface ではない
- `Workers<$Worker>` と `WorkerLease<$Worker>` も `process.srt` の builtin type として定義する
- これらは REPL / project compile の両方で同じ stdlib ルートから見える
- hidden lower 名は compiler-managed であり、user code から直接参照・import できない
- user-facing process surface には lowering 都合の `name: String` のような中間引数を出さない

process owner ごとの compiler-managed 名は共通予約集合として扱う。

- `pid`
- `spawn`
- `adopt`
- `status`
- `workers`

### 3.9 Agent

Agent は 1 state / 1 read path / 1 write path の簡潔 API とする。複数 protocol が必要な場合は GenServer を使う。

Agent kind は `@set` の有無から導出する。

| 条件 | 導出 kind |
|---|---|
| `@set` なし | ReadOnly Agent |
| `@set` あり | State Agent |

制約:

| annotation | 個数 |
|---|---:|
| `@init` | 1 |
| `@get` | 1 |
| `@set` | 0 または 1 |

Eager Agent:

```surtr
defstruct CounterState {
  value: Int,
}

impl CounterState {
  def new(value: Int) -> Self {
    CounterState { value: value }
  }
}

defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: CounterState
  }

  @init
  def init() -> Result<CounterState> {
    Ok(CounterState::new(0))
  }

  @get
  def get(state: CounterState) -> Result<Int> {
    Ok(state.value)
  }

  @set
  def set(state: CounterState, delta: Int) -> Result<CounterState> {
    next = state.value + delta
    if(next >= 0,
      Ok(CounterState::new(next)),
      Err(NoneError)
    )
  }
}
```

外部 surface:

```surtr
value = Counter::get()
_ = Counter::set(1)
```

`@set` の `Err` では state を更新しない。外部 surface の戻り値は `Result<()>` とする。

Lazy Agent:

```surtr
defstruct CacheState {
  client: Client,
}

defagent CacheClient {
  meta {
    instance: Singleton
    init_policy: Lazy
    state: CacheState
  }

  @init
  def init() -> Result<ProcessInit<CacheState>> {
    if(CacheService::ready?()) {
      client = CacheService::connect()
      Ok(ProcessInit::Ready(CacheState { client: client }))
    } else {
      Ok(ProcessInit::PendingAfter(100ms))
    }
  }

  @get
  def get(state: CacheState, key: String) -> Result<String> {
    state.client.get(key)
  }
}
```

Worker Agent:

```surtr
defstruct ImageWorkerState {
  jobs: Int,
}

impl ImageWorkerState {
  def new(jobs: Int) -> Self {
    ImageWorkerState { jobs: jobs }
  }
}

defagent ImageWorker {
  meta {
    instance: Worker
    init_policy: Eager
    state: ImageWorkerState
  }

  @init
  def init(start: Int) -> Result<ImageWorkerState> {
    Ok(ImageWorkerState::new(start))
  }

  @get
  def value(state: ImageWorkerState) -> Result<Int> {
    Ok(state.jobs)
  }

  @set
  def assign(state: ImageWorkerState, delta: Int) -> Result<ImageWorkerState> {
    Ok(ImageWorkerState::new(state.jobs + delta))
  }
}

pid =? ImageWorker::init(0)
_ =? ImageWorker::assign(pid, 3)
jobs =? ImageWorker::value(pid)
```

### 3.10 GenServer

GenServer は複数 query / command を持つ stateful process とする。

```surtr
defstruct CounterServerState {
  value: Int,
}

impl CounterServerState {
  def new(value: Int) -> Self {
    CounterServerState { value: value }
  }
}

defgenserver CounterServer {
  meta {
    instance: Singleton
    init_policy: Eager
    state: CounterServerState
  }

  @init
  def init() -> Result<CounterServerState> {
    Ok(CounterServerState::new(0))
  }

  @call
  def view(state: CounterServerState, label: String) -> Result<CallResult<String, CounterServerState>> {
    Ok(CallResult::Reply(label ++ "=" ++ to_string(state.value), state))
  }

  @cast
  def add(state: CounterServerState, delta: Int) -> Result<CastResult<CounterServerState>> {
    next = state.value + delta
    if(next >= 0,
      Ok(CastResult::Next(CounterServerState::new(next))),
      Err(NoneError)
    )
  }

  def format(label: String, value: Int) -> String {
    label ++ "=" ++ to_string(value)
  }
}
```

GenServer 内では `def` のみを使う。公開性は annotation の有無で決める。

| 関数 | 外部公開 |
|---|---|
| `@init` | いいえ |
| `@call` | はい |
| `@cast` | はい |
| annotation なし `def` | いいえ。内部 helper |

Handler 契約:

| handler | 内部 signature | 外部 surface |
|---|---|---|
| singleton `pid` | hidden lower helper | `Type::pid() -> PID<Type>` |
| `@init` Eager | `(...) -> Result<State>` | なし |
| `@init` Lazy | `(...) -> Result<ProcessInit<State>>` | なし |
| `@call` | `(State, Input...) -> Result<CallResult<Reply, State>>` | `Type::name(...Input) -> Result<Reply>` |
| `@cast` | `(State, Input...) -> Result<CastResult<State>>` | `Type::name(...Input) -> Result<()>` |

import / 可視性ルール:

- `@call` / `@cast` により公開された concrete 関数名は、通常の module 関数と同じ規則で `import` できる
- singleton `Type::pid` により公開された concrete 関数名も、通常の module 関数と同じ規則で `import` できる
- annotation なし `def` は内部 helper であり、`defp` 相当として `import` できない
- compiler-managed hidden surface (`Agent::pid`, `GenServer::pid`, `GenServer::spawn`, common owner helper, hidden lower 名) は `import` 対象外であり、user code から直接参照できない

Singleton GenServer は PID なし call を推奨する。explicit PID API は残す。

```surtr
// 推奨
text = CounterServer::view("count")

// 明示 API
pid = CounterServer::pid()
text = CounterServer::view(pid, "count")
```

Worker GenServer も public surface は自然な process owner API を使う。

```surtr
pid =? QueueServer::init("image")
_ =? QueueServer::push(pid, "a.png")
size =? QueueServer::size(pid)
```

compiler は generated owner helper をまず `GenServer` / `Supervisor` などの common owner module へ lower し、その後 canonical runtime builtin 名 (`__process_*`, `__supervisor_*` など) に接続する。common owner module と hidden lower 名は user code から直接使わない。

### 3.11 Supervisor / DynamicSupervisor

Supervisor は次の層で整理する。

| supervisor | 役割 |
|---|---|
| RootSupervisor | アプリ起動のルート。boot failure を集約 |
| RuntimeSupervisor | singleton 群、standard singleton、runtime / bridge singleton を管理 |
| DynamicSupervisor | 動的に増減する worker を管理。restart / cleanup を行う |

初期フェーズでは、restart policy の主対象は Worker とする。Lazy singleton と worker restart は分離する。

```text
restart = Worker lifecycle の話
Lazy singleton = init 完了保証の話
```

DynamicSupervisor は singleton process として扱い、user-facing API に `sup: PID<_>` を出さない。

```surtr
pid = DynamicSupervisor::spawn(MyWorker::init(args))
```

`defsupervisor` は policy-only declaration とし、`meta` には supervisor policy だけを置く。

- `strategy`
- `max_restarts`
- `max_seconds`
- `child_restart_default`
- `allow_adopt`
- 必要なら `shutdown_timeout`

`instance` / `init_policy` / user-defined helper `def` / public handler (`@call`, `@cast`, `@get`, `@set`) は `defsupervisor` では受理しない。
`spawn` / `adopt` / `status` / `workers` は compiler-generated wrapper であり、同名 user 定義は compile error とする。

```surtr
defsupervisor ImageWorkerSupervisor {
  meta {
    strategy: OneForOne
    max_restarts: 5
    max_seconds: 10
    child_restart_default: Transient
    allow_adopt: True
  }
}
```

init route が first-class surface value になる前段として、generated Worker wrapper は次の public façade を呼ぶ。

```surtr
DynamicSupervisor::spawn(init: (-> Result<State>)) -> Result<PID<Worker>>
```

custom supervisor surface も同じ形に揃える。

```surtr
ImageWorkerSupervisor::spawn(MyWorker::init(args))
ImageWorkerSupervisor::adopt(pid)
ImageWorkerSupervisor::status()
ImageWorkerSupervisor::workers(MyWorker::init(args), WorkerStrategy::fixed(4))
```

`status()` は `SupervisorStatus` を返し、policy 表示として `strategy`、
`max_restarts`、`max_seconds`、`allow_adopt`、`shutdown_timeout` を含める。
`shutdown_timeout` は `Option<Duration>` とし、未指定時は `Option::None`、
定義または `supervisor_init` override で指定された場合は
`Option::Some(duration)` を返す。

`adopt / handoff` は runtime が原子的に処理する。PID は維持する。

### 3.11.1 Workers surface

`Workers<$Worker>` は runtime-managed な worker 集合 handle であり、`List<PID<$Worker>>` ではない。`WorkerLease<$Worker>` は `Workers::reserve` が返す予約 handle で、裸 PID 抽出 API の代替である。

- membership は closed である
- user code は worker 集合を直接組み立てない
- `Workers` API は worker message template だけを受ける
- `reserve` は `WorkerLease<$Worker>` を返し、裸の PID 抽出 API は出さない
- `WorkerScale` / `WorkerStrategy` は pure Surtr data であり、通常の struct / enum として任意の module や helper で生成してよい
- v1 の executable scale は `WorkerScale::Fix(n)` のみである
- `WorkerStrategy::default()` は `init=1, min=1, max=1, scale=Fix(1)` を返す
- `WorkerStrategy::fixed(size)` は `init=size, min=size, max=size, scale=Fix(size)` を返す
- `Sup::workers(init, strategy)` は Singleton GenServer の `@init` で worker pool state を作る経路としてだけ使う
- `Sup::workers(..., 2)` の旧 `Int` surface は廃止する
- `Workers<$Worker>` は Singleton GenServer の state として保持する。state そのものを `Workers<$Worker>` にしてよいし、user-defined state struct の field に含めてもよい
- public surface は `Workers::submit` / `Workers::reserve` / `Workers::broadcast` / `Workers::size` に限る
- `Workers::submit` / `Workers::reserve` / `Workers::broadcast` / `Workers::size` は Singleton GenServer の `@call` / `@cast` / 同じ `defgenserver` 内 helper から使う
- `snapshot` / `idle_count` / `busy_count` / `drain` / `set_target` は public `Workers` API ではない。pool 固有の観測は VM dump / process runtime snapshot で扱い、post-init に strategy を runtime へ渡す public API は持たない
- timeout は `submit_timeout` のような別 public API ではなく、`Workers::*` 呼び出しに付く `@timeout(...)` modifier を使う

runtime は `WorkerStrategy` を worker set state に保持し、`Fix(n)` について `init == n` かつ `0 <= min <= n <= max` を検証する。条件を満たさない場合、`Sup::workers` は `Err(InvalidWorkerStrategy)` を返す。

worker exit 時、runtime は dead PID を membership から除去する。`Workers<$Worker>` handle 自体は削除せず、supervisor policy 配下で target 数まで worker を refill する。したがって user code は closed-set handle を保持し続け、reconcile loop や target 更新 API を持たない。

正規系は WorkerPool 役の Singleton GenServer に閉じる。state がそのまま `Workers<$Worker>` の場合:

```surtr
defgenserver ImagePool {
  meta {
    instance: Singleton
    init_policy: Eager
  }

  @init
  def init() -> Result<Workers<ImageWorker>> {
    ImageWorkerSupervisor::workers(ImageWorker::init(0), WorkerStrategy::fixed(2))
  }

  def assign_reserved(workers: Workers<ImageWorker>, job: ImageJob) -> Result<Unit> {
    lease =? Workers::reserve(workers)
    ImageWorker::assign(lease, job)
  }

  @cast
  def submit(workers: Workers<ImageWorker>, job: ImageJob) -> Result<CastResult<Workers<ImageWorker>>> {
    _ =? Workers::submit(workers, ImageWorker::assign(job))
    Ok(CastResult::Next(workers))
  }

  @call
  def values(workers: Workers<ImageWorker>) -> Result<CallResult<List<Result<Int>>, Workers<ImageWorker>>> {
    Ok(CallResult::Reply(Workers::broadcast(workers, ImageWorker::value()), workers))
  }

  @call
  def count(workers: Workers<ImageWorker>) -> Result<CallResult<Int, Workers<ImageWorker>>> {
    Ok(CallResult::Reply(Workers::size(workers), workers))
  }
}

_ =? ImagePool::submit(job)
values =? ImagePool::values()
count =? ImagePool::count()
```

追加 state と一緒に保持する場合は user-defined state struct の field に含める。

```surtr
defstruct ImagePoolState {
  workers: Workers<ImageWorker>,
  accepted: Int,
}

impl ImagePoolState {
  def new(workers: Workers<ImageWorker>, accepted: Int) -> Self {
    ImagePoolState { workers: workers, accepted: accepted }
  }
}

defgenserver ImagePool {
  meta {
    instance: Singleton
    init_policy: Eager
    state: ImagePoolState
  }

  @init
  def init() -> Result<ImagePoolState> {
    workers =? ImageWorkerSupervisor::workers(
      ImageWorker::init(0),
      WorkerStrategy::fixed(2),
    )
    Ok(ImagePoolState::new(workers, 0))
  }

  @cast
  def submit(state: ImagePoolState, job: ImageJob) -> Result<CastResult<ImagePoolState>> {
    _ =? Workers::submit(state.workers, ImageWorker::assign(job))
    Ok(CastResult::Next(ImagePoolState::new(state.workers, state.accepted + 1)))
  }

  @call
  def count(state: ImagePoolState) -> Result<CallResult<Int, ImagePoolState>> {
    Ok(CallResult::Reply(Workers::size(state.workers), state))
  }
}
```

generated supervisor owner helper は compiler が compiler-managed `Supervisor::*` owner module を経由して hidden `__supervisor_*` runtime lower へ接続する。user code では常に `MySupervisor::spawn(...)` / `MySupervisor::workers(...)` のような process owner API を使う。

process runtime snapshot / VM dump は worker set の観測情報を `worker_sets` として出す。

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

`member_pids` は closed membership 内の現在の PID 列、`live_count` は runtime process table 上で live と見なせる member 数である。busy / idle などの詳細状態は v1 public surface には含めない。

### 3.12 Worker lifecycle

Worker は `spawn` で生成し、`PID<Proc>` を通して扱う。

| 項目 | 仕様 |
|---|---|
| default owner | current process |
| lifecycle sink | spawn 直後から 1 つ持つ |
| singleton explicit exit | なし |
| worker exit | lifecycle sink に配送 |
| generic receive | 導入しない |

top-level の plain Worker `spawn` には current process が存在しないため、
初期実装では `DynamicSupervisor` を default lifecycle sink として登録する。
process handler 内など current process context が確立している経路では current process owner を優先する。

`Process::link` / `Process::monitor` / `Process::join` は v2 初期 surface には出さない。
link は `owner` / `lifecycle_sink` / supervisor tree / restart policy の runtime 内部関係として扱う。
monitor は generic receive を公開しない方針と相性が悪いため、必要になった場合は
typed `on_down` など用途別 API として検討する。join は Task では `Task::call` / `Task::async`
と `@timeout` に寄せ、Worker 終了待ちは後続の Worker 専用 API として検討する。

Exit reason 候補:

```rust
enum ExitReason {
    Normal,
    Exit(ErrorValue),
    RuntimeFault(RuntimeError),
    InitFailed(ErrorValue),
}
```

### 3.13 Task

初期フェーズの Task は使い捨て process として扱う。

```surtr
task = Task::async({||
  Process::sleep(10ms)
  Ok("ready")
})
result = Task::await(task) @timeout(100ms)
```

`@timeout` は直前の runtime-managed call に timeout policy を付与する。timeout した場合、結果値は `Err(TimeOutError)` になる。

初期フェーズでは、Task.Supervisor / DynamicSupervisor link は扱わない。

### 3.14 Process::sleep

`Process::sleep(duration)` は scheduler timer であり、呼び出した process のみに作用する。VM 全体や host thread を block しない。

```surtr
Process::sleep(10ms)
```

内部的には caller process を `Waiting(Timer)` に移す。

### 3.15 `@timeout`

`@timeout` は runtime-managed call の後方 modifier とする。

```surtr
result = CacheClient::get("key") @timeout(100ms)
task = Task::async({|| Ok("done") })
result = Task::await(task) @timeout(1s)
```

| timeout | 起点 | 終点 | timeout 時 |
|---|---|---|---|
| init timeout | process 起動 | `Ready(state)` | `RuntimeError::ProcessInitTimeout` |
| call timeout | call 開始 | reply | `Err(TimeOutError)` |
| task timeout | task 開始 | result | `Err(TimeOutError)` |
| sleep | sleep 開始 | timer wake | error ではない |

Ready 前に call した場合、call timeout は Ready 待ち時間を含む。

### 3.16 singleton PID API

singleton は compiler / BootPlan / Exit rule により常に存在する前提とする。explicit PID API は `Result` を返さない。compile unit で available singleton に含まれない参照は codegen 前に reject する。

```surtr
Env::pid() -> PID<Env>
```

singleton public surface は hidden lower helper とは分けて扱う。

- `Agent::pid` / `GenServer::pid` は compiler-managed lower helper であり、user code から直接 import / call しない
- `Counter::pid()` / `QueueServer::pid()` のような concrete singleton `pid()` は public surface であり、通常の process owner API と同様に query / import / call できる
- singleton public API は、PID を省略した direct sugar と explicit PID-first form の両方を許す generated surface では両方の呼び出し方を持ってよい

singleton が存在しない場合は business error ではなく、VM / supervisor / BootPlan の不整合である。

### 3.17 `supervisor_init`

`supervisor_init` は top-level 起動構成 block とし、通常式評価とは分ける。ここで定義されるのは singleton boot entry と handler override を含む `RuntimeBootPlan` の入力であり、VM は surface DSL を直接読まない。

`boot: Required` / `boot: ExplicitOnly` のような boot policy 指定は、定義側にも Boot 側にも置かない。
起動対象は、`supervisor_init` / project runner に記載された singleton entry と、runtime が自動提供する builtin standard I/O から決まる。

役割:

- 起動対象に含める singleton を明示する
- 起動対象に含める supervisor を明示し、その policy override を指定する
- init route を選ぶ
- init timeout を指定する
- standard singleton を override する
- process-local handler を override する
- standard I/O handler の差し替えを指定する
- runner-only runtime target を指定する

例:

```surtr
supervisor_init {
  singleton Logger {
    timeout: 5s
  }

  DynamicSupervisor {}

  ImageWorkerSupervisor {
    max_restarts: 10
    allow_adopt: True
  }
}
```

初期フェーズでは supervisor 親は固定で、DSL `parent` override は受理しない。

- `RuntimeSupervisor -> RootSupervisor`
- `DynamicSupervisor -> RootSupervisor`
- `custom supervisor -> RootSupervisor`
- `singleton process -> RuntimeSupervisor`

起動ルール:

| 対象 | 起動条件 |
|---|---|
| `StdIn` / `StdOut` / `StdErr` | runtime builtin standard I/O として常に自動起動 |
| Std 内 `Env` / `Logger` など | 任意。`supervisor_init` / project runner に記載された場合に起動 |
| ユーザ定義 singleton | 任意。`supervisor_init` / project runner に記載された場合に起動 |
| 記載なし、かつプロセス呼び出しなし | 起動しない |
| プロセス呼び出しあり、かつ available singleton に含まれない | compile-time singleton 利用検査で error |

`init_policy` は定義側にあるため、Boot 側は Lazy の採否を決めない。Boot 側は起動対象、timeout、handler override、supervisor policy override を指定する。

### 3.18 I/O handler dependency

I/O handler は、process init 引数や State に混ぜるのではなく、process-local readonly dependency として扱う。

process 定義側では `meta.handlers` に handler slot、capability、default target を宣言する。

```surtr
defgenserver Logger {
  meta {
    instance: Singleton
    init_policy: Eager

    handlers {
      out: OutHandler = StdOut
    }
  }

  @init
  def init() -> Result<LoggerState> {
    Ok(LoggerState {})
  }

  @cast
  def info(state: LoggerState, message: String) -> Result<LoggerState> {
    OutHandler::write(ctx.out, message)
    Ok(state)
  }
}
```

`ctx.out` は通常の変数ではなく、`meta.handlers.out` から導出される process-local readonly context である。

| 項目 | 仕様 |
|---|---|
| 参照形式 | `ctx.<slot>` |
| 裸の slot 参照 | 禁止。`out` ではなく `ctx.out` と書く |
| 書き換え | 禁止 |
| public API への返却 | 禁止 |
| State への格納 | 不要 |
| `@init` 引数への混在 | 不要 |

handler dependency は process の実行構成に属するが、process が必要とする slot と default target は定義側に書く。これにより、標準定義とユーザ定義 process の温度感を揃える。

### 3.19 handler target と override

`supervisor_init` は、process 定義の `meta.handlers` にある default target を override できる。override は process init 引数ではなく BootPlan 側の実行構成として扱う。

```surtr
supervisor_init {
  singleton Logger {
    handlers {
      out: FileOutHandler(path: "./logs/app.log")
    }
  }
}
```

handler target の指定形式は次とする。

```text
HandlerName
HandlerName(named_args...)
```

`HandlerName` は `HandlerName()` と同義である。Boot 設定では named args を基本形とし、位置引数は初期フェーズでは扱わない。

例:

```surtr
supervisor_init {
  singleton Logger {
    handlers {
      out: StdOut
    }
  }
}
```

```surtr
supervisor_init {
  singleton Logger {
    handlers {
      out: NullOutHandler
    }
  }
}
```

```surtr
supervisor_init {
  singleton Logger {
    handlers {
      out: FileOutHandler(path: "./logs/app.log")
    }
  }
}
```

handler override の検査:

| 検査 | 内容 |
|---|---|
| slot 存在 | override 対象 slot が process `meta.handlers` に存在すること |
| capability | override target が slot の要求 capability を満たすこと |
| args | handler target の init route に named args が一致すること |
| target visibility | handler target が Boot 設定から参照可能であること |

#### `FileOutHandler`

`FileOutHandler(path)` は append-only の `OutHandler` とする。

| 項目 | 仕様 |
|---|---|
| mode | append 固定 |
| file missing | create |
| file exists | append |
| truncate | しない |
| open timing | handler init 時 |
| lifecycle | handler lifecycle 中は open したまま保持 |
| shutdown | flush / close |
| open failure | `RuntimeError::HandlerInitFailed` |
| write failure | `OutHandler::write` の `Err` |

同一 VM 内で同じ canonical path を指す `FileOutHandler(path)` が複数出現した場合、runtime は同一 file sink に正規化する。

```text
FileOutHandler identity:
  kind = FileOutHandler
  canonical_path
  mode = Append
```

同一 file sink に到着した write message は、その file sink の mailbox order で書き込む。異なる producer 間の wall-clock 順序までは保証しない。

`/dev/null` を直接 path として扱うのではなく、OS 非依存の handler として `NullOutHandler` を使う。

### 3.20 標準 I/O handler とテスト利用目標

`StdIn` / `StdOut` / `StdErr` は runtime builtin singleton handler とする。Surtr コードで実装する対象にはしない。

| builtin | capability | 役割 |
|---|---|---|
| `StdIn` | `InHandler` | 標準入力 |
| `StdOut` | `OutHandler` | 標準出力 |
| `StdErr` | `OutHandler` | 標準エラー |

標準 I/O への読み書きは、VM 内部リストへの直接操作ではなく、builtin handler への message call として扱う。

```surtr
OutHandler::write(ctx.out, "message")
```

`OutHandler::write` は同期 call とし、戻り値は `Result<()>` とする。`StyledDoc` は呼び出し側で `to_ansi` により escape literal 付き `String` に変換済みとし、handler 側では `String` のみを扱う。

```surtr
OutHandler::write(pid: PID<OutHandler>, text: String) -> Result<()>
```

標準 I/O 差し替えは、Rust からのテストと Pure Surtr test DSL の双方で同じ意味にする。

契約:

- test mode では標準 stdout / stderr / stdin を buffer handler に差し替えられる
- `supervisor_init` では buffer mode を選ぶだけにし、テストデータを init 引数として埋め込まない
- `it` ごとに stdout / stderr / stdin buffer を分離できる
- `File` module の host filesystem access はこの handler 差し替え機構には乗せず、process runtime とは独立した File v1 surface として current working directory 基準で扱う
- Pure Surtr code から `capture_stdout()`、`assert_stdout_eq(...)`、`push_stdin(...)` のような補助 API を使える
- Rust 側テストからも同じ buffer backend を観測できる

内部実装方式は固定しないが、公開観測上は標準 I/O handler の差し替えとして振る舞うことを必須とする。

想定される利用イメージ:

```surtr
it("runs only the selected blocks") {
  capture_stdout()

  print("run-if")
  print("if-then")

  assert_stdout_eq(["run-if", "if-then"])
}
```

buffer を使う場合でも、`supervisor_init` に `lines: [...]` のようなテストデータを埋め込む方式は採用しない。1 テスト = 1 スクリプトになることを避けるためである。

### 3.21 singleton 利用検査

compile unit 単位で次を検査する。対象は singleton direct call と singleton PID lookup の両方である。

```text
required_singletons ⊆ available_singletons
```

| 集めるもの | 内容 |
|---|---|
| `required_singletons` | singleton surface call を参照している file から収集 |
| builtin standard I/O set | `StdIn` / `StdOut` / `StdErr`。runtime が常に提供 |
| DSL 明示 singleton set | `supervisor_init` / project runner から収集 |

`available_singletons` は builtin standard I/O set と DSL 明示 singleton set の和集合として扱う。制御フロー到達性までは見ない。

---

## 4. VM 最終型の概念説明と使われ方

### 4.1 VM が読むもの

VM は surface syntax を直接読まない。Compiler が process 定義と Boot 設定を解析し、immutable な spec と boot plan を生成する。

```text
source code
  -> parser / AST
  -> semantic check
  -> RuntimeProcessSpec table
  -> RuntimeBootPlan
  -> VM
```

実行中に新しい spec を流さない。動的生成は、既存 spec に基づく process instance 生成に限定する。

### 4.2 `RuntimeProcessSpec`

```rust
struct RuntimeProcessSpec {
    process_id: RuntimeProcessId,
    type_name: TypeName,
    kind: RuntimeProcessKind,
    instance: RuntimeProcessInstance,
    state: RuntimeStateSpec,
    init: RuntimeInitSpec,
    handlers: Vec<RuntimeHandlerSpec>,
    dependencies: RuntimeProcessDependencies,
    lifecycle: RuntimeLifecycleSpec,
    supervision: RuntimeSupervisionSpec,
}
```

意味:

| field | 用途 |
|---|---|
| `process_id` | VM 内で process spec を一意参照する ID |
| `type_name` | source 上の process 型名 |
| `kind` | Agent / GenServer / Supervisor / Task など |
| `instance` | Singleton / Worker |
| `state` | live state 型と ownership 情報 |
| `init` | init callable、policy、result shape |
| `handlers` | message dispatch 用 handler table |
| `dependencies` | process-local handler dependency / context slot 情報 |
| `lifecycle` | worker owner / exit sink / restart 対象情報 |
| `supervision` | supervisor tree / restart policy 情報 |

### 4.3 `RuntimeProcessKind`

```rust
enum RuntimeProcessKind {
    Agent,
    GenServer,
    Supervisor,
    RuntimeSupervisor,
    DynamicSupervisor,
    Task,
}
```

### 4.4 `RuntimeProcessInstance`

```rust
enum RuntimeProcessInstance {
    Singleton,
    Worker,
}
```

### 4.5 `RuntimeInitSpec`

```rust
struct RuntimeInitSpec {
    callable: CallableRef,
    policy: InitPolicy,
    result_shape: InitResultShape,
    state_type: TypeRef,
    init_route: Option<InitRouteRef>,
}
```

```rust
enum InitPolicy {
    Eager,
    Lazy,
}
```

```rust
enum InitResultShape {
    EagerState {
        result_type: TypeRef, // Result<State>
    },
    LazyProcessInit {
        result_type: TypeRef, // Result<ProcessInit<State>>
    },
}
```

VM は `policy` と `result_shape` に従って init result を decode する。

```rust
match init_spec.policy {
    InitPolicy::Eager => {
        // Ok(state) -> Ready(state)
        // Err(error) -> RuntimeError::ProcessInitFailed
    }
    InitPolicy::Lazy => {
        // Ok(ProcessInit::Ready(state)) -> Ready(state)
        // Ok(ProcessInit::Pending) -> retry by runtime default policy
        // Ok(ProcessInit::PendingAfter(d)) -> retry after d
        // Err(error) -> RuntimeError::ProcessInitFailed
    }
}
```

### 4.6 `RuntimeHandlerSpec`

```rust
struct RuntimeHandlerSpec {
    handler_id: RuntimeHandlerId,
    name: Symbol,
    kind: RuntimeHandlerKind,
    callable: CallableRef,
    input: Vec<TypeRef>,
    reply: Option<TypeRef>,
    state_in: Option<TypeRef>,
    state_out: Option<TypeRef>,
}
```

```rust
enum RuntimeHandlerKind {
    Init,
    Get,
    Set,
    Call,
    Cast,
    Spawn,
    System,
}
```

使われ方:

| handler kind | VM 側の扱い |
|---|---|
| `Init` | process lifecycle 開始時に呼ぶ |
| `Get` | Agent read path |
| `Set` | Agent write path。Err なら state 更新なし |
| `Call` | reply を返す GenServer handler |
| `Cast` | state 更新のみ。外部 `Result<()>` |
| `Spawn` | Worker / Task 起動 route |
| `System` | RuntimeSupervisor / DynamicSupervisor 内部 handler |


### 4.7 `RuntimeProcessDependencies`

process-local handler dependency は State ではなく、process context として保持する。

```rust
struct RuntimeProcessDependencies {
    handlers: Vec<RuntimeHandlerDependency>,
}
```

```rust
struct RuntimeHandlerDependency {
    slot: Symbol,
    capability: HandlerCapability,
    default_target: RuntimeHandlerTarget,
}
```

```rust
enum RuntimeHandlerTarget {
    BuiltinStdIn,
    BuiltinStdOut,
    BuiltinStdErr,
    NullOut,
    FileOut {
        canonical_path: CanonicalPath,
        mode: FileOutMode,
    },
    Process(RuntimeProcessId),
}
```

```rust
enum FileOutMode {
    Append,
}
```

`ctx.<slot>` は、この dependency slot から runtime が解決した PID として扱う。

```rust
struct ProcessContext {
    handlers: HashMap<Symbol, Pid>,
}
```

`ctx.<slot>` は readonly であり、ユーザコードから変更できない。public API に返すこともできない。

### 4.8 `RuntimeBootPlan`

`RuntimeBootPlan` は、VM 起動時に実際に確保・起動する process / handler target を表す。
`boot: Required` のような policy enum は持たない。

```rust
struct RuntimeBootPlan {
    root: RootSupervisorPlan,
    singletons: Vec<SingletonBootEntry>,
    standard_overrides: Vec<StandardOverrideEntry>,
    handler_overrides: Vec<RuntimeHandlerOverride>,
    runtime_limits: RuntimeLimitConfig,
}
```

```rust
struct SingletonBootEntry {
    process_id: RuntimeProcessId,
    init_route: Option<InitRouteRef>,
    init_timeout: Duration,
    source: BootEntrySource,
}
```

```rust
enum BootEntrySource {
    ExplicitConfig,
    BuiltinStandardIo,
}
```

```rust
struct RuntimeHandlerOverride {
    target_process: RuntimeProcessId,
    slot: Symbol,
    handler_target: RuntimeHandlerTarget,
}
```

`BuiltinStandardIo` は `StdIn` / `StdOut` / `StdErr` のように、Pure Surtr コードで表現せず runtime が自動起動する builtin process に使う。
Std 内の `Env` / `Logger` やユーザ定義 singleton は自動起動対象ではなく、`ExplicitConfig` として Boot 設定に現れた場合に起動する。

### 4.9 `RuntimeLimitConfig`

```rust
struct RuntimeLimitConfig {
    default_init_timeout: Duration, // 5s
    min_init_timeout: Duration,     // 1ms
    max_init_timeout: Duration,     // 60s
    pending_initial_retry: Duration, // 10ms
    pending_max_retry: Duration,     // 1s
    min_scheduler_tick: Duration,    // 1ms
}
```

### 4.10 `ProcessInstance`

```rust
struct ProcessInstance {
    pid: Pid,
    spec_id: RuntimeProcessId,
    status: ProcessStatus,
    state: Option<Value>,
    context: ProcessContext,
    init_waiters: VecDeque<PendingCall>,
    mailbox: VecDeque<RuntimeMessage>,
    execution_context: ExecutionContext,
    owner: Option<Pid>,
    lifecycle_sink: Option<LifecycleSink>,
}
```

`state: Option<Value>` は VM 内部表現であり、Surtr surface に Option 型を導入する意味ではない。

### 4.11 `ProcessStatus`

```rust
enum ProcessStatus {
    Allocated,
    Initializing {
        started_at: RuntimeInstant,
        deadline: RuntimeInstant,
        retry_policy: RetryPolicyState,
    },
    Ready,
    Waiting(WaitReason),
    Exited(ExitReason),
    Failed(RuntimeError),
}
```

### 4.12 `WaitReason`

```rust
enum WaitReason {
    Timer {
        wake_at: RuntimeInstant,
    },
    InitReady {
        process_id: RuntimeProcessId,
        timeout: Option<RuntimeInstant>,
    },
    Reply {
        correlation_id: CorrelationId,
        timeout: Option<RuntimeInstant>,
    },
}
```

### 4.13 `StepOutcome`

```rust
enum StepOutcome {
    Continue,
    Return(Value),
    Pending(WaitReason),
    Halt,
    RuntimeError(RuntimeError),
}
```

### 4.14 Lazy Ready 前 call の扱い

Lazy singleton が Ready になる前に到着した message call は、通常 mailbox ではなく `init_waiters` に FIFO で保存する。

Ready 到達時の処理:

```text
1. Lazy init が Ready(state) を返す
2. runtime が state slot に state をセットする
3. process status を Ready に変更する
4. init_waiters を到着順に通常 mailbox の front 側へ移す
5. process を runnable queue に 1 回 enqueue する
6. scheduler が通常 message dispatch として順に処理する
```

順序例:

```text
t1: call A arrives while Initializing
t2: call B arrives while Initializing
t3: Ready
t4: call C arrives after Ready

処理順: A -> B -> C
```

call timeout は call 開始から reply までを対象とする。Ready 待ち時間も call timeout に含む。

### 4.15 scheduler queue

VM は少なくとも次の queue / table を持つ。

| 構造 | 用途 |
|---|---|
| runnable queue | 実行可能 process を保持 |
| deadline queue | timer / timeout deadline を保持 |
| waiting table | reply / init ready / task completion 待ちを保持 |
| singleton slot | singleton process の current PID を保持 |
| process table | PID から process instance を引く |
| spec table | RuntimeProcessId から immutable spec を引く |
| handler target registry | handler target identity から shared sink / builtin handler を引く |

---

## 5. diagnostics 例

### 5.1 process 定義

| 発生箇所 | 条件 | error id | 簡易メッセージ | help |
|---|---|---|---|---|
| `meta` | `@agent(...)` を使っている | `process-meta-deprecated` | `@agent(...)` metadata is no longer supported. | Use `meta { instance, init_policy, state }` inside the process definition. |
| `meta` | `boot` を定義側に置いた | `process-meta-boot-not-allowed` | boot settings must be declared in `supervisor_init`. | Move boot policy and timeout to Boot configuration. |
| `meta` | `registry` を定義側に置いた | `process-meta-registry-not-allowed` | registry settings are runtime / boot concerns. | Remove registry from process meta. |
| `meta` | `init_policy: Lazy` を Worker に付けた | `process-lazy-not-allowed` | Lazy init is only allowed for Singleton Agent / Singleton GenServer. | Use `Eager`, or define an async call API. |
| `meta` | `init_policy: Lazy` を Supervisor に付けた | `process-lazy-supervisor` | Supervisor does not support Lazy init. | Remove `init_policy: Lazy`. |
| `defagent` | `@init` がない | `agent-init-missing` | Agent requires exactly one `@init` handler. | Add one `@init` handler. |
| `defagent` | `@get` がない | `agent-get-missing` | Agent requires exactly one `@get` handler. | Add one `@get` handler. |
| `defagent` | `@set` が複数ある | `agent-set-duplicate` | Agent allows at most one `@set` handler. | Use GenServer for multiple write protocols. |
| `defagent` | `@get` が複数ある | `agent-get-duplicate` | Agent allows exactly one `@get` handler. | Use GenServer for multiple query protocols. |
| `defgenserver` | `defp` を使った | `genserver-defp-not-allowed` | GenServer body uses `def`; visibility is controlled by annotations. | Replace `defp` with annotation-less `def`. |
| `defgenserver` | `@call` の戻り値が `Result<Reply>` | `genserver-call-return-mismatch` | `@call` must return `Result<CallResult<Reply, State>>`. | Return `CallResult::Reply(...)`, `ReplyLater(...)`, or `Stop(...)`. |
| `defgenserver` | `@cast` の戻り値が `Result<()>` | `genserver-cast-return-mismatch` | `@cast` must return `Result<CastResult<State>>`. | Return `CastResult::Next(...)` or `Stop(...)`. |
| `meta.handlers` | default target が slot capability を満たさない | `handler-default-capability-mismatch` | handler default does not satisfy required capability. | Use a handler that implements the required capability. |
| process body | handler slot を裸で参照した | `process-context-bare-access` | handler dependency must be accessed through `ctx.<slot>`. | Use `ctx.out` instead of `out`. |
| process body | `ctx.<slot>` に代入した | `process-context-readonly` | process context handler is readonly. | Override it from `supervisor_init`. |
| public API | `ctx.<slot>` / handler PID を返した | `process-context-leak` | handler dependency cannot be returned from public API. | Keep handler access inside the process. |

### 5.2 init / ProcessInit

| 発生箇所 | 条件 | error id | 簡易メッセージ | help |
|---|---|---|---|---|
| `@init` | Eager なのに `Result<ProcessInit<State>>` | `process-init-return-mismatch` | Eager init must return `Result<State>`. | Change `init_policy` to `Lazy`, or return `Result<State>`. |
| `@init` | Lazy なのに `Result<State>` | `process-init-return-mismatch` | Lazy init must return `Result<ProcessInit<State>>`. | Wrap the initialized state with `ProcessInit::Ready(state)`. |
| `@init` | `ProcessInit::Ready<T>` の `T` が state と違う | `process-init-ready-type-mismatch` | `ProcessInit::Ready` value must match the process state type. | Return `ProcessInit::Ready` with the declared state type. |
| `@init` | `PendingAfter` に `Duration` 以外を渡した | `process-init-pending-after-type` | `PendingAfter` requires `Duration`. | Pass a `Duration` value, for example `100ms`. |
| 通常関数 | `ProcessInit<T>` を戻り値に使った | `process-init-type-position` | `ProcessInit<T>` is only allowed as Lazy `@init` return type. | Use a domain enum instead. |
| struct field | `ProcessInit<T>` を field に使った | `process-init-type-position` | `ProcessInit<T>` cannot appear in data types. | Store a domain-specific status enum instead. |
| `@call` / `@get` | `ProcessInit<T>` を返した | `process-init-type-position` | `ProcessInit<T>` must not leak into process public API. | Return a View / Reply type. |

### 5.3 process state contracts

| 発生箇所 | 条件 | error id | 簡易メッセージ | help |
|---|---|---|---|---|
| `meta` | `state` がない | `process-state-missing` | process metadata requires `state`. | Add `state: StateTy` to the process `meta` block. |
| `@init` | `Result` ok 型が `meta.state` と違う | `process-state-init-mismatch` | `@init` result type must match the declared process state type. | Return the type declared in `meta.state`. |
| handler | 第1引数 state が `meta.state` と違う | `process-state-param-mismatch` | handler state parameter must match the declared process state type. | Change the first parameter to the type declared in `meta.state`. |
| Agent `@set` | `Result` ok 型が `meta.state` と違う | `process-state-return-mismatch` | `@set` result type must match the declared process state type. | Return the type declared in `meta.state`. |
| GenServer `@call` | `CallResult<Reply, State>` の `State` が `meta.state` と違う | `process-state-call-result-mismatch` | `@call` state result must match the declared process state type. | Use the type declared in `meta.state` for `CallResult`. |
| GenServer `@cast` | `CastResult<State>` の `State` が `meta.state` と違う | `process-state-cast-result-mismatch` | `@cast` state result must match the declared process state type. | Use the type declared in `meta.state` for `CastResult`. |

### 5.4 Boot 定義

| 発生箇所 | 条件 | error id | 簡易メッセージ | help |
|---|---|---|---|---|
| `supervisor_init` | timeout 未指定 | なし | runtime default `5s` を使う | 必要なら `timeout` を指定する |
| `supervisor_init` | timeout `< 1ms` | `boot-timeout-too-small` | init timeout must be at least `1ms`. | Use `1ms` or larger. |
| `supervisor_init` | timeout `> 60s` | `boot-timeout-too-large` | init timeout must not exceed `60s`. | Use a shorter timeout, or move long work to Task. |
| `supervisor_init` | unknown singleton を指定 | `boot-unknown-singleton` | singleton process is not defined or not visible. | Check module load path / definition source. |
| `supervisor_init` | same singleton を二重指定 | `boot-duplicate-singleton` | singleton boot entry is duplicated. | Keep one entry. |
| `supervisor_init` | Worker を singleton boot に指定 | `boot-non-singleton-entry` | only Singleton process can appear in singleton boot entry. | Use Worker spawn / DynamicSupervisor. |
| `supervisor_init` | `init_policy` を書いた | `boot-init-policy-not-allowed` | init policy belongs to process definition. | Move `init_policy` to `meta`. |
| `supervisor_init` | `boot: Required` / `boot: ExplicitOnly` を書いた | `boot-policy-not-allowed` | boot policy is no longer used. | Listing a singleton entry is enough to include it in the boot plan. |
| `supervisor_init` | 存在しない handler slot を override | `handler-override-unknown-slot` | handler slot is not declared by the target process. | Add the slot to `meta.handlers` or remove the override. |
| `supervisor_init` | override target が capability を満たさない | `handler-override-capability-mismatch` | handler target does not satisfy required capability. | Use a compatible handler target. |
| `supervisor_init` | handler args が init route と一致しない | `handler-init-args-mismatch` | handler init arguments do not match the target init route. | Check named arguments and types. |
| `supervisor_init` | `FileOutHandler` に path がない | `handler-init-args-missing` | `FileOutHandler` requires `path`. | Use `FileOutHandler(path: "./logs/app.log")`. |

### 5.5 呼び出し側

| 発生箇所 | 条件 | error id | 簡易メッセージ | help |
|---|---|---|---|---|
| singleton call | boot plan にない singleton を参照 | `singleton-not-available` | required singleton is not available in this compile unit. | Add it to `supervisor_init` or standard default boot set. |
| `@timeout` | runtime-managed call 以外に付けた | `timeout-invalid-target` | `@timeout` can only be used on runtime-managed calls. | Attach it to process call / Task call. |
| singleton PID | `Env::pid()` を `Result` として扱った | `singleton-pid-not-result` | singleton `pid()` returns `PID<T>`, not `Result<PID<T>>`. | Remove `Ok/Err` handling. |
| worker call | PID が必要な Worker call で PID を省略 | `worker-pid-required` | Worker process call requires `PID<Proc>`. | Pass the worker PID as first argument. |
| singleton call | singleton direct call で PID を余分に渡した | `singleton-direct-call-extra-pid` | singleton direct call does not require PID. | Use either direct call or explicit PID API form. |

### 5.6 VM / runtime diagnostics

| 発生箇所 | 条件 | error id | 簡易メッセージ | help |
|---|---|---|---|---|
| runtime init | Lazy init deadline 超過 | `runtime-process-init-timeout` | process did not reach `Ready` before init timeout. | Increase Boot timeout or reduce init wait. |
| runtime init | Lazy/Eager init が `Err` | `runtime-process-init-failed` | process init failed. | Check init dependencies and process definition. |
| runtime dispatch | handler table に存在しない handler | `runtime-handler-not-found` | runtime process handler was not found. | This indicates compiler / VM spec mismatch. |
| runtime dispatch | singleton slot が空 | `runtime-singleton-slot-missing` | singleton slot is missing for a required process. | This indicates BootPlan / VM state mismatch. |
| scheduler | Pending が scheduler に登録できない | `runtime-pending-registration-failed` | process pending state could not be registered. | This indicates runtime scheduler inconsistency. |
| scheduler | caller timeout | `runtime-call-timeout` | process call timed out. | Returned as `Err(TimeOutError)` to user code. |
| task | task timeout | `runtime-task-timeout` | task timed out. | Returned as `Err(TimeOutError)` to user code. |
| handler init | `FileOutHandler` の open に失敗 | `runtime-handler-init-failed` | handler init failed. | Check file path, permissions, or host resources. |
| handler write | `OutHandler::write` が失敗 | `runtime-handler-write-failed` | handler write failed. | Returned as `Err` from `OutHandler::write`. |

---

## 6. 補足: 後続課題

| 項目 | 扱い |
|---|---|
| boundary layer | process 基盤安定後に domain/runtime/boot error の変換層として設計 |
| Worker async init | 非同期 call API と合わせて検討 |
| Task.Supervisor | Task を使い捨て process として安定させた後に検討 |
| DynamicSupervisor restart details | 初期は OneForOne 最小。`max_restarts`, `max_seconds` は後続 |
| REPL / tooling 表示 | `:info` に process spec / singleton slot / supervisor tree を表示する方向で後続 |
| BootPolicy enum | `Required / ExplicitOnly / StandardDefault / OnReference` は使わない。起動対象は Boot 設定に現れた entry と builtin standard I/O から決める |
| Pure Surtr test DSL の I/O capture | `capture_stdout` / `assert_stdout_eq` / `push_stdin` などは目標として保持し、内部実装は別途設計 |
