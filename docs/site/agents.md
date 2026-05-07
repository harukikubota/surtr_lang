# Agents

Surtr の `defagent` は、状態を runtime 管理下へ押し上げながら、利用側では「型付き API を呼ぶ」形を保つための process surface です。

最初に押さえるポイントは次の 3 つです。

- singleton agent は `Type::pid()` で共有インスタンスへ入る
- multi agent は `Type::init(...)` で `PID<T>` を受け取る
- `get` / `set` はどちらも `PID<T>` を受け取る型付き API として使う

`import` を使うと、`@get` / `@set` で公開された concrete 関数名は通常の関数と同じように unqualified 参照できます。いっぽうで compiler-managed な `pid` / `spawn` や hidden lower helper は import できず、直接呼び出しもできません。

## 使い分け

- singleton:
  設定ストア、メトリクス集約、キャッシュのように「1つだけあればよい状態」に向いています。
- worker:
  リクエスト単位の処理、セッション単位の状態、ジョブ単位の作業のように「複数立ち上げたい状態」に向いています。

## サンプルの置き方

`surtr run` で直接実行する script には top-level `defagent` を置かず、agent 宣言を別ファイルへ切り出して `include` します。

```surtr
include "./Agents.srt"
```

この形にしておくと、entry script はそのまま実行用、`Agents.srt` は宣言用として整理できます。

## Singleton Example

ファイル:

- `examples/process/agent_singleton_counter/Agents.srt`
- `examples/process/agent_singleton_counter/entry.srt`

この例では `Counter` が 1 つだけ起動し、`pid()` で同じ process capability を受け取ります。

```surtr
include "./Agents.srt"

pid = Counter::pid()

print("pid: " ++ inspect(pid))
print(inspect(Counter::get(pid, "count")))
print(inspect(Counter::set(pid, 3)))
print(inspect(Counter::get(pid, "count")))
```

実行:

```bash
cargo run -q -p rune -- run examples/process/agent_singleton_counter/entry.srt
```

読み方:

- `Counter::pid()` は `PID<Counter>` を返します
- `Counter::get(pid, "count")` は現在状態を `Result<String>` で返します
- `Counter::set(pid, delta)` は状態更新の成功/失敗を `Result<()>` で返します
- `@set` が `Err(...)` を返した場合、state は更新されません

## Worker Example

ファイル:

- `examples/process/agent_worker_multi/Agents.srt`
- `examples/process/agent_worker_multi/entry.srt`

この例では `Worker` を 2 つ spawn し、PID ごとに独立した state を持つことを確認できます。

```surtr
include "./Agents.srt"

alpha =? Worker::init(3)
beta =? Worker::init(7)

print(inspect(Worker::get(alpha, "jobs")))
print(inspect(Worker::get(beta, "jobs")))
print(inspect(Worker::set(alpha, 1)))
print(inspect(Worker::get(alpha, "jobs")))
```

実行:

```bash
cargo run -q -p rune -- run examples/process/agent_worker_multi/entry.srt
```

読み方:

- `Worker::init(3)` は `Result<PID<Worker>>` を返します
- `=?` を使うと `Ok(pid)` を束縛しつつ、`Err(...)` はそのまま返せます
- `alpha` と `beta` は別 PID なので、`set` しても state は混ざりません
- `init` や `set` の失敗は panic ではなく `Result` として観測します

## Singleton と Worker の選び方

- 同じ状態を全体で共有したいなら singleton
- 呼び出しごとに独立 state を持たせたいなら worker
- まず singleton で API を固め、必要になったら worker 化する、という切り方もできます

## いまの注意点

- process API は `PID<T>` で型付けされるため、別 agent の PID を混ぜると compile error になります
- `set` は利用側から見ると `Result<()>` です。内部 state 自体は返しません
- `Task::call` / `Task::async` / `Task::launch` / `Task::cast` は別の task surface で、agent の PID 管理とは役割が異なります
- `Task` 系では開始と待機を分けます。非同期開始は `Task::async(...)`、待機は `Task::await(task)` を使います
- timeout を付ける場合は、開始側ではなく待機側の call に後置します

## 関連ページ

- `include` の使い方は `./language-features.md`
- `Result` と `=?` の基本は `./error-handling.md`
