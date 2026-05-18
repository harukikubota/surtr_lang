# Project Runner Pseudo DI Draft

この文書は、`examples/mahjong/project.srt` を起点に project runner をどう洗練するかを整理する検討メモである。

正本仕様ではない。後続で `doc/lsp_analysis_context_spec_v0.md` と `docs/dev/ProcessRuntime_spec.md` に反映する前のドラフトとして扱う。

## 1. 背景

現状の Mahjong example には、次の 3 つの入口が混在している。

- `examples/mahjong/run.srt`
  - include 順を script 内に直接書き、デモ入力を top-level expr で実行する。
- `examples/mahjong/main.srt`
  - include 順を script 内に直接書き、`Project::args()` を `MahjongCli::run` に渡す。
- `examples/mahjong/project.srt`
  - `Project::config` / `Project::entrypoint` / `Config::entry_fun` / `Config::add_path` で project runner 風の構成を書く。

`run.srt` と `main.srt` は include 順を重複して持っている。`project.srt` は file set と entry function を分け始めているが、現時点では通常の Surtr source としての helper value であり、Rune が host-side project config として消費する runner contract にはなっていない。

また、`project.srt` には `test` entrypoint や `prod.srt` 差し替えの意図がコメントで残っている。これは、entrypoint で読み込む物理 file を切り替えることで pseudo DI を行いたい、という要求の初期形である。

## 2. 方針

Project runner は、単なる script entry の別名ではなく、次を返す起動設定層として扱う。

```text
Project runner
  -> selected profile
  -> compile source configuration
  -> boot / supervisor configuration
  -> optional context for Script / REPL
  -> runner args visible to LSP / cache / diagnostics
```

結論として、project mode では entrypoint/profile selection を正本にする。`DEV` / `TEST` / `PROD` のような ENV は、compiler や LSP から見えない hidden input にせず、runner args に正規化する。

例:

```text
ENV=TEST
  -> runner default selection
  -> profile = "test"
  -> AnalysisContext.runner_args.profile = "test"
```

ENV は「どの profile を既定選択するか」までは担ってよい。ただし、最終的に compiler / LSP / cache が見る入力は `profile = "test"` のような明示的な runner args とする。

さらに、REPL と Script は project runner を明示的に受け取れるようにする。Script では includeBlock に `load_project("<literal>")` を置き、REPL では起動引数で project runner を指定する。これにより、project 用の Seeder、migration、one-shot maintenance script、REPL-driven development を、project profile の compile source / boot config / external injection を借りて実行できる。

## 3. Script Mode と Project Mode

### Script mode

Script mode では、source 内のブロック順序が compile unit の正本になる。そのため `supervisor_init { ... }` DSL はそのまま維持する。

script の役割は軽量な実行入口である。

- include directive は source 先頭に固定する。
- include 先は `SourceKind::DefinitionSource` として扱う。
- `supervisor_init` は include 後の compile unit から BootPlan 入力として収集する。
- top-level expr は script 実行 section として扱う。

この形は examples / fixtures / 小さい確認用 script に向いている。script 内で順序が見えるため、DSL のままでも LSP と compiler の解釈がずれにくい。

### Project mode

Project mode では、`project.srt` を通常 script ではなく project config source として扱う。

Project runner は、選択 profile ごとに次をまとめる。

- profile name
- entry function
- ordered module paths / module stages
- boot / supervisor config
- external injection source
- host に渡す実行 args

`Project::entrypoint` は、単に entry function を登録する API ではなく、profile を構成する API として洗練させる。

```surtr
Project::entrypoint(project, "test", {|c|
  Config::entry_fun(c, "Main::test")
  |> Config::add_path("./main.srt")
  |> Config::add_path("./src/0_types.srt")
  |> Config::add_path("./test/repo_mock.srt")
})
```

この `test` profile を選ぶと、LSP は `examples/mahjong/src/*.srt` を standalone definition としてではなく、`test` profile の compile unit 文脈で解析できる。

