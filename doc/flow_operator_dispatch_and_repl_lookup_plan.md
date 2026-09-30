# Function Application / Composition Operator 改修計画（実装済み）

この計画は実装済みです。現行の利用者向け契約は [関数演算子](../docs/site/function-operators.md) と [trait 実装](../docs/site/trait-impls.md)、REPL の契約は [Xldr 仕様](../docs/dev/Xldr_spec.md) を正本とします。以下は実装時の計画を記録したものです。

## 目的

関数適用演算子 |> と、関数合成演算子 >>、>*、>=> を Bootstrap の @builtin def declaration として定義する。
fmap、ap、bind は既存 surface を維持する。

演算子トレイトはユーザ開放のポリモーフィズムの入口としてソースコードに置く。
処理系が意味を固定する演算子は、トレイトを介さず固定規則へ接続する。

## 関連変更

[標準 Eq・trait 実装制限](../docs/dev/Trait_system_spec.md) とともに、Compose と関数型への Composable / PipeApply / LiftComposable / KleisliComposable 実装を廃止した。`/` は `Facet::chain` に対応する固定構文、`|>` / `>>` / `>*` / `>=>` は `Bootstrap` 宣言を持つ固定演算子へ移行した。関数型への trait impl の暫定例外はない。

|*> は fmap implementation への trait dispatch を行うため、trait operator のままとする。>* は fmap が定義されていることを型規則として要求する builtin operator とする。

## Bootstrap declaration

各 declaration は Bootstrap に置き、@doc には処理の説明と短い使用例だけを記載する。

~~~surtr
@doc """
Apply a value to a unary callable.

## Examples
1 |> {|n| n + 1}
"""
@builtin def |>(value: $A, f: ($A -> $B)) -> $B

@doc """
Compose two unary callables.

## Examples
parse >> render
"""
@builtin def >>(left: ($A -> $B), right: ($B -> $C)) -> ($A -> $C)
~~~

>* と >=> は Functor<$A> / Monad<$A> の family positional type annotation を argument と return に使う。

~~~text
@builtin def >*(
  self: ($A -> Functor<$B>),
  mapper: ($B -> $C),
) -> ($A -> Functor<$C>)

@builtin def >=>(
  self: ($A -> Monad<$B>),
  mapper: ($B -> Monad<$C>),
) -> ($A -> Monad<$C>)
~~~

>* は既存 Functor implementation の fmap を、>=> は既存 Monad implementation の bind を内部 lowering で利用する。

## 外側の分類

| operator | category | contract |
| --- | --- | --- |
| \|> | builtin operator | plain function application |
| >> | builtin operator | plain function composition |
| >* | builtin operator | fmap requirement |
| >=> | builtin operator | bind requirement |
| / | fixed operator（関連 PR 担当） | Facet::chain の型・可視性規則 |
| \|*> | trait operator | fmap dispatch |
| \|*\| | existing operator | Applicative ap |
| \|>= | existing operator | Monad bind |

外側の分類は operator の dispatch capability を userland へ開放するかで決める。内部 lowering は builtin operator と trait operator の差を吸収する。

## REPL query

すべての operator symbolは :doc と :sig から直接引ける。

~~~text
:doc |>
:doc >>
:sig >*
:doc >=>
:sig |>=
:sig +
~~~

表示は builtin declaration、requirement、または operator trait signature を返す。
固定構文 `/` の :doc / :sig は Facet::chain の説明・signature へ接続し、
削除した Compose の文書や signature を参照しない。

## 変更対象

- lib/bootstrap.srt
  - |>、>>、>*、>=> の @doc と @builtin def declaration
- crates/spire/src/func_literal.rs
  - quoted composition operator と Bootstrap declaration の対応
- crates/scar/src/checker/expr.rs
  - composition operator の型規則と fmap / bind requirement 解決
- crates/scar/src/typed.rs
  - resolved builtin composition operator と requirement
- crates/forge/src/codegen.rs
  - composition lowering と resolved implementation call
- crates/xldr/src/repl/
  - :doc と :sig の direct operator lookup
- lib/traits/operator/pipe_apply.srt
- lib/traits/operator/composable.srt
- lib/traits/operator/lift_composable.srt
- lib/traits/operator/kleisli_composable.srt
  - PipeApply、Composable、LiftComposable、KleisliComposable を Bootstrap builtin declaration へ移行
- docs/site/function-operators.md / docs/dev/Xldr_spec.md
  - composition operator surface、dispatch、REPL query の契約

## 実装順

1. Bootstrap に |>、>>、>*、>=> の @doc / @builtin def declaration を追加する。
2. |> を builtin application、>> を builtin function composition として型検査・lower する。
3. >* と >=> に fmap / bind requirement を導入し、内部 lowering を既存 implementation に接続する。
4. PipeApply、Composable、LiftComposable、KleisliComposable の intermediate trait surface を置き換える。
5. :doc と :sig で全 operator symbol を直接検索できるようにする。
6. standard-library docs、fixture、workspace test を更新する。

## 受け入れ条件

1. |>、>>、>*、>=> は Bootstrap の @doc / @builtin def declaration を持つ。
2. |> と >> は builtin rule で解決される。
3. >* と >=> は builtin operator として fmap / bind requirement を検査する。
4. fmap、ap、bind、|*>、|*|、|>= は変更されない。
5. :doc と :sig は全 operator symbol を直接表示できる。
6. cargo nextest run --workspace が通る。
