# Spire `chumsky` 移行詳細設計

> 目的: `doc/依存整理詳細設計.md` 適用後の `Spire` を前提に、再帰下降 parser を `chumsky` ベースへ移行する。
> 本書は parser 実装の詳細設計を扱う。言語仕様の正本は `doc/要件定義v9.md`、crate 間責務の正本は `doc/依存整理詳細設計.md` を優先する。

最終更新日: 2026-04-14

---

## 1. 前提

本設計は、先に `doc/依存整理詳細設計.md` が適用され、少なくとも次が成立している前提で書く。

- `spire` は `sindr` のみへ依存する
- `Span` などの共有葉型は `sindr` 側へ整理済み、または移行途中でも `Spire` 外部契約は固定されている
- `forge` / `diagnostics` / `eldr` は `spire` の内部 parser 実装へ依存しない
- `Spire` の外部契約は `Ast`, `ParseError`, `parse`, `parse_with_context`, `ParserContext`, `ParseRules` に集約される

本設計のゴールは「文法実装を `chumsky` へ置き換えること」であり、言語 surface の追加や構文仕様の変更は目的に含めない。

---

## 2. 背景と現状課題

現行 `Spire` は以下の構造的負債を持つ。

- [crates/spire/src/parser/mod.rs](/Users/haruca/work/rust/surtr/crates/spire/src/parser/mod.rs) は分割を進めたものの、依然として cursor / helper / span 補正の集約点であり、責務の最終整理が必要
- 再帰下降 parser が token cursor 操作と手書き precedence 制御に強く依存しており、局所変更でも広範囲に影響する
- `ParserContext` / `ParseRules` による文脈制約と、純粋な構文規則が混在している
- `>>` を type 文脈で `>` `>` として扱うために token stream を途中で書き換える実装がある
- string interpolation, trailing block sugar, builtin decl などの特殊規則が個別関数に散在している

これにより、新構文追加だけでなく既存構文の保守も重くなっている。`chumsky` へ移行する主目的は、文法定義を composable にし、責務を分割した状態で parser を保守できるようにすることにある。

---

## 3. 設計方針

### 3.1 外部契約は維持する

移行後も次の public API は維持する。

- `parse(source: &str) -> Result<Vec<Ast>, ParseError>`
- `parse_with_context(source: &str, context: ParserContext) -> Result<Vec<Ast>, ParseError>`
- `ParseError::{Incomplete, SyntaxError}`
- `ParserContext`, `ParseRules`, `TopLevelDeclPolicy`, `TopLevelDeclKind`

互換方針:

- AST shape は互換維持を原則とする
- `ParseError` の variant は増やさない
- error 文面は完全一致を要求しないが、`phase=parse` 判定と span の意味は維持する
- `rune` / `xldr` が利用している `lexer::tokenize` / `token::Token` は、今回の移行では破壊的に変えない

### 3.2 parser と policy validation を分離する

`chumsky` で担う範囲は「syntax を AST に落とすこと」に集中させる。`ParseRules` に基づく compile-unit 制約は post-parse validation へ寄せる。

分離後の責務:

- syntax parser:
  - token 列から `Vec<Ast>` を構築する
  - block / module body / trait body など、構文的に異なる surface を切り替える
  - statement separator や operator precedence を扱う
- validator:
  - script / module / repl / project の top-level 制約を検査する
  - builtin decl 許可有無、top-level expr 許可有無を検査する
  - 既存 `ParseRules` と `ParserContext` 契約を維持する

### 3.3 巨大ファイルへ戻さない

`parser/mod.rs` 単一実装へ戻るのを防ぐため、移行後の責務分割を先に固定する。

---

## 4. 目標アーキテクチャ

### 4.1 データフロー

```text
source: &str
  -> lexer::tokenize()                  // 既存 public lexer。外部互換のため維持
  -> syntax_token::adapt()              // parser 専用の内部 token 列へ正規化
  -> grammar::program(surface_kind)     // chumsky parser
  -> validate::apply_source_rules()     // ParseRules / ParserContext 検証
  -> Vec<Ast>
```

重要点:

- public lexer を直接 `chumsky` 入力にしない
- parser 専用の内部 token へ正規化することで、`Token` public API を壊さずに文法都合の token 形を得る
- `ParseRules` は grammar に埋め込まず、AST 後検証に寄せる

### 4.2 想定 module 構成

