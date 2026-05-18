# Surtr LSP Analysis Context Spec v0

Surtr LSP が補完、hover、definition、diagnostics を出すときに、開いている
`.srt` source をどの compile unit 文脈で読むかを固定するための draft 仕様。

本書は LSP protocol / JSON-RPC / VSCode extension UI の仕様ではない。Surtr
言語側が LSP 実装へ渡すべき解析コンテキストの最小契約を定義する。

---

## 1. 目的

Surtr は definition source と script / CLI entrypoint が分離されている。
definition source 単体では、別 file に書かれた symbol の実体や import 可視性を
一意に決められない。

LSP は対象 file だけでなく「どの entrypoint / runner 文脈で解析するか」を持つ。
これを `AnalysisContext` と呼ぶ。

`AnalysisContext` の目的は次のとおり。

- 開いている `.srt` が `Script` / `DefinitionSource` / `StdDefinitionSource` の
  どれとして parse / resolve されるかを固定する
- script entry が `include` した definition source を同じ compile unit として扱う
- project mode では workspace root から runner が決める module stage を扱えるようにする
- Surtr 開発 repository 内の `lib/**/*.srt` や `tests/fixtures/**/*.srt` でも、
  標準定義 / fixture 文脈に沿った補完と診断を出せるようにする

---

## 2. 非目的

本書では次を固定しない。

- LSP protocol 上の request / notification 名
- VSCode command 名、status bar UI、settings schema の詳細
- project runner の設定 file format
- project runner が module stage を解決する具体アルゴリズム
- REPL live chunk の incremental evaluation 仕様

LSP 実装は本書の `AnalysisContext` を入力として、別途定義される protocol /
client UI へ写像してよい。

---

## 3. AnalysisContext

`AnalysisContext` は次の概念を持つ。

```text
AnalysisContext {
  workspace_root: Path
  mode: Project | Script | DefinitionCheck
  entry_file: Option<Path>
  active_file: Path
  source_kind: SourceKind
  stdlib_stage: StdlibStageSet
  module_stages: Vec<ModuleStage>
  include_graph: IncludeGraph
  runner_args: Option<RunnerArgs>
}
```

各 field の意味は次のとおり。

| field | 意味 |
|---|---|
| `workspace_root` | LSP client が開いている workspace root。Surtr repository では `/Users/haruca/work/rust/surtr` を想定する |
| `mode` | compile unit の入口種別。既存の `CompileUnitKind` へ対応する |
| `entry_file` | script / project runner の解析起点。standalone definition では `None` を許可する |
| `active_file` | editor で現在 diagnostics / completion の対象になっている file |
| `source_kind` | `active_file` に適用する既存の `SourceKind` |
| `stdlib_stage` | `Bootstrap` stage と標準 definition source stage をまとめた論理集合 |
| `module_stages` | entry / runner / include から解決された user definition source stage |
| `include_graph` | script `include` directive から作られる entry と include file の依存関係 |
| `runner_args` | 将来の project runner / script runner 指定を保持する予約 field |

`mode` は既存の `CompileUnitKind` に対応する。

| `AnalysisContext.mode` | `CompileUnitKind` | 用途 |
|---|---|---|
| `Script` | `CompileUnitKind::Script` | script entry を起点に解析する |
| `DefinitionCheck` | `CompileUnitKind::DefinitionCheck` | definition source を entry なしで単体検証する |
| `Project` | `CompileUnitKind::Project` | project runner が解決した compile 対象を解析する |

`source_kind` は既存の `SourceKind` をそのまま使う。

| 対象 | `SourceKind` |
|---|---|
| script entry file | `SourceKind::Script` |
| script の `include` 先 | `SourceKind::DefinitionSource` |
| project runner が追加した user module | `SourceKind::DefinitionSource` |
| repository 内の標準 definition source | `SourceKind::StdDefinitionSource` |
| standalone user definition source | `SourceKind::DefinitionSource` |

`SourceKind::ReplChunk` は REPL セッション用であり、v0 の editor file 解析
context では対象外とする。

---

## 4. Context Resolution

### 4.1 Workspace root

`workspace_root` が Surtr 開発 repository の root の場合、LSP は次の path を
特別扱いしてよい。

- `lib/**/*.srt`
  - 標準 definition source 開発対象
  - `lib/tests/**` は既存 loader と同じく default stdlib stage から除外する
- `tests/fixtures/script/pass/**/*.srt`
  - script fixture entry 候補
