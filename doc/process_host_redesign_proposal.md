# プロセスとアプリホストの再設計案

状態: 未採用の設計案。2026-10-02。level4（型、実行時契約、バックエンド境界）。

2026-10-10 追記: ホスト別の機能制限は第5節の方針に更新した。`erl` / `elixir` ホストは通常モジュールの出力に限定し、Surtr のプロセス機能を提供しない。ホスト別制限と BEAM 出力は未実装である。

## 1. 推奨方針

プロセス定義とハンドラから型付き API を生成する仕組みを中心に置く。プロセスの振る舞い、アプリの起動構成、外部言語との接続を分け、Eldr と将来の BEAM で同じ言語上の契約を実装する。

固定条件は、型付きメッセージング、通常の言語機能との接続、BEAM の部分的な利用、アプリホストごとの機能制限である。ここでホストとはユーザ指定の `surtr` / `erl` / `elixir` コマンドが用意する実行環境を指す。BEAM と Eldr はバックエンドとして別軸にする。

外部協調のないプログラムに、外部言語の例外を処理する構文や型を要求しない。停止、タイムアウト、初期化失敗の契約は `surtr` ホストのプロセス基盤に残す。

## 2. 現行から生かすもの

- `RuntimeProcessSpec` と `RuntimeBootPlan` による正規化。
- `PID<Proc>` とハンドラの引数・返答型による検査。
- 呼び出し側の `Type::method(...) -> Result<T>` と通常の import。
- `CallResult` / `CastResult` による次状態、返答、停止の明示。
- `handlers {}` による依存先の差し替え。

確認対象は `docs/dev/ProcessRuntime_spec.md`、`docs/site/process.md`、`lib/process.srt`、`lib/tests/process.srt`、`examples/process/memoized_fib_workers/Workers.srt`、PID 等価性の成功 fixture、`crates/sindr/src/ir.rs`、`crates/eldr/src/vm.rs`。コード・テストは読み取り確認のみで、動作検証はしていない。

現在の runtime spec は `fun_idx` を持つ。これを直接 BEAM に流用せず、型検査後の共通契約から Eldr の関数 ID と BEAM の生成関数へそれぞれ落とす。

## 3. 小さなプロセス基盤

利用者が書く状態付きプロセスは、最終的には一つの定義形式へまとめる案を推す。仮称を `defprocess` とし、初期化、要求への返答、一方向通知をハンドラで定義する。構文そのものは未確定。

- Agent の get / set は、状態を読み取る／更新する call の合成へ整理する。
- GenServer は状態付きプロセスの基本形に相当する。
- Task は関数を一度実行する標準 API とし、関数値を渡す使い勝手を保つ。
- Supervisor は起動・停止・再起動を管理する構成として扱う。ユーザが通常のメッセージハンドラとして supervision を実装する必要はない。

これらを同一の内部実装へ無理に押し込めない。共有するのは参照、要求と返答、停止通知、タイマーの契約であり、supervisor と task の役割は明示する。採用時は旧定義の解決・lowering 経路を削除し、別名として恒久的に残さない。

初期化ハンドラは定義側、起動対象・初期値・名前登録・依存先・再起動方針は起動構成側へ置く。Singleton / Worker の指定を起動構成へ移すかは、既存 PID の型と等価性への影響があるため別途確定する。当面の設計では現行の区別を保持する。

## 4. 通常の言語機能との接続

生成した呼び出し API は通常の型付き関数として公開する。通常の関数呼び出し、import、capture、部分適用、高階関数、pipe、`Result` の合成を同じ規則で使えることを受入条件にする。

以下は提案 API の利用形であり、現行の実行確認済みサンプルではない。

```surtr
pid =? Counter::init(0)
add = &Counter::add(pid, &1)
value =? add(3)
```

この形では `Counter::add` は `(PID<Counter>, Int) -> Result<Int>` の生成関数、`add` は部分適用した関数値である。ハンドラ本体は state と入力を受け取って次状態と返答を返し、計算の本体は通常の関数へ切り出せる。

コンパイラが特別扱いする場所は定義の検査と API 生成、および最小の runtime 操作への接続に集約する。通信の呼び出しを、ハンドラ本体の通常呼び出しで代用してはならない。

現行 Scar には、worker の PID 不足の呼び出しを関数値へ変える処理と、singleton の明示 PID を評価して捨てる処理がある（`try_check_worker_message_template_app` / `try_check_singleton_explicit_pid_app`）。後続のユーザ指定により、singleton は同名の PID 省略形・明示形を両方残す。正規シグネチャを PID 付きの一つに固定し、singleton の call だけ受信者を補完する案へ更新した。worker の PID 不足は引数数エラーとし、関数値は明示 capture で作る。詳細と未確定事項は [コンパイラ生成のプロセス呼び出し仕様案](./generated_process_call_spec.md) を参照。

