# Reader

`Reader<R, A>` は環境 `R` から値 `A` を作る関数を保持する標準型です。環境は
`run` の引数として明示的に渡します。設定 process や registry を暗黙に参照しません。

```surtr
reader: Reader<Int, Int> = Reader::asks({|environment| environment + 2})
Reader::run(reader, 10) # 12
```

## API

| 関数 | 型 | 意味 |
|---|---|---|
| `Reader::new` | `(R -> A) -> Reader<R, A>` | 環境関数を保持する |
| `Reader::run` | `(Reader<R, A>, R) -> A` | 環境を渡して実行する |
| `Reader::ask` | `() -> Reader<R, R>` | 環境全体を読む |
| `Reader::asks` | `(R -> A) -> Reader<R, A>` | 環境の射影を読む |
| `Reader::local` | `(Reader<R, A>, (R -> R)) -> Reader<R, A>` | 一つの Reader が見る環境だけを変換する |

`ask` は戻り値だけでは環境型を決められない場合があるため、型を明示します。

```surtr
asked: Reader<Int, Int> = Reader::ask::<Int>()
Reader::run(asked, 4) # 4
```

`local` は元の Reader と呼び出し元の環境を変更しません。

```surtr
original: Reader<Int, Int> = Reader::asks({|n| n * 2})
localized = Reader::local(original, {|n| n + 10})

Reader::run(original, 3)  # 6
Reader::run(localized, 3) # 26
```

## Functor / Applicative / Monad

`Reader` は `Functor`、`Applicative`、`Monad` を実装します。同じ計算の中では
環境型 `R` は変わりません。

```surtr
first: Reader<Int, Int> = Reader::new({|environment| environment + 1})
chained: Reader<Int, Int> = first |>= {|value|
  Reader::new({|environment| value + environment})
}

Reader::run(chained, 5) # 11
```

- `fmap` は環境関数の結果を変換する
- `pure` と `Monad::return` は環境を使わず値を返す
- `ap` は mapper と value の両方へ同じ環境を渡す
- `bind` は前段と後段へ同じ環境を渡し、前段の値を環境として扱わない

これらは同じ環境を観測した結果について、通常の Functor、Applicative、Monad の法則に従います。

## 制約

`Reader` に標準の `Alternative`、`Eq`、`Show` はありません。関数値の同一性を
closure identity で定義せず、payload にも `Eq` や `Show` の制約を要求しません。

保持した関数 field を直接呼ぶ必要がある場合、関数値 property の呼び出しには
`Function::apply` を使います。通常は `Reader::run` を利用してください。