```text
crates/spire/src/
├── ast.rs
├── error.rs
├── lexer.rs                // 既存 public lexer。互換維持
├── token.rs                // 既存 public token。互換維持
├── parser/
│   ├── mod.rs              // parse / parse_with_context の public entry
│   ├── context.rs          // ParserContext / ParseRules / surface enum
│   ├── syntax_token.rs     // chumsky 用の内部 token と adapter
│   ├── error_map.rs        // Rich -> ParseError 正規化
│   ├── completion.rs       // 部分入力 / 補完コンテキスト抽出
│   ├── diagnostic.rs       // ParseDiagnostic / LSP DTO 変換
│   ├── ty.rs               // type parser（現行 recursive-descent 分離済み）
│   ├── pattern.rs          // bind / match pattern（現行 recursive-descent 分離済み）
│   ├── interpolate.rs      // string interpolation 専用 helper
│   ├── validate.rs         // ParseRules / top-level policy validation
│   ├── expr.rs             // 式 parser（legacy recursive-descent 分離済み）
│   ├── decl.rs             // def / import / impl / builtin decl parser（legacy recursive-descent 分離済み）
│   ├── stmt.rs             // statement / separator parser（legacy recursive-descent 分離済み）
│   └── tests.rs            // parser unit tests
└── parser_legacy.rs        // 移行期間のみ。最終的に削除
```

補足:

- 旧 `parser.rs` は削除し、`parser/mod.rs` 起点へ分割する
- `ParserContext` 群は public 契約なので `parser/context.rs` に移して re-export する
- legacy parser は移行期間のみ残し、AST parity test が揃った時点で削除する

---

## 5. 内部 token 正規化

### 5.1 方針

`chumsky` parser がそのまま扱いやすい token 形へ、public token 列を内部 token 列へ写像する。

理由:

- `rune` / `xldr` は `lexer::tokenize` と `Token` を annotator 判定などに使っている
- parser の都合だけで public token を破壊的に変えると、移行範囲が不必要に広がる
- 一方で `chumsky` 側では `::` や `>>` の扱いを文法都合に合わせて正規化したい

### 5.2 `SyntaxToken` の役割

`SyntaxToken` は parser 専用の internal enum とする。

代表的な正規化:

- `Token::Colon` + `Token::Colon` を `SyntaxToken::PathSep` へ畳み込む
- `Token::Compose` を `SyntaxToken::Gt` + `SyntaxToken::Gt` へ分解する
- `Token::Unit` は維持してよい
- `Token::Annotator("builtin")` などはそのまま `SyntaxToken::Annotator(String)` へ写す

受け入れ条件:

- public `Token` は現状互換のまま維持できる
- type parser 側に `expect_type_gt()` 相当の token stream 書き換えロジックを持ち込まない

### 5.3 span の扱い

token 正規化で 1 個の public token を複数 `SyntaxToken` へ分解する場合も span を失わない。

例:

- `Compose` の span `a..b` を、前半 `Gt` / 後半 `Gt` の 2 span に分割する
- `Colon` + `Colon` を `PathSep` 1 span に結合する

これにより、`chumsky` 側で得た span をそのまま AST span や `ParseError` へ戻せる。

---

## 6. 文法 parser 設計

### 6.1 surface 切り替え

構文上の許可範囲が異なるため、内部では surface kind を明示する。

```rust
enum SyntaxSurface {
    Program,
    ModuleBody,
    TraitBody,
    ImplBody,
    ExprBlock,
}
```

用途:

- `Program` は top-level statement を受理する
- `ModuleBody` は module member だけを受理する
- `TraitBody` は trait method signature だけを受理する
- `ImplBody` は `def` / `defp` method body を受理する
- `ExprBlock` は expression statement のみ受理し、decl は構文段階で拒否する

これにより、現行 `DeclLevel::Expr` の「宣言は式位置で不許可」という制約を grammar 側で表現できる。

### 6.2 statement と separator

現行の `ensure_stmt_boundary()` は grammar に吸収する。

設計:

- `stmt(surface)` が 1 statement を返す
- `stmt_list(surface, terminator)` が `separator+` を伴う statement 列を返す
- separator は `Newline` または `Semicolon`
- `Ast::Semi` は現行互換のため維持する

具体方針:

- `expr ;` は `Ast::Semi` で包む
- newline 区切りは AST に現れない
- block / program / module body すべて同じ separator parser を使う

### 6.3 式 parser

式 parser は `expr.rs` に分離し、優先順位を declarative に表す。

基本構成:

- `atom`
- `postfix`
- `infix func-literal`
- `comparison / equality / concat / arithmetic`
- `flow / compose`

実装方針:

- 二項演算は `chumsky` の precedence / pratt 相当で管理する
- postfix は field access, call, constructor call, trailing block を fold する
- `match` / `cond` / closure / capture は atom レベルの特殊 form として扱う

維持すべき現行仕様:

- unary minus は numeric literal のみ許可する
- `FuncLiteral` は infix 専用
- `value._0` は許可、`.0` は不許可
- `parse() >=> validate()` や `parse() >* render()` など compose chain は引き続き左結合
- trailing block sugar は named arg と共存させない