I/O 等の依存も明示的な値として通常の helper に渡せる形を目指す。プロセス所属を理由に一般関数を呼べなくする規則や、暗黙の context 探索を増やさない。汎用 effect system や構造的な process subtyping は今回導入しない。

`@timeout` の綴りを維持するか、通常の設定値を受け取る API にするかは未確定。どちらでも capture や高階関数を経由したときの deadline を一意に定め、関数値にしたことでタイムアウト指定が黙って消える経路を作らない。

## 5. ホストの機能集合

各アプリホストが、提供モジュール、runtime ABI、外部接続、起動管理の能力を宣言する。コンパイル成果物には必要な能力と ABI を記録し、コンパイル時とロード時に適合を検査する。以下は将来の構成案である。

| アプリホスト | バックエンド | 提供する範囲 |
|---|---|---|
| `surtr` | Eldr | Surtr の型付き process、I/O、Task、supervision |
| `surtr` | BEAM | 同じ Surtr 契約を提供する runtime と必要な OTP 依存 |
| `erl` | BEAM | 通常の Surtr モジュールを `.erl` へ変換。Surtr のプロセス機能は提供しない |
| `elixir` | BEAM | 通常の Surtr モジュールを `.erl` へ変換。Surtr のプロセス機能は提供しない |

起動コマンドは既定の機能集合を選ぶ入口にする。コードはコマンド名で分岐せず、利用可能な機能の宣言を参照する。

### 5.1 `erl` / `elixir` ホストの制限（2026-10-10）

Surtr のプロセス定義は、静的に検査した宛先・引数・返答から独自の通信 API を生成する。この契約を Erlang / Elixir のプロセス管理へ持ち込まず、外部ホストでは通常の関数・型・モジュールを使う範囲に限定する。

- 出力経路は `Surtr → .erl → .beam` とする。生成した通常関数は Erlang / Elixir 側のプロセスから呼び出せる。
- Surtr のプロセス定義、生成メッセージ API、`PID<Proc>`、Task、Workers、supervision など、Surtr のプロセス runtime を必要とする機能はコンパイル時に拒否する。依存先の定義・参照も検査する。
- 非対応の定義を黙って除外したり、メッセージ呼び出しを通常関数へ置き換えたり、代替 runtime へフォールバックしたりしない。
- プロセスの起動・通信・停止・監督は Erlang / Elixir 側で扱う。Surtr の BootPlan やプロセス子登録の入口を外部ホストへ提供しない。
- 通常モジュールの変換では、Erlang term への値の表現、Surtr の評価・パターン照合・数値演算の意味、公開 module / function / arity と外部入力の境界を詰める。各型の term 表現と、通常 builtin の対応範囲は未確定である。

同じ通常 API を提供する場合は、その型と意味をホスト間で変えない。提供しないモジュールや機能の参照は診断にする。Surtr の停止・返答・タイムアウト契約は、`surtr` ホスト内の生成 API を通る通信へ適用する。Erlang / Elixir からの直接送信や内部生成関数への直接呼び出しは保証対象外とする。

### 5.2 ロードする機能の境界

