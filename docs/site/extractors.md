# Extractors

Extractor は `match` や `=?` で使う「分解の入口」です。

照合対象には concrete な静的型 head が必要です。`value: $T` のような裸の型変数や、`where $T: Trait` で無関係な型を横断する定義は使えません。`List<$T>` や `Result<$T>` のように、concrete head の型引数へ型変数を置くことはできます。型一般の処理や Trait による抽象化は通常関数へ置きます。

## builtin extractor

標準では `Kernel::uncons(term)` があります。

- `List<$A>` を `(head, tail)` へ分解
- `String` を `(head, tail)` へ分解

これは `$Tail` や Union を対象にする1個の generic Extractor ではありません。適用する値から、`List<$T>` と `String` のどちらか一方の concrete な静的契約が選ばれます。runtime 実装は共有できますが、SRT だけで定義する場合は型ごとに別の Extractor として記述します。

Pattern の `[head, ..tail]` も同じ head / tail の分解を表します。ただし、list の構造的 Pattern と `uncons(...)` では失敗 Error が異なります。空 list の `[head, ..tail]` は `EmptyList`、`uncons(head, tail)` は `PatternMismatch` を返します。分岐ではどちらも不一致ですが、`=?` や `apply_pattern` ではこの違いが保持されます。

## user-defined extractor

定義は file-oriented です。

```surtr
deferror Rejected { "input rejected" }

defmod Matchers {
  defextractor never(value: Int) -> MatchResult<Int, Error> {
    MatchResult::Err(Rejected)
  }
}
```

REPL では top-level に `defextractor` を直接置けないため、宣言は file で管理し、利用側を REPL で確認する形になります。

## match 側の見え方

```surtr
import Matchers::never

print(match 1 {
  never(value) => "bad",
  _ => "fallback",
})
```

この例では `never(...)` が常に `MatchResult::Err(Rejected)` を返すため、fallback 側に流れます。

Extractor は `MatchResult<$A, Error>` を返します（`MatchResult<$A>` も可）。
`MatchResult::Ok(payload)` は子 pattern の照合へ進み、`MatchResult::Err(error)` は
不一致になります。各 occurrence は到達時に一度だけ評価され、成功結果を再利用します。
`match` / `if_let` / `is_match` は Error を破棄します。SafeBind `=?` は元 Error の
kind / message / location / cause を保持して現在の failure target へ渡し、
do の Alternative route は破棄して `empty` へ進みます。

成功 payload が Unit の場合は `check()` と子 Pattern を省略でき、
`check(value: Unit)` や `check(_)` と明示することもできます。
単値は子 Pattern 1個、tuple は要素数と同じ個数が必要です。
Extractor 本文でも SafeBind を使えます。失敗は本文自身の MatchResult::Err となり、
成功終端には明示的な MatchResult::Ok が必要です。
MatchResult は通常の変数・引数・field に保持できず、通常 Closure へ利用権限は継承されません。

Extractor 本文の計算量や Effect は制限しません。Process messaging や IO handler も、通常の型付き API を通して呼び出せます。型を介した値の受け渡しと immutable な値は保証しますが、純粋性、実行コスト、zero-cost abstraction は保証対象ではありません。高コストな Extractor の実行コストは定義者の責任です。

## 事前引数

最後の入力が照合対象です。それ以前の入力は Pattern head の先頭に通常の式として書きます。

```surtr
deferror Outside { "outside range" }
defmod Bounds {
  defextractor between(min: Int, max: Int, value: Int) -> MatchResult<Int, Error> {
    if(and(min <= value, value <= max), MatchResult::Ok(value), MatchResult::Err(Outside))
  }
}
# 利用側
if_let(5, Bounds::between(0, 10, accepted), accepted, 0)
# 5
```

事前引数はその occurrence に到達したときだけ左から一度ずつ評価します。
同じ Pattern で新たに束縛する名前は参照できず、外側にある同名値を参照します。
成功 payload が Unit なら、事前引数だけを書いて子 Pattern を省略できます。


## ExtractorClosure

REPL では `*{|value| ...}` で、capture を持つ ExtractorClosure を作れます。

```surtr
limit = 10
greater = *{|value: Int|
  True =? value > limit
  MatchResult::Ok(value)
}
if_let(12, greater(accepted), accepted, 0)
# 12
is_match(3, greater(_))
# False
```

