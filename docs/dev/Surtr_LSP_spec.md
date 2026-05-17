# Surtr LSP 実装仕様書

> 本書は Surtr の editor tooling と `surtr-lsp` 実装の開発者向け契約を定義する。
> 入力 draft は [../../doc/lsp_analysis_context_spec_v0.md](../../doc/lsp_analysis_context_spec_v0.md) と
> [../../doc/project_runner_pseudo_di_draft.md](../../doc/project_runner_pseudo_di_draft.md) である。
> `docs/dev/` 配下に置く本書を現在の開発者向け正本とし、`doc/` 配下の draft は
> 本書へ反映済みの入力資料として扱う。未確定点は本書の「未確定事項」と
> [../../doc/open-issues.md](../../doc/open-issues.md) の `surtr-lsp` issue で追跡する。

---

## 1. 目的

`surtr-lsp` は、Surtr source を editor 上で読むための protocol adapter である。
言語意味論の正本ではなく、既存 compiler pipeline と共有 analysis service の結果を
LSP へ写像する。

LSP 実装の主目的は次のとおり。

- `.srt` source を単体 file ではなく、script / project / stdlib / REPL preload の文脈で解析する
- parser / resolver / typechecker の diagnostics を editor range へ安定して対応させる
- completion / hover / signature help / definition で、Sigil / Scar / stdlib `@doc` metadata と同じ semantic surface を使う
- project runner の profile、module stage、boot input、external injection 状態を cache key と diagnostics に含める
- REPL 補完と LSP 補完が、同じ semantic service を別 UI から使う構造へ移行できるようにする
- 将来の iOS / wasm + webview editor でも、single-thread host 上で analysis core を再利用できるようにする

---

## 2. 非目的

本書では次を正本化しない。

- VSCode extension の package name、command name、settings schema
- LSP transport 実装ライブラリ
- project runner DSL の最終 surface
- REPL live chunk の実行意味論
- rename / code action / formatter の完全仕様

VSCode extension の機能案は [../../doc/vscode_extension_features_naming_surtr.md](../../doc/vscode_extension_features_naming_surtr.md) を参照する。
REPL の外部契約は [./Xldr_spec.md](./Xldr_spec.md) を正本とする。

---

## 3. 基本方針

### 3.1 LSP は compiler pipeline の利用者である

`surtr-lsp` は独自 parser、独自 resolver、独自 typechecker を持たない。
解析は次の pipeline 境界を守る。

```text
parse      : &str -> Vec<Ast>
resolve    : Vec<Ast> -> Vec<Resolved>
typecheck  : Vec<Resolved> -> Vec<TypedNode>
analysis   : typed / resolved / doc metadata -> editor-facing semantic index
```

Forge / Eldr は通常の editor diagnostics には不要である。bytecode viewer や debug surface を
editor から起動する場合も、LSP core とは別の tool command として扱う。

### 3.2 REPL は LSP と通信しない

REPL が内部的に LSP JSON-RPC とやり取りする構造にはしない。
REPL と LSP は、同じ `surtr-analysis` 相当の service を直接呼ぶ。

```text
xldr REPL adapter   -> shared semantic service
surtr-lsp adapter   -> shared semantic service
wasm editor adapter -> shared semantic service
```

REPL 固有の `:` command、履歴、行番号、append-only binding、`$name` forced binding、
`.eldr` save / restore は Xldr に残す。共有対象は、symbol lookup、doc / signature /
completion candidate / semantic display の解決部分である。

### 3.3 wasm / iOS は single-thread を正規 target とする

LSP / analysis core は single-thread event loop で動作できることを必須にする。
native editor では worker thread や background task を使ってよいが、意味論と API は
thread availability に依存させない。

wasm + webview では次の形を許す。

- JSON-RPC LSP を立てず、editor UI から analysis API を直接呼ぶ
- Web Worker が使える host では worker に analysis service を置く
- iOS WebView など worker / shared memory 制約が強い host では main thread 上で incremental cache を小さく保つ

---

## 4. crate 境界

目標構成は次のとおり。

