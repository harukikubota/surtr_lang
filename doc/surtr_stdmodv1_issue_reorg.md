# Phase Standard Modules V1: Issue再編・課題分離・別録

最終更新日: 2026-04-05

---

## 1. 実行基盤・標準モジュール実装に必要な Issue 再編

ここは **StdModV1 を進めるために先に必要なもの** だけに絞る。  
Script は **CLI 入口のみ** を対象に残し、project / project runner / REPL コマンド詳細は外す。
以降のタスクは原則として **1タスク = 1コミット** で進める。

---

### Issue A: CompileUnit / SourceKind 境界導入

#### Title

`[Phase-StdModV1] Introduce CompileUnitKind and SourceKind boundaries for module-only loading`

#### Background

module 導入後は、`Script` / `Module` / `Project` / `Repl` の責務を明示的に分ける必要がある。  
また Loader は module source のみを扱い、script 実行は別 API として分離する必要がある。

#### Scope

- `CompileUnitKind` を導入する
  - `Script`
  - `Module`
  - `Project`
  - `Repl`
- `SourceKind` を導入する
  - `Script`
  - `Module`
  - `StdModule`
  - `ReplChunk`
- Loader は `Module` / `StdModule` のみを収集対象にする
- Script は Loader 対象外とし、別 API から評価する
- compile unit ごとの責務境界をコードで表現可能にする

#### Out Of Scope

- project runner DSL
- REPL コマンド
- script から script のロード
- runtime module system

#### Acceptance Criteria

- `CompileUnitKind` と `SourceKind` がコード上で明示される。
- Loader API が script source を直接受け取らない。
- script 実行経路と module loading 経路が分離される。
- 今後の parser / resolver / runner が compile unit ごとに分岐可能になる。

#### Implementation Notes

- Loader 入力は内部的に `SourceDescriptor` 相当を持てる形が望ましい。
- `SourceKind` は Surtr コードへ漏らさず、CLI / host 側で注入する。
- 既存の single-script 実行経路を温存した上で、module loading と混線しないようにする。

#### Dependencies

- Depends on: `Issue 5`
- Should precede: script entry / REPL 入口整理

---

### Issue B: Module source 規則と Loader 入力制約

#### Title

`[Phase-StdModV1] Enforce module-source top-level rules for loader input`

#### Background

Loader は「定義関連のみロード可能」である必要がある。  
module source にトップレベル式を許すと、script 実行と module 収集の責務が混在する。

#### Scope

- Loader 対象 source ではトップレベル式を禁止する
- function 定義の所属規則を固定する
  - 関数は `defmod` / `trait` / `impl Type` / 擬似モジュール下でのみ定義できる
  - `defmod` は関数のみをメンバーとして持つ
  - `trait` / `impl Type` はモジュール所属ではない
- module source で許可するトップレベル要素を固定する
  - `defmod`
  - `trait`
  - `impl Type`
  - `defstruct`
  - `defrecord`
  - `defenum`
  - `deferror`
  - annotation metadata
- 1 file に複数 `defmod` を許可する
- file path と module path の一致は要求しない
- module 所属は関数定義に対してのみ付与し、`defmod` 宣言単位で一意とする
- モジュール外の定義は module に属さない
- 同一 module path / 同一型名衝突を compile error にする

#### Out Of Scope

- import 解決の詳細
- script top-level 実行
- project runner による source 選択

#### Acceptance Criteria

- Loader 経由で渡された source にトップレベル式があれば compile error になる。
- 1 file 内複数 module 定義が可能である。
- module path 重複は file path に関係なく compile error になる。
- 各関数定義が所属 module または擬似 module を一意に持つ。
- モジュール外の定義は module 所属を持たない。
- 各定義が個別 Span を保持する。

#### Implementation Notes

- 診断のため、少なくとも関数定義は `Span` と `declared_module_path` を保持する。
- 型定義など module 非所属の定義には、module 所属情報を強制しない。
- 「モジュールは関心事の集合」であり、分割所属は認めない。
- module はネストしない。