型は `ExtractorClosure<(Int -> MatchResult<Int, Error>)>` です。通常の変数・引数・戻り値として
受け渡し、同じ signature の値を `if` や `match` で選択できます。引数の型注釈は推論できれば省略できます。
事前引数、payload の分解、Unit 子の省略、本文の SafeBind は named Extractor と同じ契約です。

変数へ束縛するときは入力と payload を含む signature 全体が concrete である必要があります。高階関数へ literal を直接渡す場合は、受け取り側の expected type から一意に導出できれば注釈は不要です。後続の Pattern 適用ごとに未確定型を別々の型へ generalize することはありません。

Pattern head には bind 済みの名前を使います。`greater(12)` という通常 call、生成式を直接
head にする形、普通の Closure との暗黙変換は許可しません。local の名前が named Extractor を
shadow した場合は、その local の型を検査します。

## 値として結果を受け取る

`apply_pattern` は照合の結果を通常の `Result` として返します。成功した値を `_1`〜`_16` で選び、複数なら番号順の tuple にします。番号は1から連続させ、同じ番号を二度使わないでください。型注釈は省略でき、書いた場合は静的に検査されます。

```surtr
apply_pattern([10, 20, 30], [_1: Int, .._2: List<Int>])
# Ok((10, [20, 30]))
12 |> apply_pattern(greater(_1: Int))
# Ok(12)
```

Extractor の失敗は元の Error を保持した `Err` になります。普通の Pattern 不一致も `Err` になりますが、外側の関数から早期 return しません。入力が Result でもそのまま照合するので、成功 payload を取り出す場合は `apply_pattern(Ok(3), Ok(_1))` と書きます。Pattern 内の通常 binding は外へ公開されません。

pipeは右辺の最外Callだけを操作し、直接引数のplaceholderへ、なければ先頭へ左辺を挿入します。引数数によって挿入方法を変えず、PatternやLazyへの注入を拒否します。入れ子のCallやPattern内部ではpipe slotを探しません。

projection は `apply_pattern` の Pattern 内専用です。事前引数では使えず、OR Pattern もこの consumer では使えません。`_` や `_name` は値を取り出さない wildcard のままです。

## Result を返す関数を使う

`Extractor::from_result` で、入力が1個の Result-returning callable を明示的に変換できます。

```surtr
decimal = Extractor::from_result(&Int::parse)
apply_pattern("42", decimal(_1: Int))
# Ok(42)
is_match(apply_pattern("oops", decimal(_1)), Err(_))
# True
nested = Extractor::from_result({|value: Int| Ok(Ok(value))})
apply_pattern(3, nested(_1))
# Ok(Ok(3))
```

生成時には関数を実行しません。各 occurrence へ到達したときに一度だけ実行し、外側の Result だけを外します。成功 payload が tuple / Unit の場合も同じ規則で使えます。失敗した元 Error の cause や location は保持します。入力が複数の関数は、必要な値をcaptureした単項Closureで包んでから渡します。Option-returning callableや通常の値は受理しません。

REPLで `:doc Extractor` を開くと、named Extractor、ExtractorClosure、事前引数、Unit、projection、SafeBindと変換のサンプルを参照できます。API単独の説明は `:doc Extractor::from_result` です。正本は `lib/extractor.srt` の `@doc` にあります。

## ルール

- extractor 名は constructor-style の大文字始まりにしない
- extractor の入力型と pattern 期待型が合わないと type error
- extractor の照合対象に裸の generic type parameter や Trait `where` 一般化を使わない
- 成功 payload に対応する子 Pattern 数を合わせる。単値は1個、tuple は要素数、Unit は0個または1個。literal や wildcard も子 Pattern に数える

関連する compile error 例は `../../tests/fixtures/modules/fail/resolve_extractor_*` と `../../tests/fixtures/modules/fail/type_mismatch_extractor_*` にあります。

## 関連ページ

- pattern 側の使い方は `./pattern-matching.md`
- `Kernel::uncons` は `./kernel.md`

## 確認したソース

- ソース
  - `../../lib/kernel.srt`

## 躓きやすいポイント

- extractor は普通の `def` ではなく、`MatchResult` を返す pattern-side contract として読む必要があります。
- extractor の入力型と scrutinee 型、成功 payload の arity がずれると分かりにくい type error になりやすいです。
