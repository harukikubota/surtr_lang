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

- process の定義、生成 API、起動構成、実行時の責務を整理する
- process 定義、Boot 設定、呼び出し側コードの最終形を整理する
- VM が最終的に受け取る型・概念を説明する
- I/O handler の定義、差し替え、標準 I/O の扱いを整理する
- diagnostics の発生パターンと簡易メッセージを表に落とす

Agent / GenServer / Supervisor / Task の定義と Singleton / Worker の生成・取得方針を、`RuntimeProcessSpec` と `RuntimeBootPlan` に正規化する。

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
| Worker standby init | 後回し。非同期 call API で吸収予定 |

---

## 2. 定義から runtime までの責務

### 2.1 全体方針

process 定義は `meta { ... }` と handler から構成する。Agent は `@get` / `@set`、GenServer は `@call` / `@cast`、Supervisor は policy-only declaration として扱う。個体の生成・取得方針は `Singleton / Worker`、初期化方針は `Eager / Standby` で表す。

compiler は定義から型付き公開 API と内部 wrapper を生成し、common owner module、canonical hidden builtin を経由して runtime へ接続する。呼出し側は公開 API を通常の関数として使い、内部 wrapper や state 操作 builtin を直接呼ばない。公開 API と実行境界は第3.10節、singleton PID の省略・明示形式は第3.16節に定める。

定義側の metadata と、起動対象・init route・timeout・handler / supervisor override を持つ起動構成は分ける。VM は surface DSL ではなく、正規化済みの `RuntimeProcessSpec` と `RuntimeBootPlan` を受け取る。起動構成は第3.17節、VM への正規化は第4節に定める。

停止要求と停止完了、開始済み実行の追跡、PID と本体の回収は第3.10.1〜3.10.2節、第3.12節、第4.10〜4.15節に定める。これらの実装完了を、Ready 前 FIFO、fairness、一般の supervisor restart / shutdown の実装完了とは扱わない。

### 2.2 フェーズごとの責務

| フェーズ | 責務 |
|---|---|
| parser / AST | `meta {}`、handler、`supervisor_init` の宣言構造と生成 wrapper を扱う |
| resolve / typecheck | 名前の canonical identity、handler の引数・返答型、process state、Boot と handler capability の契約を検査する |
| IR / runtime metadata | `RuntimeProcessSpec`、handler / init spec、`RuntimeBootPlan` へ正規化する |
| codegen | 正規化済み spec と関数 ID、builtin・継続呼出しを VM へ渡す |
| VM / scheduler | 初期化、受付、実行、future / timer / I/O の待機、停止完了と回収を管理する |
| standard library | 公開の `CallResult` / `CastResult`、PID、Workers、Task、Error と利用例を定義する |
| diagnostics / observability | 宣言・呼出し・内部契約違反を区別し、呼出し位置と runtime の状態・寿命を表示する |

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
| `init_policy` | `Eager` または `Standby`。明記必須 |
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

`init_policy` は process 定義側の性質であり、`defagent` / `defgenserver` の `meta` に必ず明記する。省略は parse error とし、`Eager` へ補完しない。`defsupervisor` はこの項目を受理しない。

| policy | `@init` 戻り値 | 意味 |
|---|---|---|
| `Eager` | `Result<State>` | 1 回の init 実行で state を確定する |
| `Standby` | `Result<StandbyInit<State>>` | Ready まで scheduler 管理で init を再実行する |

`Standby` は Rust の lazy loading のような初回参照時 materialize ではない。VM boot 時に初期化の管理を開始し、`StandbyInit::Ready(state)` が返るまで state value は未確定である。singleton の PID と slot は Ready 後に割り当てる。初期化中は init flight と Ready 待ちの要求を管理する（第4.11節、第4.14〜4.15節）。

### 3.4 `StandbyInit<T>`

`Standby` の `@init` だけが runtime protocol として `StandbyInit<T>` を返せる。

```surtr
defenum StandbyInit<T> {
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

`StandbyInit<T>` は Standby `@init` の戻り値以外に出現してはならない。

### 3.5 Standby retry policy

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

Standby `@init` が `Err(error)` を返した場合は、ユーザによる回復対象にしない。`RuntimeError::ProcessInitFailed` として扱う。これは VM 実装バグではなく、process init failure を表す runtime error である。

### 3.7 Standby 許可範囲

`init_policy: Standby` を許可する範囲は次に限定する。

| process | Standby |
|---|---|
| Singleton Agent | 許可 |
| Singleton GenServer | 許可 |
| Worker Agent | 禁止 |
| Worker GenServer | 禁止 |
| Supervisor | 禁止 |
| RuntimeSupervisor | 禁止 |
| DynamicSupervisor | 禁止 |
| Task | 禁止 |

Supervisor は state を持たないため Standby init の概念を持たない。Worker の Standby は将来課題とし、当面は同期 API 実装を優先する。

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

process runtime の lower surface は [`../../lib/process.srt`](../../lib/process.srt) に置く。

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

Standby Agent:

```surtr
defstruct CacheState {
  client: Client,
}

