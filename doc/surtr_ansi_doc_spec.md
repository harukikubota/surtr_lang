# Surtr ANSI Doc 標準ライブラリ仕様案

## 概要

この仕様は、Surtr の **REPL / CLI / TUI / VSCode ポップアップ** で共通利用できる  
ANSI エスケープ組み立て API の標準ライブラリ案です。

---

## 設計方針

### 1. 公開面は `Doc` ベース
利用者に見せるのは「バッファ」ではなく、**整形済み表示値** です。

- `Ansi.Doc`: スタイル付きテキスト
- `Ansi.Style`: スタイル定義
- `Ansi.Color`: 色定義

### 2. レンダリング先を分離
同じ `Ansi.Doc` を、出力先に応じて変換します。

- CLI: ANSI付き文字列にレンダリング
- TUI: plain文字列やsegment列に変換
- VSCodeポップアップ: plain文字列または別描画形式へ変換

### 3. 低レベルAPIは補助
`push/pop/reset` 的なAPIは主APIにしません。  
主APIは **値の生成** と **合成** に寄せます。

### 4. REPLの表示値として使える
`Inspect.inspect(value)` が `Ansi.Doc` を返せるようにして、  
REPL側で `render_with(profile)` を行う構成を想定します。

---

## 公開モジュール構成

```surtr
Ansi
Ansi.Doc
Ansi.Style
Ansi.Color
Ansi.Profile
Ansi.Raw
```

---

## 型

### `Ansi.Doc`

スタイル付きテキストを表す不透明型。

```surtr
type Ansi.Doc
```

想定用途:

- REPL表示値
- CLIログの整形
- エラー表示
- 構文ハイライト済み断片の合成

### `Ansi.Style`

文字装飾の定義を表す不透明型。

```surtr
type Ansi.Style
```

### `Ansi.Color`

色指定を表す不透明型。

```surtr
type Ansi.Color
```

### `Ansi.Profile`

描画先の能力差を表すプロファイル。

```surtr
type Ansi.Profile
```

想定プロファイル:

- `plain`: 装飾なし
- `ansi16`: 基本16色
- `ansi256`: 256色
- `truecolor`: 24bit色

---

## API仕様

## 1. Doc生成

```surtr
def Ansi.empty() -> Ansi.Doc
def Ansi.text(String) -> Ansi.Doc
def Ansi.line(String) -> Ansi.Doc
def Ansi.nl() -> Ansi.Doc
def Ansi.space() -> Ansi.Doc
```

### 意味

- `empty`: 空Doc
- `text`: 生テキストをDoc化
- `line`: `text(s) ++ nl()` の短縮
- `nl`: 改行
- `space`: 半角スペース1個

---

## 2. Doc合成

```surtr
def Ansi.append(Ansi.Doc, Ansi.Doc) -> Ansi.Doc
def Ansi.concat(List<Ansi.Doc>) -> Ansi.Doc
def Ansi.join(List<Ansi.Doc>, Ansi.Doc) -> Ansi.Doc
```

### 意味

- `append(a, b)`: Doc同士を連結
- `concat(xs)`: 順に連結
- `join(xs, sep)`: 区切り付き連結

---

## 3. Style適用

```surtr
def Ansi.style(Ansi.Doc, Ansi.Style) -> Ansi.Doc
def Ansi.styled(String, Ansi.Style) -> Ansi.Doc
```

### 意味

- `style(doc, style)`: Doc全体にスタイル適用
- `styled(text, style)`: `text |> style`

---

## 4. Style構築

```surtr
def Style.empty() -> Ansi.Style

def Style.fg(Ansi.Style, Ansi.Color) -> Ansi.Style
def Style.bg(Ansi.Style, Ansi.Color) -> Ansi.Style

def Style.bold(Ansi.Style) -> Ansi.Style
def Style.dim(Ansi.Style) -> Ansi.Style
def Style.italic(Ansi.Style) -> Ansi.Style
def Style.underline(Ansi.Style) -> Ansi.Style
def Style.reverse(Ansi.Style) -> Ansi.Style
def Style.strike(Ansi.Style) -> Ansi.Style
```

### 意味

`Style` は不変値として扱い、関数合成で組み立てる。

---

## 5. Color構築

```surtr
def Color.default() -> Ansi.Color

def Color.black() -> Ansi.Color
def Color.red() -> Ansi.Color
def Color.green() -> Ansi.Color
def Color.yellow() -> Ansi.Color
def Color.blue() -> Ansi.Color
def Color.magenta() -> Ansi.Color
def Color.cyan() -> Ansi.Color
def Color.white() -> Ansi.Color

def Color.index(Int) -> Result<Ansi.Color, ColorRangeError>
def Color.rgb(Int, Int, Int) -> Result<Ansi.Color, ColorRangeError>
```

### 制約

- `Color.index(n)` は `0..255`
- `Color.rgb(r, g, b)` は各 `0..255`

---

## 6. レンダリング

```surtr
def Ansi.render(Ansi.Doc) -> String
def Ansi.render_with(Ansi.Doc, Ansi.Profile) -> String
def Ansi.plain(Ansi.Doc) -> String
def Ansi.width(Ansi.Doc) -> Int
```

