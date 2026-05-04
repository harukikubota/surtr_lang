# SYP v0 初期フェーズ仕様書

> 対象: Surtr の初期 parser generator 実装  
> ゴール: `syp` 1 本、tokenizer 内蔵、header は `.srt` ファイル指定、電卓 parser が動くこと  
> 非ゴール: 完全な yacc 互換、trait/impl 自動生成、error recovery、Ariadne 診断の Surtr 側カスタム

---

## 1. 目的

SYP v0 は、Surtr 用の小さな parser generator である。

初期フェーズでは、次を実現する。

```text
calc.syp
  ↓ syp generator
calc_parser.generated.srt
  ↓ Surtr compile
CalcParser::parse("1 + 2 * 3", CalcActions()) -> Result<Expr>
```

主目的は以下である。

- `.syp` に grammar と tokenizer を書く
- AST や補助型は header `.srt` に書く
- action trait と trait 実装は手書きする
- generator は parser driver、tokenizer、PDA table、内部型を生成する
- 電卓 parser を最初の動作確認対象にする

---

## 2. 初期フェーズの範囲

### 2.1 対応するもの

```text
- syp ファイル 1 本
- parser 1 つ
- root nonterminal 1 つ
- tokenizer 内蔵
- literal terminal
- regex terminal
- skip rule
- fixed RHS grammar rule
- action name 指定: `=> action_name`
- `%left`
- `%right`
- `%nonassoc`
- `%precedence`
- rule-level `%prec`
- shift/reduce conflict の precedence 解決
- reduce/reduce conflict の検出
- generated parser source 出力
```

### 2.2 対応しないもの

```text
- 複数 parser in 1 syp
- 複数 header
- action block
- `$1`, `$2` 形式
- mid-rule action
- 空生成
- error pseudo token
- error recovery
- custom Ariadne diagnostic
- trait / trait impl 自動生成
- 既存 .srt 解析による patch
- tagged stream 汎用化
- transducer mode
- GLR
- dynamic precedence
- lexer state
```

---

## 3. 名前解決と名前衝突方針

Surtr v0 では、型は global path から解決される。

`TypeName::TypeName` のような構文は型名前空間ではなく、`Enum::Variant(...)` の構築パス専用である。

そのため、SYP 生成物は名前空間を使わず、`%parser` 名 prefix で衝突を避ける。

### 3.1 parser 名 prefix

```syp
%parser CalcParser
```

の場合、生成器は以下のような名前を使う。

```text
CalcParserToken
CalcParserTokenKind
CalcParserTokenNode
CalcParserNonTerm
CalcParserRuleId
CalcParserParseState
CalcParserParseAction
CalcParserSemValue
CalcParserFrame
CalcParserUnexpectedTokenContext
CalcParserLexError
CalcParserUnexpectedTokenError
CalcParserInternalError
CalcParser
```

### 3.2 header から読み込む型

header `.srt` から SYP generator が型として読み取る対象は以下に限定する。

```text
Struct
Record
Enum
```

以下は header 型一覧には含めない。

```text
Trait
Error
Mod
Impl
Function
Const
```

ただし、通常の Surtr compile では `deferror`, `deftrait`, `impl` なども参照できる。

### 3.3 衝突エラー

以下の場合、SYP generator は生成前にエラーにする。

```text
- SYP symbol が builtin 型名と一致する
- SYP symbol が header で読み取った Struct / Record / Enum 名と一致する
- SYP symbol が生成予約名と一致する
- header 型名が生成予定名と一致する
- 同じ `%parser` 名の生成物が同一 compile unit に複数存在する
```

例:

```syp
terminal Int = re"\d+"
```

`Int` は builtin type 名と衝突するためエラーにする。

推奨:

```syp
terminal IntLit(Int) = re"\d+" => Int::parse
```

### 3.4 SYP symbol と Surtr 型の分離

```syp
%nonterm expr : Expr
%nonterm bin_op : BinOp
```

- `expr`, `bin_op` は SYP 内 grammar symbol
- `Expr`, `BinOp` は header から読み取った Surtr 型

大文字小文字が異なるため衝突しない。

---

## 4. `.syp` 最小構文

電卓 parser の `.syp` 例:

```syp
%header "calc_ast.srt"
%parser CalcParser
%actions CalcParserActions
%root expr : Expr

%lexer

skip Whitespace = re"[ \t\r\n]+"

terminal IntLit(Int) = re"\d+" => Int::parse
terminal Plus = "+"
terminal Minus = "-"
terminal Star = "*"
terminal Slash = "/"
terminal LParen = "("
terminal RParen = ")"

%grammar

%left Plus Minus
%left Star Slash

%nonterm expr : Expr
%nonterm bin_op : BinOp

%%

expr
  : expr bin_op expr      => expr_bin
  | LParen expr RParen    => expr_group
  | IntLit                => expr_int
  ;

bin_op
  : Plus                  => bin_op_add
  | Minus                 => bin_op_sub
  | Star                  => bin_op_mul
  | Slash                 => bin_op_div
  ;
```

---

## 5. Tokenizer 仕様

### 5.1 terminal

payload なし terminal:

```syp
terminal Plus = "+"
```

payload あり terminal:

```syp
terminal IntLit(Int) = re"\d+" => Int::parse
```

### 5.2 skip