| crate | 役割 |
|---|---|
| `surtr-analysis` | protocol 非依存の context resolver、document store、semantic index、completion / hover / diagnostics service |
| `surtr-analysis::query` | REPL / editor command query surface を parse する小さい仕様ロック済み wrapper |
| `surtr-lsp` | LSP JSON-RPC adapter。URI / UTF-16 position / capability negotiation を `surtr-analysis` の DTO へ写像する |
| `xldr` | REPL session と UI adapter。`:` command routing と session state を持ち、query parser と semantic service を直接使う |
| `rune` | CLI dispatch。`surtr lsp` を追加する場合は process 起動のみ担う |

`SourceKind` は `sindr::policy` に置き、parse rule 導出は `spire` が提供する。
これにより `surtr-analysis` は Xldr の REPL session / UI / line editor に依存しない。

`surtr-analysis` は原則として次に依存する。

- `diagnostics`
- `sindr`
- `spire`
- `sigil`
- `scar`

通常の editor diagnostics では `forge` / `eldr` に依存しない。REPL adapter は Xldr 経由で
VM 状態を持つため、REPL binding completion だけは Xldr session state を入力として渡す。

REPL command query parser は `spire` に置かない。Surtr source grammar ではなく、
[../../doc/xldr_command_query_api_spec.md](../../doc/xldr_command_query_api_spec.md) に
ロックインした tooling query surface であるため、`surtr-analysis::query` の小さい module
として固定する。Xldr は `ReplQuery` 互換の alias でこの実装を呼ぶ。
`surtr-query` crate へ分離するのは、query parser が `surtr-analysis` の他責務から
独立して versioning したくなった場合だけでよい。

---

## 5. AnalysisContext

LSP は active file だけで解析しない。必ず `AnalysisContext` を作ってから
parse / resolve / typecheck / completion / diagnostics を行う。

```text
AnalysisContext {
  workspace_root: Path
  mode: Script | DefinitionCheck | Project | ReplPreview
  entry_file: Option<Path>
  active_file: Path
  source_kind: SourceKind
  stdlib_stage: StdlibStageSet
  module_stages: Vec<ModuleStage>
  include_graph: IncludeGraph
  runner: Option<RunnerContext>
  script_project: Option<ScriptProjectContext>
  repl: Option<ReplAnalysisContext>
}
```

### 5.1 mode と source_kind

`mode` は `sindr::policy::CompileUnitKind` に対応する。

| `AnalysisContext.mode` | 対応 | 用途 |
|---|---|---|
| `Script` | `CompileUnitKind::Script` | script entry を起点に解析する |
| `DefinitionCheck` | `CompileUnitKind::DefinitionCheck` | entry 未選択の definition source を単体検証する |
| `Project` | `CompileUnitKind::Project` | project runner が解決した profile / module stage で解析する |
| `ReplPreview` | `CompileUnitKind::Repl` | 将来の REPL virtual document / preload context 表示用 |

`source_kind` は `sindr::policy::SourceKind` を使う。project runner source は
CLI / host が `SelectedContext::ProjectProfile` として選択した場合だけ
`ProjectConfigSource` として扱う。通常 script として実行された同じ source は
単なる Surtr 値を作るだけであり、project runner としては機能しない。

| 対象 | `source_kind` |
|---|---|
| script entry file | `Script` |
| script `include` 先 | `DefinitionSource` |
| project runner が追加した user module | `DefinitionSource` |
| project runner source | `ProjectConfigSource` |
| stdlib source | `StdDefinitionSource` |
| REPL virtual input | `ReplChunk` |

`Project::add_path(...)`、script `include`、project runner 由来 file は token sniffing で
script に切り替えない。
project runner source の補完 / diagnostics は `ProjectConfigSource` の通常 source
diagnostics と host-side runner diagnostics を分けて扱う。

### 5.2 RunnerContext

project mode では `runner_args` を予約 field のまま扱わない。
LSP / compiler / cache が見る入力は、ENV などの hidden input を正規化した明示的な
runner context である。

```text
RunnerContext {
  project_file: Path
  selected_profile: String
  normalized_args: RunnerArgs
  resolved_paths: Vec<ResolvedProjectPath>
  active_file_profiles: Vec<String>
  module_stages: Vec<ModuleStage>
  boot_summary: ProjectBootSummary
  external_inputs: Vec<ExternalInputState>
  diagnostics: Vec<RunnerDiagnostic>
}
```

`ENV=TEST` のような入力は、profile default selection に使ってよい。
ただし analysis cache key と diagnostics の正本入力は `selected_profile = "test"` のような
正規化済み field とする。

