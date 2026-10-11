# Generator
 
必要な分だけ値を遅延生成したいときに使います。列に終わりがあるかどうかで型を選んでください。
 
| 欲しいもの | 使うもの | 1 件取り出すと |
|---|---|---|
| 終端のある有限列 | `Generator<Item>` | `Result<(Item, Generator<Item>), GeneratorExhausted>` |
| 終端のない無限列 | `InfiniteGenerator<Item>` | `(Item, InfiniteGenerator<Item>)` |
 
構築しただけでは値を生成せず、取り出したときに step が評価されます。
整数・文字の range は [Range](./range.md) を参照してください。
 
## Generator: 有限列
 
`Generator::unfold(seed, step)` で作ります。step は `Option::Some((値, 次の状態))` を返し、終わりで `Option::None` を返します。
 
```surtr
gen = Generator::unfold(1, {|n|
  if(n <= 3, Option::Some((n, n + 1)), Option::None)
})
print(inspect(Generator::to_list(gen)))  # [1, 2, 3]
```
 
- `Generator::next(gen)` は `Ok((値, rest))` か、終端なら `Err(GeneratorExhausted())` を返します。step の終端検出に使う `Option` とは別の契約です。
- `Generator::to_list(gen)` は終端まで生成して list を返します。
```surtr
match Generator::next(Generator::range(10, 12)) {
  Ok((value, rest)) => print(inspect(value)),  # 10
  Err(_) => print("end"),
}
```
 
自分で書く step は、いずれ `None` を返すようにしてください。停止はコンパイラが保証しません。
 
## InfiniteGenerator: 無限列
 
step は `(値, 次の状態)` を返します。外側の `Option` は付けません。状態が複数ある場合は tuple にまとめます。
 
```surtr
fibonacci = InfiniteGenerator::unfold((0, 1), {|s|
  (s._0, (s._1, s._0 + s._1))
})
(values, rest) = InfiniteGenerator::take(fibonacci, 6)
print(inspect(values))  # [0, 1, 1, 2, 3, 5]
```
 
`InfiniteGenerator::next` は `(値, rest)` を直接返します。無限列に `to_list` はないので、`take` 系で取り出します。
 
## 取り出す: take 系
 
次の API は両方の型で使えます。`rest` は入力と同じ型の generator です。
 
| API | 動作 | 戻り値 |
|---|---|---|
| `take(gen, count)` | 最大 `count` 件取り出す | `(List<Item>, rest)` |
| `take_exact(gen, count)` | ちょうど `count` 件取れたことを確認する | `Result<(List<Item>, rest)>` |
| `take_while(gen, pred)` | `pred` が `True` の間取り出す | `(List<Item>, rest)` |
| `take_until(gen, pred)` | `pred` が最初に `True` になる手前まで取り出す | `(List<Item>, rest)` |
 
```surtr
gen = Generator::range(1, 5)
(values, rest) = Generator::take(gen, 2)
print(inspect(values))                    # [1, 2]
print(inspect(Generator::to_list(rest)))  # [3, 4, 5]
```
 
- **続きは `rest` で進めます。** 元の generator は変わりません。同じ generator を再び使うと step を再評価します (値はキャッシュされず、副作用も再び実行されます)。
- `take` は `count <= 0` なら `([], gen)` を返します。有限列が `count` より短ければ、取れた分だけ返ります。
- `take_while` / `take_until` が止まる原因の値は list に含めず、`rest` の先頭に残します。
```surtr
(values, rest) = Generator::take_while(Generator::range(1, 5), {|n| n < 3})
print(inspect(values))                    # [1, 2]
print(inspect(Generator::to_list(rest)))  # [3, 4, 5]
```
 
### take_exact
 
`take_exact` は次の場合に `Err` を返します。
 
- `count <= 0`: 有限列は `InvalidGeneratorCount`、無限列は `InvalidInfiniteGeneratorCount`（生成前に検査）
- 有限列が `count` 件に満たない: `GeneratorShortage`
```surtr
match Generator::take_exact(Generator::range(1, 2), 3) {
  Ok((values, _)) => print(inspect(values)),
  Err(err) => print(Error::kind(err)),  # GeneratorShortage
}
```
 
## 取得した List を操作する
 
有限列・無限列とも、generator は生成と List の取得を担当します。取得した値の変換・選別・集計には List モジュールを使います。
 
```surtr
numbers = InfiniteGenerator::unfold(1, {|n| (n, n + 1)})
(values, rest) = InfiniteGenerator::take(numbers, 6)
evens = List::filter(values, {|n| n % 2 == Ok(0)})
print(inspect(List::map(evens, {|n| n * 10})))  # [20, 40, 60]
```
 
この例では元の列から 6 件取り出し、その List から偶数を選びます。取得件数は変換後の件数ではありません。

## iterate
 
`InfiniteGenerator::iterate(seed, count, step)` は、`seed` から `step` を繰り返し適用した最初の `count` 件を **List** で返します。
 
```surtr
print(inspect(InfiniteGenerator::iterate(1, 5, {|n| n * 2})))  # [1, 2, 4, 8, 16]
```
 
## 注意点
 
- `Generator` と `InfiniteGenerator` は別の型です。それぞれの API を使ってください。
- 有限列の終端は、step が返す外側の `None` だけです。item が `Option` や `Result` でも、通常のデータとして扱われます。
- 無限列でも、step が値を返すことはコンパイラが保証しません。条件が反転しない `take_while` は戻りません。
- step で起きた実行時エラーは、終端や途中までの成功 list には変換されません。
## 関連ページ
 
- [Range](./range.md)
- [関数コールと関数値](./callables.md)
- [パイプ演算子](./pipe-operators.md)
- [エラーハンドリング](./error-handling.md)
- [Standard Modules](./standard-modules.md)
