# Surtr Compiler Design

このドキュメントは、「プログラミング言語を作ってみたい人」に向けた Surtr の設計解説です。

ここでは作り方の手順ではなく、責務分離と crate 境界を中心に説明します。

## 1. 設計の考え方

Surtr は hobby 言語です。だからこそ、次の点を強く意識しています。

- 実装を追いやすいこと
- どの段階で何を確定するかが明確であること
- ランタイムに責務を押し込みすぎないこと
- 言語機能を足すときに、どの crate を触るべきか分かりやすいこと

言い換えると、Surtr は「最適化より境界の明瞭さ」を優先する設計です。

## 2. パイプライン

Surtr のコンパイルパイプラインは次の 5 段階です。

```text
Spire -> Sigil -> Scar -> Forge -> Eldr
```

CLI の入口は Rune、対話実行は Xldr が受け持ちます。

### Spire

- 入力: source text
- 出力: `Ast`
- 役割:
  - 字句解析
  - 構文解析
  - compile unit ごとの構文制約適用

### Sigil

- 入力: `Ast`
- 出力: `Resolved`
- 役割:
  - 名前解決
  - import 処理
  - auto import 処理
  - unique id の割り当て

### Scar

- 入力: `Resolved`
- 出力: `TypedNode`
- 役割:
  - 型検査
  - `match` 網羅性確認
  - 組込み関数シグネチャ検証

### Forge

- 入力: `TypedNode`
- 出力: `Bytecode`
- 役割:
  - opcode 列の生成
  - 関数テーブルの構築
  - type registry / error template の構築

### Eldr

- 入力: `Bytecode`
- 出力: 実行結果
- 役割:
  - VM 実行
  - builtin dispatch
  - runtime error 検出

## 3. 「段階ごとに型を変える」設計

Surtr は、各段階の中間表現を意図的に分けています。

```text
Source -> Ast -> Resolved -> Typed -> Bytecode
```

この分割の利点は次のとおりです。

- parser に名前解決の責務を混ぜない
- resolver に型検査の責務を混ぜない
- codegen が「解決済み・型検査済み」を前提に単純化できる
- テストを段階ごとに切り分けやすい

言語実装では「とりあえず AST を直接コード生成する」方向に流れやすいですが、Surtr ではあえて段階分離を優先しています。

## 4. 標準モジュールの設計

Surtr では、標準モジュールも source として扱います。

ロード順は固定です。

```text
Bootstrap -> [Kernel, 他標準モジュール] -> ユーザ拡張
```

### `Bootstrap`

責務:

- builtin 宣言
- 汎用 error 定義

狙い:

- 他の標準 API より先に解決できる土台を置く
- loader と auto import の起点を固定する
- universally useful な concrete error を最初の標準ステージから使えるようにする

### stage 2: `Kernel` + type modules

責務:

- `Kernel`
  - auto import される小さな標準 API
  - `defmod Kernel` の中にある `print` のような cross-cutting builtin
  - 専用 file を持たない `Unit` の type 宣言
- type modules
  - `Int`, `String`, `Boolean`, `Error`, `List`, `Result`, `Float`
  - 各 file top-level の canonical builtin type head
  - 各 `defmod Name` の module API
  - source 上の `@@doc`

狙い:

- 「処理系に埋め込む最低限」と「Surtr で書ける標準 API」を分ける
- builtin type contract を各 type file のトップレベル宣言へ寄せる
- 標準モジュールの説明も `lib/*.srt` に同居させる

### なぜ分けるのか

この分離には 2 つの意味があります。

- 設計上の意味
  - builtin と標準ライブラリ相当のコードを分けられる
- 将来拡張上の意味
  - 並列コンパイルや標準モジュール追加のときに依存順序を明確にできる
  - type ごとの API と builtin type head を file 単位で保守できる

## 5. SourceKind と CompileUnitKind

Surtr では、「何を読むか」と「どういう実行単位か」を分けて扱います。

### SourceKind

- `Script`
- `Module`
- `StdModule`
- `ReplChunk`

### CompileUnitKind

- `Script`
- `Module`
- `Project`
- `Repl`

これを分けている理由は、同じ parser でも source の性質によって許可したい構文が違うためです。

例:

- user module では `@@builtin` を禁止したい
- std module では `@@builtin` を許可したい
- REPL では top-level expression を許可したい
- project build では `set_exit_code` を entrypoint のみに制限したい

