# Surtr MVP 要件定義書 — スクリプトレベル実装（①〜⑤）

> 大元: `Surtr_v7` (draft 0.7)
> スコープ: まず動くものを作る。ユーザ拡張（マクロ・DSL）は後回し。

-----

## 目次

1. [スコープと段階](#1-スコープと段階)
2. [コンパイラアーキテクチャ](#2-コンパイラアーキテクチャ)
3. [① スクリプトレベル](#3--スクリプトレベル)
4. [② 型検査](#4--型検査)
5. [③ プリミティブ型](#5--プリミティブ型)
6. [④ 組込み関数](#6--組込み関数)
7. [⑤ リスト](#7--リスト)
8. [ランタイム設計](#8-ランタイム設計)
9. [バイトコード設計](#9-バイトコード設計)
10. [エラー報告](#10-エラー報告)
11. [実装順序](#11-実装順序)
12. [検討課題（MVP 後）](#12-検討課題mvp-後)

-----

## 1. スコープと段階

### MVP の目標

以下のコードがコンパイル・実行できること。

```surtr
num = 10
num2 = 5
print(num + num2)
# => 15

flag = True
sym = `ok`

nums: [Int] = [1, 2, 3]
print(to_string(nums))
```

### MVP で実装しないもの

| 機能 | 理由 |
|---|---|
| マクロシステム（`defmacro`） | ユーザ拡張。後回し |
| `if` / `match` / `cond` | マクロ前提の設計。MVP ではリテラル式の評価に集中 |
| `def`（関数定義） | `def` はマクロ。MVP ではトップレベル式のみ |
| `\|>` パイプライン | 関数定義が必要。MVP 後 |
| `result do` / `=?` | MatchContext 含め後回し |
| `defstruct` / `enum` | 型定義は後回し |
| モジュール / `import` | 後回し |
| レンズ（`@.field`） | 構造体が必要。後回し |

### MVP で実装するもの

| 段階 | 内容 |
|---|---|
| ① スクリプトレベル | 変数束縛（`=`）、`print` 関数、トップレベル式の暗黙 main |
| ② 型検査 | 型注釈（`num: Int = 10`）、型不一致のコンパイルエラー |
| ③ プリミティブ型 | `Int`, `Float`, `String`, `Boolean`, `Symbol`, `Unit` |
| ④ 組込み関数 | 算術演算、`to_string`、`print` |
| ⑤ リスト | リストリテラル `[1, 2, 3]`、空リスト `[]`、型推論 `[Int]` |

-----

## 2. コンパイラアーキテクチャ

### フェーズチェーン（MVP）

IDEA-001 のフェーズ分離方式に準拠。MVP ではマクロ展開フェーズをスキップする。

```
Source → parse → Ast → resolve → Resolved → typecheck → Typed → codegen → Bytecode → execute
```

各フェーズは独自の Enum を持ち、`Enum A → Result<Enum B>` の変換チェーンで進む。

```rust
fn parse(src: &str) -> Result<Vec<Ast>, ParseError>
fn resolve(ast: Vec<Ast>) -> Result<Vec<Resolved>, ResolveError>
fn typecheck(resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError>
fn codegen(typed: Vec<TypedNode>) -> Result<Bytecode, CodegenError>
fn execute(bytecode: Bytecode) -> Result<(), RuntimeError>
```

### MVP で省略するフェーズ

| フェーズ | MVP での扱い |
|---|---|
| `expand`（マクロ展開） | 省略。Ast に MacroCall が含まれたら即エラー |
| リンク | 単一ファイルのみ。依存解決不要 |

-----

## 3. ① スクリプトレベル

### 要件

トップレベルに式を書くと暗黙の `main` 関数として実行される。

```surtr
num = 10
num2 = 5
print(num + num2)
# => 15
```

### パーサ出力（Ast）

```rust
pub enum Ast {
    Lit(Span, Lit),
    Var(Span, Symbol),
    Bind(Span, AstPattern, Box<Ast>),           // =
    App(Span, Box<Ast>, Vec<Ast>),              // 関数呼び出し
    BinOp(Span, BinOp, Box<Ast>, Box<Ast>),     // 二項演算
    List(Span, Vec<Ast>),                       // リストリテラル
}

pub enum Lit {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Symbol(String),
    Unit,
}

pub enum AstPattern {
    Var(Span, Symbol),
    Annotated(Span, Symbol, AstTy),             // num: Int
    Wildcard(Span),
}

pub enum AstTy {
    Named(Span, Symbol),                        // Int, String
    Generic(Span, Symbol, Vec<AstTy>),          // List<Int> 表記は MVP では [Int] のみ
    ListOf(Span, Box<AstTy>),                   // [Int]
}

pub enum BinOp { Add, Sub, Mul, Div, Mod, Eq, Neq, Lt, Gt, Lte, Gte }

pub type Symbol = String;
pub struct Span { pub start: usize, pub end: usize }
```

`Span` を全ノードに持たせることで、後続フェーズのエラー報告でソース位置を参照できる。

### 構文規則

```
program     = stmt*
stmt        = bind | expr
bind        = pattern "=" expr
pattern     = IDENT ":" type       -- 型注釈付き
            | IDENT                -- 型注釈なし
            | "_"                  -- ワイルドカード
expr        = expr binop expr      -- 二項演算
            | IDENT "(" args ")"   -- 関数呼び出し
            | "[" list_items "]"   -- リストリテラル
            | literal
            | IDENT                -- 変数参照
literal     = INT | FLOAT | STRING | BOOL | SYMBOL | "()"
type        = IDENT                -- Int, String, etc.
            | "[" type "]"         -- [Int]
```

### `=` 演算子の仕様（IDEA-001 確定事項）

| 性質 | 仕様 |
|---|---|
| 戻り値 | `Unit` |
| 結合性 | 非結合（LHS にも RHS にも `=` を置けない） |
| 副作用 | 実行コンテキスト（ローカル変数テーブル）への書き込み |
| シャドウイング | 許可。コンパイラが内部で `x_id0`, `x_id1` に展開 |

### トップレベル式の解釈

ファイルのトップレベルに書かれた式は暗黙の `main` 関数に包まれる。

```surtr
// ユーザが書くコード
num = 10
print(num)

// コンパイラの解釈
// def main() -> Unit {
//   num = 10
//   print(num)
// }
```

### コメント構文

```surtr
# 行コメント（# から行末まで）
```

MVP ではブロックコメントは実装しない。

-----

## 4. ② 型検査

### 要件

型注釈がある場合は RHS の型と照合する。型注釈がない場合は RHS から推論する。

```surtr
# ok: 型注釈と RHS が一致
num: Int = 10

# err: 型不一致
bad: Int = "bad type"
# => TypeError: expected Int, got String at line 2
```

### 名前解決（Resolved）

```rust
pub struct ResolvedId {
    pub name: Symbol,
    pub unique_id: u32,     // シャドウイング解決
    pub span: Span,
}

pub enum Resolved {
    Lit(Span, Lit),
    Var(Span, ResolvedId),
    Bind(Span, ResolvedPattern, Box<Resolved>),
    App(Span, Box<Resolved>, Vec<Resolved>),
    BinOp(Span, BinOp, Box<Resolved>, Box<Resolved>),
    List(Span, Vec<Resolved>),
}

pub enum ResolvedPattern {
    Var(ResolvedId),
    Annotated(ResolvedId, AstTy),
    Wildcard(Span),
}
```

シャドウイングの解決:

```surtr
num = 10         // ResolvedId { name: "num", unique_id: 0 }
num = num + 1    // LHS: ResolvedId { name: "num", unique_id: 1 }
                 // RHS の num: ResolvedId { name: "num", unique_id: 0 }
```

### 型検査（Typed）

IDEA-001 確定のラッパー方式。

```rust
pub struct TypedNode {
    pub ty: Ty,
    pub span: Span,
    pub node: TypedInner,
}

pub enum TypedInner {
    Lit(Lit),
    Var(ResolvedId),
    Bind(TypedPattern, Box<TypedNode>),
    App(Box<TypedNode>, Vec<TypedNode>),
    BinOp(BinOp, Box<TypedNode>, Box<TypedNode>),
    List(Vec<TypedNode>),
}

pub enum TypedPattern {
    Var(Ty, ResolvedId),
    Wildcard(Ty),
}

pub enum Ty {
    Int,
    Float,
    Str,
    Bool,
    Symbol,
    Unit,
    List(Box<Ty>),
    Func(Vec<Ty>, Box<Ty>),     // 組込み関数の型表現用
    Var(TyVar),                  // 型変数（推論中の未確定型）
}

pub type TyVar = u32;
```

### 型検査の規則

| 場面 | 規則 |
|---|---|
| `Lit::Int(n)` | → `Ty::Int` |
| `Lit::Float(n)` | → `Ty::Float` |
| `Lit::Str(s)` | → `Ty::Str` |
| `Lit::Bool(b)` | → `Ty::Bool` |
| `Lit::Symbol(s)` | → `Ty::Symbol` |
| `Lit::Unit` | → `Ty::Unit` |
| `Var(id)` | → スコープから `id` の型を検索 |
| `Bind(pat, rhs)` | → RHS の型を検査し、パターンの型と照合。Bind 自体は `Ty::Unit` |
| `BinOp(Add, lhs, rhs)` | → 両辺が `Int` なら `Int`、両辺が `Float` なら `Float`、それ以外はエラー |
| `App(func, args)` | → `func` の型が `Func(param_tys, ret_ty)` であることを検査。引数型を照合し `ret_ty` を返す |
| `List(elems)` | → 全要素が同一型 `T` であることを検査。`Ty::List(T)` を返す |

### 型注釈の照合

```
Annotated(id, AstTy::Named("Int"))
  1. AstTy::Named("Int") → Ty::Int に解決
  2. RHS を型検査して Ty を得る
  3. 両者を unify
     一致 → id に Ty::Int を割り当て
     不一致 → TypeError
```

### Warning: 未使用変数

```surtr
x = 42
# Warning: Unused variable `x` at line 1. Prefix with `_` to suppress.

_x = 42
# Warning なし
```

-----

## 5. ③ プリミティブ型

### 型の一覧

| 型 | Rust 表現 | リテラル例 | 備考 |
|---|---|---|---|
| `Int` | `i64` | `42`, `-1`, `0` | 64bit 符号付き整数 |
| `Float` | `f64` | `3.14`, `-0.5` | 64bit 浮動小数点 |
| `String` | `String` | `"hello"`, `'world'` | ダブルクォート: 式埋め込みあり（MVP では埋め込み未実装）、シングル: なし |
| `Boolean` | `bool` | `True`, `False` | 先頭大文字（Enum バリアント扱い） |
| `Symbol` | `String` | `` `ok` ``, `` `error` `` | バッククォートで囲む。Elixir の Atom に相当 |
| `Unit` | `()` | `()` | 値なし。Bind の戻り値、副作用関数の戻り値 |

### リテラルの字句規則

```
INT       = [0-9]+ | "-" [0-9]+
FLOAT     = [0-9]+ "." [0-9]+ | "-" [0-9]+ "." [0-9]+
STRING_DQ = '"' (非'"' | 転義)* '"'
STRING_SQ = "'" (非"'")* "'"
BOOL      = "True" | "False"
SYMBOL    = "`" IDENT "`"
UNIT      = "()"
IDENT     = [a-zA-Z_][a-zA-Z0-9_]*
```

### String のダブルクォート / シングルクォート

| クォート | 式埋め込み | 転義シーケンス |
|---|---|---|
| `"..."` | あり（MVP では未実装） | `\\`, `\"`, `\n`, `\t` |
| `'...'` | なし | `\\`, `\'` |

-----

## 6. ④ 組込み関数

### MVP で実装する組込み関数

| 関数 | シグネチャ | 説明 |
|---|---|---|
| `print` | `(a: $A) -> Unit` | 任意の型を表示。内部で `to_string` を呼ぶ |
| `to_string` | `(a: $A) -> String` | 任意の型を文字列に変換 |

### MVP で実装する二項演算子

| 演算子 | 対応する型 | 戻り値 |
|---|---|---|
| `+` | `(Int, Int) -> Int`, `(Float, Float) -> Float` | 加算 |
| `-` | `(Int, Int) -> Int`, `(Float, Float) -> Float` | 減算 |
| `*` | `(Int, Int) -> Int`, `(Float, Float) -> Float` | 乗算 |
| `/` | `(Int, Int) -> Int`, `(Float, Float) -> Float` | 除算（Int は整数除算） |
| `%` | `(Int, Int) -> Int` | 剰余 |
| `==` | `($A, $A) -> Boolean` | 等値比較 |
| `!=` | `($A, $A) -> Boolean` | 非等値比較 |
| `<`, `>`, `<=`, `>=` | `(Int, Int) -> Boolean`, `(Float, Float) -> Boolean` | 比較 |

### `print` と `to_string` の振る舞い

```surtr
print(42)           # => 42
print(3.14)         # => 3.14
print("hello")      # => hello
print(True)         # => True
print(`ok`)         # => ok
print(())           # => ()
print([1, 2, 3])    # => [1, 2, 3]
```

`to_string` の出力形式:

| 型 | 出力例 |
|---|---|
| `Int` | `"42"` |
| `Float` | `"3.14"` |
| `String` | `"hello"`（そのまま） |
| `Boolean` | `"True"` / `"False"` |
| `Symbol` | `"ok"`（バッククォートなし） |
| `Unit` | `"()"` |
| `[Int]` | `"[1, 2, 3]"` |
| `[a]`（空） | `"[]"` |

### 組込み関数の型表現

組込み関数は `$A`（型変数）を使った多相型を持つ。MVP では `print` と `to_string` のみが多相。算術演算は型ごとにオーバーロード解決する。

```rust
// 組込み関数の登録（TypeChecker の初期環境）
fn builtin_env() -> TypeEnv {
    let mut env = TypeEnv::new();
    // print: forall A. (A -> Unit)
    env.register("print", Ty::Func(vec![Ty::Var(fresh_tyvar())], Box::new(Ty::Unit)));
    // to_string: forall A. (A -> String)
    env.register("to_string", Ty::Func(vec![Ty::Var(fresh_tyvar())], Box::new(Ty::Str)));
    env
}
```

### 算術演算のコード生成

TypeChecker が型を確定した後、codegen が型特化した Opcode を生成する（付録 B の設計に準拠）。

```rust
match (lhs_ty, rhs_ty, op) {
    (Ty::Int, Ty::Int, BinOp::Add)     => emit(Opcode::AddInt),
    (Ty::Float, Ty::Float, BinOp::Add) => emit(Opcode::AddFloat),
    (Ty::Int, Ty::Int, BinOp::Eq)      => emit(Opcode::EqInt),
    // ...
}
```

-----

## 7. ⑤ リスト

### 要件

リストリテラルの構築と型検査。MVP ではリストの操作関数（`map`, `filter` 等）は実装しない。

```surtr
zero = []
one = [1]
many = [1, 2, 4]
```

### リストリテラルの型推論

| リテラル | 推論結果 | 規則 |
|---|---|---|
| `[1, 2, 3]` | `[Int]` | 全要素が `Int` |
| `["a", "b"]` | `[String]` | 全要素が `String` |
| `[1, "a"]` | TypeError | 要素型が不一致 |
| `[]` | `[Ty::Var(?)]` | 空リスト。型変数が残る。文脈から推論 |
| `x: [Int] = []` | `[Int]` | 型注釈で型変数が確定 |

空リストの型推論:

```surtr
# 型注釈で確定
empty: [Int] = []

# 文脈から推論（MVP 後。関数引数の型から逆推論）
# nums = [] |> append(1)  → [Int]

# 型注釈なし、文脈なし → コンパイルエラー
empty = []
# => TypeError: Cannot infer type of empty list. Add a type annotation: `empty: [T] = []`
```

### リストのランタイム表現

```rust
// ランタイムの値表現
pub enum Value {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Symbol(String),
    Unit,
    List(Vec<Value>),     // MVP: Vec で実装
}
```

MVP ではリストを `Vec<Value>` で表現する。イミュータブルリスト（永続データ構造）への移行は MVP 後に検討する。

### リスト関連の Opcode

| Opcode | 動作 |
|---|---|
| `ListNew(n)` | スタックから `n` 個の値を取り出しリストを構築 |
| `ListEmpty` | 空リストをスタックに push |

-----

## 8. ランタイム設計

### アーキテクチャ

スタックベースの VM。BEAM（Erlang VM）を参考にしたシンプルな設計。

```
┌─────────────────────────────┐
│         Surtr VM            │
│                             │
│  ┌───────────────────────┐  │
│  │    Operand Stack      │  │  値の一時保管
│  └───────────────────────┘  │
│  ┌───────────────────────┐  │
│  │    Local Variables    │  │  変数テーブル（スロット番号でアクセス）
│  └───────────────────────┘  │
│  ┌───────────────────────┐  │
│  │    Bytecode           │  │  命令列
│  └───────────────────────┘  │
│  ┌───────────────────────┐  │
│  │    Constant Pool      │  │  リテラル定数
│  └───────────────────────┘  │
│  ┌───────────────────────┐  │
│  │    Builtin Functions  │  │  組込み関数テーブル
│  └───────────────────────┘  │
└─────────────────────────────┘
```

### Value 型

ランタイムで扱う値の統一表現。

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Symbol(String),
    Unit,
    List(Vec<Value>),
}

impl Value {
    pub fn to_display_string(&self) -> String {
        match self {
            Value::Int(n)    => n.to_string(),
            Value::Float(f)  => format!("{}", f),
            Value::Str(s)    => s.clone(),
            Value::Bool(b)   => if *b { "True".into() } else { "False".into() },
            Value::Symbol(s) => s.clone(),
            Value::Unit      => "()".into(),
            Value::List(vs)  => {
                let elems: Vec<String> = vs.iter().map(|v| v.to_display_string()).collect();
                format!("[{}]", elems.join(", "))
            }
        }
    }
}
```

### 実行モデル

```rust
pub struct VM {
    stack: Vec<Value>,                      // オペランドスタック
    locals: Vec<Value>,                     // ローカル変数（スロット番号でアクセス）
    constants: Vec<Value>,                  // 定数プール
    bytecode: Vec<Opcode>,                  // 命令列
    pc: usize,                              // プログラムカウンタ
    builtins: HashMap<String, BuiltinFn>,   // 組込み関数
}

pub type BuiltinFn = fn(&mut VM, Vec<Value>) -> Result<Value, RuntimeError>;
```

### 実行ループ

```rust
impl VM {
    pub fn run(&mut self) -> Result<(), RuntimeError> {
        while self.pc < self.bytecode.len() {
            let op = self.bytecode[self.pc].clone();
            self.pc += 1;
            match op {
                Opcode::LoadConst(idx)   => self.stack.push(self.constants[idx].clone()),
                Opcode::LoadLocal(slot)  => self.stack.push(self.locals[slot].clone()),
                Opcode::StoreLocal(slot) => {
                    let val = self.stack.pop().unwrap();
                    if slot >= self.locals.len() {
                        self.locals.resize(slot + 1, Value::Unit);
                    }
                    self.locals[slot] = val;
                }
                Opcode::AddInt => {
                    let rhs = self.stack.pop().unwrap();
                    let lhs = self.stack.pop().unwrap();
                    match (lhs, rhs) {
                        (Value::Int(a), Value::Int(b)) => self.stack.push(Value::Int(a + b)),
                        _ => return Err(RuntimeError::TypeMismatch),
                    }
                }
                Opcode::CallBuiltin(name, arity) => {
                    let args: Vec<Value> = (0..arity)
                        .map(|_| self.stack.pop().unwrap())
                        .rev()
                        .collect();
                    let func = self.builtins.get(&name)
                        .ok_or(RuntimeError::UndefinedFunction(name.clone()))?;
                    let result = func(self, args)?;
                    self.stack.push(result);
                }
                Opcode::Pop => { self.stack.pop(); }
                Opcode::ListNew(n) => {
                    let elems: Vec<Value> = (0..n)
                        .map(|_| self.stack.pop().unwrap())
                        .rev()
                        .collect();
                    self.stack.push(Value::List(elems));
                }
                Opcode::ListEmpty => {
                    self.stack.push(Value::List(Vec::new()));
                }
                Opcode::Halt => break,
                // ... 他の Opcode
                _ => return Err(RuntimeError::UnknownOpcode),
            }
        }
        Ok(())
    }
}
```

### ローカル変数の管理

codegen がシャドウイング解決済みの `unique_id` をスロット番号にマッピングする。

```
Surtr コード:     num = 10; num = num + 1
Resolved:         num_id0 = 10; num_id1 = num_id0 + 1
codegen マッピング: num_id0 → slot 0, num_id1 → slot 1
```

```
バイトコード:
  LoadConst 0       // 10 を定数プールから
  StoreLocal 0      // slot 0 に格納（num_id0）
  LoadLocal 0       // slot 0 を読み出し
  LoadConst 1       // 1 を定数プールから
  AddInt             // 加算
  StoreLocal 1      // slot 1 に格納（num_id1）
```

### GC

MVP では GC を実装しない。`Value` は Rust の所有権で管理し、スコープを出たら自動的に drop される。

MVP 後に参照カウント（RC）またはトレーシング GC を導入する。リスト操作（cons / append）が頻繁になる段階で検討する。

-----

## 9. バイトコード設計

### Opcode 一覧（MVP）

```rust
#[derive(Debug, Clone)]
pub enum Opcode {
    // 定数・変数
    LoadConst(usize),       // 定数プール[idx] をスタックに push
    LoadLocal(usize),       // locals[slot] をスタックに push
    StoreLocal(usize),      // スタック top を locals[slot] に格納

    // 算術（Int）
    AddInt,
    SubInt,
    MulInt,
    DivInt,
    ModInt,

    // 算術（Float）
    AddFloat,
    SubFloat,
    MulFloat,
    DivFloat,

    // 比較
    EqInt,
    NeqInt,
    LtInt,
    GtInt,
    LteInt,
    GteInt,
    EqFloat,
    NeqFloat,
    LtFloat,
    GtFloat,
    LteFloat,
    GteFloat,
    EqStr,
    NeqStr,
    EqBool,
    NeqBool,

    // リスト
    ListNew(usize),         // スタックから n 個取り出しリスト構築
    ListEmpty,              // 空リスト push

    // 関数呼び出し
    CallBuiltin(String, usize),  // 組込み関数名, 引数数

    // 制御
    Pop,                    // スタック top を破棄
    Halt,                   // 実行終了
}
```

### コード生成例

```surtr
num = 10
num2 = 5
print(num + num2)
```

```
定数プール: [Int(10), Int(5)]
ローカルスロット: { num_id0: 0, num2_id0: 1 }

バイトコード:
  LoadConst 0           // 10
  StoreLocal 0          // num = 10
  Pop                   // Bind は Unit。スタッククリア
  LoadConst 1           // 5
  StoreLocal 1          // num2 = 5
  Pop                   // Bind は Unit
  LoadLocal 0           // num
  LoadLocal 1           // num2
  AddInt                // num + num2 = 15
  CallBuiltin "print" 1 // print(15)
  Pop                   // print の戻り値 Unit を破棄
  Halt
```

### リストのコード生成例

```surtr
nums = [1, 2, 3]
print(nums)
```

```
定数プール: [Int(1), Int(2), Int(3)]
ローカルスロット: { nums_id0: 0 }

バイトコード:
  LoadConst 0           // 1
  LoadConst 1           // 2
  LoadConst 2           // 3
  ListNew 3             // [1, 2, 3]
  StoreLocal 0          // nums = [1, 2, 3]
  Pop
  LoadLocal 0           // nums
  CallBuiltin "print" 1 // print([1, 2, 3])
  Pop
  Halt
```

-----

## 10. エラー報告

### エラーの種別

| フェーズ | エラー種別 | 例 |
|---|---|---|
| parse | `ParseError` | 構文エラー、不正なトークン |
| resolve | `ResolveError` | 未定義変数の参照 |
| typecheck | `TypeError` | 型不一致、空リストの型推論不能 |
| codegen | `CodegenError` | 内部エラー（通常は発生しない） |
| execute | `RuntimeError` | ゼロ除算、スタックアンダーフロー |

### エラー出力形式

v6 spec の方針に従い、人間向け（ariadne）と機械向け（JSON）の両方を出力する。

人間向け:

```
Error: TypeMismatch
  --> main.surtr:2:14
  |
2 | bad: Int = "bad type"
  |            ^^^^^^^^^^ expected Int, got String
```

機械向け:

```json
{
  "errors": [{
    "kind": "TypeMismatch",
    "phase": "typecheck",
    "line": 2,
    "column": 14,
    "span": [13, 23],
    "expected": "Int",
    "got": "String",
    "hint": "The type annotation requires Int but the value is String"
  }]
}
```

### 即時停止（IDEA-003 確定事項）

最初のエラーでコンパイル停止。エラー収集・毒型は MVP では不採用。

-----

## 11. 実装順序

IDEA-003 で確定した3層構造のうち、MVP では層1（ブートストラップ）のみ。

### フェーズ 1: 足場作り

| タスク | 成果物 |
|---|---|
| プロジェクト構成 | Cargo workspace: `surtr-parse`, `surtr-resolve`, `surtr-check`, `surtr-codegen`, `surtr-vm`, `surtr-cli` |
| Ast 定義 | `Ast`, `Lit`, `AstPattern`, `AstTy`, `BinOp` の Enum |
| Value 定義 | ランタイムの `Value` enum |
| Opcode 定義 | `Opcode` enum |

### フェーズ 2: ① スクリプトレベル

| タスク | 入力 | 出力 |
|---|---|---|
| パーサ（Int リテラル + Bind + Var） | `num = 10` | `Ast::Bind(Var("num"), Lit::Int(10))` |
| resolve（シャドウイング解決） | `Ast` | `Resolved`（unique_id 付き） |
| typecheck（Int のみ） | `Resolved` | `TypedNode`（全ノードに `Ty::Int`） |
| codegen | `TypedNode` | `Bytecode`（LoadConst, StoreLocal, Halt） |
| VM（最小ループ） | `Bytecode` | 実行 |
| `print` 組込み | `print(num)` | stdout に出力 |

### フェーズ 3: ② 型検査

| タスク | 入力 | 出力 |
|---|---|---|
| 型注釈パース | `num: Int = 10` | `AstPattern::Annotated` |
| AstTy → Ty 解決 | `AstTy::Named("Int")` | `Ty::Int` |
| 型不一致エラー | `bad: Int = "hello"` | `TypeError` |
| ariadne 統合 | `TypeError` | ソース位置付きエラー表示 |

### フェーズ 4: ③ プリミティブ型

| タスク | 入力 | 出力 |
|---|---|---|
| Float パース | `3.14` | `Lit::Float(3.14)` |
| String パース | `"hello"`, `'world'` | `Lit::Str("hello")` |
| Boolean パース | `True`, `False` | `Lit::Bool(true)` |
| Symbol パース | `` `ok` `` | `Lit::Symbol("ok")` |
| Unit パース | `()` | `Lit::Unit` |
| 各型の型検査追加 | — | `Ty::Float`, `Ty::Str`, `Ty::Bool`, `Ty::Symbol`, `Ty::Unit` |
| `to_string` 組込み | `to_string(42)` | `Value::Str("42")` |

### フェーズ 5: ④ 組込み関数

| タスク | 入力 | 出力 |
|---|---|---|
| 二項演算パース | `num + 1` | `Ast::BinOp(Add, ...)` |
| 型特化 Opcode 生成 | `Int + Int` → `AddInt` | 型に応じた Opcode |
| Float 演算 | `3.14 + 1.0` | `AddFloat` |
| 比較演算 | `1 == 1` | `EqInt` → `Value::Bool(true)` |

### フェーズ 6: ⑤ リスト

| タスク | 入力 | 出力 |
|---|---|---|
| リストリテラルパース | `[1, 2, 3]` | `Ast::List(...)` |
| 空リストパース | `[]` | `Ast::List(vec![])` |
| 要素型の一致検査 | `[1, "a"]` → TypeError | — |
| 空リスト型推論 | `x: [Int] = []` → ok, `x = []` → error | — |
| `[T]` 型注釈パース | `[Int]` | `AstTy::ListOf(Named("Int"))` |
| ListNew / ListEmpty Opcode | — | リストの構築 |
| リストの `to_string` / `print` | `print([1, 2, 3])` | `[1, 2, 3]` |

-----

## 12. 検討課題（MVP 後）

MVP 完了後に着手する課題。優先度順。

| 課題 | 関連 IDEA | 備考 |
|---|---|---|
| `if` / `match` / `cond` | — | 制御構造。マクロ前提だが、MVP 後は組込みとして先行実装も可 |
| `def`（関数定義） | — | トップレベル関数。`\|>` の前提 |
| `\|>` パイプライン | — | map / bind 自動選択 |
| `defstruct` / `enum` | — | ユーザ定義型 |
| `result do` / `=?` | IDEA-001 | MatchContext |
| マクロシステム | IDEA-001, 002 | `expand` フェーズの実装 |
| モジュール / `import` | IDEA-003 | 依存解決 |
| GC | — | 参照カウントまたはトレーシング GC |
| 永続データ構造 | — | イミュータブルリスト |
| インクリメンタルコンパイル | IDEA-003 | 変更範囲の最小化 |
| 標準ライブラリプリコンパイル | IDEA-003 | キャッシュ導入 |