#### Dependencies

- Depends on: `Issue 4`
- Related to: `Issue 5`, `Issue 6`

---

### Issue C: Script 実行 API と entry 規則（CLI 入口のみ）

#### Title

`[Phase-StdModV1] Add CLI script execution API with explicit entry selection`

#### Background

script は Loader から切り離し、専用 API で評価する。  
その際、top-level 実行と function entry 実行を両立しつつ、module system と衝突しない規則が必要。

#### Scope

- CLI からの script 実行経路を整理する
- script / REPL に擬似 module identity を導入する
- script / REPL の top-level `def` は擬似 module 配下の関数定義として扱う
- script の非関数トップレベル定義の許可範囲は別 Issue と切り分ける
- script の entry 解決規則を導入する
  - `CLI --entry` が最優先
  - 次に `@@entrypoint`
  - どちらもなければトップレベル逐次実行
- script の `--entry name` は `() -> Result<()>` のみ許可
- `@@entrypoint` は 1 個まで、対象関数は `() -> Result<()>`

#### Out Of Scope

- script から script のロード
- project runner 統合
- REPL file ingest
- shell command support 自体
- script の非関数トップレベル定義規則の最終確定

#### Acceptance Criteria

- `surtr --file script.srt` が script 経路で実行できる。
- `--entry name` 指定時のみ関数 entry 実行される。
- `@@entrypoint` がある場合は自動実行される。
- `CLI --entry > @@entrypoint > top-level` の優先順位が守られる。
- `@@entrypoint` 複数指定は compile error になる。
- script の top-level `def` は擬似 module に属する。
- 擬似 module は user-visible な module namespace へ漏れない。

#### Implementation Notes

- user-facing には `--entry main` を受けるが、内部では暗黙 module identity に正規化して扱う。
- script import 解決は auto import 規則を含め、他 CLI 入口と同一規則にする。
- `defp` は script 内では実質 `def` と同じ扱いでよい。
- script の非関数トップレベル定義可否は、この Issue では確定しない。

#### Dependencies

- Depends on: `Issue A`
- Related to: `Issue 9`

---

### Issue D: EntryPoint 正規化と entry signature 統一

#### Title

`[Phase-StdModV1] Normalize entrypoint handling across module and script execution`

#### Background

module 実行と script entry 実行の両方で、入口関数の扱いを統一しておく必要がある。  
Script では短名指定、module/project では完全修飾名を扱うが、内部表現は統一できる。

#### Scope

- 内部表現として `EntryPoint` 相当を導入する
- script entry は短名から暗黙 qualified name に正規化する
- module/project entry は qualified symbol として扱う
- entry 関数の共通シグネチャを `() -> Result<()>` に固定する
- `Issue 9` の entrypoint 規則と整合する

#### Out Of Scope

- project runner DSL
- command alias / subcommand dispatch
- REPL entry 指定

#### Acceptance Criteria

- script entry と module entry が内部で共通形に正規化される。
- entrypoint シグネチャ違反は compile error になる。
- module entry 規則と script `--entry` 規則が矛盾しない。
- dump / debug 出力で entry の正規化結果が追跡可能になる。

#### Implementation Notes

- 例:
  - script: `--entry main` → internal qualified symbol
  - module/project: `Main::main`
- `EntryPoint` 自体は file path ではなく symbol 基準で保持する方がよい。

#### Dependencies

- Depends on: `Issue C`
- Depends on: `Issue 9`

---

### Issue E: エラー境界の実装反映（最低限）

#### Title

`[Phase-StdModV1] Reflect compile-error, Result::Err, and runtime-error boundaries in execution paths`

#### Background

REPL / Script / Module 実行で、`compile error`・`Result::Err`・`runtime error` を混同すると挙動が不安定になる。  
最低限、実行基盤として終了条件と継続条件を分ける必要がある。

#### Scope

- `compile error` を実行前失敗として扱う
- `Result::Err` を言語レベル失敗として扱う
- `runtime error` を VM/bytecode 異常として扱う
- script CLI 実行で上記の区別を反映する
- REPL 実装がある範囲では終了条件を崩さないよう土台を整える

