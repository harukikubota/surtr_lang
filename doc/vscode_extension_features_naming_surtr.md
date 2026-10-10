# VSCode拡張に入れておくとよいもの

## 目的

Surtr 向けの VSCode 拡張で、最初から入れておくと価値が高い機能を整理する。  
あわせて、拡張名・ language id ・コマンド名・設定キー名の命名方針を決める。

---

## 概要

VS Code の言語拡張は大きく 2 系統に分けて考えると整理しやすい。

### 1. Declarative な機能
設定ファイル中心で入れられるもの。

- 言語登録
- ファイル拡張子関連付け
- シンタックスハイライト
- コメント記号
- 括弧対応
- 自動クローズ
- インデント
- folding
- snippets

### 2. Programmatic な機能
拡張コードや LSP で提供するもの。

- 補完
- Hover
- Diagnostics
- Go to Definition
- References
- Rename
- Semantic Tokens
- Inlay Hints
- Code Actions
- REPL / コマンド連携

---

## 最低限ほしいもの

## 2. Language Configuration

### 役割
基本編集体験を作る。

### 入れるもの
- block comment
- folding markers

---

## 4. Snippets

### 役割
記述量を下げる。

### 最低限
- `if`
- テスト用テンプレート
- import / module 宣言

---

## 5. Diagnostics

### 役割
コンパイルエラーやパースエラーを editor 上で見せる。

### 最重要ポイント
Surtr は ariadne 的な分かりやすいエラーを強みにしやすいので、  
VSCode 側でも span をそのまま diagnostics に流せると価値が高い。

### 最初に出したいもの
- 構文エラー
- 未定義シンボル
- 重複定義
- import 解決失敗
- 型エラーの一部
- 特殊トークンの不正コンテキスト

---

## 強くおすすめするもの

## 6. 補完

### 優先順位
1. keyword 補完
2. ローカル変数
3. 関数名
4. module 名
5. type 名
6. variant 名
7. builtin

### Surtr で特に大事
- import 可能なものだけ出す
- コンテキストに応じて候補を絞る
- 演算子も候補に出せるようにする
- Result / List など標準 API を強めに出す

---

## 7. Hover

### 役割
カーソル位置で最小限の意味を返す。

### 表示したいもの
- 型
- シグネチャ
- doc
- builtin か user 定義か
- module 所属

### Surtr で有効
- `Ok` / `Err` の型
- 演算子の desugar 結果の簡易表示
- facet / path 参照の型

---

## 8. Document Symbol

### 役割
アウトライン表示やファイル内移動を強くする。

### 対象
- `type`

### 効果
- 長いファイルでも追いやすい
- LSP が軽くても体感がかなり良くなる

---

## 9. Go to Definition / References

### 最低限
- 関数
- 型
- モジュール
- 変数
- enum variant

### Surtr での価値
静的型付き言語では、移動系があるだけで理解コストがかなり下がる。

---

## 10. Semantic Tokens

### 役割
TextMate より正確な色分けを行う。

### 特に分けたいもの
- module
- type
- variant
- builtin
- local variable
- parameter
- property
- macro
- attribute

### Surtr で効果が大きい理由
- `Type` と `value` を見分けやすい
- `Ok` / `Err` を variant として塗れる
- builtin と user 定義を分けられる
- import されていない識別子との差も見せやすい

---

## あるとかなり便利なもの

## 11. Code Actions

### 候補
- import を追加
- 型注釈を挿入
- `match` の雛形生成
- `Ok/Err` 分岐の雛形生成
- 未使用 import を整理
- module 名補完

---

## 12. Signature Help

### 用途
- 関数呼び出し時の引数説明
- 演算子 desugar 後の理解補助

### 向いている場面
- 標準 API
- builtin
- Result / List / Process 系 API

---

## 13. Inlay Hints

### 候補
- 推論された型
- パラメータ名
- 戻り値の簡易型
- クロージャ引数型

### 注意
最初は少なめにした方がよい。  
多すぎるとノイズになる。

---

## 14. Rename

### 条件
- 定義解決が安定してから入れる
- 早すぎると誤 rename が危険

### 優先度
- MVP では低め
- LSP が安定したら高い価値が出る

---

## Surtr 向けに特に相性がよいもの

## 15. REPL / CLI 連携