ロード可能な `.beam` の集合、依存閉包、起動する OTP application、後続ロード方針を管理する。BEAM は通常、初回参照時に code path から自動ロードするため、起動時点のロード済み集合だけでは機能集合を表せない。[Erlang のコードロード](https://www.erlang.org/doc/system/code_loading.html)。これはサポート機能の境界であり、任意の BEAM コードの動作を隔離する仕組みではない。

## 6. 失敗と外部例外

以下のプロセス関連の扱いは `surtr` ホストに適用する。`erl` / `elixir` ホストは第5.1節の通常モジュール出力と外部呼び出し境界を対象とする。

| 事象 | 共通 Surtr 側の扱い |
|---|---|
| 業務上の失敗 | `Result<T>` の `Err`。現行の concrete error を維持 |
| 呼び出し先の停止、期限超過 | runtime が共通の process error として返す |
| 隔離可能なプロセスの異常終了 | 診断と supervision へ接続。業務上の Err に偽装しない |
| VM 全体の整合性を損なう内部契約違反 | ホスト実行を停止する。worker 再起動で継続しない |
| 外部関数の `error` / `exit` / `throw` | 宣言された外部接続の境界で変換する |
| 外部プロセスの終了 | monitor 等で観測し、停止通知または待機中の失敗へ変換する |

BEAM backend 内部で必要な例外・signal の処理と、利用者が外部例外を扱う API を分ける。Surtr だけで完結する構成でも前者は必要になりうるが、後者は要求しない。外部連携でも、基本は境界で `Result` へ変換するため、言語共通の `try/catch` は必須ではない。

外部例外の class / reason / stack を公開する場合は、外部接続モジュール内の不透明な診断値に閉じる。同期呼び出しの例外捕捉で非同期の exit signal まで回収できるとは定義しない。

## 7. BEAM へ持ち込む契約

この節のプロセス契約は `surtr` ホストの BEAM backend を対象とする。`erl` / `elixir` ホストの通常モジュール出力には適用しない。

- 状態付きプロセスを OTP `gen_server` に対応させる案を第一候補とする。Task は短命な process、supervision は OTP supervisor への対応を個別に検証する。
- 型付きの要求・返答にはプロトコル識別子と版を持たせる。外部から来た値は引数・返答を検証し、未検証の Erlang term や生 PID を `PID<Proc>` として信用しない。
- 一つのプロセスの state 更新は直列に行う。停止後に遅れて返った処理が state を復活させない。
- 同一送信者から同一宛先への順序を守り、異なる送信者間の全順序は保証しない。
- `cast` の成功は送信手続きの成功に限定する。配達・処理・state 更新の保証が必要なら返答を伴う call を使う。この範囲は現行契約との変更点として採用時に確定する。
- timeout は待機の終了であり、相手側の処理取り消しや state の巻き戻しを意味しない。遅延した返答は完了済みの要求を上書きしない。
- `ReplyLater` は次状態を確定してから返答を遅延させる。callback の失敗、元プロセス停止、外側 timeout の競合を一つの要求完了規則にまとめる。
- singleton PID は現行どおり同じ process 型の論理参照として扱い、再起動前後で等しい。worker PID は個体参照とし、再起動した個体は別 identity にする。等価性と生存状態は別である。
- singleton への新規 call は現在の個体へ解決する。進行中の call を再起動先へ自動再送しない。論理参照の等価性から exactly-once や再送安全性を導かない。

現行の `adopt / handoff` は PID を維持した原子的な所属変更を要求する。通常の OTP child spec への直接変換では満たせないため、BEAM 初期範囲では起動時に supervisor を固定する案を推す。`adopt` は対応能力を持つ backend / ホストだけで公開し、非対応時は拒否する。共通の必須機能へ含めるなら、この契約を満たす管理層を別途設計する。

汎用 send / receive、任意 term、分散ノード、hot code upgrade、任意 link / trap_exit は初期対象外にする。通常の関数値は Surtr 内の対応バックエンドで使える一方、外部 wire protocol では宣言した変換可能な型だけを受け入れる。

公式資料では、`gen_server:call` は timeout 等で呼び出し側を exit させ、`cast` は宛先の不存在を無視して返る。したがって公開 API を直結せず、Surtr 契約への変換が必要である。[Erlang gen_server](https://www.erlang.org/doc/apps/stdlib/gen_server.html)

Erlang の例外とプロセス間の終了観測は別の仕組みである。[Erlang のエラー処理](https://www.erlang.org/doc/system/errors.html)。メッセージ順序は同一送信者・同一宛先の保証に合わせる。[Erlang のプロセス](https://www.erlang.org/doc/system/ref_man_processes.html)

## 8. 実装へ進む際の順序と受入条件

1. 定義形式、生成関数、参照 identity、失敗、cast、deadline、停止競合を仕様化し、正本へ反映する。
2. Eldr で共通契約に一本化する。通常関数への接続を検証し、置き換えた旧 lowering を削除する。
3. ホストの機能集合、成果物の必要機能・ABI、ロード時検査を導入する。
4. `erl` / `elixir` ホスト向けに通常モジュールの `.erl` 出力と Erlang term の対応を実装し、依存先を含むプロセス機能の拒否を検証する。
5. `surtr` ホストの BEAM backend で初期化・call・cast・停止・timeout・再起動を実装し、同じ契約テストで比較する。
6. 必要な外部接続ごとに adapter を追加する。外部ホストへ Surtr のプロセス runtime を導入する経路は設けない。

成功境界は `surtr` ホストでの PID 付き API の部分適用、通常 helper の合成、ホスト間で通常の共通コードが同じ結果を返すこと。拒否境界は異なる process の PID、引数・返答型の不一致、利用不可の機能、ABI 不一致である。`erl` / `elixir` ホストでは、直接のプロセス定義と依存先経由のプロセス機能使用の両方を拒否する。`surtr` ホストでは停止と返答の競合、遅延返答、再起動 identity も固定する。外部からの直接メッセージングは保証対象外とする。

変更する正本は `docs/dev/ProcessRuntime_spec.md`、`docs/site/process.md`、`lib/process.srt` の `@doc` と関連する backend 仕様。今回は提案のみで正本の契約は変更しない。未確定事項は本書の第3〜7節で扱い、本節の手順1で仕様を確定する。

実装時は直接の契約テストから開始し、level4 の `rtk cargo nextest run --profile ci --workspace`、`cargo run -- test --quiet --all`、独立レビューを行う。今回の調査では実行テストを行わない。
