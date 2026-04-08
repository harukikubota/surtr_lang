```
defenum Color {
    Red,
    Green,
    Blue
}

try_from<Int>(Int) -> Result<Color>
from<Color>(Int) -> Color


デフォルトは0オリジン
```