#### Out Of Scope

- 全 runtime error 種の完全列挙
- Supervisor 実装詳細
- プロセス失敗モデルの完成

#### Acceptance Criteria

- compile error は bytecode 実行前に停止する。
- `Result::Err` は runtime error と別扱いになる。
- VM 実装ミス / 不正 bytecode / stack overflow 系は runtime error 経路に入る。
- 0除算・パターン不一致・null 系は `Result::Err` として扱える設計になる。

#### Implementation Notes

- runtime error 群の完全仕様は後続ドキュメント Issue で補強する。
- 「Result で吸収できるもの」と「VM 異常」は必ず分離する。

#### Dependencies

- Depends on: `Issue C`
- Related to: `Issue 8`, `Issue 9`

---

### Issue F: SourceRules 文脈導入と policy ベース検証

#### Title

`[Phase-StdModV1] Introduce SourceRules context and policy-driven compile checks`

#### Background

`SourceKind` を導入しても、parser / typechecker が同じ規則集合を共有しないと、
`set_exit_code` や source 種別ごとの許可構文を一貫して検証できない。

#### Scope

- `SourceKind` と `CompileUnitKind` と `EntryPoint` から導出される `SourceRules` を導入する
- parser / resolver / typechecker が参照可能な compile context に `SourceRules` を保持する
- `set_exit_code` の許可規則を policy として固定する
  - `Script`: どこでも許可
  - `ReplChunk`: 常に禁止
  - `Project` 実行時の `Module` / `StdModule`: 指定 `EntryPoint` 関数内のみ許可
- `set_exit_code` の検証を `main` 特別扱いから policy 判定へ移行する
- 今後の「許可構文」「builtin 使用可否」も同じ `SourceRules` に拡張可能な形へ寄せる

#### Out Of Scope

- project runner DSL
- entrypoint 解決アルゴリズムの最終仕様
- runtime error taxonomy の最終確定

#### Acceptance Criteria

- parser と typechecker が同じ `SourceRules` を参照する。
- `set_exit_code` が Script では通り、ReplChunk では compile error になる。
- Project 実行時は `EntryPoint` 外で `set_exit_code` を使うと compile error になる。
- `main` 文字列依存の判定が削除される。
- ルール追加時に `SourceRules` 拡張で対応できる。

#### Implementation Notes

- `SourceRules` は host が注入し、各フェーズは判定のみを行う。
- entrypoint 判定は短名ではなく正規化済み symbol で比較する。
- 診断文面には「どの policy で禁止されたか」を含める。

#### Dependencies

- Depends on: `Issue A`
- Depends on: `Issue D`
- Related to: `Issue C`, `Issue E`

---

## 2. それ以外の課題分離

ここは **今すぐ StdModV1 実装を進めるための必須ではないもの** を、課題単位で切る。

---

### Future Issue 1: Project runner / source 操作 API

#### Title

`[Phase-StdModV1] Add project-runner source selection API for staged module builds`

#### Scope

- project runner から Loader を操作する API
- `add_path`
- `add_glob`
- `exclude_path`
- `exclude_glob`
- stage queue 構築
- source inclusion / exclusion の決定規則
- ENV による source 選択
- 最終採用 source 集合の決定的ダンプ

#### Notes

- project / project runner はこの Issue 群に寄せる
- entry symbol 自体は static に寄せる
- 同一 module path が最終集合に複数残れば compile error

---

### Future Issue 2: Project runner DSL / command mapping

#### Title

`[Phase-StdModV1] Add project-runner DSL for command-to-entry mapping`

#### Scope

- `surtr run main`
- `surtr build main`
- command 名と entry symbol の静的紐付け
- runner DSL 内での Loader 操作
- ENV 別設定
- ひな形生成との接続

#### Notes

- 例として提示された `entry("main", Env::Prod) { ... }` 系をここで扱う
- 動的にしすぎない制御が必要

---

### Future Issue 3: Project 初期化コマンド

