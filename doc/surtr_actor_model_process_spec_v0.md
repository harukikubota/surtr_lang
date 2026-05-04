# Surtr アクターモデル・プロセス仕様書 v0

> 本書は `要件定義書 V9` の補助仕様であり、Surtr にアクターモデル由来のプロセス基盤を追加するための外部契約を定義する。  
> 初期フェーズではランタイム改修量を抑えつつ、型とメタ情報によって制御責務を押し上げ、ユーザコードを関数に近い値変換として保つことを目的とする。

---

## 1. 位置づけ

### 1.1 本書の役割

`要件定義書 V9` は、現行の言語仕様と処理系の外部契約を定義する。  
本書はその上に、将来導入するプロセス実行基盤の最小仕様を追加する。

本書が扱う範囲は次の通り。

- プロセス型の外部契約
- 型メタ情報と runtime spec への lowering
- Task / Agent / Supervisor の最小仕様
- timeout / pending / resume の境界
- メッセージング surface と内部 lower 規則

本書は Rust 実装詳細を固定しない。  
ただし、コンパイラが保証すべき制約と、VM / scheduler が見るべき spec は固定する。

### 1.2 設計目的

Surtr におけるプロセス導入の目的は、自由な並行制御の提供ではない。  
次の 2 点を優先する。

1. なるべく型で制御すること
2. 型の状態遷移を単純に保ち、関数と同レベルで融合できること

言い換えると、Surtr のプロセスは Erlang 的な自由な `receive` 中心モデルではなく、
**型が意味論を固定し、runtime が制御責務を吸収する actor 基盤**として設計する。

---

## 2. 基本原則

### 2.1 ロジックと制御の分離

プロセスは状態・待機・監視・再起動・timeout などの制御責務を持つ。  
しかし Surtr のユーザコードは、できるだけ通常の関数変換として読めるべきである。

したがって次を原則とする。

- ロジックは関数として記述する
- 制御は型メタ・runtime spec・VM / scheduler に寄せる
- `Future<A>` や `Pending` を通常のユーザ値として広く公開しない
- timeout / process down は例外ではなく `Result` の `Err` として扱う

### 2.2 型意味論の優先

プロセス関連型は、単なる `Struct` の変種ではない。  
プロセス意味論を持つ型は、runtime 管理下でのみ成立する特別な存在として扱う。

このため、プロセス型は次の性質を持つ。

- 生成経路が限定される
- `PID<T>` は runtime が発行する capability である
- 型に付属するメタ情報から supervisor / registry / messaging 契約が一意に導出される
- ユーザコードは process lifecycle の細部を通常の分岐として背負わない

### 2.3 strict evaluation を守る

プロセス呼び出しが内部的に非同期実行になり得ても、
**ソース上の評価順序は変えない**。

- 引数評価は左から右へ strict に行う
- 値が未確定なら、その時点で現在プロセスを `Pending` にする
- 後続引数は評価しない
- 並列化したい場合のみ `Task` API で明示する

### 2.4 初期フェーズは制度化を優先

初期フェーズでは自由度を下げる。

- `receive` は公開しない
- user-defined Supervisor は後段で導入する
- 動的 registry は導入しない
- 動的 link / monitor surface は後段で導入する
- メッセージング surface は関数に近い形へ制限する

目的は、後から自由度を足しやすい安定した基盤を作ることである。

---

## 3. 初期フェーズの対象

### 3.1 導入対象

初期フェーズで導入するプロセス種別は次に限定する。

- `Supervisor`
- `Task`
- `ReadOnlyAgent`
- `StateAgent`

### 3.2 後段対象

次は後段導入とする。

- `GenServer`
- user-defined `Supervisor`
- 汎用 `defmessage`
- dynamic registry
- dynamic link / monitor surface
- `TaskRef` の高機能 API 群
- `pid.message(...)` の sugar

### 3.3 `receive` 非公開

初期フェーズでは `receive` を言語 surface に導入しない。

理由:

- 呼ばれた関数が待機制御に関する知識を持ってしまう
- mailbox 制御がロジックに漏れる
- 型と runtime spec による一意な意味論が崩れる

受信は常に runtime が hidden message dispatch として行う。

---

## 4. 型と runtime spec の関係