この方針を `SourceRules` に閉じ込めることで、個別の crate に ad-hoc な分岐を書き散らさずに済みます。

## 6. builtin の設計

Surtr の builtin は、ユーザーコードに直接埋め込まれた特殊処理ではありません。

基本方針は次のとおりです。

- builtin メタデータは共有テーブルで一元管理する
- Sigil / Scar / Forge / Eldr が同じ定義を参照する
- Surtr source 側の `@@builtin def ...` は、その共有定義の宣言層として扱う
- `@@builtin type ...` は各対応 `lib/*.srt` のトップレベルで canonical head を宣言する
- `@@doc` は標準ライブラリ source と `.eldr` metadata の橋渡しに使う

この設計の利点は、段階ごとの builtin 解釈ズレを避けやすいことです。

## 7. 名前解決の設計

Sigil の役割は、単に識別子を探すことではありません。

Surtr では次も resolver の責務に寄せています。

- import 適用
- auto import 適用
- duplicate import 検出
- 宣言インデックスを前提にした前方参照解決

これは「構文として parse できる」と「その名前が有効である」を明確に分けるためです。

## 8. 型検査の設計

Scar では、式の型だけでなく「言語としての整合性」を見ます。

たとえば次のようなものは Scar の責務です。

- `Result` を返すべき位置で正しく `Result` になっているか
- `Result<T, E>` の `Err` 側契約が値表現の `Result<T>` と矛盾しないか
- `match` が網羅的か
- field access がどの index を参照するか
- builtin シグネチャと実際の呼び出しが合うか

この時点で field 名を index に解決しておくことで、Forge と Eldr を単純化できます。

### apply / compose 系も Scar で意味を確定する

Surtr の `|>`, `|*>`, `|>=`, `>>`, `|=>`, `=?` は見た目が近くても責務が違います。

- `|>`, `|*>`, `|>=` は apply
- `>>`, `|=>` は compose
- `=?` は束縛付き制御

この違いは parser ではなく Scar で型に基づいて確定します。

特に重要なのは次です。

- apply 系は call 式を受けて第一引数注入を行える
- compose 系は closure value しか受けない
- `Result` と `List` の文脈違いは Scar の型規則で分岐する

こうしておくと、Forge は「どの外部契約が確定済みか」を前提に lower できます。

## 9. Bytecode VM を後段へ押し込む設計

Surtr は stack-based VM を採用していますが、VM を賢くしすぎない方針です。

つまり、Eldr は「よく整形された bytecode を実行する」側に寄せています。

- import 解決はしない
- 型推論はしない
- field 名解決はしない
- builtin の意味解釈は `builtin_id` dispatch に寄せる

この設計にすると、VM は小さく保ちやすく、バグの責務点も追いやすくなります。

### 末尾呼び出し最適化はあるか

現時点の Surtr には、限定付きの末尾呼び出し最適化があります。

- ある:
  - user function への tail-position call
  - `if` の branch 末尾
  - `match` の arm 末尾
  - 関数本体や closure 本体の最終式
- まだ限定されている:
  - top-level call は再利用しない
  - builtin call は対象にしない
  - 判定は「bytecode 上で call の次が `Return` か」に依存する

このため、`fib_tail` のような tail-recursive な関数では call frame の増加を抑えられます。一方で、`1 + recurse(n - 1)` のような non-tail recursion は最適化されず、従来どおり frame depth が増えます。

Surtr では「なんでも自動で速くする」よりも、「どの形が最適化対象か」を説明できることを優先しています。最適化の有無や適用範囲は、公開 docs と canonical spec の両方で追跡します。

## 10. テスト戦略

Surtr のテストは、機能と責務の境界に合わせて分けています。

- `spec`
  - 言語機能として正しく動くか
- `compile_errors`
  - 失敗すべき入力が正しく失敗するか
- `integration`
  - CLI 契約が保たれているか
- `unit`
  - 各 crate の内部契約が保たれているか

言語実装では E2E だけに寄りがちですが、Surtr では「壊れた場所が分かる」ことを重視しています。

## 11. この設計が向いている人

Surtr の設計は、次のような人に向いています。

- 小さめの言語を長く育てたい
- パイプラインを分けて理解したい
- parser / resolver / typechecker / codegen / VM を分業的に考えたい
- runtime magic より、前段での確定を好む

より具体的な crate ごとの責務を見たい場合は、次に [クレート設計リファレンス](./crate-reference.md) を読むと追いやすいです。