## 4. Project Context を借りる Script / REPL

REPL と Script は、project runner を明示指定できるようにする。

目的は、app 本体の context を保ったまま補助作業を実行することである。

- Seeder
- migration
- one-shot maintenance script
- fixture 作成 script
- project profile を preload した REPL
- LSP の補完と diagnostics を受けながら進める REPL-driven development

イメージ:

```surtr
load_project("./project.srt", profile: "dev")

include "./seed_definitions.srt"

Seeder::run()
```

```text
surtr repl --project examples/mahjong/project.srt --profile test
```

`load_project` は script の includeBlock にだけ置ける loader directive として扱う。引数は literal only とし、変数、ENV、関数呼び出し、文字列結合は受理しない。

この制約により、LSP / cache / diagnostics は script を実行せずに project context を解決できる。script が追加で読みたい定義 source は、その script 自身の `include` で明示する。`load_project` は project context の選択だけを担い、script-local definition source の追加とは役割を分ける。

このとき compile unit は次のように分けて考える。

```text
project profile context
  -> Std + project module stages + project BootPlan input

script execution context
  -> project profile context + script include modules + script source

REPL preload context
  -> project profile context + live ReplChunk
```

Script は引き続き `supervisor_init { ... }` DSL を持てる。ただし project runner context を明示指定した script では、script-local `supervisor_init` は project profile の boot config へ追加または override する入力として扱う。

この merge 規則は後続で固定する必要がある。暫定方針は次のとおり。

- project profile の boot config を base とする。
- script-local `supervisor_init` は operational script 専用の追加 boot input とする。
- 同じ singleton / handler / supervisor policy を二重指定した場合は、暗黙上書きではなく diagnostics を出す。
- REPL は session 開始時に project profile の BootPlan を固定し、live chunk で boot 構成を変更しない。

これにより、Seeder や migration は app 本体と同じ module visibility、external injection、handler override を使える。一方で、script 単体実行の軽さと、source 内 DSL の読みやすさは維持できる。

LSP は `scripts/seed.srt` のような operational script を、standalone script ではなく `project profile + script` の `AnalysisContext` として解析できる。

検証用 script は、`load_project` ではなく実行時引数による DI を使ってもよい。対象値や検証条件が実行ごとに変わる場合は `Project::args()` / CLI args を使い、読み込み先が固定の Seeder や migration は includeBlock の `load_project` で project context を埋め込む、という使い分けが自然である。

## 5. supervisor_init と外部注入

Project runner では、`supervisor_init` DSL をそのまま埋め込むのではなく、runner に無理なく載せられる typed config surface に寄せる。

理由は、project runner の boot 構成には外部注入が必要になるためである。

- 外部 config file
- ENV
- test fixture
- mock repository
- local / CI / production 用 handler target
- profile ごとの singleton availability

これらを `supervisor_init { ... }` DSL に path literal として増やし始めると、compile source の宣言、runtime boot の宣言、host-side injection の宣言が混ざる。

Project mode では、runner 内で次のような typed data / builder API を使う方向が自然である。

```text
ProjectBoot
SupervisorConfig
SingletonConfig
HandlerOverride
ExternalInput
ProjectFile
```

名前は未決でよい。重要なのは、project runner が最終的に compiler へ渡す boot 入力を明示的な data として構築することである。

```text
project.srt typed config
  -> host-side project resolver
  -> RuntimeBootPlan input
  -> compiler policy / VM metadata
```

VM は surface DSL を直接読まない。Project runner 由来でも script `supervisor_init` 由来でも、compiler が読む最終形は `RuntimeBootPlan` 入力で揃える。

外部入力は、起動構成の中で使える検証 API として置く方向がよい。たとえば `include_file!` のような名前を仮に考えるなら、それは runtime 中に file を include する命令ではなく、project resolver が事前に検証し、失敗を `Result` として返せる boot input helper として扱う。

