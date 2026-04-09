```
defenum Color {
    Red,
    Green,
    Blue
}

#現時点でTraitは未対応なので、関数を自動で定義する方式を採用。
try_from(Int) -> Result<Color>
from(Int) -> Color
inspect


デフォルトは0オリジン

defenum Color {
    Red = 1,
    Green = 4,
    Blue = 16
}

バリアント毎に指定できる。

1オリジン
defenum Color {
    Red = 1,
    Green,
    Blue
}

TapleVariant
defenum KeyInput {
  Arrow(Direction)
  Enter
  Space
}

defenum Direction {
  Up
  Down
  Left
  Right
}

key = KeyInput::Arrow(Direction::Up)
* eq
* neq

arrow = Direction::Down
* .idx (0オリジン)

レコードも指定可能。
定義済みのレコードは列挙不可(今後対応可能にする)
```