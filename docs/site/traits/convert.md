# Convert / TryConvert

`Convert<$To>` と `TryConvert<$To>` は、値を明示的に別の型へ変換する標準 Trait です。

```surtr
deftrait Convert<$To> {
  def to::<$To>(self: Self) -> $To
}

deftrait TryConvert<$To> {
  def try_to::<$To>(self: Self) -> Result<$To, Error>
}
```

`Self` は変換元、ReturnTypeArgument の `$To` は変換先です。呼び出しでは変換先を直接指定します。

```surtr
text = to::<String>(42)
number = try_to::<Int>("42")
```

`Convert` は失敗しない変換、`TryConvert` は失敗を `Result` で返す変換に使います。同じ変換元・変換先の型ペアへ両方を実装することはできません。

## パイプライン

変換先が呼び出し名の直後に現れるため、パイプラインでも変換後の型を追えます。

```surtr
Ok("42")
|>= try_to::<Int>()
|>= {|number| Ok(number + 1)}
```

`try_to` は `Result` を返すため、`|>=` で後続の失敗可能な処理へ接続できます。失敗しない `to` は通常の値パイプラインで使用できます。

```surtr
42 |> to::<String>() |> print()
```

## ReturnTypeArgument

`::<$To>` は通常の値引数ではなく、変換先を選ぶ ReturnTypeArgument です。generic な変換先は、変換元と実装が共有する型引数をすべて決定できる場合に bare head でも指定できます。

```surtr
value: Option<Int> = Option::Some(1)
result: Result<Int> = to::<Result>(value)
```

型引数が決まらない場合は曖昧な候補へフォールバックせず、型エラーになります。完全な型を指定して解決することもできます。

```surtr
result: Result<Int> = to::<Result<Int>>(value)
```

## 関連項目

- [Trait システム](../trait-system.md)
- [トレイト実装](../trait-impls.md)
- [エラーハンドリング](../error-handling.md)
- [パイプ演算子](../pipe-operators.md)

標準定義の正本は `lib/traits/convert.srt` と `lib/traits/try_convert.srt` の `@doc` です。