### 4.1 メタをそのまま VM に渡さない

プロセス型には source-level のメタ情報が付く。  
ただし VM が読むのは raw メタではなく、**コンパイラが正規化した runtime spec** である。

責務分離:

- source / user code: 関数本体・annotation・型メタ
- compiler: 型検査、整合性検査、runtime spec 生成
- VM / scheduler: runtime spec に基づいて実行

### 4.2 lowering の流れ

概念上のフェーズは次の通り。

```text
Source
-> Ast
-> Resolved
-> Typed
-> ProcessMeta checked
-> RuntimeProcessSpec normalized
-> Bytecode + ProcessSpecTable
-> Execute
```

### 4.3 ProcessMeta と RuntimeProcessSpec

#### ProcessMeta

型定義上の宣言情報。コンパイラ検査用。

例:

- process kind
- singleton / multi
- registry 公開可否
- boot 対象か
- lazy 初期化か
- timeout 既定値
- 公開 message 一覧
- spawn 可否
- direct access 可否

#### RuntimeProcessSpec

VM / scheduler が読む正規化済み情報。

例:

- runtime process kind
- init / get / set handler の function id
- registry slot
- singleton slot
- init / call timeout の既定値
- owner policy
- reply / cast / launch の mode
- root supervisor boot 順序

---

## 5. 主要型

### 5.1 `PID<T>`

`PID<T>` は runtime が発行する process capability である。

性質:

- `T` は concrete process type に固定される
- `PID<T>` を持てる時点で、その process instance は生成・初期化済みとみなす
- `PID<T>` は通常の構造体として直接生成できない
- `PID<T>` は message call の静的検査に用いる

### 5.2 `TaskRef<A>`

`TaskRef<A>` は explicit な非同期制御が必要な場合だけ導入する opaque handle である。  
初期フェーズでは内部基盤を優先し、surface 露出は最小に留める。

### 5.3 hidden message enum

ユーザ向け surface では message を関数として見せる。  
内部では compiler が hidden message enum を生成して dispatch する。

例:

```text
ConfigStore$Msg::Get(key)
ConfigStore$Msg::Set(cmd)
```

ユーザはこれを直接構築しない。

---

## 6. プロセス種別

## 6.1 Root `Supervisor`

初期フェーズの `Supervisor` は root process として固定する。

役割:

- boot 対象 process の起動
- singleton slot / registry slot の初期化
- child process の監視
- restart の最小責務

初期フェーズでは:

- user-defined supervisor tree は導入しない
- 起動オプションは最低限に限定する
- restart policy は runtime の固定既定値でよい

### 6.1.1 boot 対象

`boot = true` を持つ singleton process は root `Supervisor` の boot 対象である。

不変条件:

- boot 完了後にのみ singleton は公開される
- singleton へ到達できる時点で、その process は起動済み・初期化済みとみなす
- eager singleton の init failure は通常コードの `Result` にせず、boot failure として扱う

---

## 6.2 `Task`

`Task` は単発計算・並列実行・重い処理の分離用 primitive である。

初期フェーズの surface:

- `Task.call`
- `Task.async`
- `Task.cast`
- `Task.launch`

意味:

- `call`: 呼び出し側が待機し、結果を `Result` として受け取る
- `async`: 非同期開始。明示 handle を返す段階は後段で強化する
- `cast`: fire-and-forget 的送信
- `launch`: detached に近い起動

timeout は呼び出し側が指定できる。  
`Task` は呼び出し側完結のため、timeout policy は call site 起点でよい。

---

## 6.3 `ReadOnlyAgent`

`ReadOnlyAgent` は、外部から読むだけの singleton state service である。

用途例:

- 環境変数
- logger 設定
- 標準入出力参照先
- boot 時に決まる設定値

特徴:

- 初期フェーズでは singleton のみ
- 外部公開 message は `get` のみ
- 内部実装はキャッシュ・lazy load を持ってよい
- 外部からは mutable に見えない

`ReadOnlyAgent` は外部 API を増やさない。  
外部インターフェースを増やしたくなった時点で `GenServer` 相当の導入動機とする。

---

## 6.4 `StateAgent`

`StateAgent` は、単一状態に対して `get` / `set` を持つ制限付き state process である。

