# Generator 仕様案（サンプルレベル）

## 目的

`Generator` は、有限列を逐次生成するための型です。

- `iterate` は `Generator` を内部で生成し、必要に応じて `List` へ消費する
- `range` 構文も `Generator` を通して生成する
- `Generator` は内部に `idx` を持つ
  - `with_index` を自然に扱うため
- 対応対象は当面 `Int` と `Char`（内部表現は `String`）
- step 指定は API で扱う

本ドキュメントは、**公開 API とサンプルレベルの挙動**をまとめたものです。

---

## 型シグネチャ

```surtr
@@builtin type Generator<$State, $Item>
```

- `$State`: 生成途中の内部状態
- `$Item`: 生成される要素型

`Generator` は opaque とし、利用者は内部表現へ直接アクセスしません。
`idx` も field ではなく API 経由で参照します。

---

## 公開 API

```surtr
defmod Generator {
  def unfold<$State, $Item>(
    state: $State,
    step: (($State, Int) -> Result<($Item, $State), NoneError>)
  ) -> Generator<$State, $Item>

  def next<$State, $Item>(gen: Generator<$State, $Item>)
    -> Result<($Item, Generator<$State, $Item>), NoneError>

  def idx<$State, $Item>(gen: Generator<$State, $Item>) -> Int

  def with_index<$State, $Item>(gen: Generator<$State, $Item>)
    -> Generator<$State, (Int, $Item)>

  def map<$State, $A, $B>(gen: Generator<$State, $A>, f: ($A -> $B))
    -> Generator<$State, $B>

  def take<$State, $Item>(gen: Generator<$State, $Item>, count: Int)
    -> List<$Item>

  def to_list<$State, $Item>(gen: Generator<$State, $Item>) -> List<$Item>

  def iterate<$A>(seed: $A, count: Int, step: ($A -> $A))
    -> Generator<$A, $A>

  def range(start: Int, stop: Int) -> Generator<Int, Int>

  def range_step(start: Int, stop: Int, step: Int)
    -> Result<Generator<Int, Int>, Error>

  def range_char(start: String, stop: String)
    -> Result<Generator<String, String>, Error>

  def range_char_step(start: String, stop: String, step: Int)
    -> Result<Generator<String, String>, Error>
}
```

---

## 共通ルール

### `idx`

- `idx` は **次に返す要素の index** とする
- 初期値は `0`
- 1 要素 `next` するごとに `+1`
- `with_index` はこの `idx` をそのまま外へ出す

### 終了

- 次要素が存在しない場合は `Err(NoneError)` を返す
- `Generator::to_list` は終了まで消費する
- `Generator::take(gen, count)` は `count <= 0` で `[]` を返す

### `range`

- `range(start, stop)` は当面 **両端含む** とする
- `range_step(..., step)` / `range_char_step(..., step)` は `step == 0` をエラーにする
- `Char` は surface 上は 1 文字 `String` として扱う
- `range_char*` は、1 文字でない入力をエラーにする

---

## 各 API のサンプル

## 1. `Generator::unfold`

### シグネチャ

```surtr
def unfold<$State, $Item>(
  state: $State,
  step: (($State, Int) -> Result<($Item, $State), NoneError>)
) -> Generator<$State, $Item>
```

### 役割

もっとも低レベルの Generator 構築関数です。

- `state`: 初期状態
- `step(state, idx)`:
  - `Ok((item, next_state))` なら 1 要素返して継続
  - `Err(NoneError)` なら終了

### サンプル

```surtr
countdown = Generator::unfold(3, {|n, _idx|
  if(n > 0,
    Ok((n, n - 1)),
    Err(NoneError)
  )
})

Generator::to_list(countdown)
# => [3, 2, 1]
```

---

## 2. `Generator::next`

### シグネチャ

```surtr
def next<$State, $Item>(gen: Generator<$State, $Item>)
  -> Result<($Item, Generator<$State, $Item>), NoneError>
```

### 役割

1 要素だけ消費して、

- 生成値
- 次の Generator

を返します。

### サンプル

```surtr
gen0 = Generator::range(1, 3)
r1 = Generator::next(gen0)
# => Ok((1, gen1))

(item1, gen1) =? r1
Generator::idx(gen1)
# => 1

Generator::next(gen1)
# => Ok((2, gen2))
```

---

## 3. `Generator::idx`

### シグネチャ

```surtr
def idx<$State, $Item>(gen: Generator<$State, $Item>) -> Int
```

### 役割

次に返す要素の index を返します。

### サンプル

```surtr
gen0 = Generator::range(10, 12)
Generator::idx(gen0)
# => 0

(_, gen1) =? Generator::next(gen0)
Generator::idx(gen1)
# => 1
```

---

## 4. `Generator::with_index`

