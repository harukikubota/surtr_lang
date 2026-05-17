# Surtr Open Issues

> 目的: V9 正本でまだ固定していない未解決事項だけを追跡する。
> 本ファイルは「未解決事項の台帳」であり、確定事項は `doc/要件定義v9.md`、開発者向け spec は `docs/dev/` 配下を正本とする。`doc/` は draft / input / tmp 置き場として扱う。cleanup で解消済みの項目は本ファイルに残さない。

最終更新日: 2026-05-17

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

### OI-017 Worker API 最終形

- 背景:
  - process runtime の大枠は `docs/dev/ProcessRuntime_spec.md` に固定したが、worker の user-facing API はまだ足場段階である。
  - `@call` / `@cast` は `CallResult` / `CastResult` 契約へ移行し、user-facing stop surface は `Stop(...)` に寄せた。
  - `Process::sleep` は scheduler timer、generic `Process::exit` は hidden のままにする方針も baseline として整理された。
- 未確定点:
  - `Worker::spawn` と `DynamicSupervisor::spawn` の最終 surface をどう並べるか
  - `adopt` / `handoff` を user-facing API として公開するか、runtime intrinsic に留めるか
  - `join` / `await` / `on_down` を generic `receive` の代替となる目的別 API としてどう分けるか
- 受け入れ条件:
  - worker 生成、所有権移譲、終了観測の surface が REPL / script / project で一貫する。
  - current process ownership を default とする方針と `DynamicSupervisor` 配下運用の両方を矛盾なく説明できる。
- テスト方針:
  - `tests/fixtures/script/pass/process_runtime/**` に worker spawn / await / on_down / supervisor 経由 spawn の成立ケースを追加する。
  - `tests/fixtures/script/fail/process_runtime/**` に不正な ownership / lifecycle API 組み合わせを追加する。

### OI-018 Process Runtime の最終同期項目

- 背景:
  - process runtime の基本契約は固まったが、ツーリング表示、boundary layer、VM 可視化、標準ライブラリ再編は最終段階で同期する想定のままである。
  - `Task::async -> TaskHandle`, `Task::await`, `Workers::submit/broadcast @timeout(...)`, waiting/deadline の baseline は docs と実装の主要経路で同期が進んだ。
  - worker stop の compile-time restriction と停止後 cleanup の主要経路は、現行の runtime / compile error / spec fixture で baseline 化が進んだ。
  - 現行実装でも `:doc`, `:sig`, `:type`, `:info`, `:facet`、runtime stats、標準 `Process` / `Task` API は存在するが、process-aware な見せ方は未完成である。
- 未確定点:
  - `ReplyLater` の layered timeout を正本 spec と runtime tests でどこまで明示固定するか
  - REPL / tooling 表示で init route、process API、singleton slot、supervisor tree をどこまで露出するか
  - domain error / runtime error / boot error を host outcome としてどう正規化するか
  - `RuntimeProcessSpec`, singleton slot, process table, deadline queue, waiting table, hidden message dispatch, supervisor tree をどの debug surface で見せるか
  - `Process`, `Task`, `File`, `Env`, `StdIn`, `StdOut`, `StdErr`, `Logger`, `DynamicSupervisor` の標準ライブラリ再編境界をどう切るか
- 受け入れ条件:
  - `ReplyLater` を維持するなら、outer timeout と callback 側 timeout の責務境界を正本 docs と tests の両方で説明できる。
  - 開発者向け spec と REPL / CLI / dump / viewer の観測導線が矛盾しない。
  - process runtime の host 境界と標準ライブラリ境界が `docs/dev/` と実装の両方で説明できる。
- テスト方針:
  - `ReplyLater` の timeout 契約を固定する場合は process runtime の spec / runtime test を追加して、outer timeout と callback 側 deadline の優先順位を回帰基準にする。
  - `unit/xldr` / `integration/repl` / `rune` integration で process-aware な表示と失敗形状を固定する。
  - `integration/build_roundtrip` / `run_eldr` / viewer 系テストで runtime metadata の可視化形状を固定する。

### OI-023 Task.Supervisor / Task-DynamicSupervisor link / worker lazy init

- 背景:
  - process runtime v2 では `Task`、`DynamicSupervisor`、worker lifecycle の土台は入ったが、Task supervision と worker lazy init は非対象として残っている。
  - `Task::async -> TaskHandle<T>`、`Task::await(task) -> Result<T>`、`Task::await @timeout(...)` は baseline として確定し、待機系 surface の最小契約は既存 spec と実装で同期した。
- 未確定点:
  - `Task.Supervisor` を独立 surface として持つか、`DynamicSupervisor` に統合するか
  - `Task` を supervisor 配下に link したときの ownership / restart / cancellation をどう扱うか
  - worker の lazy init や async init を `ProcessInit<T>` と別契約で導入するか