特徴:

- singleton と multi の両方を許可する
- 外部公開 message は `get` / `set` のみ
- `set` は `Result<State>` により状態遷移失敗を表現できる
- surface では `Result<()>` を返し、内部 state を外へ漏らさない

`StateAgent` は複数 message を持たない。  
`reset`, `flush`, `subscribe`, `append` など複数の外部操作を持ちたくなったら `GenServer` へ移行する。

---

## 7. 生成と所有

### 7.1 生成経路の限定

プロセス生成は自由にしない。  
生成方法ごとに意味を固定する。

#### singleton

- root `Supervisor` が boot で起動する
- registry に公開されるかどうかは型メタで決まる
- eager singleton は boot 完了時点で初期化済み

#### multi

- `spawn` によって生成する
- owner は main 相当または管理プロセス
- owner なしの orphan process は初期フェーズでは作らない

### 7.2 初期化保証

#### singleton

- singleton にメッセージングできる時点で初期化済み
- eager init failure は boot failure

#### spawned process

- `spawn` は init 成功後にだけ `PID<T>` を返す
- よって `PID<T>` を保持できるなら、その process は初期化済み

### 7.3 worker 運用

worker は main 相当または worker manager process が所有する。  
監視・再起動・復帰は VM / `Supervisor` の責務であり、通常のロジックコードへ漏らさない。

---

## 8. registry

### 8.1 core registry は compiler-managed

初期フェーズの registry は Erlang 的な動的名前解決機構ではない。  
**型定義から許可された singleton への compiler-managed 参照制度**として扱う。

### 8.2 制約

- registry 公開可能なのは `singleton` のみ
- `multi` に対する registry 公開は compile error
- registry key は user が自由に作らない
- registry 参照の返り値は常に `PID<ConcreteProcess>`

### 8.3 動的 registry は後段

文字列 / atom 類似キーによる動的 lookup は初期フェーズで導入しない。  
必要なら後で library / process layer として別導入する。

---

## 9. surface 構文

## 9.1 設計方針

メッセージングは `send(pid, EnumVariant)` を surface の正規形にしない。  
ユーザが使う API は、通常の関数呼び出しに近い形で固定する。

理由:

- message をデータ変換に近いシグネチャで見せたい
- 関数合成・pipeline に自然に接続したい
- enum 直接送信は内部表現としてはよいが surface として弱い

## 9.2 正規形

初期フェーズの正規形は次とする。

```text
Type::message(pid, arg1, ..., argN) -> Result<Reply>
```

例:

```surtr
ConfigStore::get(pid, key) -> Result<String>
ConfigStore::set(pid, cmd) -> Result<()>
```

### 9.2.1 ReadOnly singleton の direct access

`ReadOnlyAgent` の singleton は、pid を surface に出さず direct call を許可する。

```surtr
Env::get("HOME")
```

これは内部で hidden singleton slot を解決して lower される。

## 9.3 将来 sugar

将来は次の sugar を追加できる。

```surtr
pid.get(key)
pid.set(cmd)
```

ただし初期フェーズの正規形は `Type::message(pid, ...)` とする。

## 9.4 capture / partial

message 関数は関数に近い surface を持つが、通常関数と完全同一ルールにはしない。  
初期フェーズでは、message contract 自体は通常の関数型として扱えるようにする。

例:

```text
&ConfigStore::get
  : (PID<ConfigStore>, String) -> Result<String>
```

timeout は関数引数に含めない。  
timeout を含めると arity と capture の意味が崩れるためである。

必要なら後段で、message partial 専用 sugar を導入する。

例:

```surtr
ConfigStore::get(pid, _)
```

これを

```text
(String -> Result<String>)
```

として扱えるようにする。

---

## 10. timeout

### 10.1 timeout は引数ではなく call policy

Surtr は optional 引数や overload を持たない。  
このため timeout を通常の関数引数に混ぜると、message arity が崩れる。

したがって timeout は通常引数ではなく、**call expression に付く修飾子**として扱う。

概念例:

```surtr
ConfigStore::get(pid, key) @timeout(100ms)
```

これは関数引数の一部ではなく、その呼び出しの待機条件である。

### 10.2 timeout の優先順位

