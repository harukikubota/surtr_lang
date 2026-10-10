# コンパイラ生成のプロセス呼び出し仕様案

状態: 設計案、未実装。2026-10-02。level4（呼び出し・capture・生成関数の契約変更）。

2026-10-10 追記: [Process Runtime の停止契約](../docs/dev/ProcessRuntime_spec.md#3102-stop受付拒否停止完了) の共通実行境界と回収契約へ追従した。Eldr の停止改修は実装・検証済みであり、本書の PID 補完・capture・Workers template 移行の実装完了を意味しない。

## 1. ユーザが指定した境界

- プロセスの定義と呼び出しをコンパイラ生成で接続する。
- メッセージングは通常の関数呼び出し規則に従う。
- singleton は同名の PID 省略形・明示形を両方許可する。
- worker は第一引数に宛先 PID を渡す。
- `GenServer` の内部 API を扱えるのはコンパイラだけとする。

以下は、この境界を満たす具体案である。bare capture、名前付き PID の名前、周辺の Workers API の扱いは今回の提案であり、既存の実装済み仕様とは区別する。プロセス定義構文の統一やホスト能力システムの導入は対象外。

## 2. 正規シグネチャを一つにする

状態を扱うハンドラ `(State, A...) -> Result<CallResult<R, State>>` から、通信する公開関数 `(PID<P>, A...) -> Result<R>` を生成する。cast も同じ宛先規則を使い、既存の返り値契約に従う。
cast の公開署名は `Result<Unit>` を維持し、停止宛先への拒否と handler 自身の `Err` を返す。送信手続きだけの成功や fire-and-forget へ変更しない。

ハンドラ本体の state と、公開関数の receiver PID は別の役割である。公開関数の通常呼び出しが、ハンドラ本体を呼び出し元のプロセスで直接実行する経路に変わってはならない。

公開名の symbol identity と正規シグネチャは一つだけにする。一般の同名関数 overload は導入しない。singleton の PID 省略は、このシグネチャへ合わせるための受信者補完として定義する。

判定にはコンパイラが生成したメッセージ関数の identity を使う。名前文字列、同名 module、戻り値の型、process owner 内にあることだけでは判定しない。初期化、pid 取得、通常 helper に補完を適用しない。

## 3. 呼び出しの正規化

payload の引数数を N とする。位置引数の規則は次のとおり。

| 対象 | 入力 | 処理 |
|---|---|---|
| singleton | N 個 | 第一引数へその singleton の論理 PID を補完 |
| singleton | N+1 個 | 第一引数を明示 PID として検査 |
| singleton | それ以外 | 引数数エラー |
| worker | N+1 個 | 第一引数を宛先 PID として検査 |
| worker | それ以外 | 引数数エラー。関数値へ変換しない |

引数の型を見て省略形・明示形を切り替えない。形を決めた後は通常の引数検査を一度だけ行い、型エラー時に別の解釈へ戻らない。payload 自体が PID 型でもこの規則は同じである。

明示した PID は実際に送信先として使用する。型検査と評価だけを行って捨てる経路は削除する。異なる process 型の PID は型エラーにする。
指定した古い個体を新個体へ転送しない。singleton の型単位の Eq と個体の受付状態は別の契約であり、等価性を転送の根拠にしない。

PID 補完後は通常の引数評価規則を使い、引数式を複製しない。補完する式は生成済みの公開関数 `P::pid()` の呼び出しとし、その本体だけが内部 GenServer 操作へ接続する。ユーザの call site へ内部操作を直接露出させない。省略 PID の取得も補完された第一引数として評価する。singleton の参照は現在の正本にある論理 identity を維持する。

### 名前付き引数

提案する公開 receiver 名は `pid` とする。通常の関数と同じく、位置引数と名前付き引数の混在は拒否する。

- singleton: `pid:` がなければ補完し、あれば明示 PID として検査する。
- worker: `pid:` を必須とする。
- 残りの引数は通常の名前照合で、欠落・重複・未知の名前を拒否する。
- メッセージの payload 引数にも `pid` を宣言した場合は、生成する公開引数名が重複するため定義エラーにする。`pid` を言語全体の予約語にはしない。

補完は内部の引数スロットへ行い、ソースの「位置・名前付き混在」として扱わない。評価順は通常の名前付き引数の規則に合わせる。名前付き引数を使えない関数値・capture・pipe の位置は、現行の通常規則どおり拒否する。

### pipe と中置 call

通常の pipe による第一引数注入と中置 call の変換を行ってから、singleton の受信者を補完する。

```surtr
3 |> Counter::add()       # Counter::add(3) → PID を補完
pid |> Counter::add(3)    # Counter::add(pid, 3) → 明示 PID
pid |> Worker::add(3)     # Worker::add(pid, 3)
3 |> Worker::add()        # PID が不足するため引数数エラー
```

import した名前、qualified 名、backtick の名前も、同じ生成 symbol identity へ解決された場合は同じ規則を使う。名前解決・private 検査の失敗を受信者補完で回避しない。

## 4. 関数値は通常の capture で作る

bare capture は正規シグネチャをそのまま関数型にする案を推す。singleton / worker とも PID を第一引数に持つ。期待型に合わせて暗黙に PID を束縛しない。

以下では Counter が singleton、Worker が worker、add の payload が Int 一つで、返答が Int と仮定する。例は提案であり実行確認済みではない。

```surtr
all = &Counter::add            # (PID<Counter>, Int -> Result<Int>)
one = &Counter::add(&1)        # (Int -> Result<Int>)、実行時に PID 補完
two = &Counter::add(&1, &2)    # (PID<Counter>, Int -> Result<Int>)

worker_all = &Worker::add      # (PID<Worker>, Int -> Result<Int>)
worker_one = &Worker::add(pid, &1)
message = &Worker::add(&1, 3)  # (PID<Worker> -> Result<Int>)

Worker::add(3)                # エラー。関数値を作らない
&Worker::add(&1)              # エラー。完全な呼び出しに PID と payload が必要
all(3)                       # エラー。関数値への呼び出しで PID は補完しない
one(3)                       # 有効。one の本体に補完が組み込まれている
```

placeholder capture は、内部の完全な call を上記の規則で正規化する。作成時にはメッセージを送らず、通常 capture と同じく固定引数式も呼び出しのたびに評価する。singleton の PID 取得も呼び出し時に行う。

payload が 0 個の場合、`Counter::ping()` は即時の call、`&Counter::ping` は PID を受け取る関数値になる。PID を省略した 0 引数関数値は `{|| Counter::ping()}` と書く。現行 parser は `&Counter::ping()` の空括弧を bare capture と区別しないため、これも PID を受け取る bare capture と同じになる。空括弧を「PID 省略の 0 引数関数値」と再解釈しない。

関数値として変数・引数・戻り値へ渡した後は、確定した関数型だけで呼び出す。capture の origin や実行時の中身を見て PID を補完しない。

## 5. compiler-only の境界

生成した公開関数から、コンパイラ専用の GenServer 操作へ接続する。通常の Surtr コードが `GenServer::*` や canonical hidden builtin を直接呼ぶ、import する、capture する、別名経由で参照する経路は拒否する。

呼び出し元が生成関数の本体であることを内部 metadata で検証する。ユーザが同名を定義したり、同じ名前の属性を書いたりして権限を得られる仕組みにしない。公開 wrapper の capture は許可し、内部操作そのものの関数値は公開しない。

`surtr` ホストの BEAM 出力では、生成関数と runtime module の呼び出しへ変換できる。2026-10-10 のホスト別方針により、`erl` / `elixir` ホストは通常モジュールの `.erl` 出力に限定し、本案のプロセス定義・生成メッセージ API は依存先経由の使用も含めてコンパイル時に拒否する。詳細は [ホストの機能制限](./process_host_redesign_proposal.md#51-erl--elixir-ホストの制限2026-10-10) を参照。ホスト別制限と BEAM 出力は未実装である。

ここでいう compiler-only は Surtr ソースのアクセス規則であり、外部 Erlang コードから BEAM の export を呼べないという保証ではない。Erlang / Elixir からの直接メッセージ送信や内部生成関数への直接呼び出しは、Surtr のプロセス契約の保証対象外とする。外部からの入力契約違反まで一律に Result に変換することは要求しない。

### 5.1 生成 wrapper の共通実行境界

停止・完了・回収の正本は [Process Runtime のメッセージ実行](../docs/dev/ProcessRuntime_spec.md#3101-メッセージの開始と終了) と [停止契約](../docs/dev/ProcessRuntime_spec.md#3102-stop受付拒否停止完了) とする。今回の Eldr 改修では、canonical hidden builtin `__process_execute` が宛先検証、受付確認、state snapshot 取得、実行 record 登録を原子的に行う。生成 wrapper の body closure は登録後にその snapshot を消費して handler を実行し、state 保存・返答・cleanup まで同じ実行を続ける。Agent の get / set も同じ境界を使い、名前文字列や state 読取回数から実行開始を推測しない。

実行段階は `Handling → Postprocessing → Callback` とする。state snapshot は Handling で一度だけ読み取り、handler の正常復帰後に生成 wrapper の canonical hidden builtin `__process_postprocess` が Postprocessing へ移す。store / Stop / ReplyLater は後処理段階だけで許し、snapshot 読取済みという条件だけで保存を許可しない。ReplyLater は callback へ段階を移し、後処理の権限を引き継がない。境界外の process_state を生存本体への通常読取へ戻す旧成功経路は削除する。

PID 取得、capture 作成、Workers 選択、lease 取得だけでは開始済みとしない。direct / capture / 高階関数 / Workers / lease 経由でも、生成 wrapper が実行される境界で同じ受付確認をする。runtime が Stop を受理した後の新規要求は、handler を開始せず `ProcessStopped(pid, process)` を返す。型不一致、未知 PID、内部契約違反を停止として扱わない。現在の Eldr mailbox は未使用であり、非空なら内部契約違反とする。新しい未開始 queue の実装をこの改修の完了条件へ加えない。

開始済み wrapper は停止要求後も state 保存と返答を完了できるが、保存によって受付を再開しない。再入の子 call は別実行として受付を確認する。ReplyLater は同じ実行を callback へ移譲し、caller timeout 後も継続と cleanup の終了まで追跡する。callback に state 保存権限を渡さない。通常 Stop の Normal / Error は強制取消ではなく、開始済み処理の終了後に本体を回収する。返答と timeout は一度だけ確定する。

旧 PID は不変 identity を共有し、state・mailbox・継続を保持しない。回収後の同じ PID にも同じ ProcessStopped kind・payload・message を返す。Eldr の停止識別表は Weak を持ち、参照のない entry を管理境界で除く。新規拒否の source origin は呼出し位置とし、Stop handler や deferror 宣言の位置で上書きしない。元の Error の伝播も kind / message / location / cause を維持する。

この共通境界への接続と、本書の単一正規シグネチャ・PID 補完・bare capture・Workers template の一般化は別の変更である。停止改修の検証結果だけで本書全体を実装済みとは扱わない。

## 6. 現行との差分と周辺の未確定事項

- Scar の `try_check_worker_message_template_app` は PID 不足を `InjectCall` にする。これは廃止する。
- Scar の `try_check_singleton_explicit_pid_app` は明示 PID を評価した後に捨てる。PID 付きの単一正規関数への呼び出しに置き換える。
- 現行の通常 named capture は宣言の全引数を受け取る。今回の PID 付き正規シグネチャに合わせ、singleton の bare capture の型も変わる。
- `Workers::submit` / `broadcast` の現行サンプルは暗黙の message template を使っている。移行候補は `Workers::submit(workers, &Worker::assign(&1, job))` である。現行の宣言・runtime は通常の callable を受け取り、capture 由来を制限する型は持っていない。一方、正本文書には message template だけを受ける制約があるため、この境界と lease の予約・解放を確認してから移行を確定する。単なる綴りの置換で完了とはしない。
- 旧 template は payload を作成時に評価する。通常 capture への移行では実行時評価になり、broadcast で評価回数が変わりうる。一度だけ評価する必要があれば `job = make_job()` と先に束縛する。
- 現行の `WorkerLease` と PID の扱い、supervisor の init-route 特殊構文は別の境界である。直接メッセージ API の PID 契約へ無断で混ぜず、Workers API の移行仕様として確定する。
- `@timeout` の関数値を経由した意味は本案で新設しない。呼び出しの一般化に伴う制限・伝播方法を別途確定する。

## 7. 受入条件と実装順序

1. 本案の capture と named receiver の規則、Workers の移行範囲を確定し、`docs/dev/ProcessRuntime_spec.md`、`docs/site/process.md`、callables / capture 文書、`lib/process.srt` の `@doc` を整合させる。
2. 定義から公開関数の単一シグネチャと compiler-only の接続を生成する。名前解決は Sigil、型・引数の正規化は Scar、実行コードへの変換は Forge と backend の責務に置く。
3. worker template の旧経路と singleton PID 破棄経路を削除する。生成関数の正常な symbol 情報が欠ける場合も一般関数へフォールバックしない。
4. direct / import / backtick / infix / pipe / bare capture / placeholder capture / 関数値経由で、同じ引数契約とアクセス制御を検証する。analysis / LSP の signature 表示も一致させる。
5. 副作用を持つ引数が一度だけ・定めた順序で評価されること、capture 作成時に送信しないこと、singleton の補完が payload の PID 型に左右されないことを検証する。

重要な拒否例は worker の PID 不足、異なる process 型の PID、関数値への引数不足、内部 GenServer の参照、同名ユーザ関数への誤った補完である。payload 0 個、payload に PID を含む場合、名前付きの欠落・重複、singleton の省略／明示の対も固定する。

本書の呼出し一般化は仕様作成までであり、未実装の項目を停止改修の実装・検証へ混ぜない。停止改修の検証結果は[実行時の継続調査書](runtime_audit_followup_20261010.md#2026-10-10-停止回収修正の最終検証)に記録した。本書を実装する際は直接の契約テストから始め、level4 の CI workspace・標準 SRT テストと独立レビューを行う。
