# Surtr アクターモデル基盤レビュー観点チェックリスト v0

## 目的

この文書は、Surtr のアクターモデル基盤、runtime、Supervisor、worker lifecycle、message dispatch、timeout、pending/resume、singleton/worker 起動規則をレビューする際の観点を整理したチェックリストである。

主に以下を対象とする。

- process 定義と runtime spec の整合
- singleton / worker の生成・初期化・終了
- message correlation と reply 処理
- timeout / pending / resume / sleep
- Supervisor / DynamicSupervisor / ownership
- process-owned State の閉じ込め
- runtime table と lifecycle 管理
- 観測性、テスト性、リーク、stale reference

---

## 1. 最初に確認する不変条件

最優先で見るべきものは、実装が守るべき不変条件である。

### 1-1. singleton
- [ ] singleton は concrete process 型ごとに一意か
- [ ] 同じ singleton を二重 boot できないか
- [ ] boot 成功後にのみ利用可能として扱われるか
- [ ] singleton boot failure は user code の通常分岐ではなく boot/runtime failure として扱われるか

### 1-2. worker
- [ ] `PID<Worker>` が返る時点で init 完了済みか
- [ ] spawn 直後から worker は primary lifecycle sink を持つか
- [ ] 誰にも観測されていない worker を生成できないか
- [ ] `DynamicSupervisor` 配下でも owner/current process 配下でも lifecycle sink が一意に定まるか

### 1-3. state
- [ ] process-owned State は public API に出現しないか
- [ ] process-owned State は owner process 外から構築できないか
- [ ] process-owned State は owner process 外から lens root にできないか
- [ ] `@init`, `@call`, `@cast`, `@get`, `@set` の state 型が常に一致するか

### 1-4. spec
- [ ] runtime / VM が受け取る spec は concrete 型だけか
- [ ] `impl Trait` や hidden generic が runtime spec に残っていないか
- [ ] source-level sugar と runtime-level dispatch が一意に対応するか

---

## 2. 生成・初期化レビュー

### 2-1. spawn / boot
- [ ] spawn 中に半初期化状態の `PID` が外へ見えないか
- [ ] singleton boot 中に registry / slot が早すぎるタイミングで公開されないか
- [ ] init failure 時に registry slot や child table に壊れたエントリが残らないか
- [ ] boot 順序が DSL / runner の宣言どおりに解釈されるか

### 2-2. init route
- [ ] `@init` route の選択が一意か
- [ ] default boot と DSL override が二重適用されないか
- [ ] init args 型検査が compile-time で行われるか
- [ ] worker spawn 時と singleton boot 時で init route 解釈がぶれていないか

### 2-3. lazy init
- [ ] `Uninitialized -> Initializing -> Ready/Failed` の遷移が一意か
- [ ] 初回 access 時の同時呼び出しで二重初期化が起きないか
- [ ] init timeout と call timeout が混ざっていないか
- [ ] lazy init failure の扱いが singleton/runtime failure として統一されているか

### 典型的な故障例
- 二重 boot
- init failure 後に zombie slot が残る
- lazy init の同時競合で 2 回初期化する
- `PID` は返ったのに state slot が空

---

## 3. メッセージング整合性

### 3-1. request / reply
- [ ] correlation id は一意か
- [ ] reply は一度だけ waiter に配送されるか
- [ ] timeout 後に遅れて来た reply を安全に捨てられるか
- [ ] target down 後の reply を重複処理しないか

### 3-2. call / cast / get / set
- [ ] `@call` は `Result<CallResult<Reply, State>>` 契約に従っているか
- [ ] `@cast` は `Result<CastResult<State>>`、`@set` は `Result<State>` 契約に従っているか
- [ ] `ReplyLater` / `Stop(...)` を返す場合の state commit、reply、停止処理が一貫しているか
- [ ] `Err` 時に state を commit していないか
- [ ] `Ok` 時に state commit と reply が一貫しているか

