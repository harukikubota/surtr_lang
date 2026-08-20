# Function Application / Composition Operator 改修案

## 目的

関数適用演算子 |> と、関数合成演算子 >>、>*、>=> を Bootstrap の @builtin def declaration として定義する。
fmap、ap、bind は既存 surface を維持する。

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
| |> | builtin operator | plain function application |
| >> | builtin operator | plain function composition |
| >* | builtin operator | fmap requirement |
| >=> | builtin operator | bind requirement |
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
  - Composable、LiftComposable、KleisliComposable を Bootstrap builtin declaration へ移行
- doc/要件定義v9.md
  - composition operator declaration、dispatch、REPL query の契約

## 実装順

1. Bootstrap に |>、>>、>*、>=> の @doc / @builtin def declaration を追加する。
2. |> を builtin application、>> を builtin function composition として型検査・lower する。
3. >* と >=> に fmap / bind requirement を導入し、内部 lowering を既存 implementation に接続する。
4. Composable、LiftComposable、KleisliComposable の intermediate trait surface を置き換える。
5. :doc と :sig で全 operator symbol を直接検索できるようにする。
6. standard-library docs、fixture、workspace test を更新する。

## 受け入れ条件

1. |>、>>、>*、>=> は Bootstrap の @doc / @builtin def declaration を持つ。
2. |> と >> は builtin rule で解決される。
3. >* と >=> は builtin operator として fmap / bind requirement を検査する。
4. fmap、ap、bind、|*>、|*|、|>= は変更されない。
5. :doc と :sig は全 operator symbol を直接表示できる。
6. cargo nextest run --workspace が通る。