#### Title

`[Phase-StdModV1] Add project initialization command with default Main::main scaffold`

#### Scope

- `cargo init` 相当の初期化
- `src/main.srt`
- `defmod Main { def main() -> Result<()> ... }`
- runner 雛形
- 基本ディレクトリ構成生成

---

### Future Issue 4: REPL command set

#### Title

`[Phase-StdModV1] Add REPL command set for module loading, browsing, and session control`

#### Scope

- `:load <path>`
- `:module <ModulePath>`
- `:reset`
- `:type <expr>`
- `:browse <ModulePath>`
- `:env`
- `:reload`
- `:entry <name>`
- `:quit`

#### Notes

- REPL コマンドは 1 Issue にまとめる
- コマンド意味論と UI/出力整形はこの中で扱う

---

### Future Issue 5: REPL file ingest semantics

#### Title

`[Phase-StdModV1] Support atomic file-ingest execution in REPL sessions`

#### Scope

- `--file` / `:load` のファイル全体 1 チャンク評価
- success 時 commit
- compile error 時 rollback + 継続/終了規則
- `Result::Err` 時の表示と継続
- runtime error 時強制終了
- session-local への定義持ち越し

---

### Future Issue 6: Script declaration boundary and DX improvements

#### Title

`[Phase-StdModV1] Define script declaration boundaries and improve script ergonomics`

#### Scope

- script の非関数トップレベル定義の許可/禁止を確定する
- pseudo module と module 非所属定義の境界を script 上で明文化する
- `@@entrypoint`
- shell command support 前提の script UX
- script annotations の使い勝手整理
- script 向け import / doc / test フロー

#### Notes

- `Issue C` では関数 entry と pseudo module だけを固定し、この Issue で script の非関数トップレベル規則を確定する
- 既存テスト資産の移行方針もこの Issue で扱う

---

### Future Issue 7: Build metadata / dump tracing

#### Title

`[Phase-StdModV1] Expose selected sources and entry metadata in build and dump outputs`

#### Scope

- 採用 source 一覧
- stage 配置
- entrypoint
- ENV 選択結果
- module path → source path
- bytecode functions の qualified name
- annotation metadata の dump

---

### Future Issue 8: runtime error taxonomy

#### Title

`[Phase-StdModV1] Define runtime error taxonomy separate from Result-based failures`

#### Scope

- VM 実装ミス
- 不正 bytecode
- stack overflow
- trap/panic
- capability violation
- Result で吸収しない失敗の分類

---

### Future Issue 9: 仕様ドキュメント更新

#### Title

`[Phase-StdModV1] Document compile-unit semantics and error boundary rules`

#### Scope

- `CompileUnitKind` の定義
- `SourceKind` の定義
- Script / Module / Project / REPL の許可構文表
- Script / REPL の pseudo module identity
- Script local / REPL session-local の可視範囲
- auto import 共通規則
- entrypoint 優先順位
- compile error / `Result::Err` / runtime error の責務境界
- REPL atomic chunk semantics
- Project runner の責務境界
- ENV 切り替え時の再現性要件

---

## 3. 別録: 採用方針まとめ

以下は **単体で読める仕様メモ** として使える別録。

---

### 3.1 基本方針

Surtr では、`Script`・`Module`・`Project`・`Repl` を同一視しない。  
それぞれは異なる compile unit として扱い、責務を分離する。

- **Script**  
  単一ファイルを起点とする実行単位。外部へ公開されないローカル定義を持てる。
- **Module**  
  Loader により収集される定義単位。単体では実行開始しない。
- **Project**  
  複数 module source を stage queue に従って収集・解決する実行単位。
- **Repl**  
  session を保持する対話実行単位。chunk 単位で compile/eval を行う。

---

### 3.2 CompileUnitKind

```text
CompileUnitKind =
  | Script
  | Module
  | Project
  | Repl
```

`CompileUnitKind` は「今回のコンパイル / 実行全体の種別」を表す。

---

### 3.3 SourceKind