### 3-3. hidden message / surface API
- [ ] `Type::message(pid, arg)` と hidden message tag が一意に対応しているか
- [ ] same-name handler の collision が起きないか
- [ ] source 上の generic / trait bound が concrete dispatch に正しく落ちているか

### 典型的な故障例
- timeout 済み call に対し遅延 reply が二重で刺さる
- `Err` でも state が変わる
- reply と down が両方 waiter に届く

---

## 4. 待機・停止性・デッドロック系

### 4-1. pending / resume
- [ ] `Pending` から必ず resume できるか
- [ ] wait table / future table に resume 漏れがないか
- [ ] timeout で waiting から必ず抜けられるか
- [ ] target down 時に waiting caller が放置されないか

### 4-2. 評価順序
- [ ] Future readiness によって引数評価順序を変えていないか
- [ ] strict left-to-right evaluation を守っているか
- [ ] `Pending` が出た時に後続式を誤って評価していないか

### 4-3. sleep / yield
- [ ] `Process::sleep` は current process のみを待機させるか
- [ ] 他 PID に対する sleep 相当が存在しないか
- [ ] sleep と timeout が意味的に衝突していないか

### 4-4. 循環待ち
- [ ] `join` / `await` / lazy init wait が相互待ちを作らないか
- [ ] current process と child process の待機が循環しないか
- [ ] supervisor / owner の待機と child call が相互にブロックしないか

### 典型的な故障例
- waiting から戻らない
- resume が二重に発火する
- `sleep(pid, ...)` 相当で外部制御が起きる
- lazy init と caller wait が循環する

---

## 5. レースコンディション観点

shared-memory の lock race より、runtime table と lifecycle 更新の race を重点的に見る。

### 5-1. table 更新
- [ ] process table 更新が原子的か
- [ ] singleton slot / registry slot の登録・削除が原子的か
- [ ] waiting table の add/remove が原子的か
- [ ] deadline queue の追加/削除が原子的か
- [ ] child table の更新が原子的か

### 5-2. ownership 更新
- [ ] `DynamicSupervisor::adopt` が原子的か
- [ ] old sink を外す前に new sink が入るか
- [ ] ownership handoff 中に exit が起きても終了理由が失われないか
- [ ] PID を再生成せずに owner だけ差し替えられるか

### 5-3. competing events
- [ ] timeout と reply が同時到着した時の優先順位が明確か
- [ ] down と reply が競合した時に二重 resume しないか
- [ ] restart と stale PID access が混ざらないか

### 典型的な故障例
- timeout 後に reply が刺さる
- adopt 中に orphan worker が生まれる
- stale PID に対して操作できる

---

## 6. 終了・観測・ownership

### 6-1. 終了理由
- [ ] `Normal / Exit(Error) / RuntimeFault / InitFailed` が区別されるか
- [ ] worker の終了理由が必ずどこかの sink に配送されるか
- [ ] singleton は user-level exit 対象になっていないか

### 6-2. ownership
- [ ] worker は spawn 直後から primary sink を持つか
- [ ] current process ownership と DynamicSupervisor ownership が両立できるか
- [ ] 追加 monitor / join waiter が ownership を壊さないか

### 6-3. join / await
- [ ] `Worker::join` は ownership を変更しないか
- [ ] join は終了済みなら即 return できるか
- [ ] join wait 中に target down / runtime fault が起きても結果が失われないか

### 典型的な故障例
- 誰にも届かない終了理由
- join しただけで ownership が移る
- singleton が user code から exit できる

---

## 7. Supervisor / restart 観点

### 7-1. RootSupervisor
- [ ] singleton boot 順序が安定しているか
- [ ] boot failure を一箇所で集約できるか
- [ ] standard singleton と user singleton の override が一意に決まるか

### 7-2. RuntimeSupervisor
- [ ] runtime singleton と app singleton の責務が混ざっていないか
- [ ] runtime fault を適切に外部境界へ伝えられるか