### 6.4 pattern parser

pattern parser は `pattern.rs` に集約する。

対象:

- bind pattern
- match pattern
- list pattern
- tuple pattern
- constructor / call / as-pattern / annotated pattern

方針:

- bind pattern と match pattern の共通部分を最大化する
- `SafeBind.LHS` と `match` arm 左辺は同じ core parser を共有する
- pattern 内 type annotation は `ty.rs` の parser を再利用する

### 6.5 type parser

type parser は `ty.rs` に切り出し、`Self` / `self` / `TypeRef<$T>` などの制約を局所化する。

対象:

- named type
- generic type
- tuple type
- function type
- `impl Trait`

重要方針:

- `Compose` 分解済み `SyntaxToken::Gt` を使うことで、generic close と compose の衝突を parser から取り除く
- impl 文脈での `Self` 解決制約は type parser に閉じ込める
- `where` clause は staged のまま parse error にする

### 6.6 宣言 parser

宣言 parser は `decl.rs` に集約する。

対象:

- `import`
- `defmod`
- `deftrait`
- `impl`
- `defstruct`
- `defrecord`
- `deferror`
- `defenum`
- `def`
- `defextractor`
- `@@builtin def`
- `@@builtin defextractor`
- `@@builtin type`

方針:

- annotator はまず `DeclAttrs` へ正規化してから対象 decl parser へ流す
- `@@doc` の doc string 要求は annotator parser 内で扱う
- builtin decl 群は通常 decl と共有できる署名 parser を共有する
- `Result` constructor builtin だけの特殊 lower は小さな専用 helper に隔離する

---

## 7. 特殊ケース設計

### 7.1 string interpolation

double-quoted string の `#{expr}` は `interpolate.rs` に隔離する。

設計:

- lexer は従来どおり string 全体を 1 token として返す
- parser は string token を見たとき、raw text を interpolation scanner に渡す
- scanner は text fragment と embedded expr fragment に分解する
- embedded expr fragment は `parse_embedded_expr()` で再入し、得られた AST の span を元 string 内 offset で補正する

理由:

- string 内の mini parser まで grammar 本体へ混ぜると責務が悪化する
- 現行実装の span 補正ロジックを隔離しやすい

受け入れ条件:

- `"hi #{name}"`, escaped `\#{name}`, nested braces を現行互換で扱える
- `@@doc` triple quote は interpolation しない

### 7.2 trailing block sugar

`attach_trailing_block_arg()` 相当の仕様は expr parser の postfix fold に残す。

方針:

- call parser が通常引数を読んだ後、条件付きで trailing block を 1 positional arg として追加する
- `test` / `describe` / `it` の closure sugar 判定は専用 helper として残す
- `match expr { ... }` の scrutinee 読み取りでは trailing block を無効化する

`allow_trailing_call_block` は mutable state で持たず、parser 引数で明示する。

```rust
enum TrailingBlockMode {
    Enabled,
    Disabled,
}
```

### 7.3 文脈依存制約

以下は grammar と validator のどちらで扱うかを明示する。

grammar で扱うもの:

- expr block 内で宣言を禁止する
- trait body 内で method signature 以外を禁止する
- impl body 内で method 定義以外を禁止する
- empty `match {}` / `cond {}` を禁止する

validator で扱うもの:

- source kind ごとの top-level decl 許可有無
- top-level expr 許可有無
- std module 限定 builtin decl
- `set_exit_code` の source rule 依存制約

---

## 8. error 設計

### 8.1 `chumsky` error から `ParseError` への写像

外部契約維持のため、`chumsky` の rich error は最終的に `ParseError` へ畳み込む。

写像規則:

- EOF で閉じ括弧や必要 token が不足した場合は `ParseError::Incomplete`
- それ以外は `ParseError::SyntaxError`
- message は `expected ...`, `unexpected ...`, custom message を Surtr 向け文言へ正規化する

現段階では multi-error 収集を public API に出さない。最初の有意味 error を返す。

### 8.2 custom error message

現行で仕様的意味を持つ文言は `labelled()` や `validate()` で custom message を付ける。

優先して custom 化する対象:

- `Unary minus is only supported on numeric literals...`
- `FuncLiteral must appear in infix position`
- `Declarations are only allowed at the top level`
- `Expected field name after '.'`
- `Trailing block sugar cannot follow named arguments`
- `where clause is staged`

---

## 9. public API と互換境界

### 9.1 維持するもの

- `Ast` enum とその variant 構造
- `ParseError` variant
- `ParserContext` / `ParseRules` の public constructor
- `parse` / `parse_with_context` / `parse_with_context_diagnostic`

### 9.2 実装都合で変えてよいもの

- `parser/mod.rs` の内部関数構成
- token cursor ベースの helper 群
- `expect_type_gt()` のような stream 書き換え実装
- `allow_trailing_call_block` の mutable state