```text
SourceKind =
  | Script
  | Module
  | StdModule
  | ReplChunk
```

`SourceKind` は「個々の source の種別」を表す。  
これは host / CLI 側が注入し、Surtr コード側へは漏らさない。

---

### 3.4 Loader の責務

Loader は **module source のみ** を扱う。

#### Loader が扱うもの
- `Module`
- `StdModule`

#### Loader が扱わないもの
- `Script`
- `ReplChunk`

script 実行は Loader ではなく、専用の script 実行 API から行う。  
これにより、定義収集と逐次実行の責務を分離する。

---

### 3.5 Module source の規則

module source はトップレベル式を持たず、定義のみから成る。

#### 許可
- `defmod`
- `trait`
- `impl Type`
- `defstruct`
- `defrecord`
- `defenum`
- `deferror`
- annotation metadata

#### 禁止
- トップレベル式
- module 外の逐次実行コード
- module ネスト

関数は `defmod` / `trait` / `impl Type` / 擬似 module 下でのみ定義できる。  
`defmod` は関数のみをメンバーとして持つ。

1 file に複数 `defmod` を置くことは許可する。  
file path と module path の一致は必須にしない。  
ただし module 所属は関数定義に対してのみ付与され、`defmod` 宣言単位で一意であり、定義分割所属は認めない。  
`defstruct` / `defrecord` / `defenum` / `deferror` / `trait` / `impl Type` 自体は module に属さない。

同一 module path、同一型名、同一完全修飾名が衝突した場合は compile error とする。

---

### 3.6 Script の規則

Script は Loader 対象外であり、単一ファイル実行のための compile unit である。  
Script は暗黙の pseudo module identity を持つが、user-visible な module namespace には参加しない。

script 内での top-level `def` は、その script の pseudo module に属する。  
`defp` であっても script 内では実質同じである。  
ただし、他 script や loaded module には漏れない。  
非関数トップレベル定義の最終規則は別 Issue として扱う。
`set_exit_code` は script 内ではトップレベル/関数内を問わず利用可能とする。

#### import 優先順位

```text
local > script-local def > explicit import > auto import
```

#### entrypoint 優先順位

```text
CLI引数 > @@entrypoint > トップレベル逐次実行
```

- `--entry name` 指定時のみ関数 entry 実行を行う
- `@@entrypoint` は script 内 1 個まで
- `@@entrypoint` 対象関数は `() -> Result<()>`
- `CLI --entry` は `@@entrypoint` より優先する

---

### 3.7 EntryPoint の統一

内部的には script / module / project の入口を共通の `EntryPoint` へ正規化する。  
user-facing には:

- script: `--entry main`
- project/module: `Main::main`

のように受けるが、内部では qualified symbol に揃える。  
script / REPL は pseudo module 経由で qualified symbol に正規化する。

entry 関数シグネチャは `() -> Result<()>` に固定する。

---

### 3.8 Project の方針

Project は複数 source を module 単位で収集し、stage queue に従って compile する単位である。  
Project 自体の source 収集や entry 決定は project runner が担う。

- stage queue は `List<List<String>>`
- 外側の要素が stage
- 同一 stage は並列可能レイヤー
- 後段 stage は前段完了後にのみ実行される

Project runner は将来的に以下を担う。

- source path 集合の決定
- stage queue 構成
- entry symbol 決定
- ENV による source 選択

Project compile unit では、`set_exit_code` は runner が指定した entrypoint 関数内のみ許可する。

同一 build 入力、同一 ENV、同一 source tree であれば、採用 source 集合と compile 結果は再現可能でなければならない。

---

### 3.9 REPL の方針

REPL は session を保持する compile unit である。  
以下を session に保持する。

- 変数束縛
- import 短名表
- ロード済み module
- 型環境
- 関数定義

REPL も pseudo module identity を持ち、top-level `def` はその pseudo module に属する。  
`set_exit_code` は REPL の終了コード介入には使えない。  
REPL の終了コードは通常 0 であり、runtime error やプロセス異常時のみ異常値を返す。

