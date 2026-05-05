# Process Runtime 次段ゴールと不測項目

## 1. v2 範囲完全実装

### ゴール

- process runtime の正本仕様 `docs/dev/ProcessRuntime_spec.md` に対して、VM / compiler / stdlib の挙動差分を埋める。
- `defagent` / `defgenserver` / `defsupervisor` / `supervisor_init` / handler override / singleton boot / lazy init / timeout の主要経路を、仕様どおりに動く状態へ寄せる。
- 「暫定的に動く」ではなく、「仕様上ある機能は通常テストで固定できる」状態まで進める。

### 不測項目

- scheduler の基盤不足が先に露出する可能性がある。
  - `Process::sleep`, task completion, lazy init retry, call timeout はどれも `waiting_table`, `deadline_queue`, `Pending` / resume と結びつく。
  - これらが個別実装のままだと後続 API を足すたびに分岐が増えるため、v2 完全実装の途中で scheduler 共通経路を先に整理する必要が出る。
  - 判断基準:
    - `sleep`, lazy init, task timeout がそれぞれ別コードで wakeup しているなら、WorkerAPI 着手前に共通化する。
    - `Pending` が top-level / REPL / nested call で同じ resume 契約を持たないなら、そこで一度止めて VM 契約を揃える。
- process spec の compiler-to-VM 経路が不足する可能性がある。
  - parser で受理できても、Sigil / Scar / Forge / Sindr を通る間に metadata が落ちると、runtime で特別扱いが増えて設計が崩れる。
  - `RuntimeProcessSpec`, `RuntimeBootPlan`, handler metadata を bytecode に一貫して乗せることが前提になる。
  - 判断基準:
    - runtime 側で source syntax 依存の分岐が増え始めたら、先に metadata 経路を整備する。
- singleton boot と lazy init が先に複雑化する可能性がある。
  - Ready 前 call の待機、init timeout、retry policy、boot failure 集約が別々に入ると挙動説明が難しくなる。
  - Root boot 完了条件と singleton slot 公開条件を先に固定し、Ready 前 message の扱いを `init_waiters` に統一する必要が出る。
  - 判断基準:
    - singleton call が「即失敗」「暗黙 retry」「待機」の複数動作を取り始めたら、boot/lazy contract を先に詰める。
- 大量 process 前提の安全性が未達のまま先へ進みすぎる可能性がある。
  - mailbox limit, backpressure, process reaping, stats/dump が未整備だと、後続機能追加後に不具合の切り分けが難しい。
  - hobby language 前提でも、最低限「詰まった時に見える」「timeout した理由が追える」は先に必要になる。
  - 判断基準:
    - process example や integration test が増え、失敗時に waiting / deadline / owner が追えない場合は、先に観測系を足す。

## 2. WorkerAPI 追加

### ゴール

- `spawn` / ownership / lifecycle sink / `await` / `on_down` を generic `receive` なしで完結させる。
- 関数コールに近い温度感を保ったまま、worker を「動的に増える process」として扱えるようにする。
- user code に scheduler 責務を漏らさず、runtime が worker lifecycle を管理する。

### 不測項目

- `DynamicSupervisor` の責務を先に実装しないと API を固定できない可能性がある。
  - `Worker::spawn` だけ先に作ると、後で `DynamicSupervisor::spawn` と ownership / restart の意味が競合しやすい。
  - 先に「current process owner」「supervisor owner」「adopt / handoff の可否」を runtime schema に入れてから surface を決めた方が安全。
  - 判断基準:
    - spawn 後の owner が `None` のまま残る経路があるなら、API 追加前に owner model を直す。
- `await` / `on_down` の意味論が task await と衝突する可能性がある。
  - worker completion, task completion, init failure, runtime fault を同じ `await` に寄せるのか、用途別 API に分けるのかを先に決める必要がある。
  - `receive` を出さない方針なので、「何を待つ API か」を曖昧にすると呼び出し側が複雑になる。
  - 判断基準:
    - worker 終了待ちと task 結果待ちで戻り値や timeout の形が揺れるなら、surface 追加前に API 分離を決める。
- `adopt` / `handoff` は後回しでも runtime hook は先に要る可能性がある。
  - user-facing API としては未公開でも、owner を原子的に付け替える内部経路がないと supervisor 管理へ発展できない。
  - そのため public API の前に runtime intrinsic として入れる段階が必要になるかもしれない。
  - 判断基準:
    - supervisor 配下へ worker を移したいケースが出た時点で、表面 API ではなく内部 hook を先に追加する。
- `yield` を入れない前提なので fairness は runtime 側で担保する必要がある。
  - heavy worker が他 process を止めないよう、timeout / sleep / await / pending call 以外の切替条件をどこまで VM が持つかを決める必要がある。
  - ここは surface を増やさずに step budget や scheduler quantum で吸収する方が方針に合う。
  - 判断基準:
    - 重い worker のテストで他 process が全く進まない場合は、`yield` 追加ではなく runtime 側の切替条件を検討する。

## 3. FileIO 追加

### ゴール

- handler / capability 経由でファイル入出力を扱えるようにする。
- process runtime の I/O 差し替え方針を保ち、標準 I/O と同じく runtime 管理下で File 系 handler を扱えるようにする。
- user code では「ファイルに対する関数 call」に近い surface を保ち、host 依存や OS 例外は boundary で吸収する。

### 不測項目

- capability だけでは API が閉じない可能性がある。
  - open / read / write / append / flush / close / path error / permission error を全部 `OutHandler` 風に押し込むと、責務が曖昧になる場合がある。
  - File sink を「write 専用 logger 系 process」と「一般 file access」に分ける必要が出るかもしれない。
  - 判断基準:
    - append-only と random access を同一 API で表そうとして型やエラーが膨らむなら、handler 種別を分ける。