`ResolvedProjectPath` は少なくとも次を持つ。

```text
ResolvedProjectPath {
  declared_by: Path
  literal_or_glob: String
  declaration_span: Span
  expanded_files: Vec<Path>
  source_kind: DefinitionSource
}
```

glob を許可する場合、展開順は deterministic に固定する。cache key には glob pattern、
展開後 file list、各 content hash を含める。

### 5.3 ScriptProjectContext

operational script が project context を借りる場合、script include block の
`load_project` directive を LSP が実行なしに読める必要がある。

```text
ScriptProjectContext {
  directive_span: Span
  project_file: Path
  selected_profile: String
  project_context: RunnerContext
  script_local_includes: IncludeGraph
}
```

`load_project` は literal-only directive として扱う。変数、関数呼び出し、文字列結合は
context resolution では受けない。

project context 付き script の `supervisor_init` merge 規則は次の優先順位で固定する。

```text
process 定義 default < project runner boot config < script-local supervisor_init
```

同じ singleton / handler / supervisor policy を二重指定した場合は、この優先順位に従って
後段が前段を上書きする。上書きされた事実を diagnostics / trace に出すかどうかは
実装判断でよいが、意味論上は暗黙の競合エラーにはしない。

### 5.4 ReplAnalysisContext

REPL は初期段階では LSP の対象外でもよい。ただし将来の REPL virtual document や
web editor 一体型 REPL のため、次の context を予約する。

```text
ReplAnalysisContext {
  session_id: String
  phase: Bootstrap | Preload | Live
  preload_context: Option<AnalysisContext>
  live_bindings: Vec<ReplBindingSummary>
  command_surface: ReplCommandSurface
}
```

`ReplAnalysisContext` は Xldr の session state を正本とし、LSP は mirror として読む。
REPL command query grammar を通常 source の parser として使わない。

---

## 6. Context Resolution

LSP adapter は file URI から直接 parse / resolve / typecheck を呼ばない。
まず context request を作り、`surtr-analysis` の context resolver へ渡す。

```text
AnalysisContextRequest {
  workspace_root: Path
  active_file: Path
  selected_context: Option<SelectedContext>
  runner_selection: Option<RunnerSelection>
  open_documents: Vec<DocumentVersion>
}
```

`SelectedContext` は client UI や CLI 起動引数から明示された解析起点である。
自動探索より常に優先する。

```text
SelectedContext
  = ScriptEntry(Path)
  | ProjectProfile { project_file: Path, profile: String }
  | DefinitionStandalone
  | DefinitionUnderEntry { entry_file: Path }
  | StdlibDevelopment
  | ReplPreview { session_id: String }
```

### 6.1 自動探索

`selected_context` がない場合、resolver は次の順に context を選ぶ。

1. `active_file` が現在保持している script / project / repl preview context に含まれるなら、その context を使う
2. `active_file` が repository の `lib/**/*.srt` で、`lib/tests/**` ではないなら `StdlibDevelopment` として扱う
3. `active_file` が `tests/fixtures/script/{pass,fail}/**/*.srt` なら script entry 候補として扱う
4. `active_file` が `tests/fixtures/modules/{pass,fail}/**/entry.srt` なら module fixture entry 候補として扱う
5. project runner が `active_file` を含む profile を 1 つだけ返すなら、その `ProjectProfile` を使う
6. 複数 profile / 複数 entry 候補がある場合は勝手に 1 つへ固定せず、context selection diagnostics と status を返す
7. どの候補にも入らない definition source は `DefinitionStandalone` として扱う

fixture path の特別扱いは Surtr repository 開発時の利便性であり、言語意味論を追加しない。
外部 project では user / client が明示した script entry、project profile、standalone
definition の選択を正本にする。

### 6.2 script context

script entry が選択された場合、resolver は script 先頭の include block を既存 loader と
同じ規則で読む。include 先は script file からの相対 path として解決し、
`SourceKind::DefinitionSource` として扱う。include 先 file を token sniffing で
`SourceKind::Script` に切り替えない。

script context の compile unit は次の順を持つ。

```text
StdlibStageSet
  -> include graph 由来 ModuleStage
  -> script entry SourceKind::Script
```

include directive が source 先頭 block の外に出た場合や、literal path として読めない場合は、
parser / loader diagnostics として返す。