#### atomic chunk semantics

REPL は入力 chunk ごとに atomic evaluation を行う。

- compile error なら chunk を rollback して継続
- compile 成功なら VM で逐次実行
- 最後の評価値が `Err` ならメッセージ表示して継続
- runtime error は強制終了

rollback 対象:
- 変数束縛
- import 表
- 関数定義
- 型環境

ただし、すでに発生した副作用は rollback しない。

#### file ingest
REPL の `--file` / `:load` は **ファイル全体を 1 チャンク** として扱う。  
script 流し込みは「コピペ領域の代わりにファイルパス参照へ切り替わる」イメージである。

---

### 3.10 エラー境界

Surtr では、以下を明確に分離する。

#### compile error
- 構文エラー
- import 未解決
- 型不一致
- 宣言衝突
- entrypoint 条件違反

#### `Result::Err`
- 言語レベルで表現される失敗
- パターン不一致
- 0除算
- null 系
- Supervisor / process モデルで吸収可能な失敗

#### runtime error
- VM 実装ミス
- 不正 bytecode
- stack overflow
- trap/panic 相当

原則として、言語レベルで回収可能な失敗は `Result` に寄せ、VM/実装異常のみを runtime error とする。

---

### 3.10.1 SourceRules と `set_exit_code` 規則

`SourceRules` は compile context に保持し、最低限次を持つ。

- `allow_top_level_expr`
- `allowed_top_level_decl_kinds`
- `set_exit_code_policy` (`Forbidden` / `Anywhere` / `EntryOnly`)
- `normalized_entrypoint` (`Option<QualifiedSymbol>`)

`set_exit_code_policy` は次で固定する。

- `SourceKind::Script`: `Anywhere`
- `SourceKind::ReplChunk`: `Forbidden`
- `SourceKind::Module | SourceKind::StdModule` かつ `CompileUnitKind::Project`: `EntryOnly`
- それ以外: `Forbidden`

`EntryOnly` の場合、現在チェック中の関数 symbol が `normalized_entrypoint` と一致する場合のみ許可する。

---

### 3.11 annotation metadata

annotation (`@@doc`, `@@test`, `@@entrypoint` など) は module と script で同等に扱う。  
少なくとも以下へ保持されることを想定する。

- AST
- resolved / symbol metadata
- dump 出力
- REPL browse / doc 表示

---

### 3.12 将来拡張の境界

以下は現時点では採用対象外、または後続 Issue へ分離する。

- project runner DSL 詳細
- source inclusion / exclusion API
- script から script のロード
- shell command support
- REPL command 詳細
- runtime error taxonomy 完成
- build metadata / dump 拡張

---

## 4. 実装順とタスク分解

ここでは、上記 Issue をそのまま実装へ落とすための着手順とタスク粒度を定義する。  
各タスクは **1タスク = 1コミット** を原則とする。  
方針は **module 基盤を先に固定し、その後に script / REPL / entry を統一する**。

---

### 4.1 推奨実装順

1. `Issue A`
2. `Issue F`
3. `Issue B`
4. `Issue C`
5. `Issue D`
6. `Issue E`
7. `Future Issue 6`
8. `Future Issue 7`
9. `Future Issue 5`
10. `Future Issue 1`
11. `Future Issue 2`
12. `Future Issue 3`
13. `Future Issue 8`
14. `Future Issue 9`

実装ブロックとしては次の順で進める。

- 第1ブロック: `Issue A -> Issue F`
- 第2ブロック: `Issue B -> Issue C -> Issue D`
- 第3ブロック: `Issue E -> Future Issue 6 -> Future Issue 7`
- 第4ブロック: `Future Issue 5 -> Future Issue 1 -> Future Issue 2`
- 第5ブロック: `Future Issue 3 -> Future Issue 8 -> Future Issue 9`

---

### 4.2 タスク分解: Issue A