- 受け入れ条件:
  - Task と worker の lifecycle 契約が既存の singleton / worker / supervisor モデルと衝突しない。
  - supervisor / ownership / cancellation 契約を追加しても、既存の `TaskHandle` / `Task::await` baseline と衝突しない。
- テスト方針:
  - 仕様確定後に Task spawn/link/cancel、worker async init、supervisor 配下 completion の spec fixture を追加する。
  - compile-time 制約を入れる場合は process runtime 系 compile error fixture を増やす。

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
  - binary / directory / metadata を追加する場合も、`doc/要件定義v9.md`、`docs/dev/EldrVM_spec.md`、`lib/file.srt` の三者で同じ境界を説明できる。
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
  - 2026-05-16 の cleanup batches で CLI validation、std JSON docs、parser policy、source map、VM verifier の一部は処理済み。
  - ただし、process runtime / REPL 深部は今回の対象外とし、さらに大きめの panic-safe 化は個別設計が必要なため残す。
  - この issue は実装方針が固まった機能仕様ではなく、次回 cleanup の入力台帳として扱う。
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
    - process runtime の残 cleanup は、process surface / VM scheduling への影響範囲を分けてから扱う。
  - Xldr / REPL:
    - stage parser worker spawn の `expect` を diagnostic 化する。
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
  - 一方で `Enum::from(Int)` / `Enum::try_from(Int)` 相当の変換 helper 自動生成は未実装のまま残っている。
- 未確定点:
  - 暗黙生成するのが `from` だけか、`try_from` を含めた 2 系統か
  - out-of-range を compile-time ではなく runtime `Result` として扱うか
  - 生成先を enum owner module に置くか、共通 trait helper に寄せるか
- 受け入れ条件:
  - enum ordinal 変換 surface が `defenum` の public API と矛盾しない。
  - invalid ordinal の失敗形が docs / diagnostics / runtime で一貫する。
- テスト方針:
  - `unit/scar` / `unit/forge` で helper 生成契約を固定する。
  - `tests/fixtures/script/pass` と `tests/fixtures/script/fail` に valid / invalid ordinal 変換ケースを追加する。

### OI-029 `surtr-lsp` 実装ドラフト

- 背景:
  - editor 用 LSP、REPL semantic service、REPL 補完の LSP 共有化は大きな未解決領域として残っている。
  - `doc/lsp_analysis_context_spec_v0.md` では、active file を単体ではなく script / project / stdlib の `AnalysisContext` で解析する方針を整理した。
  - `doc/project_runner_pseudo_di_draft.md` では、project runner の profile selection、pseudo DI、operational script、REPL preload、LSP cache key を同じ runner context へ寄せる方針を整理した。
  - `docs/dev/Surtr_LSP_spec.md` では、`surtr-lsp` を protocol adapter とし、REPL と LSP が shared semantic service を直接使う実装方針を開発者向け正本として固定した。
  - `crates/surtr-analysis` を追加し、`LineIndex`、source-kind aware parse entry、`AnalysisContextRequest`、deterministic `AnalysisCacheKey`、semantic completion DTO の初期実装を置いた。
  - `resolve_context`、`RunnerContext`、context / runner diagnostics DTO、`AnalysisService` の最小 snapshot / diagnostics / completion API を追加し、LSP adapter が active file 単体ではなく context 経由で呼べる境界を作った。
  - 正規化済み runner 入力から literal / glob path を deterministic に展開して `RunnerContext` へ変換する `resolve_project_runner` を追加した。
  - `project.srt` の AST から現行 `Project::entrypoint(..., "profile", {|c| ...})` / `Config::add_path(...)` surface を抽出し、`resolve_context` から `RunnerContext` へ接続する最小経路を追加した。
  - active file が project profile の `Config::add_path` literal / glob 展開結果に含まれるかを `active_file_profiles` として固定し、複数 profile membership を保持する。
  - `crates/surtr-lsp` を追加し、file URI / UTF-16 position / diagnostics / completion text edit を `surtr-analysis` DTO へ写像する protocol adapter 境界を置いた。
  - `SourceKind` は `sindr::policy`、parse rule 導出は `spire` に置き、`surtr-analysis` は Xldr に依存しない構成にした。
  - project runner source は `SourceKind::ProjectConfigSource` として専用化し、CLI / host が project context として選択した場合だけ runner として機能する方針を固定した。
  - REPL command query parser は `surtr-analysis::query` に移し、Xldr は同じ parser 実装を呼ぶ。
  - VM 実行 runner result へ差し替えられる受け口として `ProjectRunnerResult` / `ProjectRunnerProfile` / `ProjectRunnerPath` DTO を追加し、現行 AST extractor は `Project::entrypoint` / `Config::entry_fun` / `Config::add_path` からこの形を生成する。
  - Eldr の `last_value()` / `TypeRegistry` から標準 `Project` / `Config` runtime value を `ProjectRunnerResult` へ decode する入口を追加した。
  - project runner の `Config::add_path` glob は deterministic order で展開し、`./src/**/*.srt` の recursive glob を `** = 0 個以上の directory segment` として扱う。
  - `AnalysisService` project mode は `RunnerContext.module_stages` を使い、active file 単体ではなく project profile の module stage として parse / resolve / typecheck できるようにした。
  - project stage の `DeclarationIndex` から補完候補を生成し、別 module の public declaration を LSP/analysis completion の初期候補へ流せるようにした。
  - completion item の `detail` / `documentation` / `sortText` と、metadata / declaration 由来情報を `surtr-analysis` の `SemanticIndex` で保持するようにした。
  - token 位置から semantic symbol を引く `lookup_symbol_at_cursor` を追加し、LSP hover は completion と同じ semantic index から detail / documentation を返す。
  - call context から active parameter を算出する signature help を `SemanticIndex` ベースで追加し、LSP DTO へ写像する最小経路を追加した。
  - active document / project stage source の declaration span を `SemanticIndex` に保持し、LSP definition DTO へ写像する最小経路を追加した。
  - active document の documentSymbol は `AnalysisService` snapshot から生成し、LSP DTO へ写像する最小経路を追加した。
  - Xldr `ReplEngine` から `surtr-analysis::SemanticIndex` を取り出せる API を追加し、REPL binding / stdlib doc / signature / declaration を shared semantic lookup へ渡せる入口を作った。
  - REPL completion の call argument 文脈では、expected type と合わない binding を除外せず、合う binding を先に出す順位付けへ変更した。