### 6.3 project context

project context は `RunnerContext` を正本にする。resolver は `project_file`、明示 profile、
host default、ENV などを読んでよいが、analysis cache key と diagnostics に残す入力は
`selected_profile` と `normalized_args` に正規化する。

`Project::add_path(...)` 由来 file は `ResolvedProjectPath` に残す。glob を使う場合は
展開順を deterministic に固定し、no match / unreadable / schema mismatch は
`RunnerDiagnostic` として保持する。

project source 自体は `SourceKind::ProjectConfigSource` として扱う。project runner として
機能するのは CLI / host が project context として起動した場合だけである。同じ file を
script として実行した場合は、標準 `Project` / `Config` API が値を作るだけであり、
module stage / boot / external input は host へ反映されない。

project runner source は標準定義の拡張を素直に取り込めるよう、最終的には Surtr VM 実行で
抽出する。restricted evaluator は採用しない。LSP / analysis は VM 実行結果から正規化された
`RunnerContext` を受け取り、profile unknown、glob no match、external input failure などは
`ProjectRunner` diagnostics として返す。

### 6.4 standalone definition context

standalone definition は `CompileUnitKind::DefinitionCheck` と
`SourceKind::DefinitionSource` を使う。file 単体で確定できる parse / declaration /
duplicate / builtin misuse error は通常 diagnostics として表示する。

外部 entry context があれば解決できる可能性のある unresolved symbol は、
通常の unresolved symbol と区別して `ContextSelection` diagnostics にできる。
client はこの diagnostics に対して「script entry / project profile を選ぶ」action を
提示してよい。

### 6.5 stdlib development context

`lib/**/*.srt` は `SourceKind::StdDefinitionSource` として扱う。
`@builtin def` / `@builtin type` / `@autoimport` は標準定義 source の宣言層として許可するが、
builtin 追加・変更の正本は `crates/sindr/src/builtin.rs` の `BUILTIN_METAS` である。

`lib/tests/**` は default stdlib stage へ含めない。stdlib fixture や test DSL として
解析する場合は、script / project / test context を明示的に選ぶ。

---

## 7. Semantic Service

`surtr-analysis` は protocol 非依存 API を提供する。

```text
AnalysisService
  resolve_context(request) -> AnalysisContext
  update_document(uri, text, version) -> DocumentId
  analyze(context) -> AnalysisSnapshot
  diagnostics(snapshot, active_file) -> Vec<AnalysisDiagnostic>
  completions(snapshot, position) -> CompletionResult
  hover(snapshot, position) -> Option<HoverResult>
  signature_help(snapshot, position) -> Option<SignatureHelpResult>
  definition(snapshot, position) -> Vec<Location>
  document_symbols(snapshot, active_file) -> Vec<DocumentSymbol>
```

`AnalysisSnapshot` は parse / resolve / typecheck の結果、doc metadata、visible scope、
type environment、declaration index、import surface、source map を保持する。

REPL と共有する semantic resolver は次を担う。

- public / private / hidden / user-callable 判定
- `Global::` を user-facing 表示から隠す surface name 変換
- `Self` を concrete owner type へ正規化した signature 表示
- type constructor、module owner、qualified member、function、operator/helper の候補収集
- `@doc` と signature metadata の lookup
- call context から active parameter と expected type を出す signature help
- typed call / typed operator query の意味解決

### 7.1 Command Query Parser

command query parser は Surtr source parser ではない。`spire` の責務は `.srt` source の
正本 grammar と CST / AST 生成であり、REPL query surface はその外側に置く。

query parser は次の入力だけを扱う。

- `:doc <target>`
- `:sig <target>`
- `:info <target>`
- `:type <binding>`
- `:facet <facet-target>`

parser の出力は、評価可能な式ではなく query AST である。

```text
CommandQuery
  = DefinitionLookup(...)
  | BindingLookup(...)
  | ConstructorLookup(...)
  | ExtractorLookup(...)
  | TypedCallDispatch(...)
  | TypedOperatorDispatch(...)
  | FacetLookup(...)
```

この parser は、`literal`、任意式、generic type variable、nested function call as value、
pipe placeholder の式利用を受けない。受けないものを明確にすることで、LSP / REPL の
doc / signature / info query が Surtr 本体 grammar と独立して安定する。

