# Surtr Open Issues

> 目的: 現行実装と対象領域の正本文書でまだ固定していない未解決事項だけを追跡する。
> 本ファイルは「未解決事項の台帳」であり、確定事項はソースコード、実行可能テスト、`docs/dev/`、`docs/site/`、標準定義 source の `@doc` へ置く。`doc/` は draft / input / tmp 置き場として扱い、cleanup で解消済みの項目は残さない。

最終更新日: 2026-09-29

---

## Open Issues
### OI-005 マクロ展開段階と通常解決段階の分離

- 背景:
  - 現行 baseline では macro 段階は実質 no-op 前提だが、宣言収集と通常解決の間に macro slot を置ける構成になっている。
  - 将来 macro を入れると、宣言集合・ID 割り当て・依存解決順に直接影響する。
- 未確定点:
  - macro 展開を declaration index 前後のどこに置くか
  - 展開生成物の `unique_id` / `tag` の決定規則をどうするか
- 受け入れ条件:
  - macro あり / なしで同値プログラムの解決結果が一貫する。
  - macro 段階と通常 resolve 段階の責務境界が文書化される。
- テスト方針:
  - macro 導入時に `unit/spire` / `unit/sigil` で段階境界テストを追加する。
  - 展開後 IR の決定性比較を回帰基準にする。

### OI-012 `.eldr` viewer follow-up

- 背景:
  - `.eldr` の viewer 向け chunk 基盤は入っているが、より深い debug 表示に必要な metadata はまだ最小限に留まっている。
  - `viewer.rs` 側にも source lookup や table 化の基盤はあるが、source compare や import 粒度の深掘りは未実装である。
- 未確定点:
  - `Dbgi` / `LocT` のような追加 table を入れるか
  - span に source id を持たせるか
  - import / literal metadata をどこまで viewer 向けに増やすか
- 受け入れ条件:
  - viewer が `.eldr` 単体で必要な debug 文脈を読める。
  - 追加 chunk 導入後も現行 decode 契約との後方整合が保たれる。
- テスト方針:
  - `unit/sindr` で chunk 整合性と参照先妥当性を固定する。
  - `integration` で dump 出力が必要テーブルを欠かさないことを維持する。

### OI-024 `File` v2 拡張境界

- 背景:
  - `File` v1 では `lib/file.srt` の `defmod File` として、UTF-8 text-only の `read` / `write` / `append` / `exists` / `delete` / `with_open` / `read_chunk` / `write_chunk` / `flush` を確定した。
  - resource lifetime は VM-owned open file table で管理し、`with_open` callback 終了時、VM run 終了時、interactive rollback 時の close を baseline として固定した。
  - 一方で、binary I/O、directory 操作、metadata、rename/copy、path surface、seek などは v1 から意図的に外している。
- 未確定点:
  - `Bytes` もしくは同等の binary surface を導入したうえで binary file API を追加するか
  - `mkdir`, `read_dir`, `rename`, `copy`, `metadata`, `canonicalize` のような host file-system helper を `File` に含めるか、別 module に分離するか
  - seek / cursor reposition を user-visible API として許可するか、それとも append/read sequential contract を維持するか
  - path を単なる `String` のまま扱うか、将来 `Path` 的な dedicated surface を持つか
  - file permission / mtime / size / kind を user-facing metadata としてどこまで露出するか
- 受け入れ条件:
  - v1 の cleanup guarantee と opaque `FileHandle` 契約を壊さずに拡張できる。
  - `FileOutHandler` の append-only runtime sink と、一般 file-system access の責務境界が docs と実装で混ざらない。
  - binary / directory / metadata を追加する場合も、`docs/site/file-io.md`、`docs/dev/EldrVM_spec.md`、`lib/file.srt` の三者で同じ境界を説明できる。
