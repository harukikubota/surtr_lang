> 取り込み元: `/Users/haruca/.codex/worktrees/list-runtime-remaining/surtr`（detached、HEAD `72154707`）。資料の集約。記載された製品変更・harnessはmain未統合。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](../README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# FileOutHandler の資源寿命: main の実行再現

調査日: 2026-10-13。対象は main `72154707399d6cdaf7190ab75007f2049ab010c6`。
指定された [List runtime 測定レポート](../baseline/list_runtime_measurement_report_2026_10_11.md) のRT-4に残る正本不一致を、通常のSurtrソースとCLIで再現した。
製品コード・通常テスト・言語仕様・VM/schema versionは変更していない。本書以外の再現用ソースと原本は一時ディレクトリに置いた。

## 結果

同じLoggerから2回書く間にファイルをrenameし、同じpathnameへ別ファイルを作ると、2回目のwriteは置換後のファイルへ届いた。
handler lifecycle中に元のopen descriptorを保持する正本の契約と一致しない。
また、存在しない親directoryを指定しても、eager processのinit内で`ctx.out`を参照でき、そのprocessのgetterは成功した。
open失敗は最初のwriteまで報告されなかった。

これは今回の最適化で生じた新規デグレではなく、入力レポートの基準commitから残る既存の契約不一致である。
`process_context_handler`と`out_handler_write`の関数本文は、基準`ee13e44820993fcfc56ecfa216ce4165be67f4ca`と今回のmainでbyte一致した。
基準commitのbinary実行や、最初に不一致が導入されたcommitの特定は行っていない。

## 正本とコードの対応

- [Process Runtime仕様](../../../../docs/dev/ProcessRuntime_spec.md) 第3.19節はFileOutHandlerをappend/createとし、handler init時にopen、lifecycle中は保持、shutdown時にflush/closeと定める。同一canonical pathは同じsinkに正規化し、そのmailbox順で書く。
- `crates/eldr/src/vm.rs:2184`の`process_context_handler`はhandler targetからidentityを作り、stable PIDを返す。fileをopenしない。
- 同ファイル`:2205`の`out_handler_write`はFileOutHandler分岐で`OpenOptions::new().create(true).append(true).open(path)`を毎回実行し、関数内の局所Fileへ`write_all`する。関数終了時にFileをdropする。
- `crates/eldr/src/vm.rs:9521`の既存テストはappend成功、`:9554`はwrite時の不正pathを確認する。init時のopen、write間のpath差替え、保持したsinkのshutdownは検証していない。

getter成功だけではhandler initの実行時点を特定できないため、init内で`ctx.out`を参照する補強ケースを加えた。
このケースはdependencyのPID解決ができる事実を示す。独立したFileOutHandler初期化機構が呼ばれたことを示すものではない。
現行コードにそのopen経路がないことと、write間のpath差替えを合わせて判断した。

## 実行条件と原本

mainの作業treeがcleanであることを確認し、以下を実行した。

```sh
cargo build --locked -p rune --bin surtr
```

終了コード0、dev profile、5.06秒。今回の再現で使用したbinaryはこのbuild直後の`/Users/haruca/work/rust/surtr/target/debug/surtr`である。
SHA-256は`c846bc9d47e110a921bbf4a185071940d8d85d2c4f0a290ed6bcbff5988c3345`。実行後のmainもcleanだった。

一時ディレクトリ: `/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-file-handler-repro-20261013-kj5qnxvl`。
[現行mainの実行原本](/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-file-handler-repro-20261013-kj5qnxvl/current-main-results.json)に、各command、source/binary hash、終了コード、stdout/stderr、最終file内容を保存した。
[基準commitとの本文比較](/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-file-handler-repro-20261013-kj5qnxvl/baseline-source-comparison.json)には比較した両関数の本文とhashを保存した。

現行`crates/eldr/src/vm.rs`のSHA-256は`9e80708145e52ff5654e8135eed2203ce3dc9c085183dad6d2b53f135545e52d`、正本`docs/dev/ProcessRuntime_spec.md`は`d731cf60f88f0f80eae1163d68c51c09064b7c5c8d2fca9d0de440455f729727`。
比較対象の両commitで`process_context_handler`本文は`c23144e6bcada1960bd37978641187b453e134b5f1c4f3bcd0a26a8c9456aa0c`、
`out_handler_write`本文は`a18c50f3dc1c6b4afdfb8a91511a386a7c9ce1a1c07ab2f1efe07844d05d3117`で一致した。

再現fileを次の形で実行した。各caseは別processである。

```sh
/Users/haruca/work/rust/surtr/target/debug/surtr run /var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-file-handler-repro-20261013-kj5qnxvl/missing_parent_context_access.srt
/Users/haruca/work/rust/surtr/target/debug/surtr run /var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-file-handler-repro-20261013-kj5qnxvl/missing_parent_write.srt
/Users/haruca/work/rust/surtr/target/debug/surtr run /var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-file-handler-repro-20261013-kj5qnxvl/path_replace.srt
```

`path_replace`の再実行前には、一時ディレクトリ内の`sink.log`と`sink.old.log`だけを削除する。
以下のソースは実行した原本そのもので、pathは今回の一時ディレクトリへ固定されている。
別directoryで実行するときは、そのpathのliteralだけを置き換える。

## 再現1: dependency参照を伴うinit成功と遅延したopen失敗

`absent`directoryは作成しない。

```surtr
defagent Logger {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
    handlers { out: OutHandler = StdOut }
  }
  @init
  def init() -> Result<Int> {
    print(inspect(ctx.out))
    Ok(0)
  }
  @get
  def ping(state: Int) -> Result<Int> { Ok(state) }
}
supervisor_init {
  Logger { handlers { out: FileOutHandler(path: "/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-file-handler-repro-20261013-kj5qnxvl/absent/sink.log") } }
}
print(inspect(Logger::ping()))
```

stdoutはFileOutHandlerのPID表示と`Ok(0)`、stderrは空、終了コード0だった。
PID表示には指定した`absent/sink.log`pathが含まれる。
init内の`ctx.out`表示を除いた補助case`missing_parent_no_write.srt`も`Ok(0)`、終了コード0だった。

別processで次の原本を実行した。

```surtr
defagent Logger {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
    handlers { out: OutHandler = StdOut }
  }
  @init
  def init() -> Result<Int> { Ok(0) }
  @get
  def log(state: Int, message: String) -> Result<Unit> {
    OutHandler::write(ctx.out, message)
  }
}
supervisor_init {
  Logger { handlers { out: FileOutHandler(path: "/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-file-handler-repro-20261013-kj5qnxvl/absent/sink.log") } }
}
print(inspect(Logger::log("one\n")))
```

stdoutは`Err(FileOutHandlerOpenFailed("FileOutHandler open failed for .../absent/sink.log: No such file or directory (os error 2)"))`。
stderrは空、終了コード0。これは処理系の成功退出と、返された言語ResultのErrを区別して記録したものである。
親directory不在のエラーがboot/dependency参照時ではなく最初のwrite時に返ることを確認した。

## 再現2: pathname差替えでwrite先が移る

```surtr
defagent Logger {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
    handlers { out: OutHandler = StdOut }
  }
  @init
  def init() -> Result<Int> { Ok(0) }
  @get
  def log(state: Int, message: String) -> Result<Unit> {
    OutHandler::write(ctx.out, message)
  }
}
supervisor_init {
  Logger { handlers { out: FileOutHandler(path: "/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-file-handler-repro-20261013-kj5qnxvl/sink.log") } }
}
print(inspect(Logger::log("one\n")))
print(inspect(FS::mv(FilePath::new("/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-file-handler-repro-20261013-kj5qnxvl/sink.log"), FilePath::new("/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-file-handler-repro-20261013-kj5qnxvl/sink.old.log"))))
print(inspect(File::write("/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-file-handler-repro-20261013-kj5qnxvl/sink.log", "replacement\n")))
print(inspect(Logger::log("two\n")))
print(inspect(File::read("/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-file-handler-repro-20261013-kj5qnxvl/sink.old.log")))
print(inspect(File::read("/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-file-handler-repro-20261013-kj5qnxvl/sink.log")))
```

stdoutは次のとおり。stderrは空、終了コード0だった。

```text
Ok(())
Ok(())
Ok(())
Ok(())
Ok("one\n")
Ok("replacement\ntwo\n")
```

4つの`Ok(())`は、最初のwrite、rename、置換file作成、2回目のwriteに対応する。
2回とも同じ`Logger::log`と同じhandler target設定を使い、handlerの再設定・再起動は行っていない。
最終fileの直接読取りでも`sink.old.log`は`one\n`、`sink.log`は`replacement\ntwo\n`だった。

今回のmacOSで元descriptorを保持していれば、rename後の2回目のwriteも元fileへ届き、
old側が`one\ntwo\n`、replacement側が`replacement\n`となる。
実行結果は現行コードのwriteごとのopenと一致する。

## 未検証範囲と修復前の受入条件

この再現は現行のFileOutHandler契約不一致の確認であり、修復の実装・全体検証ではない。
canonical path正規化、複数producerのmailbox順、shutdownのflush/close、取消・timeout・REPL rollback、他OSは実行検証していない。
時間、allocation、RSS、公平性は測定していない。

修復時には、少なくとも次を満たす移行仕様と直接の回帰テストが必要になる。

- handler init時のopen失敗を、最初のwriteに先送りせず所定の初期化エラー経路へ配送する。
- 同じsinkが生存する間はopen descriptorを保持し、pathname差替えだけでwrite先を変更しない。
- canonical pathを指す複数dependencyを同じsinkへ正規化し、mailbox順序を維持する。
- shutdownでflush/closeする。取消時点、複数所有者の寿命、最後の所有者のcleanupを先に定義し、他の利用者が使うsinkを早期closeしない。
- sinkの共有とcheckpoint保存・復元・破棄の所有関係を定義する。実行済みfile I/Oをrollback対象にせず、保存側のdropだけでlive資源をcloseしない。

元の4c18ではこの問題が未コミットの`doc/open-issues.md`内のOI-038に記録されている。
mainのOI-038はHashMap RSSを指すため、そのIDを流用しない。本書では新IDを割り当てない。

## 検証とレビュー

補正後の4つのCLI実行はすべて終了コード0で、結果は上記のとおり。初回の再現用ソースは同じprocessに`@get`を2つ置きparseで拒否されたため、caseごとに1つへ分けた。この準備ミスはFileOutHandler不一致の証拠に含めない。
古いdebug binaryによるpilotはbuild前の記録として一時directoryに保存し、今回の現行main再現とは区別した。
製品コード・通常テストの変更がないため、workspace CIと標準Surtr全件はこの調査で実行していない。

Astraへ正本・現行source・再現を相談した。descriptor保持を直接区別するpath差替えは妥当と確認し、
init getterだけの断定を避けること、旧binaryと現行buildの区別、未検証のcanonical/mailbox契約、
複数所有者・取消・checkpoint資源寿命を別途定義することを指摘した。上記の補強と制限へ反映した。
製品修復は今回のRegex入力共有化と分けた残件とする。