初期フェーズでは次の優先順位を採用する。

```text
call-site timeout override
> message / handler default
> process default
> runtime default
```

### 10.3 timeout を PID の可変状態にしない

`PID.set_timeout()` のような mutable API は導入しない。

理由:

- `PID` が共有された時に競合する
- timeout は process の性質より call の性質に近い
- `PID` は capability に留める方が型意味論が明確

### 10.4 timeout の確定

timeout は例外ではなく `Result::Err` として確定させる。

最低限の失敗種別:

```surtr

defenum ProcessCallError {
  Timeout,
  Cancelled,
  ProcessDown,
  InvalidPid,
}
```

後段で `MailboxFull` 等を追加してよい。

---

## 11. `ReadOnlyAgent` の定義

### 11.1 宣言形式

初期フェーズでは `defagent` 1 形式に統一し、違いはメタで切る。

```surtr
@agent(
  kind: ReadOnly,
  instance: Singleton,
  boot: true,
  registry: false,
  lazy: false,
)
defagent Env {
  @init
  def init() -> Result<State>

  @get
  def get(state: State, arg1: A1, ..., argN: AN) -> Result<R>

  defp helper(...) -> ...
}
```

### 11.2 必須要素

`ReadOnlyAgent` では次を必須とする。

- `@init`
- `@get`

`@set` は不許可。

### 11.3 compile-time 制約

- `kind = ReadOnly`
- `instance = Singleton`
- `@set` 禁止
- `registry = true` は原則不要
- `boot = true` を許可
- `lazy = true | false` を許可

### 11.4 external API

surface では `Type::get(...)` のみを公開する。

```surtr
Env::get("HOME") -> Result<String>
```

### 11.5 lazy read-only

lazy read-only は **process の遅延起動** ではなく、**内部状態の遅延初期化** として扱う。

- process 自体は boot 時に生成・登録する
- 初回 `get` 時にのみ内部値を初期化する
- lazy 初期化失敗は `get` の `Err` として観測しうる

runtime 内部状態の概念:

```text
Uninitialized
Ready(State)
Failed(Error)
```

これは surface 型ではなく runtime 内部状態である。

---

## 12. `StateAgent` の定義

### 12.1 宣言形式

```surtr
@agent(
  kind: State,
  instance: Singleton | Multi,
  boot: true | false,
  registry: true | false,
  lazy: false,
)
defagent ConfigStore {
  @init
  def init(...) -> Result<State>

  @get
  def get(state: State, arg1: A1, ..., argN: AN) -> Result<R>

  @set
  def set(state: State, input: In) -> Result<State>

  defp helper(...) -> ...
}
```

### 12.2 必須要素

`StateAgent` では次を必須とする。

- `@init`
- `@get`
- `@set`

### 12.3 compile-time 制約

- `kind = State`
- `lazy = false`
- `registry = true` は `instance = Singleton` のときのみ許可
- `boot = true` は `instance = Singleton` のときのみ許可
- `@init` 戻り値の状態型と `@get` / `@set` 第1引数型は一致必須

### 12.4 external API

#### singleton + registry

```surtr
pid = ConfigStore::pid()
value = ConfigStore::get(pid, key)
_ =? ConfigStore::set(pid, cmd)
```

#### multi

```surtr
pid = Session::spawn(user_id)
value = Session::get(pid, key)
_ =? Session::set(pid, cmd)
```

### 12.5 `@set` 契約

`@set` は内部では純粋な状態遷移関数に近い形を取る。

```surtr
@set

def set(state: State, input: In) -> Result<State>
```

意味:

- `Ok(next_state)` のとき runtime は state を更新する
- `Err(error)` のとき runtime は state を変更しない

surface の `Type::set(pid, input)` は `Result<()>` を返す。

```surtr
Type::set(pid, input) -> Result<()>
```

これにより:

- 実装側は新状態を返せる
- 利用側は成功 / 失敗だけ見ればよい
- 内部 state 構造を漏らさない

### 12.6 `Result` の未使用

`Result` の未使用強制は初期フェーズでは導入しない。  
ただし API は失敗可能性を `Result<()>` で明示する。  
将来 lint で補強してよい。

---

## 13. messaging と hidden lowering

### 13.1 user-facing surface

