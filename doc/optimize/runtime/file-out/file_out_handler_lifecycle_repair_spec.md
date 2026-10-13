> 取り込み元: `/Users/haruca/.codex/worktrees/list-runtime-remaining/surtr`（detached、HEAD `72154707`）。資料の集約。記載された製品変更・harnessはmain未統合。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](../README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# FileOutHandler の資源寿命を修復する仕様案

作成日: 2026-10-13。基準は main `72154707399d6cdaf7190ab75007f2049ab010c6`。
状態: 調査に基づく仕様案。採用方式と以下の未確定事項を決めてから実装する。
本書では製品コード、実行可能テスト、正本文書を変更していない。

入力は [FileOutHandler の実行再現](file_out_handler_lifecycle_reproduction_2026_10_13.md) と、指定された [List runtime 測定レポート](../baseline/list_runtime_measurement_report_2026_10_11.md) のRT-4。
root担当がこの問題をOI-040として追跡する。本書はissue一覧を変更しない。

## 目的と対象外

FileOutHandlerを、初期化時にopenして同じsinkのlifecycle中保持するappend専用の出力先へ移行する。
writeごとのopen/dropを削除し、同じcanonical pathのdependencyを同じsinkに接続する。
process初期化、資源共有、scheduler、checkpointの接続に関わるためlevel4とする。

公開構文、`OutHandler::write`の署名、一般の`File`・`FS`・`Shell`の役割、VM/schema versionは変更しない。
通常のFileHandleをprocess出力先として公開しない。標準StdOut/StdErrのcapture・tee・REPL出力方針とNullOutHandlerの無出力を保つ。
非同期OS I/O、worker threadへの移管、同期write一回の実時間上限、一般process restart、Logger API拡張は対象外とする。

## 現行仕様で確定している条件

正本は [ProcessRuntime_spec.md](../../../../docs/dev/ProcessRuntime_spec.md) 第3.18・3.19節と [EldrVM_spec.md](../../../../docs/dev/EldrVM_spec.md) 第3.9節。

- FileOutHandlerはappend固定、存在しないfileをcreateし、既存fileをtruncateしない。
- handler init時にopenし、lifecycle中は保持し、shutdown時にflush/closeする。
- 同じVMの同じcanonical pathは同じsinkに正規化し、そのsinkのmailbox順で書く。別producerのwall-clock順を保証しない。
- open失敗は`FileOutHandlerOpenFailed(path, detail)`、write失敗は`OutHandler::write`の`Err(FileOutHandlerWriteFailed(path, detail))`として扱う。診断表はhandler init失敗を`runtime-handler-init-failed`に区別する。
- process contextはreadonly dependencyであり、公開APIにhandler PIDを返さず、Stateにも混ぜない。
- 通常の再開でcallbackや結果を二重配送しない。call timeoutは開始済みcalleeを取り消さず、Task自身の取消ではcleanup後に結果を配送する。
- checkpointは保存位置へ復帰する。ファイルのパスを開き直したり、OSの読み書き位置を戻したり、完了した外部I/Oを取り消したりしない。
- top-levelが終了してもbackground処理が残る間は必要な資源を保持する。

次節以降の内部構造は提案である。これらの確定条件から、未規定の初期化時点、物理close時点、flush失敗の配送先まで自動的に決まるとは扱わない。

## ソースとの対応

| 領域 | 現行コード | 修復で必要な責務 |
|---|---|---|
| 宣言・Boot設定 | `crates/sindr/src/ir.rs`の`RuntimeHandlerTarget`は`name`と構造化した`named_args`を保存する。canonical pathは持たない | serialized metadataを維持し、host環境を扱うEldr側で実行時targetへ解決する |
| capability・引数検査 | `crates/forge/src/codegen.rs`の`validate_runtime_handler_target`がFileOutHandlerのpath必須・未知引数を拒否する | 既存検査を保つ。host pathの存在や権限をcompilerで検査しない |
| 設定 | `vm.rs`の`ProcessSpecRegistry.handler_defaults`、`ProcessRuntime.handler_contexts`、`apply_runtime_handler_overrides` | default/overrideから解決する設定と、生存中のinstanceが取得済みのbindingを区別する |
| `ctx.out` | `process_context_handler`がprocess名から設定を読み、文字列identityとhashからPIDを作る | 初期化ownerまたは実行中instanceの取得済みbindingを返す。表示文字列を逆parseして資源を探さない |
| singleton init | `process_continuation.rs`の`SingletonInit`はuser initを実行し、成功後にprocessをallocateする。StandbyのPendingは同じflightでretryする | process本体がない期間にもdependencyを保持できるinit ownerを設ける |
| Worker init | `start_process_spawn`／`start_supervisor_spawn`がinit closureを実行し、`RuntimeContinuation::Spawn`が成功後に本体をallocateする | singletonと同じ資源取得・失敗・移譲の境界を使い、closureによるspawn入口を取りこぼさない |
| 本体・停止 | `ProcessInstance`はhandler bindingを持たない。`reclaim_stopped_process`は開始済み実行がなくなった後に本体を回収する | logical dependency所有を保存し、受付停止だけで開始済みwriteの資源を失わない |
| write | `out_handler_write`がPIDの表示文字列を解析し、毎回open/write/dropする | 解決済みsinkへ要求を登録し、保持したdescriptorで処理する |
| File資源 | `VmOpenFile`は`Rc<RefCell<File>>`のleaseを共有する。checkpointはleaseを保持し、rollbackで対応を戻す | この既存の所有原則に合わせる。一般FileHandleとsinkの論理所有を混同しない |
| 保存・破棄 | `checkpoint_for_chunk`／`rollback_to_checkpoint`、`rollback_open_files`／`shutdown_file_resources` | binding・未完了要求・復帰先と資源leaseを同じ保存位置へ対応させる |

現行の`handler_target_identity`／`parse_handler_target_identity`は、名前と引数を表示用文字列へまとめてから分解する。
修復ではFileOutHandlerの経路からこの処理を除き、原本の`RuntimeHandlerTarget.named_args`を直接読む。
path中のcommaや`=`を表示用区切りとして解釈しない。残る用途がなければ旧helperを削除する。
未知slot、未登録sink、異なるPID種別、binding欠落は内部不整合として拒否し、raw pathを開き直すfallbackを置かない。

## 提案する内部境界

### 1. 初期化とbinding

候補方式は、Boot override適用後、user initを呼ぶ前にdependencyを解決・取得する方式とする。
singletonのinit flightとWorkerのinit closure実行に、process本体とは別の一時ownerを持たせる。
取得済みbindingはそのownerに保存し、Standby retryでは取得し直さず、Ready成功時にinstanceへ移譲する。
失敗・init timeout・取消では、そのownerが取得したbindingだけを解放する。

これは採用済みの初期化規則ではない。未使用dependencyを先にopenする時点、Standby待機中の保持、Worker起動の失敗結果との整合を決める必要がある。
既存の成功後allocateを維持し、依存資源を持つためだけに偽のReady processを公開しない。
部分的な取得後に他slotのopenが失敗した場合、既存共有sinkを閉じず、新しいownerの取得分だけを解放する。

`ctx.<slot>`のloweringは現在process名とslotを渡す。新しいbindingを検索するにはinit ownerまたはinstanceを識別する実行contextが必要になる。
通常のhandler、init、callable/capture、内部wrapper、process内で起動する独立Taskの各入口を照合する。
「現在のtaskがないのでprocess名の全体設定へ戻る」というfallbackは設けない。
Taskへ持ち越すdependencyのleaseと、定義名だけをcaptureする現行経路の扱いは未確定事項に含める。

### 2. canonical sink registry

実行時keyはhandler種別、canonical path、append modeから構成する。PID表示とは別の型で保持する。
sink registryはkeyからsink本体を引き、同じkeyの取得は既存sinkを共有する。
sinkのIDはruntime管理の識別子とし、hashの一致だけで異なるkeyを同一視しない。
通常PIDの個体IDと同様、rollback後に失敗枝の資源IDを別資源へ再利用しない方式を候補とする。

sinkは、pathと不変identity、Fileの共有lease、論理受付状態、write queue、処理中の要求、利用ownerを区別して持つ。
Fileの可変OS状態を`ProcessRuntime`のCOW entry cloneで複製しない。leaseはdescriptorを共有し、offset・書いたbytesは外部状態として保持する。
一般のFile APIからsink内部のhandleをcloseできる経路は作らない。
既存`VmOpenFile`のlease処理を共通化する場合も、利用者FileHandleとsink bindingのnamespace・終了責務を分ける。

canonical pathには次の未確定事項がある。

- 相対pathをVMの`cwd`へ結び付ける時点。現行の一般File APIは`resolve_host_path`を使うが、FileOutHandlerはhostの`OpenOptions`へraw pathを渡す。
- create対象のfileがまだない場合のcanonical化。候補は存在する親directoryをcanonical化してleafを接続する方式だが、symlink、`..`、欠けた親、OS固有pathを定義する必要がある。
- 既存pathのsymlinkを解決する時点と、open前後のpath差替えの扱い。pathname identityと保持したdescriptorを区別し、TOCTOUがないと主張しない。
- 同じinodeを指す別hardlinkを統合するか。正本はcanonical path同一を条件としているため、inode同一での統合は追加仕様であり、この修復で暗黙に導入しない。
- shutdown後に同じcanonical keyを再取得する場合のlifecycle・identity。以前のdescriptorと新しいopenを同じ生存sinkとして混ぜない。

compilerのserialized targetをcanonical-path enumへ変更することは今回の第一候補に含めない。
正本第4.7節の構造例と現行metadataの差は、実行時解決の責務を記載する正本更新で整合させる。schema/versionを上げて旧形式を並行維持しない。

### 3. mailbox順のwriteと結果配送

FileOutHandlerのwriteは、取得済みbindingからsinkを選び、所有するtextと結果futureを一つの要求として登録する。
caller側は既存`BuiltinOutcome::Wait`とcontinuationで結果を待つ候補とする。
sink queueを進めるdriverは既存の共通schedulerへ接続し、一つのsinkにつき処理中driverを一つだけ持つ。
新しいbytecode opcodeやFileOutHandler専用process宣言を追加する前に、既存runtime continuation／detached executionで表せる範囲を確認する。

候補の要求状態は`Accepted → Executing → Completed`、受付後・未開始の取消は別の`Cancelled`とする。
受付時にqueue順を確定し、同じ要求を二重登録しない。通常再開では、完了済み要求にもう一度`write_all`したり、結果を二重配送したりしない。
write開始後はその同期OS呼出しが戻るまでVM側の取消を観測できない。戻った後に結果・受付状態・cleanupを処理する。
writeが部分成功してから失敗しても、書いたbytesを削除して回復しない。

要求順はsinkへの登録順を基準とする候補であり、別producerの起動順やwall-clock順を約束しない。
未開始要求の取消、caller timeout、結果futureを失った開始済み要求の扱いは既存call/Task規則と照合して決める。
Task自身の取消と、call timeoutで終了した待機を混同しない。他の依存processが同じsinkを使っている間、そのsink全体をcloseしない。

queueに包んでも、open、write_all、flushのOS待機は同期のままである。
この段階の受入はlifecycle、順序、再開・配送であり、RT-4全体のfairnessやOS I/Oの実時間上限を解決したとは扱わない。

### 4. checkpointと資源の終了

保存する論理状態は、init/instanceのbinding、受付、queue、進行状態、復帰先futureとの対応である。
通常切替えでは状態を移動し、checkpointは独立した論理保存位置を持つ。
資源leaseは保存側も保持し、rollbackは同じdescriptorへ対応を復元する。path差替え後に再openして復旧しない。
保存したFDをdropするだけの操作でlive側のbindingをcloseしない。

ここには正本同士を具体化する必要がある。
FileOutHandlerの「shutdown時close」と、Eldrの「checkpointが閉じる前のdescriptorを保持して復帰する」は、保存参照が残る場合に物理close時点が一致しない。
候補は、live側の論理終了で新writeを拒否・flush・live leaseを解放し、最後の保存lease破棄でOS descriptorを閉じる方式である。
保存位置へ復帰した枝が再びwriterになれる条件、flushの再実行、最後のactive ownerと保存ownerの区別を決めるまでは採用しない。
Rcの参照数をactive owner数として使わず、checkpoint cloneを新しい利用processとして登録しない。

`VM::clone`／`InteractiveVm::clone`は、読み取り専用checkpointとは別の問題である。
両cloneを実行したときのsink queue、active owner、physical FDの共有単位と終了権限を定める必要がある。
片方のVM終了で他方のFDを早期closeしないこと、保存状態の破棄だけでwriteを起動しないことを固定する。

外部writeが完了した後にrollbackすると、論理queueの保存位置は戻ってもbytesは残る。
現在の「外部I/Oを巻き戻さない」から、rollbackを跨ぐ厳密なexactly-onceは導けない。
保存済み未完了要求が失敗枝で実行済みになった場合の復帰を別途決める。
候補には、完了記録をrollback外へ残して同じ要求の再実行を抑える方式と、既存外部I/Oと同じ保証範囲に限定する方式がある。
前者を採る場合はrequest identity、記録の保持期間、保存位置のfuture復帰、失敗結果まで仕様化する必要がある。receipt registryを暗黙に追加しない。
通常再開の二重実行防止と、失敗chunk後の外部副作用に対する保証は別の受入条件として書く。

## エラー配送で未確定の点

| 場所 | 維持する契約 | 実装前に決める接続 |
|---|---|---|
| dependency open | 正本の`FileOutHandlerOpenFailed(path, detail)`とhandler-init診断 | singleton bootの`ProcessInitFailed`／初期化contextへどうcauseを保持するか。Worker spawnの`Result`へどう返すか |
| 通常write | `Err(FileOutHandlerWriteFailed(path, detail))` | queue driverでError constructorを既存Call/Resume経路に接続し、元caller位置とtraceを保持する |
| 不正metadata | RuntimeError | 未知slot・sink・PIDをpath reopenで救済しない |
| flush／shutdown | flush/closeを実行する | 現行FileOutHandlerにはflush専用Error定義がない。WriteFailedへ分類できるか、終了結果と報告先、既存エラーとの優先順位を確定する |
| 部分初期化の後処理失敗 | 元のinit失敗を失わずcleanupする | cleanupの副次エラーをどの構造へ残すか。stderrだけへ落として成功扱いにしない |

`RuntimeErrorKind`には現在`ProcessInitFailed`と`ProcessLifecycleFailed`があり、handler-init専用kindはない。
正本の診断コード、言語Error、Rust RuntimeErrorの3層を混ぜず、必要な正本を先に整合させる。
新しいError宣言やRuntimeErrorKind追加は、この仕様案では決定しない。
Errorを作るためにVMをRust helperから同期再帰駆動しない。利用者constructorは既存の継続経路で一度だけ呼ぶ。

## 未確定事項の優先順位

1. canonical path、絶対化時点、keyとlifecycle identityを決める。
2. dependency取得時点、init owner、Standby・Worker・捕捉Task、初期化失敗配送を決める。
3. logical shutdownとphysical close、checkpoint復帰、VM cloneの所有規則を決める。
4. writeの受付／取消／完了境界、rollbackを跨ぐ保証、flush失敗の配送先を決める。

未確定事項を残したままRustの実装へ進まない。局所的なcontainerやhelper名の選択と、利用者から観測できる上記の規則を区別する。

## 実装段階と直接の検証

| 段階 | 成果と依存 | 最初に固定するテスト |
|---|---|---|
| 0: 仕様確定 | 上記4群を確定し、ProcessRuntime・EldrVM・必要ならdiagnosticsの正本を更新 | 原本の再現と仕様の成功／拒否例を対応させる。仕様案の都合だけで現行契約を書き換えない |
| 1: 解決と初期化 | runtimeの構造化target解決、registry、init ownerとbindingの移譲。旧write-time openは最終差分で削除 | invalid parentを最初のwrite前に拒否、append/create、unknown metadata拒否、同じcanonical key共有、異なるkey分離、path中comma／`=` |
| 2: 寿命と復帰 | Standby retry、Worker init、停止、取消、checkpoint／clone資源の所有 | partial init失敗、retryでopen一度、複数ownerの片側失敗、開始済みwrite中のStop、保存前descriptorへ復帰、保存側dropでliveが有効 |
| 3: queueと配送 | 共通driver、FIFO、write失敗、shutdown処理 | 複数producerの登録順、small budgetで二重登録なし、未開始取消と開始済みcalleeの区別、結果一度、部分write失敗、flush失敗の決定した配送 |
| 4: 通常入口 | batch／InteractiveVm／REPLとbackground終了 | 公開Loggerのpath差替え、失敗chunkの復帰、外部bytesが残ること、rollback保証範囲、stdio capture／tee／REPL、NullOutHandler無出力 |

descriptorのopen／flush／close回数とqueue状態はEldrのcrate-localテストで決定的に確認する。
OSのwrite／flush失敗は制御可能な内部resourceテスト境界で固定し、権限設定や時間閾値に依存するテストにしない。
公開path差替えは対象OSでのfixture/integrationに置き、descriptor保持が観測できる結果を検証する。
同じ契約を全層へ重複配置しない。既存`namespaces.rs`のFileOutHandler入口、`vm.rs`のFile資源保存テスト、process停止テストを優先して拡張する。

段階ごとに最小範囲でRed→Greenを確認し、最終変更後にlevel4の全体gateを1回通す。

```sh
rtk cargo nextest run --profile ci --workspace
rtk proxy cargo run -- test --quiet --all
cargo fmt --all -- --check
git diff --check
```

最終差分、正本、失敗・取消・rollbackの結果を独立レビューへ渡す。デグレや新規問題はAstraへ相談する。
旧経路、表示文字列からの資源復元、raw pathへのfallback、schema/version更新は最終実装に残さない。

## この仕様案の検証状態

本書ではsource・正本・既存テストを読み、[実行再現の証拠](file_out_handler_lifecycle_reproduction_2026_10_13.md)へ対応させた。
新たなbuild、CLI、correctness test、性能測定は実行していない。
Astraへcanonical path、init owner、checkpointと物理close、VM clone、rollbackを跨ぐ保証、flush失敗の未確定事項を相談した。
指摘した境界は本書へ反映した。Astraが最終仕様案を読み、確定契約と候補・未決の区別、level4の段階案、直接テストの範囲に修正必須の指摘はなかった。
これは製品修復の完了や方式の承認ではない。