Project runner は構造体を返すため、各 builder closure の戻り値を `Result<Config>` / `Result<ProjectBoot>` のようにすれば、file missing、schema mismatch、profile 設定漏れを自然に伝播できる。`Facet::over_result` や `Facet::set` と組み合わせると、各 section で検出した `Err` を runner diagnostics へそのまま渡しやすい。

```surtr
Project::entrypoint(project, "test", {|c|
  c
  |> Config::add_path("./src/**/*.srt")
  |> Config::boot_result({|boot|
    boot
    |> ProjectBoot::external_file("seed_data", "./fixtures/seed.json")
    |> ProjectBoot::handler("Repo", "RepoMock")
  })
})
```

上の API 名は仮である。正本にしたい性質は、外部入力の existence / decode / schema check が project runner の `Result` として表現され、compiler / LSP がその失敗を project context diagnostics として表示できることである。

## 6. 起動オプションの責務分離

現状の起動オプションは、compiler 解釈、標準定義、runtime policy が混ざりやすい。

Project runner では、次の境界を分ける。

| 層 | 責務 |
|---|---|
| project runner source | profile、module path、boot config、external input request を宣言する |
| host-side resolver | ENV / file / CLI args / explicit project arg を解決し、選択 profile と外部入力状態を固定する |
| compiler | source kind、module stages、entrypoint、BootPlan 入力を検査し bytecode metadata に変換する |
| standard library | user code から参照する typed API を提供する |
| VM | compiler が生成した immutable metadata を実行する |

`Env` のような runtime-facing API は standard library 側にあってよい。ただし、`Env` がどの file から読み込まれたか、どの profile でどの値が注入されたかは project runner / host-side resolver 側の責務に閉じる。

## 7. LSP との接続

LSP は project runner の恩恵を受ける必要がある。つまり `project.srt` 内の runner API 自体が補完、hover、diagnostics の対象になる。

LSP の `AnalysisContext` は、project mode で少なくとも次を保持する。

- selected profile
- ENV 由来 default を正規化した runner args
- resolved module stages
- `Project::add_path(...)` 由来 paths
- boot / supervisor config
- external injection source の解決状態
- includeBlock の `load_project` literal と span
- active file がどの profile に含まれているか
- active script / REPL session がどの project runner context を明示指定しているか

`Project::add_path(...)` 由来 file は常に `SourceKind::DefinitionSource` とする。token sniffing で script / definition を推測しない。`add_path` が glob に対応する場合も、展開後の file はすべて definition source として扱う。展開順は deterministic に固定し、cache key には glob pattern と展開後 path / content hash を含める。

`available_singletons`、handler override、custom supervisor 登録は profile ごとに変わる。そのため LSP diagnostics も profile 文脈で出す。

例:

```text
Surtr: project examples/mahjong profile=test
```

この状態で `src/6_cli.srt` を開いた場合、LSP は `test` profile が追加した mock repository や boot config を前提に補完と診断を出す。

Operational script の表示例:

```text
Surtr: script scripts/seed.srt under examples/mahjong profile=dev
Surtr: repl examples/mahjong profile=test
```

この状態では、Seeder / migration script 内の補完と diagnostics は project profile の module stages と boot config を前提にする。

## 8. ENV 直接分岐案との比較

### ENV 直接分岐

ENV で `DEV` / `TEST` / `PROD` を読み、entrypoint や file set を直接切り替える案は CLI では簡単である。

ただし、次の弱さがある。

- LSP が現在の解析文脈を安定して表示しにくい。
- cache key に hidden ENV を入れ忘れると diagnostics と実行結果がずれる。
- CI / editor / CLI で同じ context を再現しにくい。
- active file がどの profile に属するかを user に説明しにくい。

ENV は消さなくてよいが、正本入力にはしない。

### Entrypoint / profile selection