```syp
skip Whitespace = re"[ \t\r\n]+"
```

`skip` は token を生成しない。

### 5.3 正規表現 matching

正規表現は現在位置からの prefix match として扱う。

```text
input: "123 + 45"
position: 0
re"\d+" -> "123"
```

ユーザは `^` を書かなくてよい。

### 5.4 競合解決

tokenizer の競合規則は次で固定する。

```text
1. 現在位置から prefix match
2. 最長一致
3. 同じ長さなら宣言順
4. skip は token を生成しない
5. どの terminal / skip にも一致しなければ LexError
```

### 5.5 converter

converter の型は次に固定する。

```surtr
String -> Result<T>
```

例:

```syp
terminal IntLit(Int) = re"\d+" => Int::parse
```

lowering:

```surtr
raw = matched_text
value =? Int::parse(raw)
Token::IntLit(value)
```

### 5.6 EOF

`EOF` は generator が自動追加する。

ユーザは `.syp` に `EOF` を書かない。

```text
lexer output:
  [IntLit(1), Plus, IntLit(2)]

parser input:
  [IntLit(1), Plus, IntLit(2), EOF]
```

---

## 6. Token 型

SYP v0 では、生成 parser ごとに token 型を生成する。

```surtr
defenum CalcParserToken {
  IntLit(Int),
  Plus,
  Minus,
  Star,
  Slash,
  LParen,
  RParen,
  EOF,
}
```

parser table 用に payload なし kind も生成する。

```surtr
defenum CalcParserTokenKind {
  IntLit,
  Plus,
  Minus,
  Star,
  Slash,
  LParen,
  RParen,
  EOF,
}
```

terminal action へ渡す単位は `TokenNode` とする。

```surtr
defrecord CalcParserTokenNode(
  token: CalcParserToken,
  kind: CalcParserTokenKind,
  span: SourceSpan,
  raw: String,
)
```

- PDA table は `CalcParserTokenKind` を見る
- action は `CalcParserTokenNode` を受け取る
- Rust 側診断は `SourceSpan` を使う

---

## 7. Span 方針

SYP v0 では、span は Ariadne 連携を前提に `SourceSpan` として扱う。

ただし、Surtr コード上で Ariadne diagnostic を直接構築するわけではない。

```surtr
defrecord SourceId(
  name: String,
)

defrecord SourceSpan(
  source_id: SourceId,
  char_start: Int,
  char_end: Int,
)
```

内部表現は character offset を正本にする。

line / column / byte range は Rust 側診断レンダラで必要に応じて計算する。

---

## 8. Action 規約

### 8.1 基本形

action は RHS の意味値を受け取り、LHS nonterminal の型を返す。

```surtr
(RHS values...) -> Result<LhsType>
```

例:

```syp
expr : expr bin_op expr => expr_bin
```

```surtr
def expr_bin(
  left: Expr,
  op: BinOp,
  right: Expr,
) -> Result<Expr>
```

### 8.2 NonTerminal 引数

RHS に nonterminal が現れた場合、action 引数は `%nonterm` で宣言した型になる。

```syp
%nonterm expr : Expr
%nonterm bin_op : BinOp
```

```syp
expr : expr bin_op expr => expr_bin
```

```surtr
def expr_bin(left: Expr, op: BinOp, right: Expr) -> Result<Expr>
```

### 8.3 Terminal 引数

RHS に terminal が現れた場合、action 引数は `CalcParserTokenNode` になる。

```syp
bin_op : Plus => bin_op_add
```

```surtr
def bin_op_add(token: CalcParserTokenNode) -> Result<BinOp>
```

### 8.4 ParserContext は action に渡さない

SYP v0 では action に `ParserContext` を渡さない。

理由:

```text
- action を AST 構築に集中させる
- parser stack / 外側 nonterminal / 制御文脈を隠す
- lens による深い外部更新を避ける
- action の破壊的変更を抑える
```

### 8.5 action error の変換

action が `Err(error)` を返した場合、parser driver は以下を知っている。

```text
- reduce 中の RuleId
- LHS NonTerm
- RHS 全体 span
```

そのため、将来的には parser driver 側で NonTerminal 単位のエラー変換ができる。

ただし、SYP v0 では Surtr 側 custom diagnostic は対象外にする。

---

## 9. Span と action 戻り値

ZeroDivision など action 内で source 位置付きエラーを検出したい場合、action が扱う値から span を辿れる必要がある。

ただし、SYP v0 ではすべての戻り値を強制的に `Spanned<T>` にしない。

方針:

```text
- parser 内部 stack は span を常に保持する
- action 引数の型は `%nonterm name : Type` をそのまま使う
- terminal 引数は TokenNode なので span を持つ
- NonTerminal の型が診断に必要なら、その型が span を持つ
- 即時評価 parser なら `%nonterm expr : SpannedInt` のように明示する
```

### 9.1 AST が span を持つ例

```surtr
defrecord SpannedInt(
  value: Int,
  span: SourceSpan,
)

defenum BinOp {
  Add(SourceSpan),
  Sub(SourceSpan),
  Mul(SourceSpan),
  Div(SourceSpan),
}

defenum Expr {
  Int(SpannedInt),
  Bin(Expr, BinOp, Expr, SourceSpan),
}
```