- `A-1`: `CompileUnitKind` を `Script / Module / Project / Repl` に拡張する（1コミット）
- `A-2`: `SourceKind` と `SourceDescriptor` 相当を導入する（1コミット）
- `A-3`: Loader API を module/stdmodule 専用に寄せ、script 入力を切り離す（1コミット）
- `A-4`: script 実行入力 API を別口で導入し、run 経路を Loader 非依存にする（1コミット）
- `A-5`: REPL 入力を `ReplChunk` として明示し、host 注入に統一する（1コミット）

---

### 4.2.1 タスク分解: Issue F

- `F-1`: `SourceRules` / `BuiltinPolicy` / `SetExitCodePolicy` の型を導入する（1コミット）
- `F-2`: `SourceKind + CompileUnitKind + EntryPoint` から `SourceRules` を導出する（1コミット）
- `F-3`: parser context に `SourceRules` を保持し、トップレベル規則を policy 判定へ移す（1コミット）
- `F-4`: typechecker へ `SourceRules` を伝播し、`set_exit_code` 判定を policy 化する（1コミット）
- `F-5`: `Script(許可) / ReplChunk(禁止) / Project entry-only` の fixture テストを追加する（1コミット）

---

### 4.3 タスク分解: Issue B

- `B-1`: module source から `defmod` を抽出する lower 層を追加する
- `B-2`: `defmod` 配下は関数のみを持てることを parse / validate で強制する
- `B-3`: module 所属関数と module 非所属定義を分離した中間表現を導入する
- `B-4`: duplicate module path 判定を file 単位ではなく抽出済み module 単位へ変更する
- `B-5`: module fixture と compile-error fixture を `defmod` ベースへ移行する

---

### 4.4 タスク分解: Issue C

- `C-1`: script 用 pseudo module identity を導入する
- `C-2`: REPL 用 pseudo module identity を導入する
- `C-3`: CLI の script 実行経路を Loader 非依存に差し替える
- `C-4`: script `--entry` の骨格を導入する

---

### 4.5 タスク分解: Issue D

- `D-1`: `EntryPoint` 型を導入し、script / module / project entry を同一表現へ正規化する
- `D-2`: entry 関数のシグネチャ検証を `main` 特別扱いから entrypoint 検証へ移す
- `D-3`: dump / debug で entrypoint 正規化結果を追跡できるメタデータを追加する

---

### 4.6 タスク分解: Issue E

- `E-1`: `compile error` と `runtime error` の終了経路を run / repl で整理する
- `E-2`: REPL の runtime error を継続ではなく終了に変更する
- `E-3`: `Result::Err` の表示経路を run / repl で共通化し、runtime error と混ざらないようにする

---

### 4.7 タスク分解: Future Issue 6

- `F6-1`: script の非関数トップレベル定義の許可/禁止を確定する
- `F6-2`: pseudo module と module 非所属定義の境界を script 上で文書化する
- `F6-3`: 既存テスト資産のうち、module Loader 経由へ移すものと script に残すものを整理する

---

### 4.8 タスク分解: Future Issue 7

- `F7-1`: dump に採用 source 一覧を出す
- `F7-2`: dump に module path / source path / entrypoint を出す
- `F7-3`: bytecode function の qualified name と annotation metadata を追跡可能にする

---

### 4.9 タスク分解: Future Issue 5

- `F5-1`: REPL `:load` / `--file` を 1 チャンク atomic 実行にする
- `F5-2`: compile error / `Result::Err` / runtime error の file ingest 時挙動を REPL 本体と揃える

---

### 4.10 推奨コミット分割（Issue = コミット単位）

- `Commit-01`: `A-1`
- `Commit-02`: `A-2`
- `Commit-03`: `A-3`
- `Commit-04`: `A-4`
- `Commit-05`: `A-5`
- `Commit-06`: `F-1`
- `Commit-07`: `F-2`
- `Commit-08`: `F-3`
- `Commit-09`: `F-4`
- `Commit-10`: `F-5`
- `Commit-11` 以降: `B-*`, `C-*`, `D-*`, `E-*` を同様に 1タスク1コミットで進める

この分割では、`A-*` と `F-*` が基盤、以降が規則強制と実行統一になる。
