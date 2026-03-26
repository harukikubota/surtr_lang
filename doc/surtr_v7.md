# Surtr 要件定義書

> **V7** — V6 言語仕様 + コンパイラ設計決定（IDEA-001〜003）+ MVP 実装仕様を統合
>
> 変更履歴:
> - V6: 言語仕様 draft 0.6
> - V7: コンパイラ設計（フェーズ分離・マクロ展開・依存解決）、MVP 実装仕様、ランタイム設計を追加

-----

## 目次

**Part I: 言語仕様**
1. [言語思想](#1-言語思想)
2. [言語設計](#2-言語設計)
3. [言語仕様](#3-言語仕様)
4. [文法](#4-文法)
5. [サンプル](#5-サンプル)
6. [開発ツールと Claude 協調](#6-開発ツールと-claude-協調)
7. [段階的導入カリキュラム](#7-段階的導入カリキュラム)

**Part II: コンパイラ設計**
8. [コンパイラアーキテクチャ](#8-コンパイラアーキテクチャ)
9. [AST 設計 — フェーズ分離方式](#9-ast-設計--フェーズ分離方式)
10. [`=` / `=?` 演算子の設計](#10---演算子の設計)
11. [マクロ展開フェーズ](#11-マクロ展開フェーズ)
12. [コンパイル順序と依存解決](#12-コンパイル順序と依存解決)
13. [モジュール変数 — コンパイル時マクロ間共有状態](#13-モジュール変数--コンパイル時マクロ間共有状態)
14. [エラー報告](#14-エラー報告)

**Part III: ランタイム設計**
15. [VM アーキテクチャ](#15-vm-アーキテクチャ)
16. [バイトコード設計](#16-バイトコード設計)

**Part IV: MVP 実装仕様**
17. [MVP スコープ](#17-mvp-スコープ)
18. [MVP 実装順序](#18-mvp-実装順序)

**付録**
- [付録 A: コンパイラ主要型（Rust 実装）](#付録-a-コンパイラ主要型rust-実装)
- [付録 B: コンパイラ実装方針](#付録-b-コンパイラ実装方針)
- [付録 C: 設計検討課題](#付録-c-設計検討課題)
- [付録 D: 不採用の機能](#付録-d-不採用の機能)
- [付録 E: 標準モジュール関数（追加候補）](#付録-e-標準モジュール関数追加候補)
- [付録 F: Haskell からの導入候補（仕様検討中）](#付録-f-haskell-からの導入候補仕様検討中)

-----

# Part I: 言語仕様

> Part I は V6 の言語仕様をそのまま収録する。
> 変更点がある場合は各セクション冒頭に `[V7 変更]` で注記する。

（※ Part I の本文は `Surtr_v6` を参照。本ドキュメントではコンパイラ設計以降を収録する。）

-----

# Part II: コンパイラ設計

## 8. コンパイラアーキテクチャ

### フェーズチェーン

各フェーズが独自の Enum を持ち、`Enum A → Result<Enum B>` の変換チェーンでコンパイルを進める。Surtr のパイプライン（`context<A> |> (A -> context<B>)`）と同じ構造がコンパイラ実装にそのまま現れる。

```
Source → parse → Ast → expand → Ast → resolve → Resolved → typecheck → Typed → codegen → Bytecode → execute
```

```rust
fn parse(src: Source)        -> Result<Ast>
fn expand(ast: Ast)          -> Result<Ast>       // Ast → Ast（MacroCall を除去）
fn resolve(ast: Ast)         -> Result<Resolved>   // MacroCall が残っていればエラー
fn typecheck(r: Resolved)    -> Result<Typed>
fn codegen(t: Typed)         -> Result<Bytecode>
```

フェーズの性質:

- フェーズのスキップ・順序入れ替えが型レベルで防止される
- フェーズ単位で stub 実装・テスト・差し替えが可能
- エラー発生フェーズが型から自明
- Rust の match 網羅性検査により追加漏れがコンパイルエラーになる

### エラー回復

即時停止。最初のエラーでコンパイル停止する。エラー収集・毒型は不採用。

-----

## 9. AST 設計 — フェーズ分離方式

### 設計方針

型情報は TypeChecker の出力である `Typed` フェーズで初めて確定する。パーサ出力（Ast）は型情報を一切持たない。各フェーズ間の変換は `Enum A → Result<Enum B>` の関数で行う。

### Phase 1: Ast（パーサ出力）

型情報を一切持たない。ユーザが書いた型注釈は `AstTy` として構文的に保持するだけ。`Annotated` はパーサが「ユーザがここに型を書いた」という事実を記録するだけであり、型の妥当性は後のフェーズが判断する。

```rust
pub enum Ast {
    Lit(Span, Lit),
    Var(Span, Symbol),
    App(Span, Box<Ast>, Vec<Ast>),
    Pipe(Span, Box<Ast>, Box<Ast>),
    Block(Span, Vec<Ast>),
    Lambda(Span, Vec<AstPattern>, Box<Ast>),
    Bind(Span, AstPattern, Box<Ast>),
    TestBind(Span, AstPattern, Box<Ast>),
    Capture(Span, Box<Ast>),
    CaptureArg(Span, u8),
    FuncRef { span: Span, path: Vec<Symbol> },
    MacroCall(Span, Symbol, Vec<MacroArg>),
    BinOp(Span, BinOp, Box<Ast>, Box<Ast>),
    List(Span, Vec<Ast>),
    Match(Span, Box<Ast>, Vec<(Ast, Ast)>),
    Unwrap(Span, Box<Ast>),
}

pub enum AstPattern {
    Wildcard(Span),
    Var(Span, Symbol),
    Annotated(Span, Symbol, AstTy),
    Lit(Span, Lit),
    Constructor(Span, Symbol, Vec<AstPattern>),
    Spread(Span, Symbol),
}

pub enum AstTy {
    Named(Span, Symbol),
    Generic(Span, Symbol, Vec<AstTy>),
    Func(Span, Vec<AstTy>, Box<AstTy>),
    TypeVar(Span, Symbol),
    ListOf(Span, Box<AstTy>),
}

pub struct Span { pub start: usize, pub end: usize }
```

`Span` を全ノードに持たせることで、後続フェーズのエラー報告でソース位置を参照できる。

### Phase 2: Resolved（名前解決後）

変数参照を一意な ID に解決する。シャドウイングの `x_id0`, `x_id1` もここで付与する。モジュールパス・Enum バリアントの解決もこのフェーズ。

```rust
pub struct ResolvedId {
    pub name: Symbol,
    pub unique_id: u32,
    pub span: Span,
}

pub enum Resolved {
    Lit(Span, Lit),
    Var(Span, ResolvedId),
    App(Span, Box<Resolved>, Vec<Resolved>),
    Pipe(Span, Box<Resolved>, Box<Resolved>),
    Block(Span, Vec<Resolved>),
    Lambda(Span, Vec<ResolvedPattern>, Box<Resolved>),
    Bind(Span, ResolvedPattern, Box<Resolved>),
    TestBind(Span, ResolvedPattern, Box<Resolved>),
    BinOp(Span, BinOp, Box<Resolved>, Box<Resolved>),
    List(Span, Vec<Resolved>),
    Match(Span, Box<Resolved>, Vec<(Resolved, Resolved)>),
    // ...
}

pub enum ResolvedPattern {
    Wildcard(Span),
    Var(ResolvedId),
    Annotated(ResolvedId, AstTy),   // 型注釈はまだ AstTy のまま
    Lit(Span, Lit),
    Constructor(ResolvedId, Vec<ResolvedPattern>),
    Spread(ResolvedId),
}
```

シャドウイングの解決例:

```
num = 10         → ResolvedId { name: "num", unique_id: 0 }
num = num + 1    → LHS: unique_id: 1, RHS の num: unique_id: 0
```

### Phase 3: Typed（型検査後）

全ノードに確定した `Ty` が付く。ラッパー方式（`TypedNode` + `TypedInner`）を採用する。

```rust
pub struct TypedNode {
    pub ty: Ty,
    pub span: Span,
    pub node: TypedInner,
}

pub enum TypedInner {
    Lit(Lit),
    Var(ResolvedId),
    App(Box<TypedNode>, Vec<TypedNode>),
    Pipe(Box<TypedNode>, Box<TypedNode>),
    Block(Vec<TypedNode>),
    Lambda(Vec<TypedPattern>, Box<TypedNode>),
    Bind(TypedPattern, Box<TypedNode>),
    TestBind(TypedPattern, Box<TypedNode>),
    BinOp(BinOp, Box<TypedNode>, Box<TypedNode>),
    List(Vec<TypedNode>),
    Match(Box<TypedNode>, Vec<(TypedNode, TypedNode)>),
    // ...
}

pub enum TypedPattern {
    Wildcard(Ty),
    Var(Ty, ResolvedId),
    Lit(Ty, Lit),
    Constructor(Ty, ResolvedId, Vec<TypedPattern>),
    Spread(Ty, ResolvedId),
}
```

ラッパー方式を採用する理由:

- `node.ty` で任意ノードの型を統一的に取得できる
- codegen で子ノードの型にアクセスする際にネストした match が不要
- `Pipe` の左辺型を参照する場面（map/bind 判定）で `lhs.ty` と直接書ける
- LSP の hover 実装が `node.ty` を返すだけで済む

型注釈（`Annotated`）は Typed フェーズで消滅する。TypeChecker が検査を完了した時点で役割は終わり、全ての変数が `TypedPattern::Var(Ty, ResolvedId)` に統一される。エラーメッセージで「ユーザが書いた型」を参照する必要がある場合は、TypeChecker がエラー生成時に `Resolved` の `Annotated` を参照する。

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

### 型注釈の照合フロー

```
Ast:      Bind(Annotated("num", AstTy::Named("Int")), Lit::Int(10))
                ↓ resolve
Resolved: Bind(Annotated(ResolvedId("num", 0), AstTy::Named("Int")), Lit::Int(10))
                ↓ typecheck
          1. AstTy::Named("Int") → Ty::Int に解決
          2. RHS の Lit::Int(10) → Ty::Int に解決
          3. 両者を照合 → Ok
          4. TypedNode { ty: Ty::Unit, node: Bind(Var(Ty::Int, ..), Lit(Ty::Int, ..)) }
```

-----

## 10. `=` / `=?` 演算子の設計

### 関数的な側面

`=` / `=?` は実行コンテキスト（ローカル変数テーブル）への書き込み副作用を持つ操作であり、値を返さない（Unit）。式ではなく文に近い性質を持つ。書き込みがトップレベルに集約されることで、コードの各行を見たときに「ここで変数が生まれる」ことが視覚的に明白になる。

```
行の役割が一目でわかる:
  x = expr        → 変数 x がここで生まれる
  x =? expr       → 変数 x がここで生まれる（失敗しうる）
  expr            → 値を計算する（書き込みなし）
  expr |> expr    → 値を変換する（書き込みなし）
```

| 性質 | `=` | `=?` |
|---|---|---|
| 戻り値 | Unit | Unit |
| 副作用 | 実行コンテキストへの書き込み | 同左 + 失敗時の早期リターン |
| 結合性 | 非結合（ネスト不可） | 同左 |
| 優先度 | `=` と `=?` で同一 | 同左 |

### ネスト禁止

`=` と `=?` は同じ結合優先度を持ち、LHS にも RHS にも再出現できない。

```surtr
# NG: コンパイルエラー
x = y = 10
a =? b =? parse("10")

# OK: 独立した行として書く
y = 10
x = y

# OK: 同時束縛はアズパターンで表現
x @ y = calc_x()
```

パーサでの実装: RHS をパースする際に `=` / `=?` を演算子として認識しないことで、構文レベルでネストを禁止する。

### ブロック末尾が Bind の場合

戻り値の型が合っていればコンパイルは通す。ただし束縛した変数が使われないため Warning を出す。`_` プレフィックスで Warning を抑制できる（Rust / Elixir 共通の慣習）。

```surtr
def do_something() {
  x = compute()       # Warning: Unused variable `x`
}

def get_value() -> Int {
  x = 42               # Bind は Unit → Int を期待 → 型エラー
}
# 正しくは:
def get_value() -> Int {
  x = 42
  x                    # 末尾に式として置く
}
```

### Typed AST での表現

```rust
// x = 42
TypedNode {
    ty: Ty::Unit,                                       // Bind 自体は Unit
    node: TypedInner::Bind(
        TypedPattern::Var(Ty::Int, ResolvedId { .. }),  // x: Int
        Box::new(TypedNode {
            ty: Ty::Int,                                // RHS: Int
            node: TypedInner::Lit(Lit::Int(42)),
        }),
    ),
}
```

### コード生成での戻り値の扱い

関数（組込み関数含む）は常に戻り値をスタックに積んで終了する。呼び出し側が文脈に応じて消費する。

| 文脈 | 戻り値の扱い |
|---|---|
| `= expr` の RHS | `StoreLocal` で変数に格納 |
| 文の途中（最終行でない） | `Pop` で破棄 |
| ブロック最終行 | スタックに残す（関数の戻り値になる） |

-----

## 11. マクロ展開フェーズ

### タイミング

parse 直後。入出力は同じ `Ast` 型。

`expand` は「Ast の書き換え」であり新しいフェーズの Enum を生まない。展開済みの `Ast` に `MacroCall` が残っていないことを `resolve` が入口で検査する（残っていればエラー）。

### 走査戦略: トップダウン + コンテキスト伝搬

```rust
struct ExpandContext {
    block_type: BlockType,       // Expr / Cond / Match
    match_context: MatchContext, // Bind / Test
}
```

構文ノード（`Match` / `Bind` / `TestBind`）を走査する際にコンテキストを適切に設定し、`MacroCall` の展開時に `env.with_context(ctx)` でマクロに注入する。これは context matrix の「AST解釈/マクロ」列そのものである。

### 引数の展開順序

マクロの引数に別のマクロがある場合、内側から外側へ展開される（関数適用の評価順序と一致）。

```
A(B(arg))
  → B が arg を受け取り展開 → B_expanded
  → A が B_expanded を受け取り展開 → A_expanded
```

マクロ呼び出し前に引数内のマクロが展開済みになる。ただしマクロ自身が `unquote_for_*` で埋め込んだ AST は、展開結果の再帰走査で展開される。

### expand の実装

```rust
fn expand(
    ast: Ast,
    env: &MacroEnv,
    ctx: ExpandContext,
    depths: &mut HashMap<Span, u32>,
) -> Result<Ast> {
    match ast {
        Ast::MacroCall(span, name, args) => {
            check_depth(span, depths)?;
            // 1. 引数内のマクロを先に展開（内側から外側へ）
            let expanded_args = args.into_iter()
                .map(|a| expand(a, env, ctx, depths))
                .collect::<Result<Vec<_>>>()?;
            // 2. マクロ本体を実行（展開済み引数を渡す）
            let macro_env = env.with_context(ctx);
            let result = invoke_macro(name, expanded_args, &macro_env)?;
            // 3. 展開結果を再帰走査
            expand(result, env, ctx, depths)
        }
        Ast::Match(span, scrutinee, arms) => {
            let expanded_scrutinee = expand(*scrutinee, env, ctx, depths)?;
            let expanded_arms = arms.into_iter()
                .map(|(lhs, rhs)| {
                    let lhs = expand(lhs, env, ctx.with_block_type(Match), depths)?;
                    let rhs = expand(rhs, env, ctx.with_block_type(Expr), depths)?;
                    Ok((lhs, rhs))
                })
                .collect::<Result<_>>()?;
            Ok(Ast::Match(span, Box::new(expanded_scrutinee), expanded_arms))
        }
        Ast::TestBind(span, pat, rhs) => {
            let pat = expand(pat, env, ctx.with_match_context(Test), depths)?;
            let rhs = expand(rhs, env, ctx.with_block_type(Expr), depths)?;
            Ok(Ast::TestBind(span, pat, Box::new(rhs)))
        }
        _ => walk_with_ctx(ast, env, ctx, depths, expand),
    }
}
```

### 再帰展開と深度制限

展開元のソース位置（Span）単位で深度を追跡する。異なるソース位置のマクロは独立にカウントされる。

| 項目 | 仕様 |
|---|---|
| 深度追跡の単位 | 展開元の Span |
| 上限 | 128（初期値、設定変更可能） |
| 上限到達時 | コンパイルエラー |

自己再帰（`cond` → `if` + `cond`）は元の `cond` のソース位置を引き継ぎ、同一カウンタで追跡する。

-----

## 12. コンパイル順序と依存解決

### コンパイル 3 層

```
層1: ブートストラップ（Rust 実装）
  組み込みマクロ・組み込み関数
  defmacro, if, match, cond, result do, ++, def, defstruct, enum, deferror ...

層2: 標準ライブラリ（Surtr 記述）
  層1 のマクロ・関数に依存
  List, Map, String, Generator, Show, Eq, Ord ...

層3: ユーザ定義
  マクロ定義 → 関数定義
  ユーザが import するモジュール間の依存を解決
```

### 依存解決の粒度

ファイル単位ではなく関数/マクロ/型定義の単位で解決する。これにより循環参照（関数同士）が許可される。

#### Callable 条件

| 種別 | Callable の条件 | 循環参照 |
|---|---|---|
| 関数 | シグネチャ（入出力の型）が確定 | 許可 |
| マクロ | 本体がコンパイル済み + 依存マクロが全て解決済み | 定義の循環はエラー |
| 型定義 | フィールドの型が全て解決済み | 依存先の型が解決済みなら可 |

関数の循環参照が許可される理由: シグネチャが確定していれば本体のコンパイルは後回しにできる。

マクロの定義循環がエラーになる理由: マクロは展開時に本体を実行するため、本体が完全にコンパイル済みである必要がある。双方が相手を待つ状態は解決不能。

注意: マクロ定義の循環（コンパイル時にどちらを先にコンパイルするかの問題）と、マクロ展開の無限再帰（展開実行時の問題）は別の問題。後者は深度上限（128）で検出する。

#### CallableStatus の型表現

```rust
enum CallableStatus {
    FuncCallable {
        signature: FuncSignature,
    },
    MacroCallable {
        compiled_body: MacroBody,
        resolved_deps: Vec<MacroName>,
    },
    TypeDefined {
        type_info: TypeInfo,
    },
}
```

### 依存解決アルゴリズム

固定点ループ。1 周して何も解決できなかったら未解決エラー。

```rust
fn resolve_all(units: Vec<CompileUnit>) -> Result<CompileOrder> {
    let mut resolved: HashMap<Symbol, CallableStatus> = HashMap::new();
    let mut pending: Vec<CompileItem> = collect_all_items(&units);

    // 層1: ブートストラップを resolved に登録
    resolved.extend(bootstrap());

    loop {
        let mut progress = false;
        let mut still_pending = Vec::new();

        for item in pending {
            match try_resolve(&item, &resolved) {
                Ok(status) => {
                    resolved.insert(item.name, status);
                    progress = true;
                }
                Err(_unresolved_deps) => {
                    still_pending.push(item);
                }
            }
        }

        if still_pending.is_empty() {
            break;
        }
        if !progress {
            return Err(unresolvable_deps_error(&still_pending, &resolved));
        }
        pending = still_pending;
    }

    Ok(build_order(&resolved))
}
```

### ファイル内の順序

同一ファイル内にマクロ定義と関数定義が混在する場合、マクロ定義を先にコンパイルする。

```rust
fn compile_unit(unit: CompileUnit, env: Env) -> Result<Bytecode> {
    let ast = unit.ast;
    let (macro_defs, other_items) = partition_macros(ast);
    let macro_env = compile_macros(macro_defs, &env)?;
    let expanded = expand(other_items, &macro_env)?;
    let resolved = resolve(expanded)?;
    let typed = typecheck(resolved)?;
    let bytecode = codegen(typed)?;
    Ok(bytecode)
}
```

### 標準ライブラリのプリコンパイル

段階的に導入する。

| フェーズ | 方式 | 理由 |
|---|---|---|
| 初期開発 | 毎回コンパイル | 標準ライブラリ自体が頻繁に変わる |
| 標準ライブラリ安定後 | バイトコード + シグネチャキャッシュ | 変更頻度が下がり、キャッシュの恩恵が出る |

キャッシュ導入時の構成:

```
surtr init（初回 or 標準ライブラリ更新時）
  → 層2 コンパイル
  → stdlib.bytecode  （ランタイム用: バイトコード）
  → stdlib.sig       （コンパイラ用: 型シグネチャ + マクロ定義）

surtr build main.surtr（通常のビルド）
  → 層1 ロード
  → stdlib.sig ロード（コンパイルせず）
  → 層3 コンパイル（stdlib.sig 参照）
  → 層3 バイトコード + stdlib.bytecode をリンク
```

```rust
struct StdlibSignature {
    functions: HashMap<Symbol, FuncSignature>,    // シグネチャのみ
    macros: HashMap<Symbol, MacroCallable>,       // コンパイル済み本体を含む
    types: HashMap<Symbol, TypeInfo>,
    traits: HashMap<Symbol, TraitInfo>,
}
```

キャッシュの無効化条件: 標準ライブラリのソースハッシュ、ブートストラップバージョン、コンパイラバージョンのいずれかが変わった場合。

-----

## 13. モジュール変数 — コンパイル時マクロ間共有状態

> 本章はユーザによる拡張を支える言語設計であり、最低限動かすレベルでは不要。
> 仕様書とメモの中間として扱う。

### 概要

宣言的 DSL のためにマクロ間で共有する状態を「モジュール変数」として提供する。呼び出し元のローカル変数への書き込みは禁止（Elixir の `var!` に相当する機構は不採用）。モジュール変数はコンパイル時にのみ存在し、ランタイムには残らない。

### モジュール変数の定義

`@ModuleVar(Pairs)` のレコード形式で定義する。プリミティブ型単独は不可。named access を強制することでモジュール変数の参照であることが分かりやすくなる。

```surtr
defmod TestDSL {
  @ModuleVar(cases: [TestCase], current_path: [String])

  defmacro describe(env: Env<Self>, label: String, block) -> Result<AST> {
    env.current_path = [eval_for_expr(label)]
    expanded = unquote_for_expr(block)
    build_test_suite(env.cases)
  }

  defmacro it(env: Env<Self>, desc: String, block) -> Result<AST> {
    env.cases = [TestCase { name: desc, body: block }, ..env.cases]
    Ok(Unit)
  }
}
```

### `Env<Self>` とアクセス

- `Env<ModName>` は高階型として定義する
- `env.macro_var.field` のフルパスを `env.field` にレンズで省略できる
- `Self` がモジュール自身を指すことで、マクロが自モジュールの `@ModuleVar` にのみアクセスできることが型レベルで保証される

### スコープの原則

| 書き込み先 | 許可 | 用途 |
|---|---|---|
| 呼び出し元のローカル変数 | 禁止 | — |
| 同モジュールのモジュール変数 | 許可 | DSL・メタプログラミング |
| 他モジュールのモジュール変数 | 禁止 | モジュール境界を超えない |

### `@` の解決: コンテキストで分岐

| コンテキスト | `@` の解釈 | 例 |
|---|---|---|
| トップレベル | 宣言的マクロの接頭辞 | `@ModuleVar(...)`, `@test` |
| Expr 内 | レンズの接頭辞（必ず `@.` で始まる） | `@.name`, `@.address.city` |

パーサが判別できる根拠は `@` の直後のトークン。構文的に排他。

```
@.field        → 必ず Expr 内。レンズ
@Ident(...)    → 必ずトップレベル。宣言的マクロ
@ident         → 必ずトップレベル。宣言的マクロ（引数なし）
```

### コンテキスト判定

```
トップレベル
├── モジュール宣言 (defmod, module)
├── 型定義 (defstruct, enum, newtype, type)
├── 宣言的マクロ (@test, @ModuleVar, @derive)
├── impl ブロック
│   └── def → Expr コンテキストに切り替わる
└── 裸の Expr → 暗黙の main に包まれる

Expr コンテキスト
├── = / =? (束縛)
├── |> (パイプライン)
├── @.field (レンズ)
├── match / if / cond
└── 関数呼び出し
```

### モジュール変数の初期化

デフォルト値の有無でフィールドの型が決まる。

| 宣言 | 実際の型 | 初期値 |
|---|---|---|
| `field: T = default` | `T` | `default` |
| `field: T` | `Result<T>` | `Err(NoneError)` |

デフォルト値なしのフィールドは `Result<T>` に自動昇格する。これは `defstruct` の `String?`（`Result<String, NoneError>` の糖衣）と同じパターン。

### イミュータブル原則との整合性

| 層 | イミュータブル原則 | 理由 |
|---|---|---|
| ランタイム（Surtr コード） | 厳守 | 並列安全性、巻き戻しコスト 0 |
| コンパイル時（マクロ環境） | 例外として許容 | ビルダーパターン。シングルスレッドで逐次実行 |

`env.field = ...` はローカル変数への束縛ではなく `env` オブジェクトのフィールド更新。`=` 演算子の「実行コンテキストへの書き込み」の拡張として「マクロ環境への書き込み」に位置づける。`=` の規則（Unit を返す、非結合）にそのまま準拠する。

### モジュール変数の生存期間

トップレベルのマクロコールが終了したらモジュール変数は破棄される。

-----

## 14. エラー報告

### エラーの種別

| フェーズ | エラー種別 | 例 |
|---|---|---|
| parse | `ParseError` | 構文エラー、不正なトークン |
| expand | `MacroError` | マクロ展開エラー、深度上限到達 |
| resolve | `ResolveError` | 未定義変数の参照、未解決の MacroCall |
| typecheck | `TypeError` | 型不一致、空リストの型推論不能 |
| codegen | `CodegenError` | 内部エラー（通常は発生しない） |
| execute | `RuntimeError` | ゼロ除算、スタックアンダーフロー |

### エラー出力形式

人間向け（ariadne）と機械向け（JSON）の両方を出力する。

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

### Warning

```json
{
  "kind": "UnusedVariable",
  "severity": "Warning",
  "variables": ["x"],
  "line": 3,
  "hint": "Prefix with `_` to suppress this warning"
}
```

-----

# Part III: ランタイム設計

## 15. VM アーキテクチャ

### 概要

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
    stack: Vec<Value>,
    locals: Vec<Value>,
    constants: Vec<Value>,
    bytecode: Vec<Opcode>,
    pc: usize,
    builtins: HashMap<String, BuiltinFn>,
}

pub type BuiltinFn = fn(&mut VM, Vec<Value>) -> Result<Value, RuntimeError>;
```

### ローカル変数の管理

codegen がシャドウイング解決済みの `unique_id` をスロット番号にマッピングする。

```
Surtr コード:     num = 10; num = num + 1
Resolved:         num_id0 = 10; num_id1 = num_id0 + 1
codegen マッピング: num_id0 → slot 0, num_id1 → slot 1
```

### GC

MVP では GC を実装しない。`Value` は Rust の所有権で管理する。MVP 後に参照カウント（RC）またはトレーシング GC を導入する。

-----

## 16. バイトコード設計

### Opcode 一覧

```rust
#[derive(Debug, Clone)]
pub enum Opcode {
    // 定数・変数
    LoadConst(usize),       // 定数プール[idx] をスタックに push
    LoadLocal(usize),       // locals[slot] をスタックに push
    StoreLocal(usize),      // スタック top を locals[slot] に格納

    // 算術（Int）
    AddInt, SubInt, MulInt, DivInt, ModInt,

    // 算術（Float）
    AddFloat, SubFloat, MulFloat, DivFloat,

    // 比較
    EqInt, NeqInt, LtInt, GtInt, LteInt, GteInt,
    EqFloat, NeqFloat, LtFloat, GtFloat, LteFloat, GteFloat,
    EqStr, NeqStr,
    EqBool, NeqBool,

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
  Pop                   // Bind は Unit
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

# Part IV: MVP 実装仕様

## 17. MVP スコープ

### 目標

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

### MVP で実装するもの

| 段階 | 内容 |
|---|---|
| ① スクリプトレベル | 変数束縛（`=`）、`print` 関数、トップレベル式の暗黙 main |
| ② 型検査 | 型注釈（`num: Int = 10`）、型不一致のコンパイルエラー |
| ③ プリミティブ型 | `Int`, `Float`, `String`, `Boolean`, `Symbol`, `Unit` |
| ④ 組込み関数 | 算術演算、比較演算、`to_string`、`print` |
| ⑤ リスト | リストリテラル `[1, 2, 3]`、空リスト `[]`、型推論 `[Int]` |

### MVP で実装しないもの

| 機能 | 理由 |
|---|---|
| マクロシステム（`defmacro`） | ユーザ拡張。後回し |
| `if` / `match` / `cond` | マクロ前提の設計 |
| `def`（関数定義） | `def` はマクロ |
| `\|>` パイプライン | 関数定義が必要 |
| `result do` / `=?` | MatchContext 含め後回し |
| `defstruct` / `enum` | 型定義は後回し |
| モジュール / `import` | 後回し |
| レンズ（`@.field`） | 構造体が必要 |

### MVP のフェーズチェーン

マクロ展開フェーズをスキップする。

```
Source → parse → Ast → resolve → Resolved → typecheck → Typed → codegen → Bytecode → execute
```

### MVP のプリミティブ型

| 型 | Rust 表現 | リテラル例 |
|---|---|---|
| `Int` | `i64` | `42`, `-1`, `0` |
| `Float` | `f64` | `3.14`, `-0.5` |
| `String` | `String` | `"hello"`, `'world'` |
| `Boolean` | `bool` | `True`, `False` |
| `Symbol` | `String` | `` `ok` ``, `` `error` `` |
| `Unit` | `()` | `()` |

### MVP の組込み関数

| 関数 | シグネチャ | 説明 |
|---|---|---|
| `print` | `($A) -> Unit` | 任意の型を表示。内部で `to_string` を呼ぶ |
| `to_string` | `($A) -> String` | 任意の型を文字列に変換 |

### MVP の二項演算子

| 演算子 | 対応する型 | 戻り値 |
|---|---|---|
| `+` `-` `*` `/` | `(Int, Int) -> Int`, `(Float, Float) -> Float` | 算術 |
| `%` | `(Int, Int) -> Int` | 剰余 |
| `==` `!=` | `($A, $A) -> Boolean` | 等値比較 |
| `<` `>` `<=` `>=` | `(Int, Int) -> Boolean`, `(Float, Float) -> Boolean` | 比較 |

### MVP のリスト

| リテラル | 推論結果 | 規則 |
|---|---|---|
| `[1, 2, 3]` | `[Int]` | 全要素が `Int` |
| `["a", "b"]` | `[String]` | 全要素が `String` |
| `[1, "a"]` | TypeError | 要素型が不一致 |
| `[]` | `[Ty::Var(?)]` | 型注釈なし → コンパイルエラー |
| `x: [Int] = []` | `[Int]` | 型注釈で確定 |

### MVP の構文規則

```
program     = stmt*
stmt        = bind | expr
bind        = pattern "=" expr
pattern     = IDENT ":" type | IDENT | "_"
expr        = expr binop expr | IDENT "(" args ")" | "[" list_items "]" | literal | IDENT
literal     = INT | FLOAT | STRING | BOOL | SYMBOL | "()"
type        = IDENT | "[" type "]"
```

-----

## 18. MVP 実装順序

### フェーズ 1: 足場作り

| タスク | 成果物 |
|---|---|
| プロジェクト構成 | Cargo workspace: `surtr-parse`, `surtr-resolve`, `surtr-check`, `surtr-codegen`, `surtr-vm`, `surtr-cli` |
| Ast 定義 | `Ast`, `Lit`, `AstPattern`, `AstTy`, `BinOp` の Enum（MVP サブセット） |
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
| Float / String / Boolean / Symbol / Unit パース | 各リテラル | 対応する `Lit` バリアント |
| 各型の型検査追加 | — | `Ty::Float`, `Ty::Str` 等 |
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

# 付録

## 付録 A〜F

> 付録 A〜F は V6 からそのまま引き継ぐ。`Surtr_v6` を参照。

-----

## 付録 G: 検討課題（V7 時点の未解決）

| 課題 | 関連 | 備考 |
|---|---|---|
| `if` / `match` / `cond` の実装 | Part I §4 | マクロ前提だが、組込みとして先行実装も可 |
| `def`（関数定義） | Part I §4 | トップレベル関数。`\|>` の前提 |
| `\|>` パイプライン | Part I §2 | map / bind 自動選択 |
| `defstruct` / `enum` | Part I §4 | ユーザ定義型 |
| `result do` / `=?` | Part II §10 | MatchContext |
| マクロシステム実装 | Part II §11 | `expand` フェーズの実装 |
| モジュール / `import` | Part II §12 | 依存解決 |
| GC | Part III §15 | 参照カウントまたはトレーシング GC |
| 永続データ構造 | Part III §15 | イミュータブルリスト |
| インクリメンタルコンパイル | Part II §12 | 変更範囲の最小化 |
| 標準ライブラリプリコンパイル | Part II §12 | キャッシュ導入 |
| `defmacro` のブートストラップ順序 | Part II §11 | ユーザ定義マクロの処理順序 |
| モジュール変数の初期化タイミング詳細 | Part II §13 | コンパイラ検査の具体実装 |

-----

*Surtr — 既存の妥協を、型で焼き払う。*