この場合、action は次の形を保てる。

```surtr
def expr_bin(left: Expr, op: BinOp, right: Expr) -> Result<Expr>
```

### 9.2 即時評価 parser の場合

AST を作らず値だけを返す場合は、NonTerminal 型を明示的に span 付きにする。

```syp
%nonterm expr : SpannedInt
%nonterm bin_op : BinOp
```

---

## 10. precedence / associativity

### 10.1 対応宣言

```syp
%left Plus Minus
%left Star Slash
%right Pow
%nonassoc EqEq NotEq
%precedence UMinus
```

### 10.2 宣言順

後に書いた precedence group ほど高優先度にする。

```syp
%left Plus Minus   # low
%left Star Slash   # high
```

### 10.3 rule precedence

rule の precedence は RHS 右端の terminal から取る。

```syp
expr : expr Plus expr
```

この rule の precedence は `Plus`。

### 10.4 `%prec`

単項 minus のように rule precedence を明示したい場合に使う。

```syp
%precedence UMinus

expr : Minus expr => expr_neg %prec UMinus
```

### 10.5 conflict 解決

shift/reduce conflict のみ precedence で解決する。

```text
- token precedence > rule precedence: shift
- rule precedence > token precedence: reduce
- same precedence + left: reduce
- same precedence + right: shift
- same precedence + nonassoc: parse error action
- precedence 不明: generator error
```

reduce/reduce conflict は常に generator error。

---

## 11. Error 方針

### 11.1 `deferror` の制約

`deferror` の本文は message `String` を返す。

そのため、`deferror` 単体では Ariadne diagnostic を構築できない。

```surtr
deferror CalcParserUnexpectedTokenError(message: String) {
  message
}
```

`deferror` は以下に限定する。

```text
deferror = 失敗値 + message String
```

### 11.2 SourceSpan を持つだけでは Ariadne 診断にはならない

以下のように payload に span を入れることはできる。

```surtr
deferror SomeError(span: SourceSpan, message: String) {
  message
}
```

しかし、Surtr コード上では Ariadne の以下を構築できない。

```text
- primary label
- secondary label
- expected list
- source slice
- note
- help
- color / style
```

### 11.3 SYP v0 の診断担当

SYP v0 では、ソースコード付き診断は Rust 側の固定 renderer が担当する。

```text
LexError
  -> Rust 側 lexer diagnostic renderer

UnexpectedTokenError
  -> Rust 側 parser diagnostic renderer

ParserInternalError
  -> Rust 側 internal diagnostic renderer
```

Surtr 側は `Result<T>` / `Err(Error)` を返すだけである。

### 11.4 UnexpectedTokenContext

`UnexpectedTokenContext` は Ariadne 診断を Surtr 側で作るための型ではない。

これは Rust 側 renderer が診断を組み立てるための材料、または将来の custom error message 用の材料である。

```surtr
defrecord CalcParserUnexpectedTokenContext(
  state: CalcParserParseState,
  found: CalcParserTokenNode,
  expected: List<CalcParserTokenKind>,
)
```

### 11.5 custom error の段階

SYP v0:

```text
- Rust 側固定 diagnostic
- Surtr 側 custom diagnostic なし
```

将来:

```text
Phase 1:
  UnexpectedTokenContext -> String
  message のみ custom

Phase 2:
  Ariadne wrapper / Diagnostic builder を Surtr 標準ライブラリに公開
  Surtr 側 custom diagnostic を可能にする
```

---

## 12. 電卓 parser ファイル構成

```text
examples/calc/
  calc_ast.srt
  calc_actions.srt
  calc.syp
  calc_parser.generated.srt
  calc_main.srt        # 任意
```

---

## 13. `calc_ast.srt`

役割:

```text
SYP header として指定するファイル。
SYP generator が読み取る対象は Struct / Record / Enum のみ。
AST、span 付き値、演算子、domain error を置く。
```

### 13.1 header 取り込み対象

```text
Record:
  SourceId
  SourceSpan
  SpannedInt

Enum:
  BinOp
  Expr
```

### 13.2 通常 compile では使うが header 型一覧には含めないもの

```text
Error:
  CalcZeroDivisionError
  CalcInvalidIntegerError
```

### 13.3 定義例

```surtr
defrecord SourceId(
  name: String,
)

defrecord SourceSpan(
  source_id: SourceId,
  char_start: Int,
  char_end: Int,
)

defrecord SpannedInt(
  value: Int,
  span: SourceSpan,
)

defenum BinOp {
  Add(SourceSpan),
  Sub(SourceSpan),
  Mul(SourceSpan),
  Div(SourceSpan),
}

defenum Expr {
  Int(SpannedInt),
  Bin(Expr, BinOp, Expr, SourceSpan),
}

deferror CalcZeroDivisionError(message: String) {
  message
}

deferror CalcInvalidIntegerError(message: String) {
  message
}
```

---

## 14. `calc_actions.srt`

役割:

```text
action trait と手書き実装を置く。
初期フェーズでは generator はこのファイルを生成しない。
```

### 14.1 定義型一覧

```text
Trait:
  CalcParserActions

Struct:
  CalcActions
```

### 14.2 action trait