- host error の正規化が先に必要になる可能性がある。
  - path not found, permission denied, already exists, invalid encoding などをそのまま runtime error にすると user code の recover が難しい。
  - process runtime で扱う以上、domain error / runtime error / host failure の線引きを早めに決める必要がある。
  - 判断基準:
    - `Result` で返すべき失敗と VM 継続不能な失敗の境界が曖昧になったら、FileIO 実装を止めて boundary 契約を先に詰める。
- blocking I/O と scheduler の相性が問題になる可能性がある。
  - hobby language 前提でも、長い file read/write が VM 全体を止めると process runtime 方針と相性が悪い。
  - すべて非同期にする必要はないが、「どこまで block を許すか」は決めておかないと後で Logger や Env に波及する。
  - 判断基準:
    - file access 中に timeout / sleep / 他 process が全く進まないなら、I/O 専用 process か host adapter 分離を検討する。

## 4. Logger 追加

### ゴール

- stdout/stderr 直結ではなく、差し替え可能な runtime handler / process として Logger を入れる。
- 開発時は標準出力、テスト時は buffer、将来的には file sink などへ差し替えられる logging 基盤を整える。
- user code では logger を「呼ぶだけ」にし、ordering や flush の責務は runtime / logger process に寄せる。

### 不測項目

- logging の ordering 契約を先に決める必要がある。
  - 単一 sink 内 FIFO だけで十分か、複数 producer 間の順序をどこまで保証するかで実装が変わる。
  - Elixir 風に process が多い世界では「完全順序」はコストが高いので、sink 単位 order に留める方が現実的。
  - 判断基準:
    - 複数 worker からの log の順序を user-visible に固定したくなったら、その時点で契約を明文化する。
- durability / flush / crash 時の扱いが未定のまま進む可能性がある。
  - stdout 相当なら即時 write でよいが、file sink を見据えると flush policy を決めないと FileIO と衝突する。
  - Logger 追加段階では「best effort」「flush guarantee なし」に留めるかを明記した方が実装者が迷いにくい。
  - 判断基準:
    - FileIO より先に durability 要件が欲しくなったら、Logger を append-only / best-effort に限定して先に出す。
- Logger を singleton process として扱うか、handler target 群として扱うかで責務が揺れる可能性がある。
  - standard singleton として持つなら boot / override / capability が自然だが、内部実装は handler dispatch だけで済むかもしれない。
  - public surface を先に作るより、runtime 内 identity と override 経路を先に固める方が安全。
  - 判断基準:
    - `StdOut` / `NullOutHandler` / file sink の差し替えで十分なら handler として開始し、状態や buffering が増えたら singleton process 化する。

## 5. Env の外部ファイル注入からロジック参照まで

### ゴール

- 外部ファイルから読み込んだ設定を boot 時に注入し、Surtr ロジックから通常の process / module API として参照できるようにする。
- `project runner` / `supervisor_init` / `Config` / `Env` の責務を整理し、起動時構成と実行時参照の境界を明確にする。
- user code では「環境値を読む」ことだけを意識し、どこから注入されたかは runtime / boot 側に閉じる。

### 不測項目

- runner 側仕様の前倒し整理が必要になる可能性がある。
  - 外部ファイル注入は `supervisor_init` だけでは表現しきれず、project runner や Config 読み込み順まで決める必要があるかもしれない。
  - 特に path 解決、複数 env source の優先順、missing file の扱いは boot 契約に直結する。
  - 判断基準:
    - `supervisor_init` に path literal を増やし始めたら、runner 側仕様へ切り出す。
- `Env` を singleton process にするか、boot 時に固定された readonly view にするかで実装が変わる可能性がある。
  - runtime override や test inject を重視するなら singleton process が自然だが、単純参照だけなら readonly でも十分かもしれない。
  - ただし Logger や FileIO と揃えるなら、process runtime 上の standard singleton として扱う方が一貫性は高い。
  - 判断基準:
    - test inject / project override / script mode の3経路を同じ形で扱えないなら、Env を standard singleton process として寄せる。
- 値の decode と domain validation の境界が曖昧になる可能性がある。
  - 文字列をどこで型に変換するか、欠損値や不正値を boot failure にするか user code の `Result` にするかを決める必要がある。
  - ここを曖昧にすると Env と Config の責務が崩れる。
  - 判断基準:
    - 「ファイルは読めたが値が不正」ケースで recovery 方針が揺れたら、I/O failure と decode failure を別 error 系統に分ける。

## 6. 後回し前提の項目

### ゴール

- custom Supervisor と PubSub は、基盤が整うまで着手しない。
- 先に process runtime の単純な中核を固め、後続機能はその上に自然に載るようにする。

### 不測項目

- 後回しでも最小 hook だけは先に必要になる可能性がある。
  - custom Supervisor を入れなくても、supervision metadata や restart strategy slot を VM schema に置いておかないと後で破壊的変更になりやすい。
  - PubSub を入れなくても、message routing や capability naming を狭く作りすぎると将来拡張しづらい。
  - 判断基準:
    - 今回の実装で schema を固定する箇所が将来 feature を完全に塞ぐなら、surface ではなく metadata hook だけ残す。
- 「後回し」が「設計しない」になってしまう可能性がある。
  - hobby language でも、今決めないことで後から無理が出る箇所だけは最小限メモを残した方が安全。
  - そのため `docs/dev/ProcessRuntime_spec.md` の後続課題と `doc/open-issues.md` の両方で、未着手でも境界だけは残す。
