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
  - editor 用 LSP、REPL command query parser、REPL 補完は大きな未解決領域として残っている。
  - `doc/lsp_analysis_context_spec_v0.md` では、active file を単体ではなく script / project / stdlib の `AnalysisContext` で解析する方針を整理した。
  - `doc/project_runner_pseudo_di_draft.md` では、project runner の profile selection、pseudo DI、operational script、REPL preload、LSP cache key を同じ runner context へ寄せる方針を整理した。
  - `docs/dev/Surtr_LSP_spec.md` では、`surtr-lsp` を protocol adapter とし、REPL と LSP が shared semantic service を直接使う実装方針を draft として固定した。
  - REPL command query parser は `spire` の source parser ではなく、仕様にロックインした小さい tooling query wrapper として整理する。
- 未確定点:
  - `surtr-analysis` 相当の shared analysis crate を新設するか、既存 `xldr` helper を先に分割して段階移行するか。
  - command query parser を `surtr-analysis::query` に留めるか、`surtr-query` crate として分離するか。
  - project runner 専用 `SourceKind` / `ProjectConfigSource` が必要か。
  - `RunnerArgs` の最終構造、`selected_profile` を top-level field に置くか runner args 内に置くか。
  - project runner source を Surtr VM で実行して抽出するか、restricted project config evaluator を用意するか。
  - typed boot builder API の正本名と、LSP が boot / supervisor config をどこまで semantic に理解するか。
  - external file missing / schema mismatch / handler override conflict を runner diagnostics と compile diagnostics のどちらへ所属させるか。
  - project context 付き script の `supervisor_init` merge 規則。
  - active file が複数 profile に属する場合の UI / diagnostics 優先順位。
  - REPL virtual document をどこまで LSP 対象に含めるか。
  - completion の型文脈利用を候補除外にするか、順位付けに留めるか。
  - iOS / wasm adapter が JSON-RPC LSP を使うか、editor UI から direct API を呼ぶか。
- 受け入れ条件:
  - `surtr-lsp` は active file を単体推測せず、必ず `AnalysisContext` 経由で parse / resolve / typecheck / completion / diagnostics を行う。
  - command query parser は `spire` へ入れず、REPL / LSP editor command から使える tooling query wrapper として配置される。
  - script entry を選択すると、script include 先 definition source が同じ compile unit 文脈で解析される。
  - project mode では selected profile、normalized runner args、module stage、project path 展開、boot / external input summary が cache key と diagnostics に反映される。
  - REPL は内部で LSP JSON-RPC と通信せず、Xldr と `surtr-lsp` が同じ semantic service を別 adapter として利用できる。
  - LSP / analysis core は single-thread wasm host でも動作でき、multi-thread availability に意味論を依存させない。
- テスト方針:
  - `surtr-analysis` 導入時は context resolver、include graph、cache key、LineIndex の byte / character / UTF-16 変換を unit test で固定する。
  - command query parser 導入時は `:doc` / `:sig` / `:info` / `:type` / `:facet` の query AST と validation diagnostics を unit test で固定する。
  - `surtr-lsp` 導入時は diagnostics / completion / hover / signatureHelp / definition の protocol DTO 変換を unit test で固定する。
  - `tests/fixtures/script/**`、`tests/fixtures/modules/**`、`lib/**/*.srt` を LSP analysis context の integration fixture として流用する。
  - project runner 実装後は profile 切り替え、glob 展開、external input diagnostics、active file profile membership の fixture を追加する。
  - REPL 共有化時は `cargo nextest run -p xldr` で既存 REPL completion / command query 表示を回帰基準にする。

### OI-030 Process runtime scheduler / Lazy init convergence

- 背景:
  - Process Runtime v2 の public surface は `docs/dev/ProcessRuntime_spec.md` へ整理済みだが、Lazy init、Ready 前 call、`Pending` / resume、init timeout、runtime status 表現はまだ VM 内部契約として完全に畳み切れていない。
  - `Process::sleep`、Task timeout、ReplyLater timeout、worker call timeout は deadline / future / waiting table を共有し始めており、今後の cleanup は surface 追加ではなく scheduler 内部契約の収束として扱う。
  - Worker wait API、generic receive、Task supervision は v2 public surface ではないため、この issue の対象外とする。
- 未確定点:
  - Lazy `@init` の `Pending` / `PendingAfter` retry と `init_waiters` を、通常の future / deadline queue とどこまで共通化するか
  - Ready 前 call を FIFO 待機にする場合の caller timeout、init timeout、init failure の優先順位
  - runtime process status を `Allocated` / `Initializing` / `Ready` / `Waiting` / `Failed` のどこまで VM snapshot / diagnostics に出すか
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

## 更新ルール

- 解決済み事項は本ファイルに残さず削除する。必要な履歴は正本仕様・関連 spec・コミット履歴で追跡する。
- 新規 Issue を追加するときは、少なくとも `背景`、`未確定点`、`受け入れ条件`、`テスト方針` を埋める。
- 実装先行で仕様が変わる場合は、先に本ファイルと正本仕様の整合を確認してからコード変更する。