### シグネチャ

```surtr
def with_index<$State, $Item>(gen: Generator<$State, $Item>)
  -> Generator<$State, (Int, $Item)>
```

### 役割

各要素に index を付与します。

### サンプル

```surtr
gen = Generator::range(5, 7)
indexed = Generator::with_index(gen)

Generator::to_list(indexed)
# => [(0, 5), (1, 6), (2, 7)]
```

`iterate` や `range` のどちらでも同じ index 規約を使えます。

---

## 5. `Generator::map`

### シグネチャ

```surtr
def map<$State, $A, $B>(gen: Generator<$State, $A>, f: ($A -> $B))
  -> Generator<$State, $B>
```

### 役割

Generator の要素だけを変換します。

- 進行状態と `idx` の進み方は維持
- 返却要素型だけ変わる

### サンプル

```surtr
gen = Generator::range(1, 4)
texts = Generator::map(gen, {|x| "n=#{x}" })

Generator::to_list(texts)
# => ["n=1", "n=2", "n=3", "n=4"]
```

---

## 6. `Generator::take`

### シグネチャ

```surtr
def take<$State, $Item>(gen: Generator<$State, $Item>, count: Int)
  -> List<$Item>
```

### 役割

先頭から最大 `count` 個だけ取り出して `List` にします。

### サンプル

```surtr
gen = Generator::iterate(1, 10, {|x| x * 2})
Generator::take(gen, 4)
# => [1, 2, 4, 8]
```

```surtr
gen = Generator::range(1, 3)
Generator::take(gen, 0)
# => []
```

---

## 7. `Generator::to_list`

### シグネチャ

```surtr
def to_list<$State, $Item>(gen: Generator<$State, $Item>) -> List<$Item>
```

### 役割

終了まで全要素を消費して `List` にします。

### サンプル

```surtr
gen = Generator::range(2, 5)
Generator::to_list(gen)
# => [2, 3, 4, 5]
```

---

## 8. `Generator::iterate`

### シグネチャ

```surtr
def iterate<$A>(seed: $A, count: Int, step: ($A -> $A))
  -> Generator<$A, $A>
```

### 役割

`seed` から始めて、前の値へ `step` を繰り返し適用して有限列を生成します。

- 先頭要素は `seed`
- 生成数は `count`
- `count <= 0` なら空 Generator 相当

### サンプル

```surtr
gen = Generator::iterate(1, 5, {|x| x * 2})
Generator::to_list(gen)
# => [1, 2, 4, 8, 16]
```

```surtr
gen = Generator::iterate("a", 4, {|s| s ++ "!" })
Generator::to_list(gen)
# => ["a", "a!", "a!!", "a!!!"]
```

### 実装イメージ

```surtr
def iterate<$A>(seed: $A, count: Int, step: ($A -> $A))
  -> Generator<$A, $A> {
  Generator::unfold(seed, {|current, idx|
    if(idx < count,
      Ok((current, step(current))),
      Err(NoneError)
    )
  })
}
```

---

## 9. `Generator::range`

### シグネチャ

```surtr
def range(start: Int, stop: Int) -> Generator<Int, Int>
```

### 役割

`Int` の連続値を生成します。

- 当面は `start <= stop` の昇順のみを素直な基本形とする
- step は `1`
- 両端含む

### サンプル

```surtr
gen = Generator::range(3, 6)
Generator::to_list(gen)
# => [3, 4, 5, 6]
```

### 実装イメージ

```surtr
def range(start: Int, stop: Int) -> Generator<Int, Int> {
  Generator::unfold(start, {|current, _idx|
    if(current <= stop,
      Ok((current, current + 1)),
      Err(NoneError)
    )
  })
}
```

---

## 10. `Generator::range_step`

### シグネチャ

```surtr
def range_step(start: Int, stop: Int, step: Int)
  -> Result<Generator<Int, Int>, Error>
```

### 役割

step 指定付きの `Int` range を生成します。

- `step > 0` なら昇順
- `step < 0` なら降順
- `step == 0` はエラー

### サンプル

```surtr
gen =? Generator::range_step(1, 7, 2)
Generator::to_list(gen)
# => [1, 3, 5, 7]
```

```surtr
gen =? Generator::range_step(7, 1, -2)
Generator::to_list(gen)
# => [7, 5, 3, 1]
```

```surtr
Generator::range_step(1, 5, 0)
# => Err(InvalidRangeStep(...))
```

### 実装イメージ

```surtr
def range_step(start: Int, stop: Int, step: Int)
  -> Result<Generator<Int, Int>, Error> {
  assert(step != 0, InvalidRangeStep("step must not be 0"))
  Ok(
    Generator::unfold(start, {|current, _idx|
      cond {
        step > 0 and current <= stop => Ok((current, current + step)),
        step < 0 and current >= stop => Ok((current, current + step)),
        True => Err(NoneError)
      }
    })
  )
}
```