### 9.3 今回は変えないもの

- public `lexer::tokenize`
- public `token::Token`
- `rune` / `xldr` の annotator 検出ロジック

補足:

将来的に source classifier が `spire` 外へ出たら、public lexer/token の縮退は別設計として扱う。今回の移行に含めない。

---

## 10. 実装ステップ

### Step 0. 前提適用

- `doc/依存整理詳細設計.md` の `Spire` 前提を適用する
- `Span` など共有葉型の置き場を安定させる
- `parse` API の caller を固定する

### Step 1. parser 境界の前処理

- `ParserContext` / `ParseRules` 群を `parser/context.rs` へ分離する
- `validate_stmt_by_context` 相当を `validate.rs` へ抽出する
- 既存 parser を動かしたまま責務だけ切り分ける

### Step 2. syntax token adapter 導入

- public `Token` から internal `SyntaxToken` への adapter を追加する
- `::` 結合, `>>` 分解, span 補正をここへ集約する
- 既存 parser はまだ使い続けてよい

### Step 3. `chumsky` parser の骨格導入

- `stmt_list`, `expr atom`, `type atom` だけ先に `chumsky` で組む
- `parse_with_context` の裏で legacy/new を切り替えられる状態にする
- 切り替えは private feature flag か test-only path とし、public API は増やさない

### Step 4. 文法単位で置き換え

順序:

1. type
2. pattern
3. simple expr
4. decl
5. match / cond / closure / capture / interpolation
6. module/trait/impl body

理由:

- type / pattern は比較的閉じており、`chumsky` 化の効果確認がしやすい
- expr はもっとも広く、後ろへ回したほうが安全

### Step 5. parity 固定

- 既存 parser unit test をすべて `chumsky` path で通す
- spec / compile_errors / integration を通す
- legacy parser と AST parity 比較を追加する

### Step 6. legacy 削除

- `parser_legacy.rs` を削除する
- 不要 helper と token cursor 系関数を削除する

---

## 11. テスト戦略

### 11.1 parser 単体テスト

既存 parser test 群は [crates/spire/src/parser/tests.rs](/Users/haruca/work/rust/surtr/crates/spire/src/parser/tests.rs) を起点に分割維持する。

配置方針:

- `expr.rs` 付近の test は `parser/expr.rs` へ移す
- type / pattern / decl も同様に局所化する
- interpolation, trailing block, builtin decl は専用 test module を持つ

### 11.2 AST parity test

移行期間のみ、同一 source を legacy parser と chumsky parser の両方へ通し、AST と error 種別を比較する。

比較対象:

- AST 完全一致
- `ParseError` variant 一致
- span の start/end 一致

message 文字列の完全一致は必須にしない。

### 11.3 E2E

最低限、次を通す。

- `cargo nextest run --workspace`
- `cargo nextest run -p rune --test run_srt`
- `tests/spec/**`
- `tests/compile_errors/**`

追加したい回帰群:

- generic nest `Result<List<Int>>`
- compose と generic close の衝突
- string interpolation の span
- module / std-module / repl ごとの source rule
- builtin decl 許可有無

---

## 12. リスクと対策

### リスク 1. `chumsky` 化で error 文面が大きく変わる

対策:

- `ParseError` への正規化層を必須にする
- custom message が必要な箇所は `validate()` で明示する

### リスク 2. token public API を巻き込む

対策:

- public `Token` は維持し、internal `SyntaxToken` を導入する
- parser 都合の token 正規化は adapter に閉じ込める

### リスク 3. context-sensitive rule が grammar に漏れ続ける

対策:

- `SyntaxSurface` と `ParseRules validator` を分ける
- compile-unit policy は validator 側に限定する

### リスク 4. 置き換え中に parser が二重化して保守負債になる

対策:

- legacy parser は parity test 専用の暫定物と位置付ける
- Step 5 完了後に即削除する

---

## 13. 完了条件

移行完了の定義は次とする。

- `parse` / `parse_with_context` が `chumsky` 実装だけを使う
- legacy parser が削除されている
- `parser/mod.rs` 1 枚物ではなく、責務別 module 構成へ分割されている
- `expect_type_gt()` のような token stream 破壊的補正が消えている
- `ParseRules` 検証が `validate.rs` に分離されている
- parser unit test, spec test, compile error test, integration test が通る
- public AST / `ParseError` / `ParserContext` 契約が維持される

---

## 14. 今回の設計で意図的に見送るもの

- 言語 surface の追加
- `where` clause 実装
- public lexer/token API の廃止
- string interpolation の lexer 段統合
- parser error の multi-diagnostic 化

これらは `chumsky` 移行と同時にやると切り分けが悪くなるため、別タスクとして扱う。