- テスト方針:
  - binary surface を導入する場合は `unit/sindr` / `unit/eldr` で runtime value と builtin contract を固定し、`lib/tests/*.srt` では `./tmp/sandbox/` 配下だけを使う。
  - directory / metadata surface を増やす場合は Rust integration で実ファイル状態を検証しつつ、spec/compile error のどこに置くかを `docs/dev/テスト方針.md` と同期する。
  - seek や cursor API を導入する場合は rollback / shutdown cleanup と両立することを `eldr` unit test で固定する。

### OI-026 FS / Shell surface naming and generic import ergonomics

- 背景:
  - FS / Shell v1 では `FileSystemPermissions` の permission flag を当初 `readonly` としていたが、`readonly` は field modifier として予約されているため field 名には使えない。
  - 実装では `FileSystemPermissions.read_only` として surface を固定した。
  - 同名 helper を持つ module を同じ file で unqualified import すると import conflict になる。`File.exists` と `FS.exists` はその具体例で、qualified call 自体は問題なく使える。
- 未確定点:
  - 予約語と同名の field を将来 escape syntax で許可するか、標準 surface では今後も別名を採用するか
  - `import FS::{path, join}` のような選択 import を推奨導線にするか
  - 同名 helper を持つ標準 module 同士を同時 import した場合の ergonomics を、alias import などで改善するか
- 受け入れ条件:
  - `FileSystemPermissions` の field 名が docs / stdlib / runtime display / tests で一致する。
  - `File` と `FS` の責務境界を崩さず、qualified call で常に曖昧性なく使える。
  - import ergonomics を改善する場合も、既存の import collision diagnostics を弱めない。
- テスト方針:
  - `lib/tests/file_system.srt` では `FS::*` を qualified call で使う形を維持する。
  - escape syntax や alias import を導入する場合は `compile_errors/modules` と `spec/modules` の両方に fixture を追加する。

### OI-027 Cleanup handoff backlog after 2026-05-16 batches

- 背景:
  - ただし、process runtime / REPL 深部は今回の対象外とし、さらに大きめの panic-safe 化は個別設計が必要なため残す。
  - この issue は実装方針が固まった機能仕様ではなく、次回 cleanup の入力台帳として扱う。