- 固定済み仕様:
  - shared analysis は `crates/surtr-analysis` として crate 新設で進める。既存 Xldr helper の段階移行は、この crate へ利用側を寄せる形で行う。
  - command query parser は `surtr-analysis::query` に留め、現時点では `surtr-query` crate へ分離しない。
  - project runner source は `SourceKind::ProjectConfigSource` として扱う。script として実行された場合は値を作るだけで、runner としては機能しない。
  - project runner source の抽出は、標準定義拡張を取り込めるよう最終的に Surtr VM 実行で行う。restricted evaluator は採用しない。
  - project context 付き script の `supervisor_init` merge は `process 定義 default < project runner boot config < script-local supervisor_init` の優先順位とする。
  - completion の型文脈利用は候補除外ではなく順位付けに留める。
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
  - completion の型文脈順位付けで使う score / sortText 規則。
  - iOS / wasm adapter が JSON-RPC LSP を使うか、editor UI から direct API を呼ぶか。
- 受け入れ条件:
  - `surtr-lsp` は active file を単体推測せず、必ず `AnalysisContext` 経由で parse / resolve / typecheck / completion / diagnostics を行う。
  - command query parser は `spire` へ入れず、REPL / LSP editor command から使える tooling query wrapper として配置される。
  - script entry を選択すると、script include 先 definition source が同じ compile unit 文脈で解析される。
  - project mode では selected profile、normalized runner args、module stage、project path 展開、boot / external input summary が cache key と diagnostics に反映される。
  - `AnalysisService` は parse / resolve / typecheck diagnostics、completion、hover、signatureHelp、definition、documentSymbol の protocol 非依存 DTO を返す。
  - REPL は内部で LSP JSON-RPC と通信せず、Xldr と `surtr-lsp` が同じ semantic service を別 adapter として利用できる。
  - LSP / analysis core は single-thread wasm host でも動作でき、multi-thread availability に意味論を依存させない。
- テスト方針:
  - `surtr-analysis` 導入時は context resolver、include graph、cache key、LineIndex の byte / character / UTF-16 変換を unit test で固定する。
  - `AnalysisService` の snapshot / parse diagnostics / completion と、project runner literal / glob 展開は `surtr-analysis` unit test で固定する。
  - command query parser は `surtr-analysis::query` の unit test と `cargo nextest run -p xldr` の既存 REPL command tests で固定する。
  - `surtr-lsp` 導入時は diagnostics / completion / hover / signatureHelp / definition / documentSymbol の protocol DTO 変換を unit test で固定する。
  - `tests/fixtures/script/**`、`tests/fixtures/modules/**`、`lib/**/*.srt` を LSP analysis context の integration fixture として流用する。
  - project runner 実装後は profile 切り替え、glob 展開、active file profile membership の fixture を追加する。external input diagnostics は boot / external summary 実装時に追加する。
  - REPL 共有化時は `cargo nextest run -p xldr` で既存 REPL completion / command query 表示を回帰基準にする。

## 更新ルール

- 解決済み事項は本ファイルに残さず削除する。必要な履歴は正本仕様・関連 spec・コミット履歴で追跡する。
- 新規 Issue を追加するときは、少なくとも `背景`、`未確定点`、`受け入れ条件`、`テスト方針` を埋める。
- 実装先行で仕様が変わる場合は、先に本ファイルと正本仕様の整合を確認してからコード変更する。
