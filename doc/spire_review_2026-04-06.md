# Spire レビュー — 2026-04-06

## 対象範囲

`crates/spire/src/` 以下の全ファイル

| ファイル | 役割 |
|---------|------|
| `token.rs` | トークン定義 |
| `error.rs` | `ParseError` 型 |
| `ast.rs` | AST 定義 |
| `lexer.rs` | トークナイザ |
| `parser.rs` | パーサ本体 |

---

## 1. テスト網羅性

### 1-1. Lexer — 未カバー項目

| 項目 | 説明 |
|------|------|
| `DotDot (..)` トークン | `test_two_char_ops` のテスト文字列から `..` が抜けている |
| コメント (`#...`) | スキップされることを確認するテストがない |
| `i64` オーバーフロー | `9999999999999999999` のような超大整数がエラーになることのテストがない |
| 空入力 | `tokenize("")` が `[Eof]` を返すことのテストがない |

```rust
// 追加すべきテスト例
#[test]
fn test_dotdot_token() {
    let tokens = tokenize("..").unwrap();
    assert!(matches!(tokens[0].token, Token::DotDot));
}

#[test]
fn test_comment_is_skipped() {
    let tokens = tokenize("# this is a comment\nx = 1").unwrap();
    assert!(matches!(tokens[0].token, Token::Ident(_)));
}

#[test]
fn test_integer_overflow_is_error() {
    tokenize("99999999999999999999").expect_err("should fail on i64 overflow");
}
```

### 1-2. Parser — 未カバーの AST ノード

以下の AST バリアントを直接検証するユニットテストが存在しない。

| AST ノード | 構文例 |
|-----------|--------|
| `StructDef` | `defstruct User { name: String }` |
| `StructLit` | `User { name: "alice" }` |
| `RecordDef` | `defrecord Point(x: Float, y: Float)` |
| `ConstructorCall` | `Point(1.0, 2.0)` / `Point(x: 1.0, y: 2.0)` |
| `DeferrorDef` | `deferror NotFound(id: Int) { "not found" }` |
| `FieldAccess` 連鎖 | `user.address.city` |
| `Path` 付き関数呼び出し | `Kernel::add(1, 2)` |

### 1-3. Parser — 未カバーのシナリオ

| シナリオ | 期待結果 |
|---------|---------|
| `import Mod::{}` 空インポートリスト | エラー（コード自体はチェックあり、テストなし） |
| `import Foo::Bar::baz` 深いパス | `Single("baz")` で module_segments が `["Foo","Bar"]` |
| `Result<Int, ParseError>` 2引数型 | `ResultOf(_, Int, Some(ParseError))` |
| `match x {}` 空アーム | `Ast::Match(_, _, [])` を生成 |
| `def f() {}` 空ブロック | `Ast::Block(_, [])` を生成 |
| `#{}` 空補間式 | エラー |
| コメントのみのソース | 空 AST |
| `Ok()` / `Ok(a, b)` 不正コンストラクタ | エラー（コード自体はチェックあり、テストなし） |

### 1-4. Parser — 比較演算子のテスト不足

`test_binop` / `test_precedence` は `+`, `*` のみ。`==`, `!=`, `<`, `<=`, `>`, `>=` の結合性テストが存在しない。

```rust
#[test]
fn test_comparison_ops() {
    let ast = parse("a == b").unwrap();
    assert!(matches!(&ast[0], Ast::BinOp(_, BinOp::Eq, _, _)));
}
```

---

## 2. エッジケース

### 2-1. 負の `i64::MIN` — オーバーフロー (現状バグ)

`parse_bind_pattern_atom` と `parse_match_pattern` の両方で、パターン内の負整数に `-n` を使用している。
`n = i64::MIN` のとき `checked_neg()` が `None` になり、`debug` ビルドでは panic する。

```rust
// parser.rs:1411-1428, 2060-2078 (現状)
Token::Int(n) => {
    self.advance();
    Ok(AstPattern::IntLit(sp, -n))  // i64::MIN で overflow
}
```

**対処**: `-n` → `n.checked_neg().ok_or_else(|| ParseError::syntax(...))`

> **Note**: `i64` を `BigInt` に切り替えることでこのバグは自動的に解消される。

### 2-2. `Token::Unit` がマッチパターンとして使用不可

```surtr
match x {
  () => "unit",   # parse_match_pattern で Token::Unit が未処理 → エラー
  _ => "other",
}
```

`parser.rs:2106-2110` の `match` に `Token::Unit => Ok(AstMatchPattern::...)` のアームがない。
Unit 型を返す関数の結果を match する際に必要になる。