置き場は `surtr-analysis::query` とする。Xldr の `:doc` / `:sig` / `:info` /
`:type` / `:facet` はこの parser を共有する。semantic resolver は `surtr-analysis`
に置き、query parser は query AST と validation diagnostics だけを返す。

LSP は通常 source の completion / hover に command query parser を使わない。
ただし editor command として `Surtr: Query Signature` のような入口を持つ場合は、
LSP adapter が query text を parse し、同じ semantic resolver に渡してよい。

REPL に残すものは次である。

- `:` command head と command routing
- `:v`, `:vars`, `:history`, `:reload`, `:save`, `:clear`
- 行番号、履歴、last result、append-only binding
- `$name` forced binding や `_1` / `&1` を含む command query surface の UX
- TTY / TUI の表示制限や pane state

---

## 8. LSP Adapter

`surtr-lsp` は LSP protocol 境界だけを担う。

| LSP 機能 | analysis service への写像 |
|---|---|
| `textDocument/publishDiagnostics` | `diagnostics(snapshot, active_file)` |
| `textDocument/completion` | `completions(snapshot, position)` |
| `textDocument/hover` | `hover(snapshot, position)` |
| `textDocument/signatureHelp` | `signature_help(snapshot, position)` |
| `textDocument/definition` | `definition(snapshot, position)` |
| `textDocument/documentSymbol` | `document_symbols(snapshot, active_file)` |
| `workspace/didChangeConfiguration` | selected context / runner args / cache policy の更新 |
| `workspace/didChangeWatchedFiles` | include graph / project paths / stdlib stage の invalidation |

LSP の position は UTF-16 code unit である。analysis core は parser が返す span と
Surtr diagnostics の user-facing span を扱うため、document store は必ず `LineIndex` を持つ。

`LineIndex` は次を相互変換できること。

- byte offset
- Unicode scalar / character column
- LSP UTF-16 position

diagnostics JSON や ariadne 表示の契約を LSP position に引きずらない。
protocol 境界でだけ変換する。

---

## 9. Completion Policy

completion は `AnalysisContext` の可視性を超えない。

### 9.1 v0

v0 completion は低リスクな候補に限定する。

- keyword / declaration head
- local binding / parameter
- same file declaration
- visible top-level function
- type constructor / type owner
- `Bootstrap` / `Kernel` / `@autoimport` surface
- import path segment
- call site signature help と active parameter

REPL 補完は現状の初期段階を維持してよい。LSP 側の設計は、他言語と大きく離れない
一般的な進化を前提にする。

### 9.2 v1

v1 では context-aware candidate を増やす。

- project profile / script include の stage 可視性
- qualified path completion
- module member completion
- constructor / extractor completion
- enum owner / variant display
- process public surface completion
- stdlib `@doc` を documentation field に流す

later stage の symbol は候補にしない。explicit import / auto-import の shadowing と衝突は
Sigil 規則に従う。

### 9.3 v2

v2 では型文脈を使う。

- expected type に合う local binding / function を上位に出す
- pipeline / operator helper の候補を出す
- `Result<T>` / `Option<T>` / `Process` 系 API の signature help を強化する
- completion item の `detail` / `documentation` / `sortText` を安定化する

型文脈つき候補は便利だが、候補から不一致 symbol を完全に消すか、順位だけ下げるかは
順位だけ下げる。型が合わない候補も visibility / scope 上候補である限り残し、`sortText`
または ranking score で後方へ送る。これにより未完成コードや inference 未確定時でも
発見可能性を落とさない。

---

## 10. Diagnostics Policy

diagnostics は `AnalysisContext` と `source_kind` に従う。

| context | policy |
|---|---|
| `Script` | script parse rules。include は先頭 block、top-level expr を許可する |
| `DefinitionSource` under entry | entry context の module stage 可視性で resolve / typecheck する |
| standalone `DefinitionCheck` | file 単体で確定できる error は通常表示し、外部 symbol 未確定は context 未選択 diagnostics として扱う |
| `StdDefinitionSource` | `@builtin` / `@autoimport` を標準定義 source として許可する |
| `Project` | runner diagnostics と compile diagnostics を同じ context status に紐づける |
| project runner source | 標準 `Project` / `Config` API の通常 diagnostics と host-side `ProjectRunner` diagnostics を分けて表示する |

