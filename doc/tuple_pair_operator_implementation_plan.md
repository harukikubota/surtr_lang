# `(,)` Pair Constructor Operator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Add the right-associative builtin pair-constructor operator `(,)` without changing tuple runtime representation. To satisfy equality acceptance for its ordinary tuple lowering, add the canonical componentwise `Eq` implementation for arity-2 tuples.

**Architecture:** Declare `(,)` in `lib/bootstrap.srt` as `@builtin def (,)(lhs: $A, rhs: $B) -> ($A, $B)`, and register it as a compiler-resolved builtin operator. Parse its infix spelling directly into the existing two-element tuple-literal AST. Quoted FuncLiteral/capture forms resolve to the same operator descriptor and lower to a two-argument closure only when a callable value is required. All later compiler stages and the VM continue using their existing tuple paths. `lib/types/tuple.srt` owns the arity-2 `Eq`/`Compare` implementations and explanatory module prose, not the `(,)` callable surface.

**Tech Stack:** Rust workspace; Spire parser; Sigil resolver; Scar type checker; Forge/Eldr existing tuple codegen/runtime; Rune fixtures.

**Spec:** The user-approved surface contract in this document’s “Surface 契約” section.

## Global Constraints

- Do not add a VM opcode, runtime builtin, builtin metadata entry, or a new tuple runtime representation.
- Resolve `(,)` directly as a builtin operator; do not declare or resolve an operator trait for it.
- Do not change existing tuple-literal, tuple-pattern, comma-separator, or tuple-Compare semantics.
- Use backtick quote for all FuncLiteral, capture, and pipeline-RHS callable references to `(,)`.
- Keep `(,)` right-associative, weaker than Expr and stronger than Compare.

## 目的

(,) を、常に 2 要素の tuple を構築する右結合の中置演算子として追加する。
既存 tuple literal の別表記・置換にはしない。

本改修は compiler-resolved builtin operator、parser、lowering の追加である。利用者向けの standard-source declaration は次に固定する。

~~~surtr
defmod Bootstrap {
    @builtin def (,)(lhs: $A, rhs: $B) -> ($A, $B)
}
~~~

これは `lib/bootstrap.srt` の compiler surface declaration であり、runtime builtin ID を持つ `BUILTIN_METAS` への追加を意味しない。VM opcode、runtime builtin、operator trait、tuple representation は変更しない。

## Surface 契約

### Standard-source declaration

~~~surtr
@doc """
Construct a 2-tuple from `lhs` and `rhs`.
"""
@builtin def (,)(lhs: $A, rhs: $B) -> ($A, $B)
~~~

この declaration は `lib/bootstrap.srt` の `defmod Bootstrap` に置く。`:doc (,)` は alphabetic helper や trait 名を経由せず、この `Bootstrap::(,)` builtin operator の doc entry を直接表示する。`lib/types/tuple.srt` は tuple equality/comparison の module doc と implementation だけを置く。

### 中置

~~~surtr
left (,) right
~~~

は次と等価である。

~~~surtr
(left, right)
~~~

(,) は右結合である。

~~~surtr
a (,) b (,) c
# => (a, (b, c))
~~~

したがって作られる値は常に arity 2 である。既存の flat tuple literal、たとえば (a, b, c) へ暗黙 flatten してはならない。

### FuncLiteral、capture、pipeline

function value として参照する文脈では、既存の記号演算子と同じく backtick quote を必須とする。

~~~surtr
pair = &`(,)`
pair(1, "one")

left |> `(,)`(right)
# => (left, right)
~~~

bare の (,) は中置位置でのみ有効とし、quote なしの callable reference や capture は受理しない。

### 優先度・結合性

強い順の relevant な階層は次とする。

~~~text
Cond / literal / range > Expr > (,) > Compare > AndOr > flow > StdOn > Bind
~~~

(,) は Expr（+、-、*、++）より弱く、Compare（==、!=、<、<=、>、>=）より強い。

~~~surtr
a + b (,) c + d
# => (a + b) (,) (c + d)

