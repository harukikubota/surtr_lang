# Identity

`Identity<A>` は値を一つだけ保持する標準型です。追加の作用や暗黙の変換はなく、
`run` は保持した値をそのまま取り出します。

```surtr
value: Identity<Int> = Identity::new(7)
Identity::run(value) # 7
```

## API

| 関数 | 型 | 意味 |
|---|---|---|
| `Identity::new` | `A -> Identity<A>` | 値を包む |
| `Identity::run` | `Identity<A> -> A` | 値を取り出す |

## Functor / Applicative / Monad

`Identity` は `Functor`、`Applicative`、`Monad` を実装します。

```surtr
mapped: Identity<Int> = Identity::new(7) |*> {|n| n + 1}
applied: Identity<Int> = Applicative::ap(
  Identity::new({|n| n * 2}),
  mapped,
)
bound: Identity<Int> = applied |>= {|n| Identity::new(n + 3)}

Identity::run(bound) # 19
```

- `fmap` は保持した値へ関数を一度適用する
- `pure` と `Monad::return` は値を `Identity` で包む
- `ap` は包まれた関数を包まれた値へ適用する
- `bind` は mapper が返した `Identity` をそのまま返し、二重に包まない

これらは通常の Functor、Applicative、Monad の恒等則・合成則・単位元則・結合則に従います。

## 制約

`Identity` に標準の `Alternative`、`Eq`、`Show` はありません。payload にも
`Eq` や `Show` の制約を要求しません。

`Identity` は通常の構造体値なので、property、tuple、list、`Facet` の値として扱えます。