- `tests/fixtures/script/fail/**/*.srt`
  - script fixture entry 候補。ただし diagnostics は対応 `.error` の期待 phase と
    別に、通常の compile diagnostics として出してよい
- `tests/fixtures/modules/pass/**/entry.srt`
  - module fixture の entry 候補
- `tests/fixtures/modules/fail/**/entry.srt`
  - module fixture の entry 候補

これらは LSP の利便性のための context discovery 候補であり、言語意味論を
追加しない。

### 4.2 Script context

script file が `entry_file` として選択された場合、LSP は次の context を構築する。

```text
mode = Script
entry_file = script path
entry source_kind = SourceKind::Script
compile unit = StdlibStage + IncludeModuleStages + ScriptEntry
```

script 先頭の `include "<path>"` / `include '<path>'` は既存 loader と同じ規則で
script file からの相対 path として解決する。解決された include file は再度
script として sniff せず、`SourceKind::DefinitionSource` として扱う。

script 自身の parse では include directive を loader directive として扱い、
include 先 file の parse では module / definition source rules を使う。

### 4.3 Project context

project mode では、LSP は `workspace_root` を起点に project runner を解決する。
runner 設定形式は本書では固定しない。

v0 では次の予約契約だけを置く。

```text
mode = Project
entry_file = runner が決める entry または代表 file
runner_args = LSP client / future CLI / source API から渡された runner 指定
module_stages = runner が解決した compile 対象 stage
```

project runner が `Project::add_path(...)` などで file を追加する場合、その file は
definition source として扱う。追加 file を token sniffing して script に切り替えない。

project mode の completion / diagnostics は、runner が解決した stage 可視性を
正本にする。

### 4.4 DefinitionCheck context

definition source を直接開いた場合、LSP は次の優先順で context を選ぶ。

1. active file が現在選択中の script / project context に含まれる場合、その context を使う
2. user が明示的に `entry_file` を選択している場合、その entry context 下の definition として扱う
3. `active_file` が `lib/**/*.srt` に含まれる場合、stdlib development context として扱う
4. それ以外は standalone definition context とする

standalone definition context は `CompileUnitKind::DefinitionCheck` +
`SourceKind::DefinitionSource` を基本とする。ただし外部 symbol の実体は確定しない。
compiler pipeline が unresolved external symbol を返す場合、その diagnostic 自体は
保持する。LSP client は表示上、entry context 未選択の影響を受ける可能性がある
diagnostic として severity / hint を調整してよい。

definition source が選択済み script / project context に含まれる場合、その file の
`source_kind` は `SourceKind::DefinitionSource` のままだが、`mode` は entry 側の
`Script` または `Project` を使う。`DefinitionCheck` は standalone 検証専用であり、
entry context 下の include / project module には使わない。

### 4.5 Stdlib development context

`/Users/haruca/work/rust/surtr/lib/**/*.srt` を開いている場合、LSP は標準定義の
開発文脈として扱える。

この context では `active_file` の `source_kind` は `SourceKind::StdDefinitionSource`
とする。`@builtin def` / `@builtin type` / `@autoimport` を標準定義 source として
解釈し、`Bootstrap -> [SpecialTypes, Function, Kernel, ...]` の既存 stdlib load order
と矛盾しない stage を使う。

`lib/tests/**` は default stdlib stage には含めない。test DSL や stdlib fixture として
解析する場合は、明示的な script / project / test context を別に選ぶ。

---

## 5. Completion Policy

LSP completion は `AnalysisContext` の可視性を超えて候補を混ぜない。

### 5.1 共通候補

すべての context で次を候補にしてよい。

- 現在の parse rule で有効な keyword / declaration head
- 同一 file 内の local binding / declaration
- `Bootstrap` / `Kernel` / auto-import された標準 surface
- `@doc` metadata が取れる symbol の hover / documentation

### 5.2 Standalone definition

standalone definition context では、候補は次に限定する。

- active file 内で宣言済み、または同一 file 内で前方参照可能な declaration
- `Bootstrap` / `Kernel` / `@autoimport` された標準 surface
- definition source で宣言可能な top-level surface

その他の標準 module surface は、qualified path または明示 import の文脈がある場合に
候補にする。別 file の user symbol は、選択済み entry context がない限り確定候補にしない。

### 5.3 Entry context 下の definition

active file が script include / project runner stage に含まれる場合、LSP はその
compile unit の stage 可視性を使う。