### 意味

- `render`: デフォルトプロファイルで文字列化
- `render_with`: 指定プロファイルで文字列化
- `plain`: ANSIなしの文字列を返す
- `width`: 表示幅を返す

### 幅計算の扱い

`width` は **エスケープ列を幅に含めない**。  
将来的に以下も吸収できる設計が望ましいです。

- 全角文字
- combining mark
- emoji width
- 改行

ただし初期実装は以下でも可です。

- ANSI列は無視
- 基本ASCIIを1幅
- 改行は行分割扱い、最大幅または最終行幅のどちらを返すか仕様で固定

### 推奨

初期仕様では明確さのため、`width` は **単一行Doc向け** とし、  
複数行の扱いは将来 `line_widths(doc)` などに分離してもよいです。

---

## 7. Profile

```surtr
def Profile.plain() -> Ansi.Profile
def Profile.ansi16() -> Ansi.Profile
def Profile.ansi256() -> Ansi.Profile
def Profile.truecolor() -> Ansi.Profile
```

### ダウングレード方針

`render_with` は指定プロファイルで表現不能な色を適切に縮退させます。

例:

- truecolor -> ansi256
- ansi256 -> ansi16
- ansi16 -> plain

---

## 8. Raw API

低レベルの逃げ道としてのみ提供します。

```surtr
def Ansi.Raw.sgr(List<Int>) -> Ansi.Doc
def Ansi.Raw.csi(String) -> Ansi.Doc
def Ansi.Raw.reset() -> Ansi.Doc
```

### 方針

- 標準用途では使わない
- 端末制御や実験用途向け
- `Ansi.Doc` に合流できるようにする

---

## 推奨しない公開API

以下は主APIにしない方がよいです。

### 色ごとのショートカット乱立

```surtr
def Ansi.red(String) -> Ansi.Doc
def Ansi.green(String) -> Ansi.Doc
```

これは組み合わせ増加に弱く、APIがすぐ膨張します。

### 状態的な push/pop 主体

```surtr
def push_red(Buffer) -> Buffer
def pop(Buffer) -> Buffer
```

- reset漏れ
- ネスト崩壊
- 可読性低下

を招きやすいため、公開の中心には置きません。

### IO抱き込み

```surtr
def Ansi.println(Ansi.Doc) -> ()
```

整形と出力を混ぜない方が再利用しやすいです。

---

## REPL統合

REPL表示値として `Ansi.Doc` を使えるようにします。

```surtr
def Inspect.inspect($A) -> Ansi.Doc
```

これにより、以下を分離できます。

- 値の見せ方
- 出力先
- 色能力
- plainフォールバック

### REPL側の責務

```surtr
doc = Inspect.inspect(value)
text = Ansi.render_with(doc, profile)
IO.print(text)
```

---

## TUI / VSCode への適用方針

## CLI
`Ansi.render_with(doc, terminal_profile)` を使う。

## TUI
最低限は `Ansi.plain(doc)` を使える。  
将来的には内部segment列を直接取り出すAPIを追加してもよい。

## VSCodeポップアップ
ANSI対応に依存せず、まずは `Ansi.plain(doc)` を使う。  
将来的に semantic token や decoration 向け変換を追加できる。

---

## 将来拡張候補

現時点では必須でないもの:

```surtr
def Ansi.indent(Int, Ansi.Doc) -> Ansi.Doc
def Ansi.repeat(Ansi.Doc, Int) -> Ansi.Doc
def Ansi.pad_left(Ansi.Doc, Int) -> Ansi.Doc
def Ansi.pad_right(Ansi.Doc, Int) -> Ansi.Doc
def Ansi.truncate(Ansi.Doc, Int) -> Ansi.Doc
```

また、TUIやエディタ向けには次も候補です。

```surtr
def Ansi.segments(Ansi.Doc) -> List<Ansi.Segment>
```

ただし最初から公開しなくても構いません。

---

## 内部実装メモ
想定内部構造の例:

```text
DocImpl
- buffer: StringBuffer
- visible_width: Int
- needs_reset: Bool
```

またはより意味的な実装:

```text
DocImpl
- segments: List<Segment>
```

### 初期実装としての推奨
まずは **bufferベース** で開始し、必要になったら segment へ移行。

理由:

- 実装が軽い
- CLI用途に十分
- REPL統合が早い

---

## サンプルコード

## 1. タイトルとエラー表示

```surtr
title_style =
  Style.empty()
  |> Style.fg(Color.blue())
  |> Style.bold()

error_style =
  Style.empty()
  |> Style.fg(Color.red())
  |> Style.bold()

doc =
  Ansi.concat([
    Ansi.styled("CompileError", error_style),
    Ansi.text(": "),
    Ansi.styled("src/main.surtr", title_style),
    Ansi.nl(),
    Ansi.text("unexpected token")
  ])

text = Ansi.render(doc)
```

---

## 2. joinによるメッセージ整形

