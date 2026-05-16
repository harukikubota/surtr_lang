# Float

Surtr の `Float` は、有限値だけを扱う `f64` ラッパーです。

狙いは「小数点を含む計算を自然に書けること」であり、`NaN` や `Infinity` を
language surface に持ち込むことではありません。

## 契約

- `Float` literal は有限値だけを受け入れます
- `safe_div(left, right)` は `right == 0.0` のとき `Err(ZeroDivisionError)` を返します
- builtin constant と runtime arithmetic も non-finite value を返しません
- 表示は通常の `f64` 表示を基礎にし、整数値に見える場合は `.0` を補います

## 基本

```surtr
left = 1.5
right = 2.0

print(to_string(left + right))
print(inspect(safe_div(3.0, 2.0)))
```

```text
3.5
Ok(1.5)
```

## helper

`Float` には次の helper があります。

- `Float::abs(value)`
- `Float::min(a, b)`
- `Float::max(a, b)`
- `Float::floor(value)`
- `Float::ceil(value)`
- `Float::round(value)`
- `Float::trunc(value)`
- `Float::pi()`
- `Float::e()`

```surtr
print(to_string(Float::floor(1.8)))
print(to_string(Float::ceil(1.2)))
print(to_string(Float::round(-1.5)))
print(to_string(Float::trunc(-1.8)))
print(to_string(Float::pi()))
print(to_string(Float::e()))
```

```text
1.0
2.0
-2.0
-1.0
3.141592653589793
2.718281828459045
```

## 比較

`Float` は `Eq`, `Neq`, `Compare`, `Numeric` を実装しています。

```surtr
print(to_string(1.5 < 2.0))
print(to_string(compare(1.5, 2.0)))
```

```text
True
Ordering::Less
```

## JSON

JSON の decimal / exponent number は `JsonValue::Float(Float)` として読みます。
ここでも finite-only の契約は同じです。
