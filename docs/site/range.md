# Range

`Range` と range literal と `Generator::range` は、見た目が近くても役割が違います。  
このページは、その違いを利用者向けにまとめたものです。

## 1. 先に結論

- `Range<$A>`
  - 境界 2 つだけを保持する値
  - list には展開しない
- `[start..stop]`
  - inclusive range literal
  - その場で `List` か `Result<List<...>, Error>` を作る
- `Generator::range(...)`
  - inclusive range を遅延 generator として作る
  - `Generator::to_list(...)` するまで全件は materialize しない

同じ「範囲」でも、必要なのが

- ただの境界値なのか
- いますぐ list が欲しいのか
- 遅延に流したいのか

で使い分けます。

## 2. `Range<$A>` は境界を持つ値

`Range` は標準ライブラリの値型です。

```surtr
left = Range(3, 1)
print(to_string((left.min, left.max)))
```

```text
(3, 1)
```

`Range` は順序を自動では直しません。  
`Range(3, 1)` はそのまま `min: 3, max: 1` を持つ値です。

### 使う API

```surtr
raw = Range(3, 1)
normalized = Range::normalized(3, 1)
advanced = Range::advance(raw, 2)
retreated = Range::retreat(raw, 2)
```

- `Range(min, max)` または `Range::new(min, max)`
  - 入力順をそのまま保持する
- `Range::normalized(a, b)`
  - `compare(a, b)` で昇順化した `Range` を返す
- `Range::advance(range, steps)`
  - `Range<Int>` の両端を同じだけ前へ動かす
- `Range::retreat(range, steps)`
  - `Range<Int>` の両端を同じだけ後ろへ動かす

### pattern でも使える

```surtr
value = Range(4, 6)

print(to_string(match value {
  Range(min, max) => (min, max),
  _ => (0, 0),
}))
```

```text
(4, 6)
```

### 比較について

`Range` の比較は endpoint 側の trait 実装に従います。

- `compare` には `Compare`
- `==` には `Eq`
- `!=` には `Neq`

がそれぞれ必要です。

```surtr
left = Range(10ms, 20ms)
right = Range(10ms, 30ms)

print(to_string(compare(left, right)))
print(to_string(left == Range(10ms, 20ms)))
```

`compare` は包含判定ではなく辞書順です。

1. 先に `min`
2. 同値なら `max`

の順で比較します。

## 3. range literal `[start..stop]` は list を作る

`[start..stop]` は `Range` の sugar ではありません。

```surtr
nums = [1..3]
chars = ["a".."c"]
```

- `[1..3]`
  - `List<Int>`
- `["a".."c"]`
  - `Result<List<String>, Error>`

整数 range literal はその場で list になります。

```surtr
print(to_string([1..3]))
```

```text
[1, 2, 3]
```

文字 range literal は char validation が入るので `Result` です。

```surtr
chars =? ["a".."c"]
print(to_string(chars))
```

```text
[a, b, c]
```

### 文字 endpoint の制約

`String` endpoint は single ASCII char 契約です。

- `"a"` は有効
- `""` は無効
- `"ab"` は無効
- `"あ"` は現状では無効

不正 endpoint は constant literal でも runtime で `InvalidCharRange` になります。

## 4. `Generator::range` は遅延 range

list がいらず、map / take / with_index などへ流したいなら `Generator::range` が向いています。

```surtr
gen = Generator::range(1, 3)
print(to_string(Generator::to_list(gen)))
```

```text
[1, 2, 3]
```

`Generator::range(start, stop)` は inclusive ascending integer range です。

- 開始値を含む
- 終了値も含む
- step は常に `1`

### `Generator::range_step`

明示 step が欲しいときは `Generator::range_step` を使います。

```surtr
stepped =? Generator::range_step(1, 5, 2)
print(to_string(Generator::to_list(stepped)))
```

```text
[1, 3, 5]
```

`step` の意味は次のとおりです。

- `step > 0`
  - 昇順
- `step < 0`
  - 降順
- `step == 0`
  - `Err(InvalidRangeStep(...))`

```surtr
down =? Generator::range_step(7, 1, -2)
print(to_string(Generator::to_list(down)))
```

```text
[7, 5, 3, 1]
```

### `Generator::range_char` と `Generator::range_char_step`

文字の generator range には専用 helper があります。

```surtr
chars =? Generator::range_char("a", "c")
print(to_string(Generator::to_list(chars)))
```

```text
[a, b, c]
```

step 付き:

```surtr
chars =? Generator::range_char_step("a", "g", 2)
print(to_string(Generator::to_list(chars)))
```

```text
[a, c, e, g]
```

こちらも endpoint は single ASCII char 契約で、`step == 0` は `InvalidRangeStep(...)` です。

## 5. どれを使うべきか

### `Range`

こういうとき:

- 境界値をひとまとまりで持ちたい
- pattern や field access で扱いたい
- list 展開は不要

例:

```surtr
window = Range::normalized(3, 1)
print(to_string((window.min, window.max)))
```

### range literal

こういうとき:

- 短い list をその場で書きたい
- fixture や REPL で即値が欲しい

例:

```surtr
print(to_string([1..5]))
```

### `Generator::range`

こういうとき:

- 遅延に流したい
- `Generator::map`, `Generator::take`, `Generator::with_index` と組み合わせたい
- 全件 list 化しないまま処理したい

例:

```surtr
mapped =
  Generator::range(1, 4)
  |> Generator::map({|x| "n=" ++ to_string(x) })

print(to_string(Generator::to_list(mapped)))
```

## 6. 迷いやすいポイント

- `Range(1, 3)` は `[1, 2, 3]` にならない
- `[1..3]` は `Range<Int>` ではなく `List<Int>`
- `["a".."c"]` は `List<String>` ではなく `Result<List<String>, Error>`
- `Generator::range(1, 3)` は generator であって `Range<Int>` ではない
- `Range::normalized` は順序を直す convenience API で、`Range::new` は順序を保持する

## 7. 関連ページ

- [Structs](./structs.md)
- [Pattern Matching](./pattern-matching.md)
- [Standard Modules](./standard-modules.md)
- [Surtr Language Guide](./language-guide.md)
