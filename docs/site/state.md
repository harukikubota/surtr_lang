# State

`State<S, A>` は状態 `S` を受け取り、値と次状態 `(A, S)` を返す純粋な状態遷移を
保持する標準型です。可変オブジェクトや process handle ではなく、次状態は
`run` の戻り値として明示されます。

```surtr
action: State<Int, Int> = State::new({|state| (state + 1, state + 2)})
State::run(action, 7) # (8, 9)
```

## API

| 関数 | 型 | 意味 |
|---|---|---|
| `State::new` | `(S -> (A, S)) -> State<S, A>` | 状態遷移を保持する |
| `State::run` | `(State<S, A>, S) -> (A, S)` | 値と次状態を返す |
| `State::eval` | `(State<S, A>, S) -> A` | 一度実行し、値だけを返す |
| `State::exec` | `(State<S, A>, S) -> S` | 一度実行し、次状態だけを返す |
| `State::get` | `() -> State<S, S>` | 現在の状態を読む |
| `State::gets` | `(S -> A) -> State<S, A>` | 状態の射影を読む |
| `State::put` | `S -> State<S, Unit>` | 状態を置き換える |
| `State::modify` | `(S -> S) -> State<S, Unit>` | 状態を関数で更新する |

```surtr
current: State<Int, Int> = State::get()
State::run(current, 7)                         # (7, 7)
State::run(State::gets({|state| state * 2}), 7) # (14, 7)
State::run(State::put(11), 7)                   # ((), 11)
State::run(State::modify({|state| state + 1}), 7) # ((), 8)
```

`get` の状態型をその入力だけで決められない場合、REPL は後続入力を待たず ambiguity
として拒否します。型注釈または `State::get::<Int>()` のような型引数で具体化してください。

## Functor / Applicative / Monad

`State` は `Functor`、`Applicative`、`Monad` を実装します。一つの計算の中では
状態型 `S` は変わらず、遷移は左から右へ進みます。

```surtr
action: State<Int, Int> = State::get() |>= {|value|
  State::put(value + 1) |>= {|_| Applicative::pure(value)}
}

State::run(action, 10) # (10, 11)
```

- `fmap` は値だけを変換し、遷移が返した次状態を保つ
- `pure` と `Monad::return` は状態を変更しない
- `ap` は mapper の遷移を先に実行し、その次状態を value の遷移へ渡す
- `bind` は前段の値を mapper へ渡し、前段の次状態を後段へ渡す

これらは同じ初期状態から実行した結果について、通常の Functor、Applicative、Monad
の法則に従います。`eval` と `exec` はそれぞれ対象の遷移を一度だけ実行します。

## 制約

`State` に標準の `Alternative`、`Eq`、`Show` はありません。関数値の同一性を
closure identity で定義せず、payload にも `Eq` や `Show` の制約を要求しません。

保持した関数 field を直接呼ぶ必要がある場合、関数値 property の呼び出しには
`Function::apply` を使います。通常は `State::run`、`eval`、`exec` を利用してください。

