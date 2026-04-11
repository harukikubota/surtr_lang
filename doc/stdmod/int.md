# Int module

`Int` は arbitrary-precision 整数の標準モジュールです。
今回の拡張では、算術の補助関数と述語を `defmod Int` に集約します。

## Exported functions

- `Int::safe_div(a, b) -> Result<Int | Float, ZeroDivisionError>`
- `Int::safe_mod(a, b) -> Result<Int, ZeroDivisionError>`
- `Int::bit_and(a, b) -> Int`
- `Int::bit_or(a, b) -> Int`
- `Int::bit_xor(a, b) -> Int`
- `Int::shl(value, bits) -> Result<Int, NegativeShiftCount>`
- `Int::shr(value, bits) -> Result<Int, NegativeShiftCount>`
- `Int::abs(value) -> Int`
- `Int::min(a, b) -> Int`
- `Int::max(a, b) -> Int`
- `Int::sign(value) -> Int`
- `Int::is_even(value) -> Boolean`
- `Int::is_odd(value) -> Boolean`

## Error contract

- `safe_div` / `safe_mod` は `Err(ZeroDivisionError)` を返します。
- `NegativeShiftCount(bits: Int)`
  - `Int::shl` / `Int::shr` で `bits < 0` のときに返します。
  - 表示メッセージは `shift amount must be non-negative: #{bits}` です。
- 今回追加した helper は pure function で、追加エラーは返しません。

## Examples

```surtr
print(to_string(Int::abs(-5)))
print(to_string(Int::bit_and(6, 3)))
print(to_string(Int::shl(1, 3)))
print(to_string(Int::sign(0)))
print(to_string(Int::is_even(8)))
```

## Notes

- `is_even` / `is_odd` は `safe_mod(value, 2)` を土台にした pure Surtr 実装です。
- `bit_and` / `bit_or` / `bit_xor` は surface は builtin 関数のまま公開し、direct call は VM の専用 Opcode に lower されます。