```surtr
deftrait CalcParserActions {
  def expr_bin(
    left: Expr,
    op: BinOp,
    right: Expr,
  ) -> Result<Expr>

  def expr_int(
    token: CalcParserTokenNode,
  ) -> Result<Expr>

  def expr_group(
    lparen: CalcParserTokenNode,
    expr: Expr,
    rparen: CalcParserTokenNode,
  ) -> Result<Expr>

  def bin_op_add(
    token: CalcParserTokenNode,
  ) -> Result<BinOp>

  def bin_op_sub(
    token: CalcParserTokenNode,
  ) -> Result<BinOp>

  def bin_op_mul(
    token: CalcParserTokenNode,
  ) -> Result<BinOp>

  def bin_op_div(
    token: CalcParserTokenNode,
  ) -> Result<BinOp>
}
```

### 14.3 実装例

```surtr
defstruct CalcActions {
}

impl CalcParserActions for CalcActions {
  def expr_bin(left: Expr, op: BinOp, right: Expr) -> Result<Expr> {
    match (op, right) {
      (BinOp::Div(op_span), Expr::Int(SpannedInt(0, _))) => {
        Err(CalcZeroDivisionError("division by zero"))
      }

      _ => {
        span = SourceSpan::merge(Expr::span(left), Expr::span(right))
        Ok(Expr::Bin(left, op, right, span))
      }
    }
  }

  def expr_int(token: CalcParserTokenNode) -> Result<Expr> {
    value =? CalcParserTokenNode::expect_int_lit(token)
    span = CalcParserTokenNode::span(token)
    Ok(Expr::Int(SpannedInt(value, span)))
  }

  def expr_group(
    lparen: CalcParserTokenNode,
    expr: Expr,
    rparen: CalcParserTokenNode,
  ) -> Result<Expr> {
    Ok(expr)
  }

  def bin_op_add(token: CalcParserTokenNode) -> Result<BinOp> {
    Ok(BinOp::Add(CalcParserTokenNode::span(token)))
  }

  def bin_op_sub(token: CalcParserTokenNode) -> Result<BinOp> {
    Ok(BinOp::Sub(CalcParserTokenNode::span(token)))
  }

  def bin_op_mul(token: CalcParserTokenNode) -> Result<BinOp> {
    Ok(BinOp::Mul(CalcParserTokenNode::span(token)))
  }

  def bin_op_div(token: CalcParserTokenNode) -> Result<BinOp> {
    Ok(BinOp::Div(CalcParserTokenNode::span(token)))
  }
}
```

---

## 15. `calc.syp`

```syp
%header "calc_ast.srt"
%parser CalcParser
%actions CalcParserActions
%root expr : Expr

%lexer

skip Whitespace = re"[ \t\r\n]+"

terminal IntLit(Int) = re"\d+" => Int::parse
terminal Plus = "+"
terminal Minus = "-"
terminal Star = "*"
terminal Slash = "/"
terminal LParen = "("
terminal RParen = ")"

%grammar

%left Plus Minus
%left Star Slash

%nonterm expr : Expr
%nonterm bin_op : BinOp

%%

expr
  : expr bin_op expr      => expr_bin
  | LParen expr RParen    => expr_group
  | IntLit                => expr_int
  ;

bin_op
  : Plus                  => bin_op_add
  | Minus                 => bin_op_sub
  | Star                  => bin_op_mul
  | Slash                 => bin_op_div
  ;
```

---

## 16. 生成型一覧

`%parser CalcParser` の場合、`calc_parser.generated.srt` には以下が生成される。

```text
Enum:
  CalcParserToken
  CalcParserTokenKind
  CalcParserNonTerm
  CalcParserRuleId
  CalcParserParseState
  CalcParserParseAction
  CalcParserSemValue

Record:
  CalcParserTokenNode
  CalcParserFrame
  CalcParserUnexpectedTokenContext

Error:
  CalcParserLexError
  CalcParserUnexpectedTokenError
  CalcParserInternalError

Mod:
  CalcParser
```

---

## 17. 公開 API

```surtr
defmod CalcParser {
  def tokenize(input: String) -> Result<List<CalcParserTokenNode>>

  def parse(
    input: String,
    actions: impl CalcParserActions,
  ) -> Result<Expr>

  def parse_tokens(
    tokens: List<CalcParserTokenNode>,
    actions: impl CalcParserActions,
  ) -> Result<Expr>
}
```

---

## 18. 実装順

```text
1. .syp parser
2. header file scan: Struct / Record / Enum のみ収集
3. generated name collision check
4. lexer definition check
5. token / token kind model 構築
6. grammar symbol table 構築
7. precedence table 構築
8. LR item / table 生成
9. conflict check
10. generated .srt 出力
11. calc_ast.srt / calc_actions.srt 手書き
12. CalcParser::parse("1 + 2 * 3", CalcActions()) を通す
```

---

# 付録: `calc_parser.generated.srt` 生成コード例

以下は SYP v0 generator が生成するコードの参考例である。

実際の LR state 数、action table、goto table は生成結果に依存するため、ここでは電卓 parser 用の出力形式と構造を示す。