```surtr
info_style =
  Style.empty()
  |> Style.fg(Color.cyan())

parts = [
  Ansi.styled("[INFO]", info_style),
  Ansi.text("server started"),
  Ansi.text("port=3000")
]

doc = Ansi.join(parts, Ansi.space())
text = Ansi.render(doc)
```

---

## 3. プロファイルごとの描画

```surtr
accent =
  Style.empty()
  |> Style.fg(Result.unwrap(Color.rgb(120, 180, 255)))
  |> Style.bold()

doc = Ansi.styled("Surtr REPL", accent)

terminal_text = Ansi.render_with(doc, Profile.truecolor())
plain_text = Ansi.render_with(doc, Profile.plain())
```

> `Result.unwrap` はあくまでサンプル表現です。  
> 実際の Surtr では `match` またはResult系APIに置き換えてください。

---

## 4. Inspect統合

```surtr
def Inspect.inspect_int(value: Int) -> Ansi.Doc
  number_style =
    Style.empty()
    |> Style.fg(Color.yellow())

  Ansi.styled(Int.to_string(value), number_style)
end

def Inspect.inspect_result(value: Result<$A, $E>) -> Ansi.Doc
  match value
    Ok(inner) ->
      Ansi.concat([
        Ansi.styled("Ok", Style.empty() |> Style.fg(Color.green()) |> Style.bold()),
        Ansi.text("("),
        Inspect.inspect(inner),
        Ansi.text(")")
      ])

    Err(err) ->
      Ansi.concat([
        Ansi.styled("Err", Style.empty() |> Style.fg(Color.red()) |> Style.bold()),
        Ansi.text("("),
        Inspect.inspect(err),
        Ansi.text(")")
      ])
  end
end
```

---

## 5. REPLでの利用イメージ

```surtr
def repl_print(value, profile) -> ()
  doc = Inspect.inspect(value)
  text = Ansi.render_with(doc, profile)
  IO.print(text)
end
```

---

## 6. VSCodeポップアップ向けフォールバック

```surtr
def hover_text(value) -> String
  Inspect.inspect(value)
  |> Ansi.plain()
end
```

---

## 7. Raw APIの限定利用

```surtr
doc =
  Ansi.concat([
    Ansi.Raw.sgr([31, 1]),
    Ansi.text("manual red bold"),
    Ansi.Raw.reset()
  ])
```

---

## 参考の公開API一覧

```surtr
module Ansi
  def empty() -> Ansi.Doc
  def text(String) -> Ansi.Doc
  def line(String) -> Ansi.Doc
  def nl() -> Ansi.Doc
  def space() -> Ansi.Doc

  def append(Ansi.Doc, Ansi.Doc) -> Ansi.Doc
  def concat(List<Ansi.Doc>) -> Ansi.Doc
  def join(List<Ansi.Doc>, Ansi.Doc) -> Ansi.Doc

  def style(Ansi.Doc, Ansi.Style) -> Ansi.Doc
  def styled(String, Ansi.Style) -> Ansi.Doc

  def render(Ansi.Doc) -> String
  def render_with(Ansi.Doc, Ansi.Profile) -> String
  def plain(Ansi.Doc) -> String
  def width(Ansi.Doc) -> Int
end

module Style
  def empty() -> Ansi.Style

  def fg(Ansi.Style, Ansi.Color) -> Ansi.Style
  def bg(Ansi.Style, Ansi.Color) -> Ansi.Style

  def bold(Ansi.Style) -> Ansi.Style
  def dim(Ansi.Style) -> Ansi.Style
  def italic(Ansi.Style) -> Ansi.Style
  def underline(Ansi.Style) -> Ansi.Style
  def reverse(Ansi.Style) -> Ansi.Style
  def strike(Ansi.Style) -> Ansi.Style
end

module Color
  def default() -> Ansi.Color

  def black() -> Ansi.Color
  def red() -> Ansi.Color
  def green() -> Ansi.Color
  def yellow() -> Ansi.Color
  def blue() -> Ansi.Color
  def magenta() -> Ansi.Color
  def cyan() -> Ansi.Color
  def white() -> Ansi.Color

  def index(Int) -> Result<Ansi.Color, ColorRangeError>
  def rgb(Int, Int, Int) -> Result<Ansi.Color, ColorRangeError>
end

module Profile
  def plain() -> Ansi.Profile
  def ansi16() -> Ansi.Profile
  def ansi256() -> Ansi.Profile
  def truecolor() -> Ansi.Profile
end

module Ansi.Raw
  def sgr(List<Int>) -> Ansi.Doc
  def csi(String) -> Ansi.Doc
  def reset() -> Ansi.Doc
end
```

---

## まとめ

この仕様の要点は次の通りです。

- ユーザ公開面は `StringBuffer` ではなく `Ansi.Doc`
- `Style` と `Color` は不変値として組み立てる
- `render_with(profile)` によって出力先差分を吸収する
- REPL表示値として `Inspect.inspect -> Ansi.Doc` を採用できる
- CLI / TUI / VSCode に同じ表示モデルを持ち込める
- 内部実装は当面 `StringBuffer` で十分