- 2026-10-10 の対応範囲:
  - [Process Runtime の停止契約](../docs/dev/ProcessRuntime_spec.md#3102-stop受付拒否停止完了) に基づき、通常 Stop の受付閉鎖、開始済み wrapper / ReplyLater の終了追跡、本体と不要な参照の回収、旧 PID への ProcessStopped、Weak 停止識別表の管理を実装した。caller timeout は callee を取り消さず、一度だけ結果を配送する。
  - state 読取・handler・後処理の段階を Handling / Postprocessing / Callback に分け、store / Stop / ReplyLater は生成 wrapper の後処理段階だけに制限する。停止前・停止要求中 checkpoint の復元と ID 非再利用、Workers / supervisor の完了時整理も対象とした。最終検証結果は[実行時の継続調査書](runtime_audit_followup_20261010.md#2026-10-10-停止回収修正の最終検証)に記録した。
  - この対応は process cleanup 全体、Lazy 初期化、未開始 FIFO、fairness、生成 call / capture 一般化、一般の restart / shutdown driver の完了を意味しない。
- 残タスク:
  - Spire:
    - process-owner pattern rewriting を `Annotated` / `Pin` / `Or` / `As` へ拡張する。これは process surface に触れるため後回し。
  - Sigil:
    - hidden builtin guidance metadata を table 化する。process hidden surface と絡むため後回し。
  - Scar:
    - supervisor intrinsic、worker message template、singleton PID rewrite の positional extraction は process 周りのため後回し。
  - Forge:
    - top-level failure path、error-result construction、variant payload extraction、result-error transform の重複 emission helper 化を検討する。
  - Eldr:
    - 停止・終了・回収と caller timeout の改修を除く process runtime の残 cleanup は、process surface / VM scheduling への影響範囲を分けてから扱う。Lazy / FIFO / fairness は OI-030 に残す。
  - Xldr / REPL:
    - `:save .eldr` / directory-ish names の validation、`:help` topic coverage、`:history` header/row format、command query pipe duplicate placeholder validationを整理する。
- 受け入れ条件:
  - process / REPL 領域は仕様・表示・integration の影響範囲を分けてから着手する。
  - panic / `unreachable!()` 除去は、既存の phase error 型 (`ParseError` / `ResolveError` / `TypeError` / `CodegenError` / `RuntimeError`) に寄せる。
  - 各 cleanup は小さな regression test または既存 targeted test で検証し、最後に `cargo nextest run --workspace` を通す。
- テスト方針:
  - Spire: `cargo nextest run -p spire`
  - Sigil: `cargo nextest run -p sigil`
  - Scar: `cargo nextest run -p scar`
  - Forge: `cargo nextest run -p forge`
  - Eldr: `cargo nextest run -p eldr`
  - REPL / Xldr: 再開時のみ `cargo nextest run -p xldr` と必要な `rune` integration を選ぶ。
  - 横断的な完了判定は `cargo nextest run --workspace` とする。

### OI-028 Enum conversion helper

- 背景:
  - `defenum` 本体や `.idx` 廃止は確定済み前提で進んでいる。
  - 一方で `Enum::to(Int)` / `Enum::try_to(Int)` 相当の変換 helper 自動生成は未実装のまま残っている。
- 未確定点:
  - 暗黙生成するのが `to` だけか、`try_to` を含めた 2 系統か
  - out-of-range を compile-time ではなく runtime `Result` として扱うか
  - 生成先を enum owner module に置くか、共通 trait helper に寄せるか
- 受け入れ条件:
  - enum ordinal 変換 surface が `defenum` の public API と矛盾しない。
  - invalid ordinal の失敗形が docs / diagnostics / runtime で一貫する。
- テスト方針:
  - `unit/scar` / `unit/forge` で helper 生成契約を固定する。
  - `tests/fixtures/script/pass` と `tests/fixtures/script/fail` に valid / invalid ordinal 変換ケースを追加する。

### OI-029 `surtr-lsp` 実装ドラフト

- 未確定点:
  - `RunnerArgs` の最終構造、`selected_profile` を top-level field に置くか runner args 内に置くか。
  - VM 実行で抽出する project runner result DTO は `ProjectRunnerResult` / `ProjectRunnerProfile` / `ProjectRunnerPath` を baseline とするが、boot config / external input facts の詳細 field は追加設計が必要。
  - `Project::entrypoint` / `Config::entry_fun` / `Config::add_path` 以外の runner facts を、VM 実行結果からどう生成するか。
  - project mode の stdlib stage injection を `xldr` と同じ semantic snapshot から共有するか、`surtr-analysis` 側に明示入力として渡すか。
  - staged project diagnostics を active file へ仮所属させず、module stage 内の source path / span へ正確に所属させる方法。
  - LSP definition の ambiguous tail match を診断化するか、qualified path 優先の解決へ寄せるか。
  - typed boot builder API の正本名と、LSP が boot / supervisor config をどこまで semantic に理解するか。
  - external file missing / schema mismatch / handler override conflict を runner diagnostics と compile diagnostics のどちらへ所属させるか。
  - active file が複数 profile に属する場合の UI / diagnostics 優先順位。
  - REPL virtual document をどこまで LSP 対象に含めるか。
  - 既存 REPL 補完 UI を、どの順序で `ReplEngine::semantic_index` ベースの候補生成へ置き換えるか。
  - `:doc` / `:sig` / `:info` の semantic resolver を `surtr-analysis` に置く境界。metadata-only symbol lookup から始め、typed call / typed operator / process metadata は段階移行する。
  - completion の型文脈順位付けで使う score / sortText 規則。
  - iOS / wasm adapter が JSON-RPC LSP を使うか、editor UI から direct API を呼ぶか。
- 受け入れ条件:
  - project mode では selected profile、normalized runner args、module stage、project path 展開、boot / external input summary が cache key と diagnostics に反映される。
  - LSP / analysis core は single-thread wasm host でも動作でき、multi-thread availability に意味論を依存させない。
- テスト方針:
  - `tests/fixtures/script/**`、`tests/fixtures/modules/**`、`lib/**/*.srt` を LSP analysis context の integration fixture として流用する。
  - project runner 実装後は profile 切り替え、glob 展開、active file profile membership の fixture を追加する。external input diagnostics は boot / external summary 実装時に追加する。
  - REPL 共有化時は `cargo nextest run -p xldr` で既存 REPL completion / command query 表示を回帰基準にする。

### OI-030 Process runtime scheduler / Lazy init convergence

- 背景:
  - Process Runtime v2 の public surface は `docs/dev/ProcessRuntime_spec.md` へ整理済みだが、Lazy init、Ready 前 call、`Pending` / resume、init timeout、runtime status 表現はまだ VM 内部契約として完全に畳み切れていない。
  - `Process::sleep`、Task timeout、ReplyLater timeout、worker call timeout は deadline / future / waiting table を共有し始めており、今後の cleanup は surface 追加ではなく scheduler 内部契約の収束として扱う。
  - Worker wait API、generic receive、Task supervision は v2 public surface ではないため、この issue の対象外とする。
- 2026-10-10 の対応範囲:
  - 通常 Stop は新規受付を閉じ、開始済み wrapper / ReplyLater callback の future / timer 待機からの再開と cleanup を終了まで追跡する。caller timeout は先に結果を配送し、callee を取り消さず、遅延 reply で結果を上書きしない。Task / init 自身の timeout 取消は別契約として維持する。
  - 停止完了時の本体、不要な waiting / reply / deadline / task / owner 参照と Workers / supervisor 所属の整理、回収後の ProcessStopped、Weak 停止識別の掃除、checkpoint 復元と ID 非再利用を実装した。scheduler 状態と受付状態、停止要求中の本体数・未完了実行数・停止識別 entry 数を観測で分ける。確定 future の結果は caller 用に保持する。
  - 現在の mailbox は未使用で、非空状態は RuntimeError とする。新たな未開始 queue を実装済みとはしない。Lazy 初期化全体、Ready 前 FIFO、fairness は残件であり、停止改修の最終検証件数・結果は既存監査に追記する。
- 未確定点:
  - Lazy `@init` の `Pending` / `PendingAfter` retry と `init_waiters` を、通常の future / deadline queue とどこまで共通化するか
  - Ready 前 call を FIFO 待機にする場合の caller timeout、init timeout、init failure の優先順位
  - PID 割当前の init flight、Lazy retry と Ready 前待機を VM snapshot / diagnostics へどう出すか。生存本体の scheduler 状態と停止受付状態の分離は確定済みとする
  - heavy process の fairness を step budget / scheduler quantum で扱うか、現行の pending point だけで十分とするか
- 受け入れ条件:
  - Lazy init、sleep、Task、ReplyLater、runtime-managed call timeout が同じ deadline / waiting cleanup 規則で説明できる。
  - Ready 前 call、init failure、timeout 後 reply、process down の競合で stale waiter / deadline / reply mapping が残らない。
  - process status と VM dump / Rune observability の表示が `docs/dev/ProcessRuntime_spec.md` と `docs/dev/Rune_observability.md` で矛盾しない。
- テスト方針:
  - `unit/eldr` で Lazy init retry、Ready 前 call FIFO、init timeout、timeout vs reply/down の cleanup を固定する。
  - `integration/run_srt` で singleton boot、worker call、ReplyLater timeout、Task timeout の成功・失敗 fixture を追加する。
  - VM dump / snapshot 形状を変更する場合は `cargo nextest run -p rune --test integration run_eldr` と関連 snapshot test を回帰基準にする。

### OI-031 Runtime Logger / handler target boundary

- 背景:
  - Process runtime の handler dependency は `StdIn` / `StdOut` / `StdErr`、`OutHandler` / `InHandler`、`FileOutHandler` override までを baseline としている。
  - Logger を public API として追加するか、handler target の差し替えだけで十分かは未確定である。
  - File v2 / FileOutHandler / runtime standard singleton の責務境界と衝突しやすいため、process runtime の標準 handler 拡張として別 issue で扱う。
- 未確定点:
  - Logger を singleton process として持つか、`OutHandler` target 群の一種として扱うか
  - 複数 producer から同一 sink へ出力する場合、sink 内 FIFO だけを保証するか、VM 全体の完全順序を保証するか
  - durability / flush / crash 時の扱いを best-effort に留めるか、明示 API を持つか
  - file sink と一般 `File` API の lifecycle / permission / error boundary をどう分けるか
- 受け入れ条件:
  - Logger を導入しても `Process`, `Task`, `Workers`, generated owner helper の public surface が増えすぎない。
  - handler override と VM dump / Rune observability から、どの sink に出ているか追跡できる。
  - FileOutHandler と一般 File API の責務境界が `docs/dev/ProcessRuntime_spec.md`、`docs/dev/EldrVM_spec.md`、`lib/*.srt` で一致する。
- テスト方針:
  - handler target を増やす場合は `unit/sigil` / `unit/scar` / `unit/forge` で capability と init args を固定する。
  - runtime sink を増やす場合は `unit/eldr` で ordering、flush、error result、resource cleanup を固定する。
  - public Logger surface を追加する場合は `tests/fixtures/script/pass` と `tests/fixtures/script/fail` に最小 fixture を置く。

### OI-032 Mass process benchmark harness

- 背景:
  - Process runtime の correctness fixture は増えているが、大量 process、worker set、message dispatch、deadline queue、reply future の負荷傾向を比較する標準 benchmark はまだない。
  - 旧ドラフトでは単一 manager と大量 worker の採取シナリオで、process 数、message 量、waiting / timeout / reply 処理、最大 RSS を測る案を整理していた。
  - これは言語仕様ではなく開発用 benchmark harness であり、正本仕様には測定対象と基準シナリオだけを残す。
- 未確定点:
  - benchmark を Rust integration / standalone CLI / Surtr script fixture のどこに置くか
  - RSS / CPU / frame count / message count / deadline queue count をどの形式で記録するか
  - 乱数 seed、worker 数、終了条件、timeout policy を CLI option と fixture のどちらで固定するか
  - 単一 manager 集中モデルに加えて、manager shard モデルをいつ追加するか
- 受け入れ条件:
  - 同一 seed / 同一 worker 数で比較可能な benchmark 結果を得られる。
  - 少なくとも worker count、total messages、waiting max、future/deadline count、elapsed time、max RSS を記録できる。
  - benchmark は通常の correctness suite から分離し、`cargo nextest run --workspace` の安定性を悪化させない。
- テスト方針:
  - harness 自体は小規模設定で deterministic に終了する smoke test を持つ。
  - 大規模設定は手動または専用 profile で実行し、CI の通常 profile には入れない。
  - VM stats / dump の field を増やす場合は `docs/dev/Rune_observability.md` と snapshot test を同期する。

### OI-033 Compiler warning public surface follow-up

- 背景:
  - ただし、現時点では warning buffer は compiler 内部 API のみに留め、Rune CLI / JSON / REPL / LSP 表示には接続していない。
  - 利用者向けの現状説明は `docs/site/warnings.md` にまとめたが、表示・抑制・厳格化の policy は未確定である。
- 未確定点:
  - `surtr check` / `run` / `build` / `test` / REPL で warning をいつ、どの format で表示するか。
  - JSON diagnostics に `warnings` を追加する場合、既存 `errors` と同じ location / kind / phase / hint 形状に揃えるか。
  - `--deny-warnings`、`--warn` / `--allow`、project-level lint config、source-level suppress attribute のどこまでを導入するか。
  - warning の severity、warning code、machine-readable stable id を持つか。
  - `WarningSpan` が現在持つ plain offset を、将来 `SourceId` / file path / module stage とどう結びつけるか。
  - `import Mod` の unused member 警告を style/lint phase として追加するか。
  - `_name` を unused variable escape として扱うか、現行どおり `_` のみを特別扱いするか。
  - standard library / generated code / compiler-generated ids 由来の warning をどこまで抑制するか。
  - Phase1 以外の warning 候補をどの段階で導入するか。
    - `UnusedImportModule`: `import Mod` で取り込んだ module member が file 内で一度も unqualified use されていない。
    - `RedundantQualifiedImport`: 明示 import した名前を qualified call でしか使っていない。
    - `UnreachableMatchArm`: 先行 pattern に覆われる match arm。Rust の DeadCode 相当ではなく pattern 到達不能性に限定する。
    - `RedundantWildcardArm`: enum / boolean などで前段 arm が全ケースを覆っており、最後の `_` が到達不能。
    - `SuspiciousShadowing`: 近い scope で同名 binding を shadow しており、外側 binding がその後参照されない。既存の shadowing 許可方針を弱めず warning に留める。
    - `RedundantTypeAnnotation`: 推論結果と完全一致する局所型注釈。公開 signature や docs として意味を持つ注釈は対象外にする。
    - `RedundantTraitBound`: 型引数 bound が signature / method body 解決に寄与していない。trait の単純方向解決を壊さない範囲に限定する。
    - `IgnoredResult`: `Result<T>` を明示 `;` で捨てている。現行の unused value escape と衝突するため、導入するなら opt-in lint から始める。
    - `DeprecatedSurface`: stdlib API を置き換えるときの移行 warning。標準定義ソースの `@doc` / annotation と連動させるかは未確定。
    - `NonCanonicalImport`: auto import される標準 helper を明示 import しているなど、意味は同じだが読み味が揺れる import。
    - `SuspiciousUnitReturn`: `Result<Unit>` や `Unit` が絡む block で、最後の式だけが意図と逆に見える形。誤検出しやすいため候補止まり。
- 受け入れ条件:
  - warning 表示を追加しても、error の exit code / JSON 契約 / Ariadne 表示の後方互換性を壊さない。
  - warning の location が multi-source module、include、stdlib、REPL preload で正しい source に対応する。
  - Phase1 の `_with_warnings` API と既存 API の互換関係を保ち、既存 caller は引き続き warning を無視できる。
  - suppress / deny を導入する場合は、利用者向け docs と `docs/dev/` の診断仕様が一致する。
  - Phase1 以外の warning は DeadCode 全般検出へ拡張せず、scope / import / pattern / type signature の局所解析で説明できる範囲に留める。
- テスト方針:
  - `unit/sindr` で warning kind / code / serialization 契約を固定する。
  - `unit/sigil` / `unit/scar` で warning 抑制・厳格化しても phase 内検出が変わらないことを固定する。
  - `unit/diagnostics` で warning rendering と JSON shape を固定する。
  - `integration/rune` で `check` / `run` / `build` / `test` / REPL の human / JSON 出力と exit code を固定する。
  - LSP 接続時は `docs/dev/Surtr_LSP_spec.md` と連動し、warning severity / range / source mapping を protocol DTO test で固定する。
  - 追加 warning 候補は一括導入せず、warning kind ごとに最小 fixture と誤検出しない negative case を追加する。

### OI-034 FacetAPI 引数間推論による FacetPath 補完

- 背景:
  - REPL FacetPath 補完では、型 root / 値 root / Facet binding focus / `Result` 透過 / Tuple / List / HashMap の候補表示を強化した。
  - `over(&Us` のような capture sugar は FacetPath を値として返せないため候補を出さない方針にした。
  - `over(_.` のように source 型が未確定な placeholder path も、単独では候補を出さない方針にした。
  - 一方で、FacetAPI の後続引数から source 型が確定する場合は、第一引数が `_` root でも候補を出せる可能性がある。
- 未確定点:
  - `over(_.//, user, )` のような FacetAPI 入力中に、後続引数または既存引数から第一引数 `_` の source 型を逆算するか。
  - FacetPath 専用補完と関数入力ヘルプの責務境界を、`surtr-analysis` と Xldr REPL のどちらで固定するか。
  - チルダ展開で第一引数が埋め込み済み扱いになる場合、第二引数の completion / signature help 表示をどのタイミングで切り替えるか。
  - LSP completion / REPL completion / signature help で同じ推論結果を共有する API 形状をどうするか。
- 受け入れ条件:
  - source 型が文脈から一意に決まる場合だけ、`_` root の FacetPath segment 候補を表示する。
  - source 型が未確定または複数候補に分岐する場合は、現行どおり候補を出さず、誤った root 候補を表示しない。
  - `over(&...)` capture sugar には引き続き FacetPath root 候補を出さない。
  - FacetPath 補完の表示は FacetAPI 内外で一貫し、API 自体の説明は function input help 側に留める。
- テスト方針:
  - `cargo nextest run -p xldr --test repl_core` に、後続引数から `User` 型を推論できる FacetAPI 入力と、推論不能な `_` root 入力の両方を追加する。
  - `cargo nextest run -p surtr-analysis --test completion` に、FacetPath context と call argument context の境界テストを追加する。
  - LSP 側へ共有する場合は `surtr-lsp` の completion / signatureHelp DTO 変換テストも追加する。

### OI-035 generic `defrecord` と constructor parameter

- 背景:
  - N03のnominal constructor parameterは、MonadTに必要な`defstruct`と`defenum`を対象に実装した。
  - `defrecord`はrecord固有のgrammar、値表現、constructor surface、Facet再構築を持つため、同じ実装済み範囲には含めない。
- 未確定点:
  - generic parameterとdeclaration `where` constraintをrecord grammarへどう導入するか。
  - positional / named constructor surfaceとrecordの値表現へ型argumentをどう保持するか。
  - type-changing Facet updateでgeneric record全体をどう再構築し、destination boundをどこで検証するか。
- 受け入れ条件:
  - `defstruct`のparser経路を場当たり的に流用せず、record grammarとconstructor surfaceを別仕様で確定する。
  - generic recordの型形成、値構築、pattern、Facet再構築が同じnominal identityとdeclaration constraintを保持する。
  - MonadT N03–N05の完了条件や標準Transformerの残作業へ混入させない。
- テスト方針:
  - 仕様確定後、Spireでrecord grammar、Sigilでowner / parameter scope、Scarでwell-formednessとFacet destination、Forge / Eldrで値表現を責務ごとに固定する。
  - parserだけを先行して`defstruct`のgeneric surfaceへ合わせるテストは追加しない。

### OI-036 Facet capture の残る検証範囲

- 確定済みの契約:
  - [Eldr VM 仕様](../docs/dev/EldrVM_spec.md)はFacet API captureを `Facet` / API名で表示し、`&Type.path`・`&p`・`_.path` の読み取りを `Facet` / `view` とする。path ownerを表示identityにする案は未確定事項として残さない。
  - 通常のclosure literalとの区別、capture位置で確定したsignature、partial applicationと変数経由の再captureによる由来の保持も同仕様に従う。
- 未確認点:
  - optional / fallibleな複数segment、Facet由来のpartial application・変数経由の再capture、REPLの直接bindingとnested field表示について、契約を覆うテストの対応表がまだ揃っていない。
  - 既存fixtureの実行成功だけでは、すべての経路で表示metadataまで検証したことにはならない。
- 受け入れ条件と次の作業:
  - 上記の残る経路を既存テストと照合し、不足する境界だけを追加する。type acceptance、Facet dispatch、privacy checksは変えない。
  - 確定済みのidentity契約に沿った検証が揃った時点で、この項目を削除する。

## 更新ルール

- 解決済み事項は本ファイルに残さず削除する。必要な履歴は正本仕様・関連 spec・コミット履歴で追跡する。
- 新規 Issue を追加するときは、少なくとも `背景`、`未確定点`、`受け入れ条件`、`テスト方針` を埋める。
- 実装先行で仕様が変わる場合は、先に本ファイルと正本仕様の整合を確認してからコード変更する。
