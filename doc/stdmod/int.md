# Int module

`Int` は arbitrary-precision 整数の標準モジュールです。
今回の拡張では、算術の補助関数、bit helper、fixed-width helper を `defmod Int` に集約します。

## Exported types

- `BitWidth`
  - `BitWidth::W8`
  - `BitWidth::W16`
  - `BitWidth::W32`
  - `BitWidth::W64`
  - `BitWidth::W128`
  - `BitWidth::Any(bits)`

## Exported functions

- `Int::safe_div(a, b) -> Result<Int | Float, ZeroDivisionError>`
- `Int::safe_mod(a, b) -> Result<Int, ZeroDivisionError>`
- `Int::bit_and(a, b) -> Int`
- `Int::bit_or(a, b) -> Int`
- `Int::bit_xor(a, b) -> Int`
- `Int::bit_not(value) -> Int`
- `Int::test_bit(value, index) -> Result<Boolean, NegativeBitIndex>`
- `Int::set_bit(value, index) -> Result<Int, NegativeBitIndex>`
- `Int::clear_bit(value, index) -> Result<Int, NegativeBitIndex>`
- `Int::toggle_bit(value, index) -> Result<Int, NegativeBitIndex>`
- `Int::width_bits(width) -> Result<Int, InvalidBitWidth>`
- `Int::mask(width) -> Result<Int, InvalidBitWidth>`
- `Int::wrap_unsigned(value, width) -> Result<Int, InvalidBitWidth>`
- `Int::wrap_signed(value, width) -> Result<Int, InvalidBitWidth>`
- `Int::bit_not_in(value, width) -> Result<Int, InvalidBitWidth>`
- `Int::test_bit_in(value, index, width) -> Result<Boolean>`
- `Int::set_bit_in(value, index, width) -> Result<Int>`
- `Int::clear_bit_in(value, index, width) -> Result<Int>`
- `Int::toggle_bit_in(value, index, width) -> Result<Int>`
- `Int::shl_in(value, bits, width) -> Result<Int>`
- `Int::shr_logical_in(value, bits, width) -> Result<Int>`
- `Int::rotl_in(value, bits, width) -> Result<Int>`
- `Int::rotr_in(value, bits, width) -> Result<Int>`
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
- `NegativeBitIndex(index: Int)`
  - `Int::test_bit` / `set_bit` / `clear_bit` / `toggle_bit` で `index < 0` のときに返します。
  - 表示メッセージは `bit index must be non-negative: #{index}` です。
- `InvalidBitWidth(width: Int)`
  - `BitWidth::Any(width)` は `width > 0` だけを許可します。
  - 表示メッセージは `bit width must be positive: #{width}` です。
- `BitIndexOutOfRange(index: Int, width: Int)`
  - fixed-width helper で `index >= width` のときに返します。
  - 表示メッセージは `bit index #{index} out of range for width #{width}` です。
- `NegativeShiftCount(bits: Int)`
  - `Int::shl` / `Int::shr` で `bits < 0` のときに返します。
  - 表示メッセージは `shift amount must be non-negative: #{bits}` です。
- `bit_not` は pure function で失敗しません。
- `test_bit` / `set_bit` / `clear_bit` / `toggle_bit` は pure ですが、負 index だけは `Err(NegativeBitIndex(...))` を返します。
- fixed-width helper は pure ですが、`InvalidBitWidth`, `BitIndexOutOfRange`, `NegativeShiftCount` を recoverable error として返します。

## Examples

```surtr
print(to_string(Int::abs(-5)))
print(to_string(Int::bit_and(6, 3)))
print(to_string(Int::bit_not(6)))
print(inspect(Int::test_bit(5, 2)))
print(inspect(Int::wrap_unsigned(-1, BitWidth::W8)))
print(inspect(Int::rotl_in(129, 1, BitWidth::W8)))
print(to_string(Int::shl(1, 3)))
print(to_string(Int::sign(0)))
print(to_string(Int::is_even(8)))
```

## Notes

- `is_even` / `is_odd` は `safe_mod(value, 2)` を土台にした pure Surtr 実装です。
- `bit_and` / `bit_or` / `bit_xor` / `bit_not` は surface は builtin 関数のまま公開し、direct call は VM の専用 Opcode に lower されます。
- `test_bit` / `set_bit` / `clear_bit` / `toggle_bit` は index 検証つきの `Result` helper としてまず `CallBuiltin` で公開します。
- `BitWidth` を受ける `_in` family は既存の unbounded helper とは別系統です。
- fixed-width helper は `value` を unsigned range に wrap してから処理します。
- `bit_not_in(6, BitWidth::W8)` は `249` を返します。