ユーザは message を関数として見る。

例:

```surtr
ConfigStore::get(pid, "theme")
ConfigStore::set(pid, ConfigCmd::Put("theme", "dark"))
```

### 13.2 internal lowering

compiler は hidden message enum を生成する。

```text
ConfigStore$Msg::Get("theme")
ConfigStore$Msg::Set(ConfigCmd::Put("theme", "dark"))
```

surface call は内部で次へ lower される。

```text
send(pid, ConfigStore$Msg::Get("theme"))
send(pid, ConfigStore$Msg::Set(...))
```

### 13.3 `send(pid, EnumVariant)` を正規形にしない理由

- enum 直接送信は message 表現を露出しすぎる
- API として関数合成との接続が弱い
- message 定義変更が call site の値構築へ波及しやすい

したがって enum variant 送信は内部表現とし、surface は関数 API に固定する。

---

## 14. init と failure の扱い

### 14.1 eager singleton

- singleton が surface から到達可能なら初期化済み
- boot 時 `@init` failure は通常コードに流さず、boot failure とする

### 14.2 spawned process

- `spawn` は init success 後にだけ `PID<T>` を返す
- init failure は `spawn` の `Err` としてのみ観測する

### 14.3 lazy read-only

- process は存在しても内部 state は `Uninitialized` でよい
- 初回 `get` でだけ初期化 failure を観測しうる

### 14.4 runtime failure と domain failure

初期フェーズでは surface 上 1 つの `Result` に畳んでよい。  
ただし意味論上は次を区別する。

- domain failure: `@get` / `@set` / handler の失敗
- runtime failure: `Timeout`, `ProcessDown`, `InvalidPid`

---

## 15. timeout・pending・resume

### 15.1 `Future<A>` は通常公開しない

`Future<A>` は VM / scheduler 内部の実行状態である。  
通常のユーザコードに広く露出しない。

ユーザから見える通常値は:

- `A`
- `Result<A, E>`
- 必要時のみ `TaskRef<A>`

### 15.2 `Pending` はユーザ値ではない

`Pending` は `Result` の `Err` ではなく VM step 結果である。

概念的には:

```text
Continue | Halt(Value) | Pending(FutureId, Continuation) | RuntimeError
```

### 15.3 timeout は起動時に固定する

timeout は消費時指定ではなく、task / process call の起動時に deadline へ変換して固定する。

### 15.4 timeout は例外ではない

timeout は `Result::Err` として確定する。

### 15.5 消費時 demand

内部的に未確定値を使う時点で demand する。

- Ready: そのまま続行
- Pending: 現在 process を waiting にして scheduler へ返す
- Timeout / Cancelled / ProcessDown: `Result::Err` として確定

---

## 16. strict evaluation と関数融合

プロセス呼び出しが内部的に `Pending` し得ても、
ユーザから見える API は通常の `Result` を返す関数に近い形を保つ。

例:

```surtr
input
  |> FileWorker::read(pid)
  |> parse()
  |> validate()
```

`read` が内部的に `Pending` しても、確定後は `Result<String, FileError>` として扱える。  
このため演算子は `Future<Result<...>>` を意識しなくてよい。

評価順序は strict に固定する。

```surtr
fun(a(), b())
```

- `a()` を先に評価する
- `a()` が `Pending` なら、その場で停止する
- `b()` はまだ評価しない
- `a()` が Ready になってから `b()` を評価する

これにより、プロセス呼び出しが関数レベルの直観を崩さない。

---

## 17. timeout の設定位置

### 17.1 `Task`

`Task` は呼び出し側完結のため、call site が timeout を指定する。

### 17.2 `Agent` / 将来の `GenServer`

`Agent` / `GenServer` は定義側が default を持ち、必要なら call site が override する。

優先順位:

```text
call-site timeout
> handler default
> process default
> runtime default
```

### 17.3 `PID` に mutable timeout を持たせない

共有 `PID` に対する `set_timeout()` は導入しない。  
必要なら将来、immutable wrapper で `with_timeout(pid, d)` を導入してよいが、初期フェーズでは不要。

---

## 18. compile-time 制約まとめ

初期フェーズでは少なくとも次を compile error にする。

