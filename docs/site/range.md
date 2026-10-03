# Range
 
Surtr で「範囲」を扱う方法は 3 つあります。欲しいものに合わせて選んでください。
 
| 欲しいもの | 使うもの | 結果の型 |
|---|---|---|
| 境界の 2 値だけ | `Range(min, max)` | `Range<$A>` |
| 今すぐ list | `[start..stop]` | `List<Int>` / `Result<List<String>, Error>` |
| 必要な分だけ遅延生成 | `Generator::range(start, stop)` | `Generator<Int>` |
 
`[start..stop]` と `Generator::range` は終端を含みます (inclusive)。
 
## Range: 境界を持つ値
 
`Range` は 2 つの境界を保持するだけで、list には展開されません。入力順も直しません。
 
```surtr
left = Range(3, 1)
print(to_string((left.min, left.max)))
```
 
```text
(3, 1)
```
 
| API | 動作 |
|---|---|
| `Range(min, max)` / `Range::new(min, max)` | 入力順のまま保持 |
| `Range::normalized(a, b)` | `compare` で昇順に直して作る |
| `Range::advance(range, steps)` | `Range<Int>` の両端を `steps` 分前へ |
| `Range::retreat(range, steps)` | `Range<Int>` の両端を `steps` 分後ろへ |
 
pattern でも使えます。
 
```surtr
match Range(4, 6) {
  Range(min, max) => (min, max),
  _ => (0, 0),
}
```
 
### 比較
 
比較は endpoint の trait 実装に従います (`compare` は `Compare`、`==` は `Eq`、`!=` は `Neq` が必要)。
`compare` は包含判定ではなく辞書順で、`min`、同値なら `max` の順に比べます。
 
```surtr
print(to_string(compare(Range(10ms, 20ms), Range(10ms, 30ms))))
```
 
## range literal: その場で list を作る
 
`[start..stop]` は `Range` ではなく、list を直接作ります。
 
```surtr
print(to_string([1..3]))        # [1, 2, 3]
 
chars =? ["a".."c"]
print(to_string(chars))         # [a, b, c]
```
 
- 整数: `List<Int>`
- 文字: 文字の検証が入るため `Result<List<String>, Error>`
文字の endpoint は ASCII 1 文字のみ有効です。`""`、`"ab"`、`"あ"` は `InvalidCharRange` になります (定数でも実行時でも同じ)。
 
## Generator::range: 遅延 range
 
必要な分だけ取り出したいときに使います。構築しただけでは何も生成しません。
 
```surtr
gen = Generator::range(1, 3)
print(to_string(Generator::to_list(gen)))   # [1, 2, 3]
```
 
step は常に `1` です。step を指定する場合や文字を使う場合は次の helper を使います。
 
| API | 例 | 結果 |
|---|---|---|
| `Generator::range_step(start, stop, step)` | `range_step(1, 5, 2)` | `[1, 3, 5]` |
| `Generator::range_step(start, stop, step)` | `range_step(7, 1, -2)` | `[7, 5, 3, 1]` |
| `Generator::range_char(a, b)` | `range_char("a", "c")` | `[a, b, c]` |
| `Generator::range_char_step(a, b, step)` | `range_char_step("a", "g", 2)` | `[a, c, e, g]` |
 
- `step > 0` は昇順、`step < 0` は降順です。
- `step == 0` は `Err(InvalidRangeStep(...))` です。
- `range_step` / `range_char` / `range_char_step` は `Result` を返すので、`=?` で受けます。
- 文字 endpoint の制約は range literal と同じです。
`Generator::take` で、必要な件数と続きの generator を取得できます。
 
```surtr
(values, rest) = Generator::take(Generator::range(1, 4), 2)
print(inspect(values))                     # [1, 2]
print(inspect(Generator::to_list(rest)))   # [3, 4]
```
 
無限列 (`InfiniteGenerator`) など Generator 全般の API は [Generator](./generator.md) を参照してください。
 
## 間違えやすい点
 
- `Range(1, 3)` は `[1, 2, 3]` にならない
- `[1..3]` は `Range<Int>` ではなく `List<Int>`
- `["a".."c"]` は `Result` で返る (`=?` で受ける)
- `Generator::range(1, 3)` は `Range<Int>` ではなく generator
- 順序を直すのは `Range::normalized`、そのまま保持するのは `Range::new`
## 関連ページ
 
- [Structs](./structs.md)
- [Pattern Matching](./pattern-matching.md)
- [Standard Modules](./standard-modules.md)
- [Surtr Language Guide](./language-guide.md)