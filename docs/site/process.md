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

`defagent` / `defgenserver` の `meta` には `instance`、`init_policy`、`state` を明記します。`init_policy` を省略すると構文エラーになります。`Eager` は1回の初期化で状態を確定し、`Standby` は Singleton の Agent / GenServer で Ready になるまで初期化を続けます。Worker には `Eager` を指定します。

## 関数の可視性

`defagent` / `defgenserver` 内では、handler を `def` とアノテーションで宣言します。外部からは、コンパイラが生成する公開 API を呼びます。handler 本体へ state を渡して直接呼ぶことはできません。

内部 helper は `defp` で宣言します。`defp` は同じプロセス定義内からのみ参照でき、handler アノテーションは付けられません。handler アノテーションなしの `def` はエラーになり、`defp` への変更が案内されます。

テストでも同じ公開 API を使います。計算部分を単独でテストしたい場合は、通常モジュールの公開関数へ切り出し、プロセス内からその関数を呼びます。

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

同じ process 型の PID は `==` / `!=` で比較できます。singleton の PID は同じ型なら常に等しく、worker の PID は同じ個体を指すときだけ等しくなります。handler 用の PID は比較対象外です。

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

print(inspect(FibManager::value(10)))
print(inspect(FibManager::value(11)))
print(inspect(FibManager::value(10)))
print(inspect(FibManager::value(11)))
```

実行:

```bash
cargo run -q -p rune -- run examples/process/memoized_fib_workers/entry.srt
```

出力:

```text
Ok(("miss-even", 55))
Ok(("miss-odd", 89))
Ok(("hit-even", 55))
Ok(("hit-odd", 89))
```

ビルド済みの開発環境では、4行の出力まで1〜5秒が目安です。2026-10-03 の確認では、上のコマンドが1.34秒で終了しました。初回ビルドの時間は別です。

計算は `FibManager::slow_fib` が担当し、worker は計算済みの `n` の値を検索・保存します。再帰の中間値はキャッシュしないため、大きい入力では最初の出力まで時間がかかります。この例は偶奇での振り分けと cache miss / hit の確認を目的に、入力を10と11にしています。

読みどころ:

- `FibManager` の state 自体が `(PID<FibWorker>, PID<FibWorker>)` です
- `@call` handler は reply 値と次 state をまとめて返します
- worker を直接並べるだけでなく、GenServer を前段に置いて routing や cache policy を集約できます

## Worker の停止と呼出し失敗

Worker GenServer の handler は `CallResult::Stop(...)` / `CastResult::Stop(...)` で停止を要求できます。Stop の応答を受け取った時点で新しい要求の受付は閉じていますが、すでに開始した処理の終了までは保証しません。停止前から sleep / future / I/O を待つ処理は再開し、状態保存や返答まで進みます。終了を待つ Worker 用の join / await API はありません。

停止要求後の同じ PID への新しい call / cast は `Err(ProcessStopped(...))` です。capture、高階関数、Workers、lease を経由しても同じ kind を返し、handler は実行しません。古い PID は保持できますが、新しい個体へ自動的に転送されません。

たとえば、次の Worker 定義を `Session.srt` に置きます。

```surtr
defgenserver Session {
  meta {
    instance: Worker
    init_policy: Eager
    state: Int
  }

  @init
  def init(seed: Int) -> Result<Int> {
    Ok(seed)
  }

  @call
  def value(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Reply(state, state))
  }

  @call
  def stop(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Stop(StopReply::Normal(state)))
  }
}
```

呼出し側では message の本文ではなく `Error::kind` で停止を判定します。この例は停止を検知したときだけ新しい PID を作り、それ以外の Error はそのまま返します。

```surtr
include "./Session.srt"

def value_or_replace(pid: PID<Session>) -> Result<(PID<Session>, Int)> {
  match Session::value(pid) {
    Ok(value) => Ok((pid, value)),
    Err(error) => match Error::kind(error) {
      "ProcessStopped" => {
        replacement =? Session::init(0)
        value =? Session::value(replacement)
        Ok((replacement, value))
      },
      _ => Err(error),
    },
  }
}

pid =? Session::init(7)
_ =? Session::stop(pid)
(current, value) =? value_or_replace(pid)
print(inspect((pid == current, value)))
// (False, 0)
```

PID の差し替えが不要なら、通常どおり `value =? Session::value(pid)` で元の Error を伝播できます。呼出し側も Worker GenServer なら、受け取った Error を `CallResult::Stop(StopReply::Error(error))` や `CastResult::Stop(StopReason::Error(error))` へ渡して自身の停止理由にできます。runtime が caller を自動停止させることはありません。

call の `StopReply::Normal(reply)` は `Ok(reply)`、`StopReply::Error(error)` は元の `Err(error)` を返します。cast の Stop は Normal / Error ともに `Ok(())` を返し、Error は終了理由になります。cast handler 自身の `Err` や停止済み宛先への拒否は公開 API の `Result` で受け取ります。Stop は Worker GenServer で使い、singleton と init では使えません。

call の `@timeout` は caller の待機結果を `FutureDeadlineExceeded` にします。すでに開始した handler や ReplyLater callback はその後も進むため、timeout を見て同じ副作用の要求を再送する場合は、先の処理が後で完了することを考慮してください。ReplyLater の遅延結果は timeout 結果を上書きしません。通常 Stop は callback の終了も待ち、`shutdown_timeout` を指定しても開始済み処理を打ち切りません。

Workers は停止要求中の個体を新しい割当から外します。`Workers::size` と supervisor の child count は停止完了までその個体を含み、完了後に所属を解除して補充します。選択できる個体がないと `submit` / `reserve` は `WorkersUnavailable`、`broadcast` は空 list を返します。既存 lease も停止済み個体へ新しい要求を送ると ProcessStopped になり、補充個体へ自動的に差し替わりません。

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
  Logger {
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