- `ReadOnlyAgent` に `@set` がある
- `ReadOnlyAgent` が `Multi` である
- `StateAgent` に `@set` がない
- `registry = true` かつ `instance = Multi`
- `boot = true` かつ `instance = Multi`
- `lazy = true` かつ `kind = State`
- `PID<T>` に対して未公開 message を呼ぶ
- direct access 非対応型に対して pid なし call を行う

---

## 19. RuntimeProcessSpec の最小形

初期フェーズの VM が見る spec は、次程度で十分である。

```text
RuntimeProcessSpec =
  SupervisorSpec
  | TaskSpec
  | ReadOnlyAgentSpec
  | StateAgentSpec
```

### 19.1 `ReadOnlyAgentSpec`

- `type_id`
- `boot`
- `lazy`
- `init_fn`
- `get_fn`
- `singleton_slot`

### 19.2 `StateAgentSpec`

- `type_id`
- `instance_mode`
- `boot`
- `registry_exposed`
- `init_fn`
- `get_fn`
- `set_fn`
- `singleton_slot?`
- `registry_slot?`
- `owner_policy`

### 19.3 `TaskSpec`

- entry closure / function id
- mode (`call`, `async`, `cast`, `launch`)
- owner pid
- timeout default

### 19.4 `SupervisorSpec`

- root flag
- boot children list
- restart policy

---

## 20. 初期フェーズの推奨 surface 例

### 20.1 `ReadOnlyAgent`

```surtr
@agent(kind: ReadOnly, instance: Singleton, boot: true, lazy: true)
defagent Env {
  @init
  def init() -> Result<HashMap<String>> {
    ...
  }

  @get
  def get(state: HashMap<String>, key: String) -> Result<String> {
    HashMap::get(state, key)
  }
}

home = Env::get("HOME")
```

### 20.2 singleton `StateAgent`

```surtr
@agent(kind: State, instance: Singleton, boot: true, registry: true)
defagent ConfigStore {
  @init
  def init() -> Result<ConfigState> {
    ...
  }

  @get
  def get(state: ConfigState, key: String) -> Result<String> {
    ...
  }

  @set
  def set(state: ConfigState, cmd: ConfigCmd) -> Result<ConfigState> {
    ...
  }
}

pid = ConfigStore::pid()
value = ConfigStore::get(pid, "theme")
_ =? ConfigStore::set(pid, ConfigCmd::Put("theme", "dark"))
```

### 20.3 multi `StateAgent`

```surtr
pid = Session::spawn(user_id)
value = Session::get(pid, "token")
_ =? Session::set(pid, SessionCmd::Refresh)
```

---

## 21. V9 との整合

本書の設計は、少なくとも次の V9 方針と整合する。

- 失敗を型で表現する
- コンパイラとランタイムをシンプルに保つ
- `Result` を主たる失敗表現とする
- `Pending` をユーザ値にしない
- function / pipeline に馴染む surface を優先する
- hidden metadata と lowering を compiler が吸収し、runtime は spec を読む

---

## 22. 将来拡張

基盤安定後に追加しうるもの:

- `GenServer`
- user-defined `Supervisor`
- `pid.get(...)` sugar
- message partial sugar
- `TaskRef` の明示 surface
- `Task.join2`, `Task.all`, `Task.race`
- dynamic registry
- dynamic link / monitor surface
- timeout policy の上限 / 下限
- message mode (`sync`, `async`, `cast`) の拡張

---

## 23. 仕様要約

Surtr のプロセスは、自由な mailbox 制御を許す actor ではなく、
**型とメタで意味論を固定し、runtime が制御責務を吸収する actor 実行基盤**である。

初期フェーズでは:

- `Supervisor`, `Task`, `ReadOnlyAgent`, `StateAgent` のみを導入する
- `receive` は公開しない
- message surface は関数呼び出しに近い形へ固定する
- timeout は call policy として扱い、通常引数に混ぜない
- `Future` / `Pending` は runtime 内部状態に閉じ込める
- `PID<T>` は型付き capability として扱う
- singleton / registry / boot / lazy / state transition は型メタから runtime spec へ lower する

この設計により、Surtr のプロセスは関数型の読みやすさを保ちながら、
監視・再起動・timeout・pending を runtime 側へ押し上げられる。

