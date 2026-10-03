# LSP のエディタ解析・成果物境界の改修ドラフト

状態: **ドラフト・未実装**。2026-10-03、`d96fc81e` 時点で確認したソースと正本文書を整理した。配置先や ProjectRunner の今後の扱いは未確定。この文書は現時点の判断材料であり、LSP 全体の再設計や新しい言語仕様を確定するものではない。

関連文書:

- [Surtr LSP の正本](../docs/dev/Surtr_LSP_spec.md)
- [EldrVM の正本](../docs/dev/EldrVM_spec.md)
- [AnalysisContext の既存案](lsp_analysis_context_spec_v0.md)
- [標準定義・共有表現の調査と修正方針: SD-10](spec_bypass_audit_standard_shared.md#sd-10-任意メタデータと-editor-tolerant-parse-は許可された補助経路)

## 1. 整理する問題

エディタ用成果物をプロジェクトの `target/` 内へ配置すれば、ユーザーが明示的にコンパイルした成果物との衝突は防げる。ただし、出力先の分離だけでは、次の契約は決まらない。

1. 未保存のソースを何で解析するか。
2. 部分 AST をどの機能に使い、どこから strict な成功が必要か。
3. 任意メタデータがないとき、各機能が何を返すか。
4. ProjectRunner を VM 実行せずに取り出した情報をどう扱うか。
5. 古い成果物を現在の編集内容の結果として見せないため、何で照合するか。

コンパイラ実行ファイルを `target/debug/` に置くことと、エディタが生成する `.eldr` を専用ディレクトリに置くことは別である。現行の拡張はこの区別を自動では行わない。

## 2. 確認できた現行挙動と根拠

以下はソースと文書の読解結果。実際の VSCode セッション、JSON-RPC 通信、デバッグ起動による確認はしていない。

| 対象 | 現行挙動 | 根拠 |
|---|---|---|
| LSP の実体 | `surtr-lsp` は `LspAnalysisHost` などを提供するライブラリ。stdio JSON-RPC の独立実行ファイルと CLI の `lsp` コマンドは確認できない | `crates/surtr-lsp/Cargo.toml`、`crates/surtr-lsp/src/lib.rs:94`、`crates/rune/src/main.rs:26` |
| language-support 拡張 | 正規表現ベースの symbol provider を登録する。LSP client として server を起動していない | `vscode-extensions/surtr-language-support/src/extension.ts:16` |
| surtr-tools の診断 | 保存済みファイルパスを `surtr check ... --format json` に渡す。未保存の buffer 内容を送っていない | `vscode-extensions/surtr-tools/src/extension.ts:30-45,77-85,111-120` |
| コンパイラの選択 | `surtr.compiler.path` は実行ファイルの選択。成果物の出力先やデバッグ情報の指定ではない | 同ファイル `124-130` |
| CLI check | ファイルを読み、`compile_source_with_measurement` を実行する。codegen を含む経路だが成果物を保存しない | `crates/rune/src/commands/check.rs:81-113` |
| エディタの手動 Build | `surtr build file.srt` を実行する。独自の出力先を渡していない | 拡張 `extension.ts:91-92` |
| CLI build 出力先 | 出力先の明示がなければ入力の拡張子を `.eldr` に置換する。エディタ由来かどうかで分岐しない | `crates/rune/src/commands/build.rs:96-101`、`crates/rune/src/util.rs:5-7` |
| AnalysisService | 通常の解析は source → parse → resolve → typecheck。Forge/Eldr による実行を通常診断に要求しない | `crates/surtr-analysis/src/service.rs:169-265`、`docs/dev/Surtr_LSP_spec.md:48-59` |
| 更新中の document | host は `did_open` / `did_change` の source と version を保持する API を持つ | `crates/surtr-lsp/src/lib.rs:131-140` |
| 部分 AST | tolerant parse の補助情報を取る一方、strict parse 失敗は Parse diagnostic として保持する。strict AST が得られなければ、その document の通常の型検査に進めない | `crates/surtr-analysis/src/service.rs:185-265`、`docs/dev/Surtr_LSP_spec.md:439-448` |
| 任意チャンク | `Docs` / `SigT` は省略可能。存在しなければ空の collection にするが、存在する不正 payload は `DecodeFailed` | `crates/sindr/src/ir.rs:1440-1447,1511-1513,1659`、`docs/dev/EldrVM_spec.md:476-486` |

通常の解析 service と、現在の VSCode 拡張が CLI check を起動する経路は異なる。正本の「通常診断に Forge/Eldr は不要」という方針を、現在の拡張が既にそのまま実装しているとは扱わない。

### ProjectRunner の補助経路

現行の正本は、host に注入する VM 実行器の有無で経路を分けている。

- 実行器がある: host が runner source を実行し、得られた `ProjectRunnerResult` を解析へ渡す。
- 注入済み実行器が失敗した: `ProjectRunner` diagnostic を返し、同じ source の source-only 抽出へ戻らない。
- 実行器がない: source-only 抽出へ進むことを現行仕様が認めている。

根拠は `docs/dev/Surtr_LSP_spec.md:783-791`、`crates/surtr-lsp/src/lib.rs:153-183`、`crates/surtr-analysis/src/context.rs:401-427`。source-only 側は `crates/surtr-analysis/src/project_runner.rs:113` 以下で AST を走査して設定情報を取り出す。これは名前解決・型検査・VM 評価後の結果ではない。`service.rs` の `load_project` 解析経路にも source-only 抽出があるため、host の実行器を注入するだけで全経路が揃うとは判断しない。

VM 実行側の境界は `crates/xldr/src/project_runner.rs` にある。補助的な source-only 抽出を今後も残すか、実行結果を必要とする機能では拒否するかは未確定。配置先の問題とは切り離して判断する。

## 3. 改修案: ソース解析と成果物を分離する

### 3.1 通常の診断・補完・outline

通常のエディタ機能は document の source と version を入力とする。未保存内容を扱う場合は `LspAnalysisHost` / `AnalysisService` の document 更新 API を使う構成を候補にする。現在の保存ファイル向け CLI check は、未保存内容の解析結果として表示しない。

strict parse の成否を保持し、成功した source だけを resolve/typecheck の入力にする。tolerant parse の情報は、入力途中の outline、補完位置、構文 context などの補助に限る。

```surtr
def incomplete(
# エディタでの期待値: Parse diagnostic と、取得可能な部分情報が併存する。
# コンパイルの期待値: 失敗。部分 AST を実行用の定義として採用しない。
```

この例は現行の許可範囲を維持する受入条件であり、parse Error を成功へ変える案ではない。

### 3.2 エディタ用の成果物

bytecode viewer や debugger など、成果物を必要とする機能は通常解析とは別の明示的な生成経路にする。生成する場合は strict なコンパイル成功を条件にする。

候補配置はプロジェクト内の `target/surtr/editor/`。このパス名、下位のディレクトリ構造、生成タイミングは未確定である。単に `target/debug/` とすると compiler executable との意味が混ざるので、エディタ所有と分かる場所を用意する。

| 成果物の用途 | 配置・扱いの案 |
|---|---|
| エディタの解析用・viewer/debug 用に自動生成 | エディタ専用ディレクトリに配置し、ユーザーの `.eldr` と衝突させない |
| ユーザーが明示して実行した CLI build | ユーザー指定先または CLI の既定出力先。エディタ専用成果物として回収しない |
| 現在の拡張の手動 Build コマンド | 自動生成へ移すか、ユーザーの明示 build として残すかを決め、コマンド説明と出力先を合わせる |

現在の手動 Build は CLI build と同じ出力先になる。今後の分類を実装せずに「既に区別される」とは説明しない。

生成・再利用時は source の内容または hash、project/compile-unit、profile、正規化済み runner args、compiler の識別情報など、結果へ影響する入力を照合する。開いている document は version も照合する。具体的な照合データの保持形式は未確定で、既存の AnalysisContext と cache key を確認してから決める。

コンパイル失敗、入力変更、runner 失敗時に古い成果物を現在の成功結果として再利用しない。古い snapshot を閲覧できる設計を選ぶ場合も、対象 source と古いことを明示する。保存の途中失敗でも不完全な `.eldr` を成功結果として公開しない。清掃対象はエディタ所有のディレクトリに限定する。

### 3.3 metadata の不足と破損

`Docs` / `SigT` は現行形式の任意チャンクであり、欠損自体を不正 bytecode としない。存在する payload が decode できない場合は、現在どおりエラーにする。実行用の必須チャンク不足も成功へ変えない。

doc 表示、signature 表示、debugger などの機能ごとに、必要な情報と、不足時に返す「情報なし」または診断を決める。エディタ側の生成条件で必要情報を用意できるかも別途確認する。配置先を変更しただけでは metadata は増えない。

`@doc` が書かれていない定義に doc entry がないことと、必要な型・source 対応情報が壊れていることを区別する。全定義への `@doc` 記載を暗黙に義務化しない。現行 encoder は空の `Docs` / `SigT` を省略するため、空と欠損の違いだけで生成不良とは断定できない。

この改修のためにスキーマ・VM バージョンを引き上げたり、旧形式を救済する経路を追加したりしない。

### 3.4 ProjectRunner の結果の扱い

注入済み VM 実行器の失敗を source-only 抽出で救済しない現行境界は維持する。未注入時の扱いは次の判断が必要である。

| 選択肢 | 必要な整理 |
|---|---|
| source-only 抽出を補助機能として残す | 扱える構文・情報・不足条件を明示し、VM 評価結果と同じ確度の設定として公開しない |
| runner の実行結果が必要な機能では、実行器の未注入をエラーにする | host の注入契約を必須化し、`load_project` を含めて source-only の代替経路を取り除く範囲を決める |

この選択は本ドラフトで確定しない。どちらを採る場合も、解析の呼出し元によって異なる project/profile が黙って選ばれないようにする。通常 source の診断に VM を必須化する意味でもない。

## 4. 修正後に確認する例と期待値

下表は改修の受入条件案であり、今回の実測結果ではない。

| 入力・操作 | 期待値 |
|---|---|
| 未保存の正常 source を更新 | 現在の document version の解析結果を返す。保存済みファイルの結果へ黙って戻らない |
| 未保存の `def incomplete(` | Parse diagnostic を維持する。補助情報は使えるが、型検査成功・成果物生成成功と扱わない |
| 旧 version の解析が後から完了 | 新 version の現在の結果を上書きしない |
| 同じ entry をエディタと CLI が build | エディタ用生成を実装した後は専用配置に分け、ユーザー成果物を上書きしない |
| エディタ用 build が失敗 | 成功成果物を更新しない。古い成果物を新 source の結果として提示しない |
| `Docs` / `SigT` がない正常 bytecode | 任意チャンク欠損だけで decode Error にしない。該当機能は不足情報を明示する |
| `Docs` / `SigT` が存在するが payload が不正 | DecodeFailed。空 metadata へ救済しない |
| 実行必須チャンクが欠ける | MissingRequiredChunk。推測した情報で実行しない |
| 注入済み runner 実行器が失敗 | ProjectRunner diagnostic。source-only 抽出へ戻らない |
| runner 実行器が未注入 | 現行は source-only 抽出。改修後の期待値は 3.4 の選択後に確定する |
| profile / runner args / source が変わる | 古い context・成果物を現在の入力と照合せず再利用しない |
| エディタ用成果物を清掃 | エディタ所有の成果物だけを削除し、ユーザーが明示生成したものは削除しない |

## 5. 実装前に決める事項

- VSCode 拡張を `LspAnalysisHost` に接続する方式。独立 server の transport と起動方法は今回の調査範囲では未確定。
- エディタ用成果物の正確な出力先、生成を必要とする機能、生成タイミング、手動 Build との区別。
- viewer/debugger 各機能の metadata 要件。既存 compiler から生成される情報との対応。
- ProjectRunner の source-only 抽出を残す許可範囲、または削除する範囲。`load_project` を含む全入口の整合。
- context と成果物の照合情報、cache の無効化、失敗時の表示、清掃方針。

実装時は、まず source と version による通常解析を固定し、次に必要な editor artifact の生成と照合を整理する。ProjectRunner の未確定事項は現行経路を確認してから決定する。置き換えが確定した旧経路を互換フォールバックとして残さない。

本ドラフトの作成時には、本書と SD-03〜11 の方針文書だけを作成・更新した。既存 LSP 案、他の調査記録、正本文書、ソースコード、テストは変更していない。ビルド・テストも実行していない。