defagent CacheClient {
  meta {
    instance: Singleton
    init_policy: Standby
    state: CacheState
  }

  @init
  def init() -> Result<StandbyInit<CacheState>> {
    if(CacheService::ready?()) {
      client = CacheService::connect()
      Ok(StandbyInit::Ready(CacheState { client: client }))
    } else {
      Ok(StandbyInit::PendingAfter(100ms))
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

  defp format(label: String, value: Int) -> String {
    label ++ "=" ++ to_string(value)
  }
}
```

Agent / GenServer 内では `def` と `defp` を使う。`def` には対応する handler annotation が必須である。`defp` は同じプロセス定義内からのみ参照でき、handler annotation は付けられない。

外部から呼べる範囲は、コンパイラが生成する公開 API に対応する。helper は明示的に `defp` で宣言する。handler annotation のない `def` は parse error とし、`defp` への変更を案内する。`@doc` だけを付けた `def` も同じ条件で拒否する。通常の `def` を内部 helper として受理したり、private に書き換えたりする経路は設けない。

| 関数 | 外部公開 |
|---|---|
| `@init` | 本体は非公開。stateful Worker の起動 API は公開 |
| `@call` | はい |
| `@cast` | はい |
| `@get` / `@set` | はい |
| handler annotation なし `def` | 宣言を拒否。`defp` への変更を案内 |
| `defp` | いいえ。同じプロセス定義内の helper |

Handler 契約:

| handler | 内部 signature | 外部 surface |
|---|---|---|
| singleton `pid` | hidden lower helper | `Type::pid() -> PID<Type>` |
| `@init` Eager | `(...) -> Result<State>` | なし |
| `@init` Standby | `(...) -> Result<StandbyInit<State>>` | なし |
| `@call` | `(State, Input...) -> Result<CallResult<Reply, State>>` | `Type::name(...Input) -> Result<Reply>` |
| `@cast` | `(State, Input...) -> Result<CastResult<State>>` | `Type::name(...Input) -> Result<()>` |

import / 可視性ルール:

- `@call` / `@cast` により公開された concrete 関数名は、通常の module 関数と同じ規則で `import` できる
- singleton `Type::pid` により公開された concrete 関数名も、通常の module 関数と同じ規則で `import` できる
- `defp` は外部 API を持たず、外部から直接呼び出し・関数参照・`import` できない。同じプロセス定義内では通常の関数として参照できる
- helper が `ctx` や `Process::self()` を使うかどうかに応じて、外部公開範囲を変えない。各 API の既存の使用条件は維持する
- 利用者のテストも生成された公開 API を通す。helper の計算を直接テストする場合は、通常モジュールの公開関数へ切り出す
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

### 3.10.1 メッセージの開始と終了

GenServer の call / cast と Agent の get / set は、生成 wrapper 全体を同じ compiler-managed 実行境界で囲む。state 読取の直前に、PID の種別・型・identity の検証、受付確認、開始登録を一つの中断しない遷移で行う。PID 取得、capture 作成、Workers の選択、lease 取得だけでは開始済みにしない。direct / capture / 高階関数 / Workers / lease の各経路でこの境界を共有する。
canonical hidden builtin `__process_execute` は、この遷移で state snapshot の取得と実行 record の登録まで行う。生成 wrapper の body closure は、登録後にその snapshot を一度だけ消費して handler を実行する。snapshot を後続の別実行の state へ読み替えない。

実行段階は `Handling → Postprocessing → Callback` とする。`Handling` では `__process_state` が登録時の state snapshot を一度だけ読み取る。handler が正常に戻った後、生成 wrapper の canonical hidden builtin `__process_postprocess` が `Postprocessing` へ移す。state 保存、Stop、ReplyLater への移譲はこの後処理段階でだけ許す。`__process_state` を実行境界外、別個体、後処理中、callback 内、二度目の読取から呼ぶ場合は RuntimeError とする。境界外から生存本体の state を読む旧成功経路は残さない。

runtime は実行ごとに、実行 ID、対象個体の identity、親実行 ID、段階、結果の配送先、呼出し元の source origin を保持する。PID や reply future ID を実行 ID の代用にしない。`ExecutionContext` と継続は現在の実行 ID を保持する。通常の helper は同じ実行を続けるが、生成 API の呼出しは同じ PID への再入でも新しい実行として受付を確認する。

wrapper の state 取得、handler、state 保存、返答までを一実行とする。正常復帰、言語の `Err`、SafeBind の早期復帰、RuntimeError、取消後の cleanup は共通の終了経路を通り、一度だけ終了する。tail call や待機からの再開でも境界を失わない。

state 保存は、対象個体、現在の未完了実行、wrapper の `Postprocessing` 段階が一致する場合に一度だけ許す。別個体、完了済み実行、保存済み実行、実行境界外、および `Handling` / `Callback` 段階からの store は RuntimeError とする。snapshot を読み終えたことだけでは保存権限を与えない。再入の保存は wrapper の完了順を維持し、内側の保存後に外側が保存すれば外側の state が最終値になる。lock、state revision、暗黙 merge は追加しない。

`CallResult::ReplyLater(next_state, callback)` は `Postprocessing` で state を保存してから、同じ実行 record を `Callback` 段階へ移す。callback がまだ走っていなくても開始済みであり、sleep / future / I/O 待機と cleanup が終わるまで record を残す。callback に state 保存・Stop・ReplyLater 再移譲の権限は引き継がない。正常値や言語の `Err` は未確定の配送先にだけ返し、timeout 済みなら値を解放して実行を終了する。RuntimeError は遅延 reply の `Err` に変換せず、runtime 異常として報告する。handler / callback の通常の `Err` だけを新たな process 異常終了条件にしない。

### 3.10.2 Stop、受付拒否、停止完了

`Stop` は新規受付を閉じる停止要求であり、その応答は停止完了を保証しない。公開の Stop は Worker GenServer に限り、init と singleton では拒否する。外部停止 API は追加しない。`StopReply::Error` / `StopReason::Error` も終了理由であり、開始済み処理の強制取消を意味しない。

受付状態の `Accepting / Stopping` は `Runnable / Waiting(...)` 等の scheduler 状態と分ける。runtime が最初の Stop を受理した時点で `Stopping` と終了理由を記録する。後続の Stop は最初の終了理由を変更せず、各実行の結果をその実行の配送先へ返す。受付と Stop の競合は runtime の受理・開始順で決め、送信者間の wall-clock 順序は保証しない。

| 停止要求時の処理 | 扱い |
|---|---|
| 新規要求 | `Err(ProcessStopped(pid, process))`。state 読取と handler を開始しない |
| 受付済み・未開始 | 開始せず、元の配送先へ `ProcessStopped` を返す。timeout 済みの結果は変更しない |
| 開始済み wrapper / callback | 待機から再開し、保存・返答・cleanup まで続ける。保存しても受付を再開しない |
| `Stopping` かつ未完了実行が 0 | 停止完了。本体と不要な管理参照を削除する |

停止要求、未開始要求の終了予約、Workers の新規選択からの除外は一つの遷移で行う。未開始要求のエラー構築が継続を必要とする場合も、PID・呼出し位置・配送先を独立に保持し、process state は保持しない。通常 Stop で実行 record を一括削除して停止完了にしてはならない。
現在の Eldr mailbox は未開始メッセージの queue として使われていない。非空の mailbox は内部契約違反として RuntimeError にし、clear で無応答のまま破棄しない。未開始 queue を新設した場合に上記の拒否・配送契約が必要となることと、queue が実装済みであることを区別する。

自身への再入 call が Stop を返した後も、すでに開始した外側の Reply / Next / ReplyLater は完了できる。Stop を返す実行も自身の終了を待たずに応答する。停止後の新しい子 call は、開始済みの親があっても `ProcessStopped` となる。無期限待機があれば通常 Stop の完了にも有限時間を保証しない。

| handler の停止結果 | その要求への応答 | 終了理由 |
|---|---|---|
| `CallResult::Stop(StopReply::Normal(reply))` | `Ok(reply)` | Normal |
| `CallResult::Stop(StopReply::Error(error))` | 情報を保った `Err(error)` | Error(error) |
| `CastResult::Stop(StopReason::Normal)` | `Ok(())` | Normal |
| `CastResult::Stop(StopReason::Error(error))` | `Ok(())` | Error(error) |

cast の公開署名は `Result<Unit>` を維持する。停止宛先への拒否と handler 自身の `Err` はその Result で返す。cast の StopReason::Error と handler 自身の `Err` を混同しない。

`ProcessStopped(pid: Int, process: String)` は停止要求中と停止完了・本体回収後で同じ宣言 identity、payload schema、message を持つ。`process` は canonical な process 名、`pid` は内部 ID を利用者の `Int` に変換した値とし、message は `process #{pid} (#{process}) is stopping or stopped` とする。Error の生成元は拒否した要求の呼出し位置とし、Stop handler や deferror 宣言の位置で上書きしない。利用者は kind で PID 差し替え、エラー伝播、自身の停止結果への変換を選べる。caller を自動的に異常終了させない。

| 条件 | エラー境界 |
|---|---|
| 停止要求済み・回収済みへの新規要求 | `ProcessStopped`。回収を理由に UnknownPid / StateUnavailable へ変えない |
| 実在しない PID、process 型不一致 | 既存の `ProcessStateUnknownPid` / `ProcessStoreUnknownPid`、`ProcessStatePidTypeMismatch` / `ProcessStorePidTypeMismatch` |
| 生存中だが state がない | `ProcessStateUnavailable` または初期化契約違反 |
| 実行権限のない store | RuntimeError。停止エラーや成功へ変換しない |
| runtime 異常・強制打切りで開始済み reply を返せない | 未確定の reply を `ProcessReplyTargetStopped(future, pid)` で終了し、元の RuntimeError も報告する |
| caller timeout が先着 | `FutureDeadlineExceeded`。後発 reply や停止エラーで上書きしない |

通常 Stop は開始済み要求を process-down へ変換しない。runtime 異常による打切りは通常 Stop より優先するが、確定済みの結果は変更しない。

### 3.11 Supervisor / DynamicSupervisor

Supervisor は次の層で整理する。

| supervisor | 役割 |
|---|---|
| RootSupervisor | アプリ起動のルート。boot failure を集約 |
| RuntimeSupervisor | singleton 群、standard singleton、runtime / bridge singleton を管理 |
| DynamicSupervisor | 動的に増減する worker を管理。restart / cleanup を行う |

初期フェーズでは、restart policy の主対象は Worker とする。Standby singleton と worker restart は分離する。

```text
restart = Worker lifecycle の話
Standby singleton = init 完了保証の話
```

DynamicSupervisor は singleton process として扱い、user-facing API に `sup: PID<_>` を出さない。

```surtr
pid = DynamicSupervisor::spawn(MyWorker::init(args))
```

Supervisor policyと起動overrideはcanonicalな宣言名で照合する。末尾の短名が同じ別namespaceの宣言へoverrideを適用しない。標準DynamicSupervisorの内部keyへ接続するのは、canonicalな`Global::DynamicSupervisor`かつDynamicSupervisor種別の宣言だけとする。表示用の`Global::`除去をpolicy照合へ使わない。incremental compileは可視prefixのruntime metadataから宣言のcanonical名と実policyを復元し、そのpolicyへoverrideを適用する。未定義・曖昧な宣言を既定policyの合成で救済しない。

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

`adopt` は generated supervisor owner helper (`MySupervisor::adopt(pid)`) として public surface に残す。
`handoff` は runtime-internal 操作であり、user-facing API としては公開しない。
`adopt / handoff` は runtime が原子的に処理し、PID は維持する。
受付中の live Worker だけを対象にする。停止要求中・完了後・回収後は、同じ supervisor への adopt でも `SupervisorAdoptWorkerNotLive` とする。停止表でも型を検証し、未知 PID・非 Worker・adopt 禁止の既存検証を維持する。停止済みの許可された Worker を回収後に UnknownPid へ変えない。

移管が Stop より先なら新 supervisor が停止完了時の所属解除を行い、Stop が先なら移管を拒否して元 supervisor を維持する。`child_count` は停止要求中の個体も含み、停止完了時に一度だけ減る。`shutdown_timeout: Some(...)` は設定と status 表示の契約であり、通常 Stop、call timeout、開始済み処理の取消には作用しない。期限付き supervisor shutdown と一般の restart driver は別の後続課題とする。

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

runtime は `WorkerStrategy` を worker set state に保持し、`Fix(n)` について `init == n` かつ `0 <= min <= n <= max` を検証する。`init != n` は `WorkerStrategyInitTargetMismatch(init, n)`、bounds 違反は `WorkerStrategyBoundsInvalid(min, n, max)` を `Err` で返す。正規の `Int` が内部の固定幅整数へ収まらない場合は `WorkerStrategyFieldOutOfRange(field, value)` を返し、元の整数を保存する。schema・tag・field・型の内部不整合は Rust `RuntimeError` とする。

停止要求済み Worker は新規選択から直ちに外すが、membership と target 枠は停止完了まで維持する。`Workers::size` は停止要求中を含む管理下の個体数を返す。`submit` / `reserve` は受付中の個体を round-robin で選び、選択できなければ `Err(WorkersUnavailable(workers, process))` とする。暗黙の待機・再試行はしない。正当な 0 件集合・補充中も同じエラーとし、未知 handle や消失 member などの不整合は RuntimeError とする。

`WorkersUnavailable(workers: Int, process: String)` の `process` は canonical な Worker process 名、`workers` は内部集合 ID を利用者の `Int` に変換した値とし、message は `workers #{workers} for #{process} has no accepting worker` とする。宛先 PID がないため架空の PID で ProcessStopped を作らない。

`broadcast` は開始時に受付中の個体を列挙して順に要求し、対象がなければ `[]` を返す。列挙後に停止した旧対象はその要素の `ProcessStopped` となり、途中の補充個体を対象へ加えない。選択・lease 取得後の Stop は生成 wrapper の開始境界で拒否する。既存 lease は停止拒否を回避せず、新個体へ差し替えない。

停止完了時に Workers membership と supervisor child 参照を一度だけ解除し、本体を回収してから target 不足分を refill する。停止要求中の枠は補充しない。`refilling_worker_sets` による重複防止を維持し、旧 PID と lease は新個体へ転送しない。`Workers<$Worker>` handle 自体は保持でき、user code に reconcile loop や target 更新 API は不要である。target 維持の refill を一般の supervisor restart policy の実装済み根拠にしない。

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

`member_pids` は停止要求中も含む現在の所属 PID 列、`live_count` は本体が存在する member 数である。停止要求中はどちらにも残り、停止完了時に除かれる。受付可能数とは区別する。busy / idle などの詳細状態は v1 public surface には含めない。

### 3.12 Worker lifecycle

Worker は `spawn` で生成し、`PID<Proc>` を通して扱う。

`PID<Proc>` の型引数は、登録済み process 宣言の canonical identity、標準 handler capability (`OutHandler` / `InHandler`)、または宣言内の通常の型変数である。未知名、通常の型や単なる module を process marker として受理しない。無修飾の process 名は implicit root の宣言名へ正規化し、末尾の短名が同じ別 namespace の宣言を同一視しない。

`PID<$P>` の `$P` は他の signature generic と同じ変数であり、同じ宣言内の出現は同じ束縛を共有する。異なる process の PID を同じ `$P` の引数へ渡すことや、`PID<$P>` を別の具体 process の PID として返すことは拒否する。PID の marker は型検査・特殊化・canonical 型比較で通常の代入と rigid 変数の規則に従い、文字列の `$` prefix で適合を補わない。runtime には具体化済みの canonical marker を渡す。型変数を marker として使う制約は同じ変数の通常の代入にも適用し、明示的な ReturnTypeArgument で通常型へ具体化することを拒否する。ローカルの PID annotation で、marker として宣言していない rigid signature 変数へ制約を追加しない。nominal 型の generic parameter を PID marker に使う場合も、その型引数には同じ marker の解決・束縛規則を適用する。

Scar の `Pid` は marker 型を子として持つ。具体 marker は process 宣言または標準 handler 宣言の canonical identity を表す内部型とし、runtime 値にはしない。型変数の収集、occurs check、代入、fresh 化、canonical 型比較、特殊化はこの子へ再帰する。Error payload schema も同じ marker 構造を保持し、generic field の型引数を代入してから runtime PID の canonical process 名を検査する。

`PID<Proc>` の `Eq` は compiler-owned capability であり、同じ process type の PID だけを比較する。
Singleton は process type ごとに一意なので、restart 前後の handle も等しい。
Worker は同じ instance ID のときだけ等しく、終了後に保持された PID も同じ ID なら等しい。
異なる process type は型エラー、`PID<OutHandler>` 等の handler capability は Eq 対象外である。
PID を対象にしたユーザの trait impl は許可しない。

PID は不変 identity を `Rc` で共有する。identity は runtime 内の個体 ID、canonical な process 型の識別情報、PID 種別だけを持ち、state・mailbox・継続・VM を強参照しない。PID copy、lease、closure 内の PID は同じ identity を保持する。handler capability の PID は別種別として扱い、process の停止表・Worker 回収・Eq へ混ぜない。

停止完了時は本体を削除し、copy-on-write の停止識別表へ identity の `Weak` だけを登録する。停止理由の Error、state、継続、PID の強参照を停止表に残さない。保持された旧 PID への要求は、本体回収後も同じ `ProcessStopped` を返す。停止識別は数値 ID だけでなく identity の一致を検証し、生存本体にも停止表にもない identity は未知 PID または内部不整合とする。

個体 ID は同一 VM 内で再利用しない。allocator の高水位は checkpoint の巻戻し対象外とし、失敗 chunk の handle を将来の spawn や別 VM の個体へ接続しない。ID 枯渇は内部エラーとする。停止識別表は spawn / 停止完了、および chunk commit / rollback / checkpoint 破棄後の管理境界で、強参照のなくなった Weak を除く。PID を捨てた瞬間の GC は保証しないが、参照のない停止履歴を無制限に保持しない。

停止完了では run queue、waiting、不要な reply mapping・deadline・owner・callback、Workers と supervisor の参照も整理する。配送に必要な最小情報と caller / 公開 TaskHandle が読む確定 future は独立に残す。sleep など実行専用 future は最後の利用終了時に解放する。旧 PID を持つだけで本体の state・mailbox・継続を保持してはならない。

| 項目 | 仕様 |
|---|---|
| default owner | current process |
| lifecycle sink | spawn 直後から 1 つ持つ |
| singleton explicit exit | なし |
| worker 停止完了 | runtime 内部で lifecycle sink / supervisor / Workers の所属を一度だけ整理 |
| generic receive | 導入しない |

top-level の plain Worker `spawn` には current process が存在しないため、
初期実装では `DynamicSupervisor` を default lifecycle sink として登録する。
process handler 内など current process context が確立している経路では current process owner を優先する。

`Process::link` / `Process::monitor` / `Process::join` は v2 初期 surface には出さない。
link は `owner` / `lifecycle_sink` / supervisor tree / restart policy の runtime 内部関係として扱う。
monitor は generic receive を公開しない方針と相性が悪いため、必要になった場合は
typed `on_down` など用途別 API として検討する。join は Task では `Task::call` / `Task::async`
と `@timeout` に寄せ、Worker 終了待ちは後続の Worker 専用 API として検討する。
Worker 向けの `join` / `await` / `on_down` は v2 public surface ではない。
`Task::await` は `TaskHandle` 専用であり、worker PID の待機 API としては使わない。

Exit reason 候補:

```rust
enum ExitReason {
    Normal,
    Exit(ErrorValue),
    RuntimeFault(RuntimeError),
    InitFailed(ErrorValue),
}
```

singleton の新規 PID 解決は既存 slot を使い、開始済み実行は開始時の個体に固定する。型単位の Eq を旧 PID の転送根拠にしない。必須 singleton の slot 欠落は内部不整合とし、singleton restart や一般の Worker restart driver は追加しない。

### 3.13 Task

初期フェーズの Task は使い捨て process として扱う。

```surtr
task = Task::async({||
  Process::sleep(10ms)
  Ok("ready")
})
result = Task::await(task) @timeout(100ms)
```

`@timeout` は直前の runtime-managed call に timeout policy を付与する。timeout した場合、結果値は `Err(FutureDeadlineExceeded(...))` になる。この Error の `future: Int` フィールドに期限を超過した future ID を保存する。

`Task::async` は body を開始し、最初の待機・予算切れ・完了まで進んだ後に handle を返す。
たとえば body が出力してから sleep する場合、その出力は handle を受け取った後の処理より先に起きる。
CPU 処理が続く場合は予算切れで実行を切り替え、残りを背景で進める。
入れ子の通常 callback は同じ予算を使い、呼び出すたびに予算を補充しない。
timeout による取消でも、開いているファイルなどの後処理を終えてから結果を配送する。

初期フェーズでは、Task.Supervisor / DynamicSupervisor link は扱わない。
Task の `link` / `cancel` / `restart` も v2 public surface には含めない。

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
| call timeout | call 開始 | reply | `Err(FutureDeadlineExceeded(...))` |
| task timeout | task 開始 | result | `Err(FutureDeadlineExceeded(...))` |
| sleep | sleep 開始 | timer wake | error ではない |

Ready 前に call した場合、call timeout は Ready 待ち時間を含む。
`CallResult::ReplyLater(next_state, callback)` の場合も、外側の call timeout は call entry から開始し、callback の待機・`Process::sleep` 時間を含む。
外側 timeout が先に到達した場合は reply future を `Err(FutureDeadlineExceeded(...))` として解決し、reply mapping / deadline を消す。
callback completion が先に解決した場合だけ callback の reply が勝ち、timeout 後に callback が完了しても timed-out reply を上書きしない。

call timeout は結果を確定するだけで、開始済み handler / wrapper / ReplyLater callback を取り消さない。callee の wrapper 全体は runtime 所有の独立した ExecutionContext で進め、caller は結果 future の待機継続を持つ。caller の待機が終了しても callee を再開し、callee の存続を理由に timeout 結果を caller から隠さない。direct / capture / Workers / lease と timed callback 経由で同じ契約を使う。親実行 ID は呼出し関係を表し、取消伝搬を意味しない。

ReplyLater 前の通常 handler 内で timeout しても、callee は後で一度だけ保存・返答を終える。遅延完了で取消済みの caller 待機文脈を復活させたり、caller の後続処理を再実行したりしない。reply mapping が消えたことを ReplyLater callback の取消条件にしない。未開始で timeout した要求は dispatch 対象から外す。

Task 自身の timeout による取消と cleanup 後の結果配送、init timeout の取消は別契約として維持する。handler が起動した独立 Task は process の停止待ちへ暗黙に追加しない。通常 Stop の完了は開始済み process 実行の終了を待ち、call timeout や shutdown_timeout で打ち切らない。

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

`supervisor_init` は top-level 起動構成 block とし、通常式評価とは分ける。ここで定義されるのは singleton boot entry、handler override、supervisor policy override を含む `RuntimeBootPlan` の入力であり、VM は surface DSL を直接読まない。

`boot: Required` / `boot: ExplicitOnly` のような boot policy 指定は、定義側にも Boot 側にも置かない。
起動対象は、`supervisor_init` / project runner に記載された singleton entry と、runtime が自動提供する builtin standard I/O から決まる。

役割:

- 起動対象に含める singleton を明示する
- custom supervisor を runtime process space へ登録し、その policy override を指定する
- `DynamicSupervisor` の policy override を指定する
- init timeout を指定する
- process-local handler を override する

例:

```surtr
supervisor_init {
  Logger {
    timeout: 5s
  }

  DynamicSupervisor {}

  ImageWorkerSupervisor {
    max_restarts: 10
    allow_adopt: True
  }
}
```

entry は `ProcessName { ... }` のみとする。`singleton ProcessName { ... }` は廃止済みであり parse error とする。
entry 名は通常コードの型名解決と同じく bare type name として解決する。qualified name 専用の DSL ルールは持たない。同一可視圏で同名型が複数見える場合、未知名、同じ型名または同じ解決先の重複 entry は compile error とする。entry 記載順は意味を持たない。

`DynamicSupervisor` は暗黙登録済みである。DSL に記載しない場合は既定 policy のまま利用できる。`DynamicSupervisor {}` は空マージとして許可し、DSL に policy key がある場合だけ BootPlan の effective policy に反映する。

custom `defsupervisor` は `Sup::status()` / `Sup::spawn(...)` / `Sup::adopt(...)` / `Sup::workers(...)` の generated surface に依存する compile unit では `supervisor_init` 登録必須とする。include されているだけで未使用の custom supervisor は登録不要であり、未登録診断は DSL ではなく surface 依存検査で出す。

worker entry は常に禁止する。worker pool / scaling は DSL では扱わず、singleton GenServer の `@init` から `Sup::workers(...)` を呼び、runtime 管理へ渡す。

Entry key:

| entry kind | 許可 | 禁止 |
|---|---|---|
| singleton Agent / GenServer | `timeout`, `handlers` | supervisor policy keys, `parent` |
| `DynamicSupervisor` / custom `defsupervisor` | `strategy`, `max_restarts`, `max_seconds`, `child_restart_default`, `allow_adopt`, `shutdown_timeout` | `timeout`, `handlers`, `parent` |
| Worker | なし | entry 自体を禁止 |

supervisor 親は固定で、DSL `parent` override は受理しない。

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
| `DynamicSupervisor` | runtime builtin supervisor として常に登録 |
| custom `defsupervisor` | `supervisor_init` / project runner に記載された場合に登録 |
| worker | `supervisor_init` には記載不可。runtime API から生成 |
| 記載なし、かつプロセス呼び出しなし | 起動しない |
| プロセス呼び出しあり、かつ available singleton に含まれない | compile-time singleton 利用検査で error |
| custom supervisor surface 呼び出しあり、かつ登録なし | compile-time supervisor surface 依存検査で error |

`init_policy` は定義側にあるため、Boot 側は Standby の採否を決めない。Boot 側は起動対象、timeout、handler override、supervisor policy override を指定する。

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
  Logger {
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
  Logger {
    handlers {
      out: StdOut
    }
  }
}
```

```surtr
supervisor_init {
  Logger {
    handlers {
      out: NullOutHandler
    }
  }
}
```

```surtr
supervisor_init {
  Logger {
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
| open failure | `Err(FileOutHandlerOpenFailed(path, detail))` |
| write failure | `OutHandler::write` の `Err(FileOutHandlerWriteFailed(path, detail))` |

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
    Standby,
}
```

```rust
enum InitResultShape {
    EagerState {
        result_type: TypeRef, // Result<State>
    },
    StandbyProcessInit {
        result_type: TypeRef, // Result<StandbyInit<State>>
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
    InitPolicy::Standby => {
        // Ok(StandbyInit::Ready(state)) -> Ready(state)
        // Ok(StandbyInit::Pending) -> retry by runtime default policy
        // Ok(StandbyInit::PendingAfter(d)) -> retry after d
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
    admission: ProcessAdmission,
    stop_reason: Option<ExitReason>,
    state: Option<Value>,
    context: ProcessContext,
    mailbox: VecDeque<RuntimeMessage>,
    owner: Option<Pid>,
    lifecycle_sink: Option<LifecycleSink>,
}
```

`state: Option<Value>` は VM 内部表現であり、Surtr surface に Option 型を導入する意味ではない。
概念上の構造であり、実行ごとの ExecutionContext と実行 record は別に管理する。一個体の status だけから再入の配送先や実行終了を推測しない。

### 4.11 `ProcessStatus`

```rust
enum ProcessStatus {
    Runnable,
    Waiting(WaitReason),
}

enum ProcessAdmission {
    Accepting,
    Stopping,
}
```

受付状態は scheduler 状態から独立する。停止要求中かつ Waiting でも開始済み継続を再開できる。停止完了は本体を削除する遷移であり、空の Stopped / Exited 本体を常設しない。RuntimeError の報告と異常打切りは共通終了経路で処理する。

Worker は init の `Ok(state)` 後、singleton は Ready 後に PID を割り当てる。初期化中は Worker Spawn 継続と singleton init flight が管理するため、初期化中の同じ個体への公開 Stop は到達不能である。init / singleton の Stop 結果を型検査で拒否し、仮 PID と初期化中 Stop 状態を導入しない。init が別の既存 Worker を停止させる場合は、その個体の通常 Stop とする。

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
    Halt(Value),
    Pending {
        future_id: FutureId,
        resume: ExecutionContext,
    },
    RuntimeError(RuntimeError),
}
```

Scheduler 境界では `run_quantum` が `StepOutcome` を次へ正規化する。

```rust
enum ProcessRunOutcome {
    QuantumExpired,
    Halted(Value),
    Pending(FutureId),
    Failed(RuntimeError),
}
```

初期フェーズの `Pending` は既存 future/deadline table を使う。`WaitReason::Timer`
などの意味づけは process runtime 側の table / status に保持し、VM engine は
surface DSL や supervisor boot state を `ExecutionContext` へ持ち込まない。

### 4.14 Standby Ready 前 call の扱い

以下の FIFO 保存、mailbox への移動と dispatch 順序は、後続実装に向けた要求であり、現行実装の保証ではない。現在の Eldr mailbox は未開始メッセージの queue として使われず、非空なら内部契約違反となる（第3.10.2節）。

後続実装では、Standby singleton が Ready になる前に到着した message call を、通常 mailbox ではなく `init_waiters` に FIFO で保存する。

後続実装で要求する Ready 到達時の処理:

```text
1. Standby init が Ready(state) を返す
2. runtime が state slot に state をセットする
3. PID と本体を割り当て、受付を Accepting、scheduler 状態を Runnable にする
4. init_waiters を到着順に通常 mailbox の front 側へ移す
5. process を runnable queue に 1 回 enqueue する
6. scheduler が通常 message dispatch として順に処理する
```

後続実装で要求する順序例:

```text
t1: call A arrives while Initializing
t2: call B arrives while Initializing
t3: Ready
t4: call C arrives after Ready

処理順: A -> B -> C
```

現行実装では、call timeout は call 開始から reply までを対象とし、Ready 待ち時間も含む。
Ready 待ちは handler 未開始であり、PID 取得後の開始境界で受付を確認する。初期化成功で既存個体の受付を再開しない。init 失敗・timeout 時の flight / callback 整理は既存の初期化失敗契約に従う。Ready 前 FIFO と Lazy 初期化全体の実装完了は別に検証する。

### 4.15 scheduler queue

VM は少なくとも次の queue / table を持つ。

| 構造 | 用途 |
|---|---|
| runnable queue | 実行可能 process を保持 |
| deadline queue | timer / timeout deadline を保持 |
| waiting table | reply / init ready / task completion 待ちを保持 |
| init flight / init waiters | PID 割当前の初期化と Ready 待ちの未開始要求を保持 |
| execution context | process ごとの `pc` / stack / call frames と builtin / callback の継続状態を保持 |
| singleton slot | singleton process の current PID を保持 |
| process table | PID から process instance を引く |
| process execution records | 実行 ID ごとの対象 identity・親・段階・配送先・source origin を保持 |
| stopped identity table | 回収済み identity の Weak を保持。参照のない entry は管理境界で除く |
| spec table | RuntimeProcessId から immutable spec を引く |
| handler target registry | handler target identity から shared sink / builtin handler を引く |

`RuntimeBootPlan`、`effective_supervisors`、singleton slot、`DynamicSupervisor` の既定 policy は
runtime global state であり、process-local `ExecutionContext` には入れない。

通常の batch / REPL chunk、process handler、Task::async / launch と detached task の再開は、
共通の予算付き driver を通す。callback の開始や builtin の再開によって予算を補充せず、
scheduler が runnable な実行を選び直したときに次の quantum を与える。

- CPU の予算切れでは継続状態を Runnable として保存し、runnable queue へ一度だけ戻す。
- Future 待機では待機先と継続状態を Waiting として保存し、解決時に runnable へ戻す。
- 完了結果と失敗は保存した復帰先へ一度だけ配送する。process state の更新や wrapper の後処理も、この復帰先の責務に含む。
- builtin 内に event loop を入れ子にして callback を完走させない。実行可能な処理がなく timer / I/O を待つときの待機は共通 driver が行う。

継続状態は VM 内部にあり、利用者の process state / payload には入らない。
immutable な値の backing storage は同一 VM 内で共有してよい。REPL checkpoint は中断中の状態も保存し、
rollback 後は保存位置から再開する。外部 I/O の副作用を巻き戻す保証は加えない。
checkpoint は受付状態、実行 record、ExecutionContext、future / reply / waiting / deadline / task、Workers・supervisor、停止識別表を同一時点で保存・復元する。表と entry は copy-on-write で共有し、変更する部分だけを複製する。不変 identity は共有できるが、Cell 等の共有可変な停止 flag は使わない。live 側の停止は保存側を変更しない。
停止前へ rollback すれば当時の受付・実行へ戻り、停止要求中へ戻れば新規要求を拒否したまま継続を再開する。個体 ID の高水位は戻さない。active VM の本体を削除しても停止前の checkpoint は state と継続を保持でき、物理解放はその checkpoint の破棄後まで含めて測る。
観測は本体の scheduler 状態と受付状態を分け、停止要求中の本体数、未完了実行数、保持中の停止識別 entry 数を区別する。回収済み個体は process 一覧と本体件数から除き、診断用の無期限停止履歴を残さない。
中断中の継続状態を変更する場合は、そのentry内の可変Builderも独立した状態へ複製し、保存位置を維持する。
未分割の Rust loop・外部呼出し・要素の clone / drop は、実時間の公平性保証の対象外とする。

---

## 5. diagnostics 例

### 5.1 process 定義

| 発生箇所 | 条件 | error id | 簡易メッセージ | help |
|---|---|---|---|---|
| `meta` | `@agent(...)` を使っている | `process-meta-deprecated` | `@agent(...)` metadata is no longer supported. | Use `meta { instance, init_policy, state }` inside the process definition. |
| `meta` | `boot` を定義側に置いた | `process-meta-boot-not-allowed` | boot settings must be declared in `supervisor_init`. | Move boot policy and timeout to Boot configuration. |
| `meta` | `registry` を定義側に置いた | `process-meta-registry-not-allowed` | registry settings are runtime / boot concerns. | Remove registry from process meta. |
| `meta` | `init_policy: Standby` を Worker に付けた | `process-standby-not-allowed` | Standby init is only allowed for Singleton Agent / Singleton GenServer. | Use `Eager`, or define an async call API. |
| `meta` | `init_policy: Standby` を Supervisor に付けた | `process-standby-supervisor` | Supervisor does not support Standby init. | Remove `init_policy: Standby`. |
| `defagent` | `@init` がない | `agent-init-missing` | Agent requires exactly one `@init` handler. | Add one `@init` handler. |
| `defagent` | `@get` がない | `agent-get-missing` | Agent requires exactly one `@get` handler. | Add one `@get` handler. |
| `defagent` | `@set` が複数ある | `agent-set-duplicate` | Agent allows at most one `@set` handler. | Use GenServer for multiple write protocols. |
| `defagent` | `@get` が複数ある | `agent-get-duplicate` | Agent allows exactly one `@get` handler. | Use GenServer for multiple query protocols. |
| process helper | handler annotation なしで `def` を使った | `process-helper-requires-defp` | Process helpers must use `defp`; change `def` to `defp` | Change the helper declaration to `defp`. |
| process handler | handler annotation に続けて `defp` を使った | `process-handler-requires-def` | Handler marker must be followed by def. | Use `def` for handlers; use annotation-less `defp` for local helpers. |
| `defgenserver` | `@call` の戻り値が `Result<Reply>` | `genserver-call-return-mismatch` | `@call` must return `Result<CallResult<Reply, State>>`. | Return `CallResult::Reply(...)`, `ReplyLater(...)`, or `Stop(...)`. |
| `defgenserver` | `@cast` の戻り値が `Result<()>` | `genserver-cast-return-mismatch` | `@cast` must return `Result<CastResult<State>>`. | Return `CastResult::Next(...)` or `Stop(...)`. |
| `meta.handlers` | default target が slot capability を満たさない | `handler-default-capability-mismatch` | handler default does not satisfy required capability. | Use a handler that implements the required capability. |
| process body | handler slot を裸で参照した | `process-context-bare-access` | handler dependency must be accessed through `ctx.<slot>`. | Use `ctx.out` instead of `out`. |
| process body | `ctx.<slot>` に代入した | `process-context-readonly` | process context handler is readonly. | Override it from `supervisor_init`. |
| public API | `ctx.<slot>` / handler PID を返した | `process-context-leak` | handler dependency cannot be returned from public API. | Keep handler access inside the process. |

### 5.2 init / StandbyInit

| 発生箇所 | 条件 | error id | 簡易メッセージ | help |
|---|---|---|---|---|
| `@init` | Eager なのに `Result<StandbyInit<State>>` | `process-init-return-mismatch` | Eager init must return `Result<State>`. | Change `init_policy` to `Standby`, or return `Result<State>`. |
| `@init` | Standby なのに `Result<State>` | `process-init-return-mismatch` | Standby init must return `Result<StandbyInit<State>>`. | Wrap the initialized state with `StandbyInit::Ready(state)`. |
| `@init` | `StandbyInit::Ready<T>` の `T` が state と違う | `process-init-ready-type-mismatch` | `StandbyInit::Ready` value must match the process state type. | Return `StandbyInit::Ready` with the declared state type. |
| `@init` | `PendingAfter` に `Duration` 以外を渡した | `process-init-pending-after-type` | `PendingAfter` requires `Duration`. | Pass a `Duration` value, for example `100ms`. |
| 通常関数 | `StandbyInit<T>` を戻り値に使った | `process-init-type-position` | `StandbyInit<T>` is only allowed as Standby `@init` return type. | Use a domain enum instead. |
| struct field | `StandbyInit<T>` を field に使った | `process-init-type-position` | `StandbyInit<T>` cannot appear in data types. | Store a domain-specific status enum instead. |
| `@call` / `@get` | `StandbyInit<T>` を返した | `process-init-type-position` | `StandbyInit<T>` must not leak into process public API. | Return a View / Reply type. |

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
| runtime init | Standby init deadline 超過 | `runtime-process-init-timeout` | process did not reach `Ready` before init timeout. | Increase Boot timeout or reduce init wait. |
| runtime init | Standby/Eager init が `Err` | `runtime-process-init-failed` | process init failed. | Check init dependencies and process definition. |
| runtime dispatch | handler table に存在しない handler | `runtime-handler-not-found` | runtime process handler was not found. | This indicates compiler / VM spec mismatch. |
| runtime dispatch | singleton slot が空 | `runtime-singleton-slot-missing` | singleton slot is missing for a required process. | This indicates BootPlan / VM state mismatch. |
| scheduler | Pending が scheduler に登録できない | `runtime-pending-registration-failed` | process pending state could not be registered. | This indicates runtime scheduler inconsistency. |
| scheduler | caller timeout | `runtime-call-timeout` | process call timed out. | Returned as `Err(FutureDeadlineExceeded(...))` to user code. |
| task | task timeout | `runtime-task-timeout` | task timed out. | Returned as `Err(FutureDeadlineExceeded(...))` to user code. |
| handler init | `FileOutHandler` の open に失敗 | `runtime-handler-init-failed` | handler init failed. | Check file path, permissions, or host resources. |
| handler write | `OutHandler::write` が失敗 | `runtime-handler-write-failed` | handler write failed. | Returned as `Err` from `OutHandler::write`. |

---

## 6. 補足: 後続課題

| 項目 | 扱い |
|---|---|
| boundary layer | process 基盤安定後に domain/runtime/boot error の変換層として設計 |
| Standby init / scheduler convergence | `Pending` / `PendingAfter` retry、`init_waiters`、Ready 前 call、runtime status 表示は OI-030 で扱う |
| Worker async / standby init | 非同期 call API と合わせて検討。v2 では public surface にしない |
| Task.Supervisor / Task link / cancel / restart | Task を使い捨て process として安定させた後に検討 |
| Task-DynamicSupervisor link | 初期フェーズでは扱わず、Task supervision 設計時に再検討 |
| DynamicSupervisor restart details | 初期は OneForOne 最小。`max_restarts`, `max_seconds` は後続 |
| Runtime Logger / handler target | Logger を singleton process とするか handler target とするかは OI-031 で扱う |
| Mass process benchmark | 大量 worker / message / deadline queue の性能比較 harness は OI-032 で扱い、通常 correctness suite から分離する |
| REPL / tooling 表示 | `:info` に process spec / singleton slot / supervisor tree を表示する方向で後続 |
| BootPolicy enum | `Required / ExplicitOnly / StandardDefault / OnReference` は使わない。起動対象は Boot 設定に現れた entry と builtin standard I/O から決める |
| Pure Surtr test DSL の I/O capture | `capture_stdout` / `assert_stdout_eq` / `push_stdin` などは目標として保持し、内部実装は別途設計 |

### 6.1 大量 process benchmark の基準メモ

大量 process benchmark は言語 surface ではなく、process runtime 実装の比較用 harness として扱う。
通常の correctness suite には入れず、専用 profile または手動 benchmark として実行する。

基準シナリオは、単一 manager process と多数 worker process の構成を使う。
worker は「指示受信、移動、採取、帰還、報告、休憩」を繰り返し、manager が累計採取量 `T` 到達を終了条件として管理する。
単一 manager 集中モデルは初期 runtime の bottleneck を検出するための基準であり、純粋な scheduler 性能だけを測る場合は後続で manager shard 版を別 harness として追加する。

入力は worker 数 `W` と累計目標 `T` とし、平均 1 cycle あたりの採取量を約 `300`、平均 cycle 長を約 `400` frame として見積もる。
1 worker あたり平均 `K` cycle 実行させる基準式は次のとおり。

```text
T ~= W * 300 * K
```

推奨する代表ケース:

| 用途 | W | T | 目安 |
|---|---:|---:|---|
| 軽量確認 | 1,000 | 3,000,000 | K ~= 10 |
| 標準比較 | 10,000 | 30,000,000 | K ~= 10 |
| 重量確認 | 50,000 | 150,000,000 | K ~= 10 |
| 上限調査 | 100,000 | 300,000,000 | K ~= 10 |

測定項目は、elapsed time、frame count、max RSS、process count、worker live count、waiting max、mailbox max、future / deadline count、timeout count、message send/reply count、manager / worker receive count を最低限とする。
乱数 seed、timeout policy、worker / manager spec は明示的に固定し、同一 seed で比較可能にする。