### 2-3. 補間式内での改行

```surtr
msg = "#{x
y}"
```

`parse_interpolated_parts` が改行込みの `expr_src` を内部で `parse()` に渡すと、`x` と `y` が別々の文として解析されて `parsed.len() != 1` エラーになる。
エラーメッセージ「must contain exactly one expression」だけでは原因が分かりにくい。

### 2-4. クロージャパラメータへの型アノテーション

```surtr
{|x: Int| x}  # 意図: 型アノテーション付きクロージャ
```

`parse_closure_literal` は `expect_ident()` のみでパラメータを読む。
`:` を見た時点で「Expected `|`, got `:`」という分かりにくいエラーになる（`parser.rs:1604-1617`）。
型アノテーションが書けないことをエラーメッセージで明示すべき。

### 2-5. シングル/ダブルクォート文字列のエスケープ非対称

| エスケープ | `"..."` | `'...'` |
|-----------|:-------:|:-------:|
| `\n` | ✓ | ✗ (そのまま `\n` 2文字) |
| `\t` | ✓ | ✗ |
| `\\` | ✓ | ✓ |
| `\"` / `\'` | ✓ | ✓ |

`lexer.rs:82-90` と `lexer.rs:116-121` の実装差分。仕様ならドキュメントに明記が必要。

### 2-6. `\#` による補間エスケープが `\` を残す

`"\\#{name}"` という入力に対して：

1. Lexer が `\\` → `\` に変換し、raw 文字列に `\#{name}` が入る
2. `parse_interpolated_parts` で `chars[i-1] == '\\'` が機能し補間をスキップ ✓
3. しかし出力文字列に `\` が残る（ユーザーは `#{name}` を文字通り出力したいはず）