Profile selection を正本にすると、CLI、LSP、REPL preload、test harness が同じ resolver を共有しやすい。

- `profile = "main"` は通常 CLI。
- `profile = "test"` は test helper / mock / test boot config。
- `profile = "prod"` は production adapter / production boot config。

ENV は profile default を選ぶ薄い layer に留める。

## 9. Mahjong への適用イメージ

Mahjong example では、共通 domain source を 1 つの profile builder に集約し、profile ごとの差分だけを追加する。

```text
common paths:
  ./src/0_types.srt
  ./src/1_parser.srt
  ./src/2_normalize.srt
  ./src/3_solver.srt
  ./src/9_extractor.srt
  ./src/7_yaku_catalog.srt
  ./src/4_judge.srt
  ./src/5_score.srt
  ./src/8_view_components.srt
  ./src/6_cli.srt

main profile:
  entry_fun = Main::main
  args = Project::args()

test profile:
  entry_fun = Main::test
  extra paths = ./test_helper.srt, ./test/repo.srt
  boot config = test handlers / mocks

prod profile:
  entry_fun = Main::main
  extra paths = ./prod.srt
  boot config = production handlers / external config source
```

`run.srt` はデモ用 script として残してよい。`project.srt` は app-like example の文脈正本として育てる。

Project runner を明示指定する operational script も追加できる。

```text
scripts/seed_scores.srt
  -> includeBlock: load_project("./project.srt", profile: "dev")
  -> profile = dev
  -> script-local includes = seed 用 definition source
  -> project module stages + dev boot config を借りて実行

REPL
  -> project = ./project.srt
  -> profile = test
  -> mock repository / test handler を preload して対話開発
```

## 10. 未決事項

- typed boot builder API の具体名。
  - `ProjectBoot` / `BootConfig` / `SupervisorConfig` のどれを正本名にするか。
- 外部 file 解決失敗の diagnostics 所属。
  - runner diagnostics として出すか、compile diagnostics として出すか。
- `Project` / `Config` standard library surface と host-side schema の分離方法。
  - Surtr value を VM 実行して抽出するか、restricted project config evaluator を持つか。
- project runner source の `SourceKind`。
  - 通常 script と分けた `ProjectConfigSource` 相当が必要か。
- boot config builder を LSP がどこまで semantic に理解するか。
  - 型検査済み Surtr code として見るだけで足りるか、host resolver 用の追加 diagnostics が必要か。
- project context 付き script の boot merge 規則。
  - project profile と script-local `supervisor_init` の重複を error にするか、明示 override API を用意するか。
- REPL 起動後に boot config を変更できるか。
  - 暫定方針は session 開始時固定で、live chunk からの変更は禁止する。
- `load_project` の profile 指定構文。
  - `load_project("./project.srt", profile: "dev")` のような named literal を許すか、`load_project("./project.srt", "dev")` にするか。
- project runner 外部入力 API の名前。
  - `include_file!` 風の directive にするか、`ProjectBoot::external_file` のような typed builder に寄せるか。
- `Config::add_path` の glob 対応範囲。
  - `./src/**/*.srt` のような glob を許す。binary blob など source ではない artifact を扱うなら、`add_path` ではなく external input API 側に分ける。

## 11. 暫定結論

Project runner は、entrypoint 起点のまま進める。ただし「entrypoint を複数持つ script」ではなく、「profile ごとに compile source と boot input を返す project config source」として洗練する。

Script mode は `supervisor_init` DSL を維持する。Project mode は外部注入を前提に typed config surface へ寄せる。

REPL と Script は project runner を明示指定できるようにする。Script では includeBlock の literal-only `load_project`、REPL では起動引数を使う。これにより、Seeder、migration、maintenance script、REPL-driven development を project profile の文脈で扱える。

これにより、pseudo DI、LSP context、cache key、singleton availability diagnostics、handler override、operational script、REPL preload を同じ `AnalysisContext` / project resolver の上で扱える。
