# Boolean module

`Boolean` は真理値の補助関数をまとめる標準モジュールです。
`if` と組み合わせやすい connective を `defmod Boolean` に置きます。

## Exported functions

- `Boolean::not(value) -> Boolean`
- `Boolean::xor(left, right) -> Boolean`
- `Boolean::eqv(left, right) -> Boolean`
- `Boolean::implies(left, right) -> Boolean`

## Error contract

- すべて pure function で、追加エラーは返しません。

## Examples

```surtr
print(to_string(Boolean::not(False)))
print(to_string(Boolean::xor(True, False)))
print(to_string(Boolean::implies(True, False)))
```

## Notes

- `xor` / `eqv` / `implies` は short helper として提供し、surface 側で論理式の意図を読みやすくします。