補間をエスケープした場合、`\` を除去する処理が必要。

---

## 3. 構文則考慮漏れ

### 3-1. ジェネリック型パラメータのサイレント消失 — **設計課題**

**現状** (`parser.rs:1584-1592`):

```rust
if name == "Result" {
    return Ok(AstTy::ResultOf(span, Box::new(first), second));
}
if name == "List" && second.is_none() {
    return Ok(AstTy::ListOf(span, Box::new(first)));
}
// ↓ List<X,Y> や Foo<T> はここに落ち、型引数が無言で捨てられる
return Ok(AstTy::Named(span, name));
```

- `List<Int, String>` → `AstTy::Named("List")` （型引数消失）
- `Option<Int>` → `AstTy::Named("Option")` （型引数消失）

**方針決定**: 型引数の個数・型名の妥当性検証は型チェッカーの責務とし、パーサーは構文を一様に受理する。

**`ast.rs` への変更**:

```rust
pub enum AstTy {
    Named(Span, Symbol),
    /// `List<T>`, `Result<T, E>`, `Option<T>`, ユーザー定義ジェネリック型など
    /// 型引数の個数・型名の妥当性は型チェッカー (Scar) が検証する
    Generic(Span, Symbol, Vec<AstTy>),
    Func(Span, Vec<AstTy>, Box<AstTy>),
}
```

`ListOf` / `ResultOf` を廃止し `Generic` に統一。

**`parser.rs:parse_type` への変更**:

```rust
if matches!(self.peek(), Token::Lt) {
    self.advance();
    self.skip_newlines();
    let mut args = vec![self.parse_type()?];
    self.skip_newlines();
    while matches!(self.peek(), Token::Comma) {
        self.advance();
        self.skip_newlines();
        args.push(self.parse_type()?);
        self.skip_newlines();
    }
    let end = self.expect(&Token::Gt)?;
    return Ok(AstTy::Generic(
        Span { start: sp.start, end: end.end },
        name,
        args,
    ));
}
```

**影響範囲**: `sigil`, `scar`, `forge` で `AstTy::ListOf` / `AstTy::ResultOf` をパターンマッチしている箇所を `AstTy::Generic` に対応させる。

### 3-2. マッチパターンにおける識別子の非対称性

| 文脈 | 小文字識別子の扱い |
|------|----------------|
| `parse_match_list_item_pattern` | `AstMatchPattern::Binding` として受理 |
| `parse_match_pattern` (アームトップ) | Phase 1 エラー |

```surtr
match xs {
  [head, ..tail] => ...   # OK — head, tail は Binding として機能
  head => ...             # ERROR — 同じ小文字なのに拒否される
}
```

Phase 1 の意図的制限だが、エラーメッセージに開発者向けの実装コメントが露出している：

```
"Phase 1 match patterns only support ...; remove this test when CamelCase patterns are implemented"
```

ユーザー向けエラーと開発者向けコメントを分離すること。

### 3-3. 空マッチアームがエラーにならない

```surtr
match x {}   # Ast::Match(_, _, []) を生成 — パーサーはエラーにしない
```

意味論的に無効なため、パーサーレベルでエラーにするか、後続フェーズでの検証を明示的に保証すること。

### 3-4. `def` 空ブロックとクロージャ空ブロックの非対称

クロージャ: `parser.rs:1620-1622` に `body_stmts.is_empty()` チェックあり → `ParseError::incomplete`
`def` 本体: 対応するチェックなし → `Ast::Block(_, [])` を生成

```surtr
def f() {}  # Ast::Block(_, []) — エラーにならない
{|| }       # ParseError::incomplete — エラーになる
```

`def` でも同様のチェックを行うか、どちらかに統一すること。

---

## 4. アーキテクチャ決定事項

### 4-1. Int 型を `BigInt` に切り替える

**理由**: `i64` では整数オーバーフローを隠蔽するためにすべての算術演算が `Result` を返す必要があり、二項演算子が使いにくくなる。`BigInt` を採用することでオーバーフローを言語レベルで排除する。

**影響クレート**:

| クレート | 変更箇所 |
|---------|---------|
| `spire` | `Token::Int(i64)`, `Lit::Int(i64)`, `AstPattern::IntLit`, `AstMatchPattern::IntLit`, lexer の `parse::<i64>()` |
| `sigil` | `resolved.rs` の `Int(i64)` |
| `scar` | `typed.rs`, `checker.rs` |
| `sindr` | `Constant::Int(i64)`, `Value::Int(i64)` |
| `forge` | `codegen.rs` |
| `eldr` | `vm.rs` (算術 opcode, `pop_int`, `int_binop`), `builtin.rs` |

**注意点 — VM のタグ使用**:

`eldr/vm.rs:907, 937` では `Value::Int` が enum discriminant (タグ値) としても使われている：

```rust
Value::Int(tag) => u32::try_from(tag)...       // タグ読み取り
self.stack.push(Value::Int(tag as i64));        // タグ書き込み
```

`Value::Int` を `BigInt` に変更するタイミングで、数値としての `Int` とタグとしての識別子を分離することを推奨：

```rust
pub enum Value {
    Int(BigInt),          // ユーザー向け整数値
    Tag(u32),             // 内部 enum discriminant (変更後)
    // ...
}
```

**推奨移行手順**:

1. 共有クレート（`sindr` または新設 `primitives`）に型エイリアスを定義する
   ```rust
   // 段階1: まずエイリアス化
   pub type SurtrInt = i64;
   ```
2. 全クレートで `i64` を `SurtrInt` に置換する
3. VM の `Value::Int` と `Value::Tag` を分離する
4. `num-bigint` を追加し `SurtrInt = BigInt` に切り替える
   ```toml
   [workspace.dependencies]
   num-bigint = "0.4"
   ```
   `BigInt` は `FromStr`, `Neg`, 四則演算トレイトを実装済みのため、lexer の `text.parse()` や `-n` はほぼそのまま動作する。

**副次効果**: 前述の「2-1. 負の `i64::MIN` オーバーフロー」バグが自動的に解消される。

---

## 優先度まとめ

| 優先度 | 分類 | 内容 |
|--------|------|------|
| **高** | 設計 | `AstTy::Generic` 導入（型引数のサイレント消失を解消） |
| **高** | 設計 | `Int` → `BigInt` 切り替え（段階的移行） |
| **高** | バグ | `Token::Unit` のマッチパターン未対応 |
| **高** | バグ | 負 `i64::MIN` パターンのオーバーフロー（BigInt 移行で解消） |
| **中** | テスト | `defstruct` / `defrecord` / `deferror` / `ConstructorCall` / `StructLit` の直接テスト追加 |
| **中** | テスト | `DotDot`, コメント, オーバーフロー, 空入力 の Lexer テスト追加 |
| **中** | UX | マッチパターンエラーメッセージから開発者向け文言を分離 |
| **中** | 仕様 | シングルクォート文字列のエスケープ仕様をドキュメント化 |
| **中** | 仕様 | `\#{...}` 補間エスケープの `\` 除去 |
| **中** | 一貫性 | `def` 空ブロックの扱いをクロージャと統一 |
| **低** | テスト | 比較演算子の結合性テスト追加 |
| **低** | テスト | `import` エッジケース（空リスト, 深いパス）のテスト追加 |
| **低** | 仕様 | 空マッチアームのエラー化を検討 |