### 7-3. DynamicSupervisor
- [ ] child restart policy が一貫して適用されるか
- [ ] strategy が DSL / spec と一致しているか
- [ ] restart storm を制御できるか
- [ ] restarted child と古い PID を混同しないか

### 典型的な故障例
- child が restart policy 無視で再起動する
- stale PID が有効に見える
- restart 後に child table が壊れる

---

## 8. API 境界・抜け道

### 8-1. process surface
- [ ] `receive` 相当の自由な mailbox 操作が露出していないか
- [ ] generic `send(pid, msg)` を user-facing に出していないか
- [ ] `Process::sleep(pid, ...)` のような危険 API がないか
- [ ] `PID` に mutable timeout 設定を持たせていないか

### 8-2. process-owned state
- [ ] `@process_state` が public API に漏れないか
- [ ] `State` 型に対する lens root が外へ出ていないか
- [ ] snapshot / view が通常型として分離されているか

### 8-3. singleton 利用検査
- [ ] compile unit 単位で required singleton が集計されるか
- [ ] `boot_policy = Required / OnReference / ExplicitOnly` が一貫しているか
- [ ] `required_singletons ⊆ available_singletons` を保証しているか

---

## 9. 資源管理・リーク

### 9-1. runtime table
- [ ] waiting entry が timeout/down/reply 後に消えるか
- [ ] completed future が不要に残り続けないか
- [ ] deadline entry が解放されるか
- [ ] join waiter が終了後に残らないか

### 9-2. handler target / IO
- [ ] `StdOut / StdErr / Logger` の target 差し替えで orphan resource が残らないか
- [ ] `BufferOutput` / `FileLogger` などの backing resource が適切に flush/close されるか
- [ ] override 時に default target が二重起動しないか

### 9-3. lazy singleton
- [ ] `Failed` 状態の保持方針が明確か
- [ ] retry 不可なら無限に再試行しないか
- [ ] retry 可なら backoff / reset 条件が定義されているか

---

## 10. 観測可能性

レビューでは「壊れないか」だけでなく「壊れたら追えるか」も見る。

### 最低限ほしい統計
- [ ] process 作成数
- [ ] 生存 process 数
- [ ] waiting 数
- [ ] pending future 数
- [ ] timeout 数
- [ ] target down 数
- [ ] restart 数
- [ ] mailbox 最大長
- [ ] child table 数
- [ ] boot failure 理由

### 望ましいログ
- [ ] singleton boot 開始/成功/失敗
- [ ] worker spawn / adopt / handoff
- [ ] timeout 発生箇所
- [ ] down reason
- [ ] restart reason

---

## 11. テスト観点

### 11-1. 必須シナリオ
- [ ] singleton boot success / failure
- [ ] worker spawn success / init failure
- [ ] timeout vs reply race
- [ ] down vs reply race
- [ ] adopt / handoff race
- [ ] lazy init 同時呼び出し
- [ ] join on already-exited worker
- [ ] restart 中 stale PID access

### 11-2. 再現性
- [ ] single-thread scheduler で deterministic に再現できるか
- [ ] short timeout で競合を再現できるか
- [ ] `Process::sleep` で待機順序をテストできるか

### 11-3. 負荷試験
- [ ] 大量 worker で process table が壊れないか
- [ ] mailbox / waiting / restart の統計が伸びすぎないか
- [ ] memory leak がないか

---

## 12. レビュー優先順位

時間が限られている場合は、以下の順に見る。

1. 不変条件
2. spawn / init / boot
3. message correlation
4. pending / timeout / resume
5. exit / ownership / supervision
6. resource cleanup
7. observability

---

## 13. かなり短い要約

### race condition で特に見るもの
- registry / singleton slot
- waiting table
- timeout と reply の競合
- adopt / handoff
- restart と stale PID

### deadlock / 停止性で特に見るもの
- 相互 await / join
- lazy init wait の循環
- resume 漏れ
- current process 以外への sleep

### Surtr で特に重要なもの
- 終了理由の喪失
- 誰にも観測されない worker
- process-owned state の漏洩
- runtime spec に抽象型が残ること