---

## 11. `Generator::range_char`

### シグネチャ

```surtr
def range_char(start: String, stop: String)
  -> Result<Generator<String, String>, Error>
```

### 役割

1 文字 `String` を連続生成します。

- `"a".."d"` のような range を想定
- 内部的には codepoint を進める
- 入力が 1 文字でない場合はエラー

### サンプル

```surtr
gen =? Generator::range_char("a", "d")
Generator::to_list(gen)
# => ["a", "b", "c", "d"]
```

```surtr
Generator::range_char("ab", "d")
# => Err(InvalidCharRange(...))
```

### 実装イメージ

```surtr
def range_char(start: String, stop: String)
  -> Result<Generator<String, String>, Error> {
  Generator::range_char_step(start, stop, 1)
}
```

---

## 12. `Generator::range_char_step`

### シグネチャ

```surtr
def range_char_step(start: String, stop: String, step: Int)
  -> Result<Generator<String, String>, Error>
```

### 役割

step 指定付きの文字 range を生成します。

- `String` は 1 文字のみ許可
- `step == 0` はエラー
- 内部的には codepoint を `Int` として扱い、生成時に 1 文字 `String` へ戻す

### サンプル

```surtr
gen =? Generator::range_char_step("a", "g", 2)
Generator::to_list(gen)
# => ["a", "c", "e", "g"]
```

```surtr
gen =? Generator::range_char_step("g", "a", -2)
Generator::to_list(gen)
# => ["g", "e", "c", "a"]
```

### 実装イメージ

```surtr
def range_char_step(start: String, stop: String, step: Int)
  -> Result<Generator<String, String>, Error> {
  start_cp =? String::to_codepoint(start)
  stop_cp =? String::to_codepoint(stop)
  base =? Generator::range_step(start_cp, stop_cp, step)

  Ok(
    Generator::map(base, {|cp| String::from_codepoint(cp) })
  )
}
```

> 注: `String::to_codepoint` / `String::from_codepoint` はここでは説明用の仮 API です。
> 実際には既存の文字列 API に合わせて調整してください。

---

## 想定エラー

サンプルレベルでは、以下のエラーを用意すると整理しやすいです。

```surtr
deferror InvalidRangeStep(message: String) { "#{message}" }
deferror InvalidCharRange(message: String) { "#{message}" }
```

例:

- `InvalidRangeStep("step must not be 0")`
- `InvalidCharRange("start must be a single char")`
- `InvalidCharRange("stop must be a single char")`

---

## range 構文との対応

構文糖衣として次の lower を想定できます。

```surtr
1..5
# => Generator::range(1, 5)

1..10..2
# => Generator::range_step(1, 10, 2)

"a".."d"
# => Generator::range_char("a", "d")

"a".."g"..2
# => Generator::range_char_step("a", "g", 2)
```

必要なら、式文脈では `Generator` のまま扱い、
`List` が必要な箇所で `Generator::to_list(...)` を明示または lower します。

---

## 代表ユースケース

## `iterate` + `with_index`

```surtr
gen = Generator::iterate(10, 4, {|x| x + 10})
indexed = Generator::with_index(gen)

Generator::to_list(indexed)
# => [(0, 10), (1, 20), (2, 30), (3, 40)]
```

## `range` + `map`

```surtr
gen = Generator::range(1, 4)
texts = Generator::map(gen, {|x| "item=#{x}" })

Generator::to_list(texts)
# => ["item=1", "item=2", "item=3", "item=4"]
```

## `next` を使った手動消費

```surtr
gen0 = Generator::range(100, 102)
(v0, gen1) =? Generator::next(gen0)
(v1, gen2) =? Generator::next(gen1)
(v2, gen3) =? Generator::next(gen2)

# v0 = 100
# v1 = 101
# v2 = 102
# Generator::next(gen3) => Err(NoneError)
```

---

## 採用理由

この形の利点は次の通りです。

- `iterate` と `range` を同じ抽象で扱える
- `idx` を Generator 自身が持つため `with_index` が自然
- `List` へ変換する前の逐次処理を共通 API にまとめられる
- `range` 構文を surface sugar として lower しやすい
- 将来 `filter`, `flat_map`, `scan` などへ拡張しやすい

---

## 当面の決め方

実装着手時は、まず次を固定すると進めやすいです。

1. `idx` は「次に返す要素の index」
2. `range` は両端含む
3. `range(start, stop)` は `step = 1` の昇順基本形
4. 降順や飛び幅変更は `range_step` に集約
5. `Char` range は 1 文字 `String` のみ許可
6. `step == 0` は必ず `Err`

