# Float module

`Float` は暫定仕様のまま維持しつつ、丸め規約に依存しない小さな helper を提供する標準モジュールです。

## Exported functions

- `Float::abs(value) -> Float`
- `Float::min(a, b) -> Float`
- `Float::max(a, b) -> Float`

## Error contract

- すべて pure function で、追加エラーは返しません。

## Examples

```surtr
print(to_string(Float::abs(-1.5)))
print(to_string(Float::min(2.5, 1.25)))
print(to_string(Float::max(2.5, 1.25)))
```

## Notes

- 今回は `Float` の厳密契約を広げず、比較と減算だけで表現できる helper に限定します。
