# Range 標準型 実装メモと制約

## 目的

`Range` を標準ライブラリへ追加し、`[start..stop]` の list range literal とは別に、
「境界だけを保持する値」を導入するための実装メモを残す。

今回の実装 API は次のとおり。

- `Range::new(min, max)`
- `Range::normalized(a, b)`
- `impl Compare for Range<Int>`
- `impl Eq for Range<Int>`
- `impl Neq for Range<Int>`
- `Range::advance(range, steps)` / `Range::retreat(range, steps)` の Int 専用移動
- `Range(min, max)` pattern を支える `deconstruct`

## 先に確定してよい仕様

- `Range` は `[start..stop]` の sugar ではない
- `[start..stop]` は従来どおり展開済み `List`
- `Range` は「境界だけ持つ値」
- `new` は引数順を保存する
- `normalized` は `compare(a, b)` で昇順化する
- `Compare` は包含順序ではなく辞書順
  - まず `min`、同値なら `max`
- `Eq` / `Neq` は構造的等値
- ただし現行実装では比較 trait は `Range<Int>` に限定する
- 平行移動は `Range + Range` ではなく、`Range<Int>` の境界を同じだけ動かす helper として扱う

## 今回確認できた実装上の制約

### 1. `new` による構築境界は field modifier ではなく struct literal 制約で守られる

Surtr では `Type { ... }` 構造体リテラルが `impl Type` の中でしか使えない。
そのため「公開された構築入口を `new` に寄せる」目的は、
field modifier の有無に関係なく既に成立している。

関連ドキュメント:

- `docs/site/structs.md`

つまり `Range` で field を公開しても、
構築 API を `new` / `normalized` に寄せる方針自体は維持できる。

### 2. `Range` は public field の plain struct として扱うのが自然

`Duration` のような値は内部不変条件を強く持つため、`private` field にして
constructor 経由へ閉じる意味が大きい。

一方 `Range` は今回の想定では次の性質を持つ。

- `new(min, max)` は引数順を保存する
- `normalized(a, b)` は convenience API であり、唯一の正規表現ではない
- `new(3, 1)` も有効な値として保持する
- `impl Compare` により構造体など広い型が入りうる
- `Facet` などで field access を活かしたい

このため、`readonly` で mutation 経路を狭めるより、
public field の plain struct として定義し、意味づけは API と利用者側に委ねる方が
Surtr の設計に合う。

この方針では次を採る。

- `defstruct Range<$A> { min: $A, max: $A }`
- `min/max` は public
- `new` / `normalized` / `deconstruct` / `advance` / `retreat` を API として提供する
- field を直接更新して `min > max` になっても、それは compile error や runtime error ではなく、
  利用者が選んだ `Range` の状態として扱う

### 3. generic `defstruct` 対応が前提になる

`Range` を当初想定どおり `Range<$A>` で実装するには、
generic `defstruct` が言語機能として使える必要がある。

現時点では、この改修を進めている前提で `Range` 設計を進める。
したがって `Range` 側では `defenum` への載せ替えや monomorphic への縮退は考えない。

### 4. `Add` / `Sub` trait では `Range + Scalar` を表現できない

既存 `Add` / `Sub` は `rhs: Self` 制約なので、
`Range<Int> + Int` を trait で素直に表現できない。

さらに `add` / `sub` という名前は operator helper (`Add::add`, `Sub::sub`) の語彙と衝突しやすい。
そのため、初版は「平行移動」を表す通常関数にするのが自然。

候補:

- `Range::advance(self: Range<Int>, rhs: Int) -> Range<Int>`
- `Range::retreat(self: Range<Int>, rhs: Int) -> Range<Int>`

もし method surface で `self: Range<Int>` が受け付けられない場合は、
さらに次へ落とす。

- `Range::advance(range: Range<Int>, rhs: Int) -> Range<Int>`
- `Range::retreat(range: Range<Int>, rhs: Int) -> Range<Int>`

### 4.5. generic な比較 trait 実装は現行コンパイラ制約に当たる

`impl Compare for Range<impl Compare>` のような surface は構文上は書けるが、
現時点では runtime で `Call arity mismatch` に当たり安定しない。

そのため今回の着地では:

- `Range` 自体は `Range<$A>` の generic struct として提供する
- `Range::normalized` / `Range::deconstruct` は generic のまま提供する
- `Compare` / `Eq` / `Neq` は `Range<Int>` に限定して実装する

完全な generic 比較は別タスクで compiler/runtime 側を詰めてから再拡張する。

### 5. 標準型追加は preload 配線が複数箇所にまたがる

新しい std type module を足すときは、少なくとも次を揃える必要がある。

- `crates/xldr/src/loader.rs`
- `crates/forge/src/lib.rs`
- `crates/scar/tests/support/mod.rs`

`Range` 本体だけ追加しても、preload 経路を揃えないと
REPL / compiler test support / semantic snapshot がずれる。

`Range` は `@builtin def` / `@builtin type` を持たない pure stdlib module なので、
`crates/eldr/src/builtin.rs` の builtin surface alignment 対応は不要。

### 6. テストは `lib/tests` に置くのが素直

標準ライブラリ回帰としては `lib/tests/range.srt` が第一候補。
必要なら `lib/tests/spec.srt` から include して aggregate suite にも入れる。

最小確認フロー:

```bash
cargo run -q -p rune -- test range
cargo nextest run --workspace
```

## 実装するなら次の順が安全

1. `Range` の実体表現を確定する
   - `defstruct Range<$A>` を前提に進める
2. `new` / `normalized` / `deconstruct` の surface を確定する
3. `Compare` / `Eq` / `Neq` を `Range<Int>` で固定する
4. `Int` 専用 `advance/retreat` を通常関数で載せる
5. preload 配線を 3 箇所に追加する
6. `lib/tests/range.srt` と必要最小限の aggregate 連携を足す

## 今回の結論

`Range` は generic `defstruct` 前提で実装する。

field は `readonly` にせず public のままにし、
不変条件を型側で強制するよりも、`new` / `normalized` などの API を公開して
利用者がどの意味で range を持つかを選べる形に寄せる。

つまり、`Range` は `Duration` のような「常に内部不変条件を守らせる値」ではなく、
「比較・変換 API を持つ plain data」として扱う。

ただし比較 trait だけは現状の実装制約に合わせて `Range<Int>` 限定で提供する。

追加後も `[start..stop]` literal は従来どおり `List` / `Result<List<String>, Error>` 側へ lower され、
`Range` には下がらない。