- 同一 stage と前 stage の visible symbol を候補にする
- later stage の symbol は候補にしない
- explicit import / auto-import の shadowing と衝突は既存 Sigil 規則に従う

### 5.4 Stdlib development

stdlib development context では、標準 source の `@doc` と canonical builtin surface を
優先して候補にする。builtin の追加・変更の正本は共有メタデータテーブルであり、
source 側の `@builtin` 宣言は宣言層として扱う。

---

## 6. Diagnostics Policy

diagnostics は `active_file` の `source_kind` に対応する parse rules と runtime source
policy で作る。

| `source_kind` | diagnostics policy |
|---|---|
| `Script` | script parse rules。include は source 先頭のみ許可し、top-level expr を許可する |
| `DefinitionSource` | module / definition source rules。top-level expr と `@builtin` を禁止する |
| `StdDefinitionSource` | std module rules。top-level expr を禁止し、`@builtin` を許可する |

context 未確定の standalone definition では、次を区別する。

- file 単体で確定できる parse / declaration / duplicate / builtin misuse error
  - 通常 diagnostics として出す
- 別 entry context なら解決できる可能性がある unresolved external symbol
  - context 未確定 diagnostics として出す
  - LSP client は「解析起点を選択してください」に相当する action を提示してよい

script / project context が選択されている場合は、その context で unresolved なら通常の
compile diagnostics として扱う。

---

## 7. UX Contract

LSP client は解析 context を明示的に切り替えられる UI を持つ。

想定操作名は実装側で自由に決めてよいが、少なくとも次の機能を提供する。

- current file を script entry として解析する
- workspace project として解析する
- current definition source を standalone として解析する
- current definition source を既存 script / project context 下で解析する
- context を解除して automatic discovery に戻す

status 表示例。

```text
Surtr: script tests/fixtures/script/pass/foo.srt
Surtr: project /Users/haruca/work/rust/surtr
Surtr: definition under tests/fixtures/script/pass/foo.srt
Surtr: stdlib lib/types/int.srt
Surtr: standalone definition
```

LSP は同一 file に複数 context 候補がある場合、現在選択中の context を優先する。
候補がない場合だけ automatic discovery を使う。

---

## 8. Cache / Invalidation

LSP は context 単位で parse / resolve / typecheck 結果を cache してよい。

cache key には少なくとも次を含める。

- `workspace_root`
- `mode`
- `entry_file`
- `active_file`
- `entry_file` の content hash
- `active_file` の content hash
- `source_kind`
- `runner_args`
- `include_graph` の edge set と include directive span hash
- `module_stages` の file path と content hash
- `stdlib_stage` の version / content hash

次の場合は context を invalidation する。

- script entry の include directive が変わった
- include file の content が変わった
- project runner 設定または runner args が変わった
- `lib/**/*.srt` の標準 definition source が変わった
- active file の selected entry context が変わった

---

## 9. 既存仕様との対応

本書は次の既存仕様に従う。

- `doc/要件定義v9.md`
  - `CompileUnitKind::{Script, DefinitionCheck, Project, Repl}`
  - `SourceKind::{Script, DefinitionSource, StdDefinitionSource, ReplChunk}`
  - script include と definition source の parse rule 境界
- `docs/dev/Xldr_spec.md`
  - `Bootstrap -> [standard definition sources]` の load order
  - `include` / `Project::add_path(...)` 由来 file を definition source として扱う契約
  - project runner 由来 module stage を Preload / compile unit に渡す契約
- `doc/vscode/implementation_plan.md`
  - VSCode extension / diagnostics JSON などの client 側入口とは責務を分ける

本書が追加するのは、LSP がこれらの既存契約を editor file 解析へ適用するための
context selection layer だけである。

LSP 実装には `AnalysisContext` を受け取れる context-aware analysis API が必要になる。
既存の `surtr check <file.srt> --format json` は単一 file diagnostics の入口として残し、
本書の context-aware API はそれを置き換えるものではない。

---

## 10. v0 Acceptance

- LSP は active file を単体で推測するのではなく、必ず `AnalysisContext` 経由で
  parse / resolve / completion / diagnostics を行う
- script entry を選択すると、script include 先 definition source の補完と診断が
  同じ compile unit 文脈で行われる
- project mode では workspace root と runner 解決結果を context として保持できる
- `lib/**/*.srt` は Surtr 開発 repository 内で stdlib development context として扱える
- standalone definition source では、外部 symbol 未確定を entry context 未選択として
  表現できる