a (,) b == c (,) d
# => (a (,) b) == (c (,) d)
~~~

後者により tuple の `Compare` 実装をそのまま中置演算子で利用できる。`==` / `!=` の受入条件は、pair を既存 `Ast::TupleLiteral` に lower するため、arity-2 tuple への canonical componentwise `Eq` 実装で満たす。pair 専用の equality 規則は作らず、すべての 2-tuple に同じ `Eq` が適用されるため、既存の trait-impl overlap 規則も通常どおり適用する。cond、range literal、tuple literal 内部の expression grammar は変えない。

### 型・評価

~~~text
(,) : ($A, $B -> ($A, $B))
~~~

- 左右の operand を各 1 回、左から右へ評価する。
- 型は既存 Ty::Tuple(vec![A, B]) を使う。
- Scar は pair operator descriptor の固定規則 ($A, $B -> ($A, $B)) を直接適用し、trait obligation を作らない。
- pipeline の pair FuncLiteral call は、既存の first-argument injection を使う。
- quote なしの callable reference / capture、入力 tuple の arity による意味変更、implicit flatten は導入しない。

## 非目標

- variadic tuple constructor や HKT 相当の抽象化
- (a, (b, c)) を (a, b, c) に変える implicit flatten
- Tuple::flatten のような新 API
- tuple pattern、tuple literal、Tuple._N、既存 arity 2..=8 の Compare 実装の意味変更
- Eldr の新 opcode、CallBuiltin、BUILTIN_METAS の更新

## 影響範囲

| 層 | 変更 | 変更しないもの |
| --- | --- | --- |
| Spire | token / parser / FuncLiteral の (,) 認識と tuple literal への lowering | 通常 tuple literal grammar |
| Sigil | lower 済み Ast::TupleLiteral を通常どおり解決 | 新しい Resolved variant |
| Scar | builtin pair operator の固定型規則と既存 TupleLiteral の型検査を使用 | 新しい Ty / trait dispatch |
| Forge / Eldr | 既存 tuple emit / runtime value を使用 | opcode、VM builtin |
| docs / tests | language spec と parser・integration fixture | 既存 fixture の期待値 |

## 実装計画

### Task 1: Spire parser surface

**Files**

- Modify: crates/spire/src/func_literal.rs
- Modify: crates/spire/src/parser/expr.rs
- Modify: crates/spire/src/parser/tests.rs
- Modify: crates/spire/src/lexer.rs
- Modify: crates/spire/src/ast.rs（中間 AST を導入する場合のみ）
- Modify: lib/bootstrap.srt
- Modify: lib/types/tuple.srt

**実装**

1. parser-only Ast variant を後段へ漏らさないため、中置 (,) は parse 時点で Ast::TupleLiteral(span, vec![left, right]) へ lower する。
2. parse_expr_class_expr と parse_logical_expr の間に parse_pair_expr を設ける。left は parse_expr_class_expr、right は parse_pair_expr とし、右結合にする。parse_logical_expr は parse_pair_expr を読む。
3. 中置位置の LParen, Comma, RParen だけを一体の operator として lookahead する。tuple/call/record の separator comma の tokenization・意味は変更しない。
4. compiler-owned operator table に PairConstructor descriptor を追加し、FuncLiteral table から quoted (,) をこの descriptor へ接続する。既存 BinOp は trait-dispatch 用であるため、pair を BinOp へ追加しない。callable value が必要な capture だけが、body を Ast::TupleLiteral([left, right]) とする 2 引数 closure に lower する。
5. pipeline RHS の quoted pair call と capture は、既存 FuncLiteral の closure/capture ルートを通す。
6. `lib/bootstrap.srt` の `defmod Bootstrap` に、指定された `@builtin def (,)(lhs: $A, rhs: $B) -> ($A, $B)` と直前の `@doc` を置く。`lib/types/tuple.srt` には pair と flat tuple literal の違い、Compare の利用例、arity-2 の componentwise `Eq` 実装を置く。

**必須 parser tests**

~~~surtr
a (,) b
# TupleLiteral([a, b])

