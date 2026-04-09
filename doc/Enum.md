# Enum 仕様メモ（2026-04-09 実装反映）

## 1. 宣言

```surtr
defenum Color {
  Red,
  Green,
  Blue,
}
```

- `defenum` はトップレベル宣言。
- 少なくとも 1 つのバリアントが必要。
- 各バリアントは次を取れる。

```surtr
# unit variant
Red

# tuple variant
Arrow(Direction)

# discriminant 明示
Blue = 16
```

## 2. discriminant（`.idx`）

- デフォルトは `0` 起点で自動採番。
- 明示値を入れた場合、次の未指定バリアントはその値 + 1。
- `.idx` はそのバリアントの discriminant (`Int`) を返す。

```surtr
defenum Code {
  A = 1,
  B,
  C = 10,
  D,
}

print(to_string(Code::B.idx)) # 2
print(to_string(Code::D.idx)) # 11
```

## 3. 生成・比較・表示

- 値生成は `Enum::Variant(...)`。
- 引数なしバリアントは `Enum::Variant` でも値として使える。
- 同一 enum 同士の `==` / `!=` をサポート。
- `to_string` / `inspect` は enum 値を表示できる。
  - 例: `Up`, `Arrow(Left)`

## 4. match

- enum に対する `match` は網羅必須。
- パターンは `Enum::Variant(...)` を使う。

```surtr
match key {
  KeyInput::Arrow(dir) => to_string(dir.idx),
  KeyInput::Enter => "E",
  KeyInput::Space => "S",
}
```

## 5. 型循環（重要）

現行実装の循環判定は次。

- `defstruct` / `defrecord` / `deferror`: 従来どおり循環参照を禁止。
- `defenum`: **全バリアントに共通して現れる参照型**だけを循環判定グラフに入れる。

つまり、質問の通り:

- 全バリアントが同じ型参照を含むなら循環エラー。
- 1 つでもその参照を含まないバリアントがあれば通る。

```surtr
# 通る
defenum Loop {
  End,
  Next(Loop),
}

# 落ちる（全バリアントが Loop を参照）
defenum Bad {
  A(Bad),
  B(Bad),
}
```

## 6. 現時点の未対応

- `Enum::from(Int)` / `Enum::try_from(Int)` の自動生成は未実装。
- バリアント内での inline レコード定義は未対応（既存 `defrecord` / `defstruct` 型参照は可）。