```surtr
// @@generated by=syp parser=CalcParser source=calc.syp
// Do not edit manually.

// ============================================================
// Token model
// ============================================================

defenum CalcParserToken {
  IntLit(Int),
  Plus,
  Minus,
  Star,
  Slash,
  LParen,
  RParen,
  EOF,
}

defenum CalcParserTokenKind {
  IntLit,
  Plus,
  Minus,
  Star,
  Slash,
  LParen,
  RParen,
  EOF,
}

defrecord CalcParserTokenNode(
  token: CalcParserToken,
  kind: CalcParserTokenKind,
  span: SourceSpan,
  raw: String,
)

impl CalcParserTokenNode {
  def new(
    token: CalcParserToken,
    kind: CalcParserTokenKind,
    span: SourceSpan,
    raw: String,
  ) -> CalcParserTokenNode {
    CalcParserTokenNode(token, kind, span, raw)
  }

  def span(self: Self) -> SourceSpan {
    self.span
  }

  def expect_int_lit(self: Self) -> Result<Int> {
    match self.token {
      CalcParserToken::IntLit(value) => Ok(value),
      _ => Err(CalcParserInternalError("expected IntLit token")),
    }
  }
}

impl CalcParserToken {
  def kind(self: Self) -> CalcParserTokenKind {
    match self {
      CalcParserToken::IntLit(_) => CalcParserTokenKind::IntLit,
      CalcParserToken::Plus => CalcParserTokenKind::Plus,
      CalcParserToken::Minus => CalcParserTokenKind::Minus,
      CalcParserToken::Star => CalcParserTokenKind::Star,
      CalcParserToken::Slash => CalcParserTokenKind::Slash,
      CalcParserToken::LParen => CalcParserTokenKind::LParen,
      CalcParserToken::RParen => CalcParserTokenKind::RParen,
      CalcParserToken::EOF => CalcParserTokenKind::EOF,
    }
  }
}

// ============================================================
// Parser internal model
// ============================================================

defenum CalcParserNonTerm {
  Expr,
  BinOp,
}

defenum CalcParserRuleId {
  ExprBin,
  ExprGroup,
  ExprInt,
  BinOpAdd,
  BinOpSub,
  BinOpMul,
  BinOpDiv,
}

defenum CalcParserParseState {
  S0,
  S1,
  S2,
  S3,
  S4,
  S5,
  S6,
  S7,
  S8,
  S9,
  S10,
  S11,
  S12,
  S13,
}

defenum CalcParserParseAction {
  Shift(CalcParserParseState),
  Reduce(CalcParserRuleId),
  Accept,
}

defenum CalcParserSemValue {
  Token(CalcParserTokenNode),
  Expr(Expr),
  BinOp(BinOp),
}

defrecord CalcParserFrame(
  state: CalcParserParseState,
  value: CalcParserSemValue,
  span: SourceSpan,
)

defrecord CalcParserUnexpectedTokenContext(
  state: CalcParserParseState,
  found: CalcParserTokenNode,
  expected: List<CalcParserTokenKind>,
)

// ============================================================
// Generated errors
//
// Note:
// deferror returns only message String.
// Ariadne source diagnostics are built by Rust-side fixed renderer.
// ============================================================

deferror CalcParserLexError(message: String) {
  message
}

deferror CalcParserUnexpectedTokenError(message: String) {
  message
}

deferror CalcParserInternalError(message: String) {
  message
}

// ============================================================
// Parser public API
// ============================================================

defmod CalcParser {
  def tokenize(input: String) -> Result<List<CalcParserTokenNode>> {
    CalcParser__tokenize(input)
  }

  def parse(
    input: String,
    actions: impl CalcParserActions,
  ) -> Result<Expr> {
    tokens =? CalcParser__tokenize(input)
    CalcParser__parse_tokens(tokens, actions)
  }

  def parse_tokens(
    tokens: List<CalcParserTokenNode>,
    actions: impl CalcParserActions,
  ) -> Result<Expr> {
    CalcParser__parse_tokens(tokens, actions)
  }
}

// ============================================================
// Tokenizer implementation
//
// Matching rules:
// 1. prefix match at current position
// 2. longest match
// 3. declaration order tie-break
// 4. skip produces no token
// ============================================================

defp CalcParser__tokenize(input: String) -> Result<List<CalcParserTokenNode>> {
  // Pseudocode-level generated Surtr.
  // Actual generator may lower this to builtin regex calls or direct lexer opcodes.

  source_id = SourceId("<input>")
  tokens = []
  pos = 0

  while pos < String::len(input) {
    // skip Whitespace = re"[ \t\r\n]+"
    if Regex::match_prefix(re"[ \t\r\n]+", input, pos) {
      m = Regex::prefix(re"[ \t\r\n]+", input, pos)
      pos = RegexMatch::end(m)
      continue
    }

    // terminal IntLit(Int) = re"\d+" => Int::parse
    if Regex::match_prefix(re"\d+", input, pos) {
      m = Regex::prefix(re"\d+", input, pos)
      raw = RegexMatch::text(m)
      value =? Int::parse(raw)
      span = SourceSpan(source_id, pos, RegexMatch::end(m))
      token = CalcParserToken::IntLit(value)
      node = CalcParserTokenNode(token, CalcParserTokenKind::IntLit, span, raw)
      tokens = List::cons(node, tokens)
      pos = RegexMatch::end(m)
      continue
    }

    // terminal Plus = "+"
    if String::starts_with_at(input, "+", pos) {
      span = SourceSpan(source_id, pos, pos + 1)
      node = CalcParserTokenNode(
        CalcParserToken::Plus,
        CalcParserTokenKind::Plus,
        span,
        "+",
      )
      tokens = List::cons(node, tokens)
      pos = pos + 1
      continue
    }

    // terminal Minus = "-"
    if String::starts_with_at(input, "-", pos) {
      span = SourceSpan(source_id, pos, pos + 1)
      node = CalcParserTokenNode(
        CalcParserToken::Minus,
        CalcParserTokenKind::Minus,
        span,
        "-",
      )
      tokens = List::cons(node, tokens)
      pos = pos + 1
      continue
    }

    // terminal Star = "*"
    if String::starts_with_at(input, "*", pos) {
      span = SourceSpan(source_id, pos, pos + 1)
      node = CalcParserTokenNode(
        CalcParserToken::Star,
        CalcParserTokenKind::Star,
        span,
        "*",
      )
      tokens = List::cons(node, tokens)
      pos = pos + 1
      continue
    }

    // terminal Slash = "/"
    if String::starts_with_at(input, "/", pos) {
      span = SourceSpan(source_id, pos, pos + 1)
      node = CalcParserTokenNode(
        CalcParserToken::Slash,
        CalcParserTokenKind::Slash,
        span,
        "/",
      )
      tokens = List::cons(node, tokens)
      pos = pos + 1
      continue
    }

    // terminal LParen = "("
    if String::starts_with_at(input, "(", pos) {
      span = SourceSpan(source_id, pos, pos + 1)
      node = CalcParserTokenNode(
        CalcParserToken::LParen,
        CalcParserTokenKind::LParen,
        span,
        "(",
      )
      tokens = List::cons(node, tokens)
      pos = pos + 1
      continue
    }

    // terminal RParen = ")"
    if String::starts_with_at(input, ")", pos) {
      span = SourceSpan(source_id, pos, pos + 1)
      node = CalcParserTokenNode(
        CalcParserToken::RParen,
        CalcParserTokenKind::RParen,
        span,
        ")",
      )
      tokens = List::cons(node, tokens)
      pos = pos + 1
      continue
    }

    span = SourceSpan(source_id, pos, pos + 1)
    Err(CalcParserLexError("unexpected character"))
  }

  eof_span = SourceSpan(source_id, pos, pos)
  eof = CalcParserTokenNode(
    CalcParserToken::EOF,
    CalcParserTokenKind::EOF,
    eof_span,
    "",
  )

  Ok(List::reverse(List::cons(eof, tokens)))
}

// ============================================================
// Parser driver
// ============================================================

defp CalcParser__parse_tokens(
  tokens: List<CalcParserTokenNode>,
  actions: impl CalcParserActions,
) -> Result<Expr> {
  initial_frame = CalcParserFrame(
    CalcParserParseState::S0,
    CalcParserSemValue::Token(CalcParserTokenNode(
      CalcParserToken::EOF,
      CalcParserTokenKind::EOF,
      SourceSpan(SourceId("<internal>"), 0, 0),
      "",
    )),
    SourceSpan(SourceId("<internal>"), 0, 0),
  )

  stack = [initial_frame]
  rest = tokens

  CalcParser__run(stack, rest, actions)
}

defp CalcParser__run(
  stack: List<CalcParserFrame>,
  rest: List<CalcParserTokenNode>,
  actions: impl CalcParserActions,
) -> Result<Expr> {
  lookahead =? List::first(rest)
  state =? CalcParser__top_state(stack)

  action =? CalcParser__action(state, lookahead.kind)

  match action {
    CalcParserParseAction::Shift(next_state) => {
      span = lookahead.span
      frame = CalcParserFrame(
        next_state,
        CalcParserSemValue::Token(lookahead),
        span,
      )
      next_stack = List::cons(frame, stack)
      next_rest =? List::drop(rest, 1)
      CalcParser__run(next_stack, next_rest, actions)
    }

    CalcParserParseAction::Reduce(rule_id) => {
      (next_stack, _span) =? CalcParser__reduce(rule_id, stack, actions)
      CalcParser__run(next_stack, rest, actions)
    }

    CalcParserParseAction::Accept => {
      CalcParser__finish(stack)
    }
  }
}

// ============================================================
// Action table
//
// The actual table is generated from LR/LALR item sets.
// This skeleton shows the intended shape.
// ============================================================

defp CalcParser__action(
  state: CalcParserParseState,
  lookahead: CalcParserTokenKind,
) -> Result<CalcParserParseAction> {
  match (state, lookahead) {
    // Example rows only. Real generator emits complete table.
    (CalcParserParseState::S0, CalcParserTokenKind::IntLit) => {
      Ok(CalcParserParseAction::Shift(CalcParserParseState::S5))
    }

    (CalcParserParseState::S0, CalcParserTokenKind::LParen) => {
      Ok(CalcParserParseAction::Shift(CalcParserParseState::S4))
    }

    (CalcParserParseState::S1, CalcParserTokenKind::EOF) => {
      Ok(CalcParserParseAction::Accept)
    }

    (CalcParserParseState::S1, CalcParserTokenKind::Plus) => {
      Ok(CalcParserParseAction::Shift(CalcParserParseState::S6))
    }

    (CalcParserParseState::S1, CalcParserTokenKind::Minus) => {
      Ok(CalcParserParseAction::Shift(CalcParserParseState::S7))
    }

    (CalcParserParseState::S2, CalcParserTokenKind::Plus) => {
      Ok(CalcParserParseAction::Reduce(CalcParserRuleId::ExprInt))
    }

    _ => {
      Err(CalcParserUnexpectedTokenError("unexpected token"))
    }
  }
}

// ============================================================
// Goto table
// ============================================================

defp CalcParser__goto(
  state: CalcParserParseState,
  nonterm: CalcParserNonTerm,
) -> Result<CalcParserParseState> {
  match (state, nonterm) {
    // Example rows only. Real generator emits complete table.
    (CalcParserParseState::S0, CalcParserNonTerm::Expr) => {
      Ok(CalcParserParseState::S1)
    }

    (CalcParserParseState::S0, CalcParserNonTerm::BinOp) => {
      Ok(CalcParserParseState::S3)
    }

    _ => {
      Err(CalcParserInternalError("invalid goto"))
    }
  }
}

// ============================================================
// Reduce dispatch
// ============================================================

defp CalcParser__reduce(
  rule_id: CalcParserRuleId,
  stack: List<CalcParserFrame>,
  actions: impl CalcParserActions,
) -> Result<(List<CalcParserFrame>, SourceSpan)> {
  match rule_id {
    CalcParserRuleId::ExprBin => {
      CalcParser__reduce_expr_bin(stack, actions)
    }

    CalcParserRuleId::ExprGroup => {
      CalcParser__reduce_expr_group(stack, actions)
    }

    CalcParserRuleId::ExprInt => {
      CalcParser__reduce_expr_int(stack, actions)
    }

    CalcParserRuleId::BinOpAdd => {
      CalcParser__reduce_bin_op_add(stack, actions)
    }

    CalcParserRuleId::BinOpSub => {
      CalcParser__reduce_bin_op_sub(stack, actions)
    }

    CalcParserRuleId::BinOpMul => {
      CalcParser__reduce_bin_op_mul(stack, actions)
    }

    CalcParserRuleId::BinOpDiv => {
      CalcParser__reduce_bin_op_div(stack, actions)
    }
  }
}

// ============================================================
// Reduce implementations
// ============================================================

defp CalcParser__reduce_expr_bin(
  stack: List<CalcParserFrame>,
  actions: impl CalcParserActions,
) -> Result<(List<CalcParserFrame>, SourceSpan)> {
  // rule: expr : expr bin_op expr => expr_bin

  (right_frame, s1) =? CalcParser__pop(stack)
  right =? CalcParser__expect_expr(right_frame)

  (op_frame, s2) =? CalcParser__pop(s1)
  op =? CalcParser__expect_bin_op(op_frame)

  (left_frame, s3) =? CalcParser__pop(s2)
  left =? CalcParser__expect_expr(left_frame)

  value =? CalcParserActions::expr_bin(actions, left, op, right)

  span = SourceSpan::merge(left_frame.span, right_frame.span)
  next_stack =? CalcParser__push_nonterm(
    s3,
    CalcParserNonTerm::Expr,
    CalcParserSemValue::Expr(value),
    span,
  )

  Ok((next_stack, span))
}

defp CalcParser__reduce_expr_group(
  stack: List<CalcParserFrame>,
  actions: impl CalcParserActions,
) -> Result<(List<CalcParserFrame>, SourceSpan)> {
  // rule: expr : LParen expr RParen => expr_group

  (rparen_frame, s1) =? CalcParser__pop(stack)
  rparen =? CalcParser__expect_token(rparen_frame, CalcParserTokenKind::RParen)

  (expr_frame, s2) =? CalcParser__pop(s1)
  expr =? CalcParser__expect_expr(expr_frame)

  (lparen_frame, s3) =? CalcParser__pop(s2)
  lparen =? CalcParser__expect_token(lparen_frame, CalcParserTokenKind::LParen)

  value =? CalcParserActions::expr_group(actions, lparen, expr, rparen)

  span = SourceSpan::merge(lparen_frame.span, rparen_frame.span)
  next_stack =? CalcParser__push_nonterm(
    s3,
    CalcParserNonTerm::Expr,
    CalcParserSemValue::Expr(value),
    span,
  )

  Ok((next_stack, span))
}

defp CalcParser__reduce_expr_int(
  stack: List<CalcParserFrame>,
  actions: impl CalcParserActions,
) -> Result<(List<CalcParserFrame>, SourceSpan)> {
  // rule: expr : IntLit => expr_int

  (token_frame, s1) =? CalcParser__pop(stack)
  token =? CalcParser__expect_token(token_frame, CalcParserTokenKind::IntLit)

  value =? CalcParserActions::expr_int(actions, token)

  span = token_frame.span
  next_stack =? CalcParser__push_nonterm(
    s1,
    CalcParserNonTerm::Expr,
    CalcParserSemValue::Expr(value),
    span,
  )

  Ok((next_stack, span))
}

defp CalcParser__reduce_bin_op_add(
  stack: List<CalcParserFrame>,
  actions: impl CalcParserActions,
) -> Result<(List<CalcParserFrame>, SourceSpan)> {
  // rule: bin_op : Plus => bin_op_add

  (token_frame, s1) =? CalcParser__pop(stack)
  token =? CalcParser__expect_token(token_frame, CalcParserTokenKind::Plus)

  value =? CalcParserActions::bin_op_add(actions, token)

  span = token_frame.span
  next_stack =? CalcParser__push_nonterm(
    s1,
    CalcParserNonTerm::BinOp,
    CalcParserSemValue::BinOp(value),
    span,
  )

  Ok((next_stack, span))
}

defp CalcParser__reduce_bin_op_sub(
  stack: List<CalcParserFrame>,
  actions: impl CalcParserActions,
) -> Result<(List<CalcParserFrame>, SourceSpan)> {
  (token_frame, s1) =? CalcParser__pop(stack)
  token =? CalcParser__expect_token(token_frame, CalcParserTokenKind::Minus)
  value =? CalcParserActions::bin_op_sub(actions, token)
  span = token_frame.span
  next_stack =? CalcParser__push_nonterm(
    s1,
    CalcParserNonTerm::BinOp,
    CalcParserSemValue::BinOp(value),
    span,
  )
  Ok((next_stack, span))
}

defp CalcParser__reduce_bin_op_mul(
  stack: List<CalcParserFrame>,
  actions: impl CalcParserActions,
) -> Result<(List<CalcParserFrame>, SourceSpan)> {
  (token_frame, s1) =? CalcParser__pop(stack)
  token =? CalcParser__expect_token(token_frame, CalcParserTokenKind::Star)
  value =? CalcParserActions::bin_op_mul(actions, token)
  span = token_frame.span
  next_stack =? CalcParser__push_nonterm(
    s1,
    CalcParserNonTerm::BinOp,
    CalcParserSemValue::BinOp(value),
    span,
  )
  Ok((next_stack, span))
}

defp CalcParser__reduce_bin_op_div(
  stack: List<CalcParserFrame>,
  actions: impl CalcParserActions,
) -> Result<(List<CalcParserFrame>, SourceSpan)> {
  (token_frame, s1) =? CalcParser__pop(stack)
  token =? CalcParser__expect_token(token_frame, CalcParserTokenKind::Slash)
  value =? CalcParserActions::bin_op_div(actions, token)
  span = token_frame.span
  next_stack =? CalcParser__push_nonterm(
    s1,
    CalcParserNonTerm::BinOp,
    CalcParserSemValue::BinOp(value),
    span,
  )
  Ok((next_stack, span))
}

// ============================================================
// Stack helpers
// ============================================================

defp CalcParser__pop(
  stack: List<CalcParserFrame>,
) -> Result<(CalcParserFrame, List<CalcParserFrame>)> {
  match stack {
    [head, ..tail] => Ok((head, tail)),
    [] => Err(CalcParserInternalError("parser stack underflow")),
  }
}

defp CalcParser__top_state(
  stack: List<CalcParserFrame>,
) -> Result<CalcParserParseState> {
  frame =? List::first(stack)
  Ok(frame.state)
}

defp CalcParser__push_nonterm(
  stack: List<CalcParserFrame>,
  nonterm: CalcParserNonTerm,
  value: CalcParserSemValue,
  span: SourceSpan,
) -> Result<List<CalcParserFrame>> {
  prev_state =? CalcParser__top_state(stack)
  next_state =? CalcParser__goto(prev_state, nonterm)
  frame = CalcParserFrame(next_state, value, span)
  Ok(List::cons(frame, stack))
}

defp CalcParser__expect_token(
  frame: CalcParserFrame,
  kind: CalcParserTokenKind,
) -> Result<CalcParserTokenNode> {
  match frame.value {
    CalcParserSemValue::Token(token) when token.kind == kind => Ok(token),
    _ => Err(CalcParserInternalError("unexpected stack value: token expected")),
  }
}

defp CalcParser__expect_expr(frame: CalcParserFrame) -> Result<Expr> {
  match frame.value {
    CalcParserSemValue::Expr(value) => Ok(value),
    _ => Err(CalcParserInternalError("unexpected stack value: Expr expected")),
  }
}

defp CalcParser__expect_bin_op(frame: CalcParserFrame) -> Result<BinOp> {
  match frame.value {
    CalcParserSemValue::BinOp(value) => Ok(value),
    _ => Err(CalcParserInternalError("unexpected stack value: BinOp expected")),
  }
}

defp CalcParser__finish(stack: List<CalcParserFrame>) -> Result<Expr> {
  frame =? List::first(stack)

  match frame.value {
    CalcParserSemValue::Expr(value) => Ok(value),
    _ => Err(CalcParserInternalError("accept state did not contain Expr")),
  }
}
```

---

## 19. 付録コードについての注意

上記の `calc_parser.generated.srt` はコードジェネレーター実装用の参考である。

実際の生成では、以下は generator の出力に応じて変わる。

```text
- ParseState の個数
- action table
- goto table
- precedence による conflict 解決後の reduce/shift
- tokenizer lowering の具体形
- regex API 名
- String helper API 名
- while / continue の surface 可否
```

SYP v0 の外部契約として固定するのは以下である。

```text
- 生成型名 prefix
- Token / TokenKind / TokenNode の役割
- action signature 規約
- stack value は span を持つ
- deferror は message String のみ
- Ariadne 診断は Rust 側固定 renderer
- public API: tokenize / parse / parse_tokens
```