project runner の失敗は compile diagnostics に混ぜてよいが、kind は区別する。

```text
DiagnosticSource
  = Parser
  | Resolver
  | TypeChecker
  | ProjectRunner
  | ContextSelection
```

たとえば external file missing、glob no match、profile unknown、handler override conflict は
`ProjectRunner` diagnostics として表示する。

---

## 11. Cache / Invalidation

cache は `AnalysisContext` 単位で持つ。

cache key には少なくとも次を含める。

- workspace root
- mode
- entry file
- active file
- source kind
- active document version / content hash
- stdlib source version / content hash
- include graph edge set と directive span hash
- module stage order
- module file path と content hash
- selected profile
- normalized runner args
- project runner source content hash
- `Project::add_path` literal / glob pattern
- glob 展開後 file list と content hash
- boot / supervisor config summary hash
- external input state
- `load_project` literal / profile / span hash
- active file の profile membership

次の event で invalidation する。

- active document text の変更
- script include directive の変更
- include 先 file の変更
- project runner source の変更
- runner args / selected profile の変更
- glob 展開結果の変更
- stdlib source の変更
- external input の existence / schema / content state の変更
- selected context の変更

native build では background analysis を行ってよい。wasm build では cancellation token を
cooperative に扱い、長い再解析を小さな単位へ分ける。

---

## 12. Project Runner との接続

project runner は LSP context selection の中心になる。

```text
project.srt
  -> selected profile
  -> normalized runner args
  -> ordered module stages
  -> boot / supervisor config summary
  -> external input states
  -> AnalysisContext
```

ENV や host default は resolver の入力になってよい。ただし LSP が表示・cache・diagnostics に使う
正本は normalized runner args である。

status 表示例。

```text
Surtr: project examples/mahjong/project.srt profile=test
Surtr: script scripts/seed.srt under examples/mahjong/project.srt profile=dev
Surtr: standalone definition
Surtr: stdlib lib/types/int.srt
Surtr: repl examples/mahjong/project.srt profile=test
```

active file が複数 profile に含まれる場合、LSP は現在選択中の context を優先する。
未選択時は diagnostics を 1 つの profile に勝手に固定せず、context selection action を返す。

`project.srt` 自体も editor surface の対象である。runner API の補完 / hover /
diagnostics は通常の semantic service から出し、host-side resolver が profile、
`Project::add_path(...)`、glob、boot summary、external input を解釈した結果は
`RunnerContext` と `ProjectRunner` diagnostics として重ねる。

---

## 13. REPL との接続

Xldr は REPL session の正本であり続ける。

LSP 実装は次を行わない。

- REPL command を LSP request として解釈する
- REPL session state を LSP server 側で再構築する
- live chunk を通常 file と同じ `SourceKind::Script` として解析する

REPL command query は、Xldr session UX から切り離せる小さい parser / validator として扱う。
Xldr は `:` command head、履歴、binding table、session state を付与し、query parser は
payload を query AST へ分類する。doc / sig / info / facet の意味解決は shared semantic
resolver へ渡す。

共有化の順序は次を推奨する。

1. command query parser を `surtr-analysis::query` に置き、Xldr がそれを呼ぶ
2. Xldr の completion / doc / signature の semantic lookup を UI 非依存 API へ切り出す
3. LSP adapter が file URI + position + `AnalysisContext` を入力として同じ semantic API を呼ぶ
4. REPL virtual document を追加し、preload context と live binding を mirror として表示する
5. command query parser を editor command の補助に使う。ただし通常 source の hover / signature help は source position から解決し、REPL query syntax と混ぜない

REPL 補完は今後も改善する。進化方向は他言語の editor experience と同じく、
keyword、local、scope、import、member、signature、type context の順で厚くする。

---

## 14. 実装フェーズ

### Phase 0: Analysis boundary

- `surtr-analysis` の protocol 非依存境界を作る
- document store と `LineIndex` を実装する
- `AnalysisContext` / `RunnerContext` / cache key 型を置く
- command query parser を `surtr-analysis::query` として置き、Xldr が共有実装を呼ぶ
- `SourceKind` を `sindr::policy` に置き、parse rule 導出を `spire` に置く

### Phase 1: Diagnostics MVP

- open / change / save された active file を context resolver に通す
- parse / resolve / typecheck diagnostics を LSP range へ変換する
- standalone definition と script entry context を扱う
- stdlib development context を扱う

