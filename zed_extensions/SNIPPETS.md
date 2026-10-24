# Surtrスニペット

採用する構文と既定の展開コードは、Surtrの言語仕様、コンパイラの実装、
標準定義と実行可能なテストを根拠に決めます。エディタ間の項目の一致は求めません。
入力を始める際に役立つ宣言と式を選び、各スニペットの既定値が有効な文脈で動くことを検証します。

## 構文と使用文脈

| prefix | 展開するもの・使用条件 | 仕様の参照先 |
|---|---|---|
| `def`・`defp`・`defexpr` | ブロック本体・private関数・同じ行の式本体 | [関数](../docs/site/language-reference.md#関数) |
| `defbool` | 戻り値がBooleanの末尾`?`付き関数 | [関数](../docs/site/language-reference.md#関数) |
| `defgeneric` | 値引数から`$T`を導入する関数。通常の関数名の後に型引数リストは付けない | [型注釈](../docs/site/type-annotations.md) |
| `defmod` | moduleファイルのモジュール宣言。script top-levelには置けない | [言語リファレンス](../docs/site/language-reference.md)、`crates/spire/src/parser/validate.rs` |
| `defstruct` | structと必須の`impl ... { def new(...) -> Self { ... } }`を一緒に定義 | [Structs](../docs/site/structs.md) |
| `defrecord` | 1個以上のpublicフィールドを持つRecord。constructorは自動生成 | [データ定義](../docs/site/language-reference.md#データ定義) |
| `defenum`・`deferror` | variantを持つEnum、messageと保存フィールドを持つError | [データ定義](../docs/site/language-reference.md#データ定義) |
| `deftrait` | `Self`を使ったmethodの契約 | [Trait System](../docs/site/trait-system.md) |
| `impl` | 既存structの唯一のimplに`new`とmethodを定義。既存implがある場合は、その中へmethodを追加する | [Structs](../docs/site/structs.md) |
| `impltrait` | 既存traitの実装。型とtraitの宣言は先に用意する | [trait実装](../docs/site/trait-impls.md) |
| `const` | file top-levelのprimitive定数 | [const](../docs/site/language-reference.md#const) |
| `type` | 標準の`Mapper`と同じ型引数付き関数シグネチャのalias。任意のデータ型aliasは使えない | [言語リファレンス](../docs/site/language-reference.md)、[標準定義](../lib/function.srt)、`crates/spire/src/parser/decl.rs` |
| `match` | 網羅的なBooleanの分岐。両armの既定値はUnit | [Pattern Matching](../docs/site/pattern-matching.md) |
| `do` | 明示carrier、`<-`によるbind、最後に同じcarrierの値を返す式 | [do](../docs/site/do.md) |
| `closure` | 型を注釈した引数を持つclosure | [Callables](../docs/site/callables.md) |
| `safebind` | `=?`でpayloadを一段取り出す文。失敗返却先のある関数やdo内に置く | [エラーハンドリング](../docs/site/error-handling.md) |
| `doc` | 後続の宣言に付ける`@doc`と三重引用符の本文 | [言語リファレンス](../docs/site/language-reference.md) |
| `test`・`describe`・`it`・`xit`・`pend` | Test DSL。`import Test;`と`surtr test`で使う。`xit`・`pend`には理由が必要 | [Test](../docs/site/test.md)、[標準定義](../lib/test.srt) |
| `assert_eq`・`assert_true`・`assert_false` | `Result<()>`を返すTestアサーション | [Test](../docs/site/test.md)、[標準定義](../lib/test.srt) |

`test`は`import Test;`を含むファイルのひな形です。他のTest用スニペットは
import済みのファイルに挿入します。ケース末尾のアサーションはResultを返すままにし、
途中のアサーションで失敗を伝播するときだけ文末に`?`を付けます。

`defstruct`で定義した型を外から作るときは`Example(1)`のようにconstructorを使います。
構造体リテラルは所有者の`impl`内に置きます。`defrecord`は`new`の実装を必要としません。

## 変更時の検証

```sh
cd zed_extensions
npm test
npm run test:snippets
npm run dev
```

`test:snippets`は現在のcheckoutのSurtrをビルドし、全登録スニペットの既定値を展開します。
宣言・式は必要な型や呼び出しを用意して`surtr run`、Test DSLは`surtr test`で検証します。
`defmod`はinclude先のmoduleファイルに配置します。`xit`は`--include-xit`で実行し、
`pend`は未実装1件として記録されることを確認します。

スニペットを追加・変更するときは、対応する仕様とこの表を確認し、
`tests/snippets-runtime.cjs`に使用文脈と期待する実行結果を追加してください。
既定値付きtab stop、リンクしたtab stop、型変数用のエスケープも、実際の展開コードで検証します。