### 候補
- 選択範囲実行
- REPL を開く
- 実行結果を Output panel に出す

### 例
- `surtr.runFile`
- `surtr.runSelection`
- `surtr.repl.open`

---

## 16. バイトコード / Viewer 連携

Surtr / Eldr はここが差別化ポイントになりやすい。

### 候補
- `.eldr` を JSON dump
- Viewer を開く
- opcode 一覧を表示
- source map 対応
- 関数単位で移動

### ただし
これは言語サポート本体と分けてもよい。

---

## 推奨構成

## B. 言語機能拡張
次に足すもの。

### 含めるもの
- completion
- hover
- go to definition
- references
- semantic tokens
- code actions

---

## C. ツール拡張
必要なら別拡張に分ける。

### 含めるもの
- REPL 連携
- bytecode dump
- viewer 起動
- debug / inspect 系コマンド

---

## 命名

## 基本方針

### 1. language id は短く固定する
- `surtr`

### 2. package name は用途が分かるようにする
- 言語サポート本体
- ツール連携
- viewer 連携
を分けて考える

### 3. command は `surtr.` prefix で揃える
拡張が増えても一覧しやすい。

### 4. config key も `surtr.` prefix にする
設定が探しやすい。

---

## 推奨命名案

## 2. LSP / 高機能版

### Display Name
- `Surtr Tools`

### Extension Identifier
- `surtr-lang.surtr-tools`

### 役割
- completion
- hover
- definition
- references
- semantic tokens
- code actions

---

## 3. バイトコード / Viewer 用

### Display Name
- `Eldr Viewer`
- または `Surtr Bytecode Tools`

### Extension Identifier
- `surtr-lang.eldr-viewer`
- または `surtr-lang.surtr-bytecode-tools`

### 役割
- dump
- inspect
- viewer 連携

---

## コマンド命名案

## 基本形式
`surtr.<domain>.<action>`

### 実行系
- `surtr.run.selection`
- `surtr.repl.open`
- `surtr.repl.sendSelection`

### 移動系
- `surtr.goto.definition`
- `surtr.goto.references`

### ツール系
- `surtr.bytecode.openViewer`
- `surtr.inspect.tokens`
- `surtr.inspect.ast`

### 補助系
- `surtr.format.document`
- `surtr.restartLanguageServer`

---

## 設定キー命名案

### 例
- `surtr.lsp.enabled`
- `surtr.semanticTokens.enabled`
- `surtr.repl.path`
- `surtr.bytecode.viewer.enabled`
- `surtr.diagnostics.onType`

---

## ファイル / grammar 命名案

### grammar
- `surtr.tmGrammar.json`

### language configuration
- `language-configuration.json`

### snippets
- `surtr.code-snippets`

### icon
- `surtr-icon.png`

---

## パッケージを分けるか

## 分ける案

### 1. `Surtr Language Support`
- syntax
- language config
- snippets

### 2. `Surtr Tools`
- LSP
- diagnostics
- semantic tokens
- hover
- completion

### 3. `Eldr Viewer`
- bytecode
- inspect
- viewer

### 利点
- 依存が分離できる
- viewer だけ別 release しやすい
- wasm や外部バイナリ依存を分けやすい

### 欠点
- ユーザーにとっては少し分かりにくい

---

## まとめて 1 本にする案

### Display Name
- `Surtr`

### 利点
- 導入が簡単
- 初期ユーザーには分かりやすい

### 欠点
- 将来肥大化しやすい
- viewer や外部ツール依存が混ざりやすい

---

## 最終提案

## MVP で入れるべきもの
- Language Configuration
- Snippets
- Diagnostics
- Document Symbol

## 次に入れるべきもの
- Completion
- Hover
- Go to Definition
- References
- Semantic Tokens

## 後でよいもの
- Rename
- Inlay Hints
- Code Actions
- REPL / Viewer 深い統合

## 命名
- language id: `surtr`
- 本体名: `Surtr Language Support`
- 高機能版: `Surtr Tools`
- ビュワー: `Eldr Viewer`

---

## 参考メモ

VS Code の設計上、基本編集機能は declarative に載せやすく、  
補完・エラー・定義ジャンプなどは programmatic feature として積み上げるのが自然。  
そのため Surtr でも、最初は syntax / config / diagnostics を固め、後から LSP を厚くする方針が扱いやすい。