### Phase 2: Navigation and docs

- document symbol
- hover
- signature help
- go to definition
- import path / qualified path の基本補完
- `:doc` / `:sig` / `:info` / `:type` / `:facet` 用 query AST と semantic resolver の境界を固定する

### Phase 3: Project context

- project runner resolver の出力を `RunnerContext` として受け取る
- selected profile と normalized runner args を cache key に入れる
- `Project::add_path` / glob / active file profile membership を diagnostics と status に反映する
- boot / supervisor config summary と external input state を cache key と diagnostics に反映する
- `load_project` 付き operational script を解析する

### Phase 4: REPL and advanced tooling

- Xldr semantic lookup を共有 service へ寄せる
- REPL virtual document / preload context 表示を追加する
- semantic tokens / code actions / references / rename を段階的に追加する
- wasm adapter を用意し、iOS / webview で single-thread analysis を検証する

---

## 15. テスト方針

### unit

- `surtr-analysis`
  - `LineIndex` の byte / char / UTF-16 変換
  - `AnalysisContext` 自動選択
  - script include graph 解決
  - standalone definition diagnostics の severity / hint
  - cache key determinism
  - completion candidate の可視性
  - command query parser の AST / validation diagnostics

- `surtr-lsp`
  - LSP position / range 変換
  - diagnostics publish の source / severity / message
  - completion / hover / signatureHelp DTO 変換
  - config change と watched files invalidation

- `xldr`
  - REPL completion が shared semantic service の候補を使う
  - REPL command routing が query parser の AST を使い、REPL 固有 UX を保つ

### integration

- `tests/fixtures/script/pass/**` を script context として解析する
- `tests/fixtures/script/fail/**` を compile diagnostics として解析する
- `tests/fixtures/modules/pass/**/entry.srt` を module context として解析する
- `lib/**/*.srt` を stdlib development context として解析する
- project runner profile 切り替えで diagnostics / completion / cache key が変わることを固定する
- glob 展開順、glob no match、unreadable path を runner diagnostics と cache key fixture で固定する
- external input missing / schema mismatch と boot / external summary hash の変化を fixture で固定する
- active file が複数 profile に含まれる場合、明示 selection が優先されることを固定する

### manual / client

- VSCode などの client で context status が正しく表示される
- active file が複数 context に属する場合、明示選択が優先される
- iOS / wasm build で single-thread analysis が動く

### implementation DoD

- docs / code を変更した実装 PR は、影響範囲に応じた targeted test に加えて
  `cargo nextest run --workspace` を完走させる
- `surtr-analysis` 追加時は context resolver、include graph、cache key、`LineIndex` の unit test を含める
- `surtr-lsp` 追加時は diagnostics / completion / hover / signatureHelp / definition の DTO 変換 unit test を含める
- REPL 共有化時は `cargo nextest run -p xldr` で既存 REPL completion / command query 表示を回帰確認する

---

## 16. 未確定事項

次は [../../doc/open-issues.md](../../doc/open-issues.md) の `surtr-lsp` issue で追跡する。

- `RunnerArgs` の最終構造と、`selected_profile` を top-level field に置くか runner args 内に置くか
- VM 実行で抽出した project runner result の最終 DTO 形状
- external input diagnostics の所属
- active file が複数 profile に属する場合の UI / diagnostics 優先順位
- REPL virtual document の範囲
- completion の型文脈順位付けで使う score / sortText 規則
- wasm adapter が JSON-RPC を使うか、direct API を使うか

---

## 17. v0 Acceptance

- `surtr-lsp` は active file を単体推測せず、必ず `AnalysisContext` 経由で parse / resolve / typecheck / completion / diagnostics を行う
- script entry を選択すると、include 先 definition source が同じ compile unit 文脈で解析される
- standalone definition source では、context 未選択由来の unresolved symbol を区別できる
- stdlib source は `StdDefinitionSource` として解析される
- project mode は selected profile、normalized runner args、module stage、project path 展開、boot / external input summary を cache key と diagnostics に反映できる
- command query parser は `spire` ではなく tooling query wrapper として置かれている
- REPL は LSP JSON-RPC ではなく shared semantic service を直接使う方針になっている
- wasm / iOS 向けに single-thread analysis を正規 target として扱える