a (,) b (,) c
# TupleLiteral([a, TupleLiteral([b, c])])

a + b (,) c + d
# TupleLiteral([a + b, c + d])

a (,) b == c (,) d
# TupleLiteral([a, b]) == TupleLiteral([c, d])
~~~

次は parse error にし、quoted spelling を案内する。

~~~surtr
&(,)
(,)
~~~

**Verification**

~~~bash
cargo nextest run -p spire
~~~

### Task 2: resolver / typechecker 境界を確認する

**Files**

- Modify: crates/sigil/src/resolver/expr.rs
- Modify: crates/sigil の既存 resolver test file
- Modify: crates/scar の既存 tuple/callable test file

Spire が中置 case を既存 Ast::TupleLiteral へ lower するなら、Sigil に新 node は不要である。Scar は `(,)` declaration の `Bootstrap::(,)` contract を固定検証し、constructor 自体の trait lookup を行わない。quoted FuncLiteral の capture だけは、既存 capture lowering で 2 引数 tuple-producing closure を作る。

**必須 tests**

~~~surtr
pair = &`(,)`
pair(1, "one")
# => (1, "one")

1 |> `(,)`("one")
# => (1, "one")
~~~

**Verification**

~~~bash
cargo nextest run -p sigil
cargo nextest run -p scar
~~~

### Task 3: fixture と正本仕様

**Files**

- Modify: doc/要件定義v9.md
- Modify: lib/bootstrap.srt
- Modify: lib/types/tuple.srt
- Create: tests/fixtures/script/pass/tuple/pair_operator.srt
- Create: tests/fixtures/script/pass/tuple/pair_operator.expected
- Modify: crates/rune/tests/integration.rs（fixture discovery が新 directory を自動探索しない場合のみ）

**Fixture**

~~~surtr
print(1 (,) "one")
print(1 (,) 2 (,) 3)
print(1 + 2 (,) 3 + 4)
print((1 (,) 2) == (1 (,) 2))
print((1 (,) 2) < (1 (,) 3))
print(1 |> `(,)`("one"))

pair = &`(,)`
print(pair("left", "right"))
~~~

期待 stdout:

~~~text
(1, "one")
(1, (2, 3))
(3, 7)
True
True
(1, "one")
("left", "right")
~~~

要件定義には operator 一覧、right associativity、優先順位、quoted FuncLiteral spelling、non-flatten 契約、および arity-2 tuple `Eq` extension の適用範囲を追記する。`lib/bootstrap.srt` には `(,)` の `@doc` と declaration を置き、tuple module doc には pair operator と flat tuple literal の違い、Compare の利用例を追加する。

`:doc (,)` は REPL で手動確認する。operator registry から pair descriptor を直接引き、上記 `@doc` と signature を表示すること。専用の自動テストは追加しない。

**Verification**

~~~bash
cargo nextest run -p rune --test integration run_srt
cargo nextest run --workspace
~~~

## 受け入れ条件

1. a (,) b は (a, b) と同じ値、型、評価順を持つ。
2. a (,) b (,) c は (a, (b, c)) になる。
3. Expr は (,) より優先し、(,) は Compare より優先する。
4. pair operator で構築した値に、canonical arity-2 tuple `Eq` と既存 `Compare` により ==、!=、<、<=、>、>= が使える。pair 専用 equality は存在しない。
5. quoted pair FuncLiteral を pipeline RHS と capture で使え、builtin operator resolution が trait lookup を行わない。quote なしの同等表記は parse error になる。
6. 新 opcode、builtin metadata、runtime builtin、implicit flatten は存在しない。
7. cargo nextest run --workspace が通る。
8. REPL で `:doc (,)` を実行すると、pair constructor の doc と `(lhs: $A, rhs: $B) -> ($A, $B)` が表示される。これは手動確認とし、テスト追加は不要である。

## 留意点

- pair を BinOp や operator trait に追加してはならない。tuple construction は compiler-resolved builtin operator の lowering である。
- cond の clause separator と body grammar は変更しない。cond value を pair の左 operand として使えることだけを parser test で確認する。
