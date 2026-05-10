# Process

Surtr の process surface は、状態や非同期実行を runtime 管理に乗せつつ、利用側では型付き API を呼ぶ形を保つための入口です。

ここでは `defagent` だけでなく、singleton / worker、`PID<T>`、`Task::*`、`defgenserver`、`handlers {}` と `supervisor_init` までをまとめます。
VM 向けの正規化仕様まで追いたいときは `../dev/ProcessRuntime_spec.md` を見てください。

## まず押さえる 4 つ

- singleton process は共有インスタンスを持ち、`Counter::get(...)` や `Counter::set(...)` のように direct surface で呼べます
- worker process は `Type::init(...)` で `PID<T>` を受け取り、その PID を使って stateful API を呼びます
- process API の失敗は panic ではなく `Result` で返ります。`=?` を使うと `Err(...)` をそのまま返せます
- `Task::*`、`defgenserver`、handler 差し替えは同じ process surface の仲間ですが、用途はそれぞれ異なります

`surtr run` で直接実行する script には top-level process 定義を置かず、定義を別ファイルへ切り出して `include` する形が扱いやすいです。

```surtr
include "./Agents.srt"
```

`include` の細かい規則は `./language-features.md`、`Result` と `=?` の読み方は `./error-handling.md` にまとめています。

## Singleton Process

singleton は「同じ状態を全体で共有したい」ときの基本形です。設定ストア、メトリクス集約、キャッシュのように、1 つだけあればよい状態に向いています。

最小の read-only 例は `examples/process/read_only_agent` です。

```surtr
include "./Agents.srt"

print(inspect(Env::get("HOME")))
```

実行:

```bash
cargo run -q -p rune -- run examples/process/read_only_agent/entry.srt
```

`examples/process/state_agent_singleton` は、singleton に `@set` を足した最小の stateful 例です。

```surtr
include "./Agents.srt"

print(inspect(Counter::get("count")))
print(inspect(Counter::set(99)))
print(inspect(Counter::get("count")))
```

実行:

```bash
cargo run -q -p rune -- run examples/process/state_agent_singleton/entry.srt
```

`examples/process/agent_singleton_counter` では、`Err(...)` を返したときに state が更新されないことまで確認できます。

```surtr
include "./Agents.srt"

print(inspect(Counter::get("count")))
print(inspect(Counter::set(3)))
print(inspect(Counter::get("count")))
print(inspect(Counter::set(-20)))
print(inspect(Counter::get("count")))
```

実行:

```bash
cargo run -q -p rune -- run examples/process/agent_singleton_counter/entry.srt
```

読みどころ:

- singleton の `@get` / `@set` は direct surface で公開されます
- 呼び出し側は `PID<T>` を意識せずに `Counter::get(...)` や `Counter::set(...)` を使えます
- `@set` が `Err(...)` を返した場合、state はそのまま残ります

## Worker Process

worker は「呼び出しごとに独立 state を持たせたい」ときの形です。セッション、ジョブ、リクエスト単位の処理に向いています。

`examples/process/agent_worker_multi` では、`Worker::init(...)` が `PID<Worker>` を返し、PID ごとに state が分かれることを試せます。

```surtr
include "./Agents.srt"

alpha =? Worker::init(3)
beta =? Worker::init(7)

print(inspect(Worker::get(alpha, "jobs")))
print(inspect(Worker::get(beta, "jobs")))
print(inspect(Worker::set(alpha, 1)))
print(inspect(Worker::set(beta, 2)))
```

実行:

```bash
cargo run -q -p rune -- run examples/process/agent_worker_multi/entry.srt
```

読みどころ:

- `Worker::init(3)` の型は `Result<PID<Worker>>` です
- `alpha` と `beta` は別 PID なので、片方を更新しても state は混ざりません
- `PID<T>` は型付きなので、別 process の PID を混ぜると compile error になります

singleton と worker の選び方は単純です。

- 同じ状態を全体で共有したいなら singleton
- 呼び出しごとに独立 state を持たせたいなら worker
- まず singleton で API を固め、必要になったら worker 化する切り方もできます

## Task

`Task::*` は stateful process を長く持つための surface ではなく、「処理を 1 回走らせて結果を受け取る」ための surface です。

最小例は `examples/process/task_call` です。

```surtr
value = Task::call({|| Ok("task:" ++ to_string(20 + 22))})
print(inspect(value))
```

実行:

```bash
cargo run -q -p rune -- run examples/process/task_call/entry.srt
```

まずは次だけ覚えておけば十分です。

- いますぐ結果が欲しいなら `Task::call(...)`
- 開始と待機を分けたいなら `Task::async(...)` と `Task::await(...)`
- timeout は開始側ではなく待機側の call に付けます

## GenServer / Worker の組み合わせ

`defgenserver` は「呼び出しを受けながら、自分の state と他 process をまとめて管理したい」ときに使います。

`examples/process/memoized_fib_workers` では、singleton の `FibManager` が 2 つの worker を抱え、偶数と奇数でキャッシュ先を分けています。

```surtr
include "./Workers.srt"

print(inspect(FibManager::value(24)))
print(inspect(FibManager::value(25)))
print(inspect(FibManager::value(24)))
print(inspect(FibManager::value(25)))
```

実行:

```bash
cargo run -q -p rune -- run examples/process/memoized_fib_workers/entry.srt
```

読みどころ:

- `FibManager` の state 自体が `(PID<FibWorker>, PID<FibWorker>)` です
- `@call` handler は reply 値と次 state をまとめて返します
- worker を直接並べるだけでなく、GenServer を前段に置いて routing や cache policy を集約できます

## Handler と supervisor_init

process は `handlers {}` で I/O 先のような dependency を宣言できます。利用側は普通の API を呼びつつ、起動時に handler を差し替えられます。

`examples/process/io_handler_switch` では、`Logger` が `ctx.out` に書きますが、`supervisor_init` 側で `StdOut` を `NullOutHandler` に差し替えています。

```surtr
defagent Logger {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
    handlers {
      out: OutHandler = StdOut
    }
  }
}

supervisor_init {
  singleton Logger {
    handlers {
      out: NullOutHandler
    }
  }
}
```

entry 側はただ API を呼ぶだけです。

```surtr
include "./Logger.srt"

print(inspect(Logger::log("this line is handled by NullOutHandler")))
print("logger output was suppressed")
```

実行:

```bash
cargo run -q -p rune -- run examples/process/io_handler_switch/entry.srt
```

読みどころ:

- default handler は process 定義側の `meta.handlers` に置きます
- 実行時の差し替えは `supervisor_init` 側で行います
- API 利用側は handler 実装を意識せず、`Logger::log(...)` だけを呼べます

## Examples

`examples/process/*` には、用途ごとに次の題材があります。

- `read_only_agent`: read-only singleton の最小形
- `state_agent_singleton`: `@set` を持つ singleton の最小形
- `agent_singleton_counter`: `Err(...)` で state が更新されないことを確認する例
- `agent_worker_multi`: worker と `PID<T>` の基本
- `task_call`: `Task::call(...)` の最小形
- `memoized_fib_workers`: GenServer と worker の協調
- `io_handler_switch`: `handlers {}` と `supervisor_init` の入口

どの例も、まず `entry.srt` を実行して挙動を見てから、隣の定義ファイルを読むと追いやすくなります。

## 関連ページ

- `include` の使い方は `./language-features.md`
- `Result` と `=?` の基本は `./error-handling.md`
- public surface 全体の位置づけは `./standard-library.md`
- runtime 契約の詳細は `../dev/ProcessRuntime_spec.md`
