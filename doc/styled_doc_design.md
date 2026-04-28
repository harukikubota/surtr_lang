# StyledDoc 設計メモ

## 目的

`StyledDoc` は、REPL / TUI / ユーザコードで共通利用する表示用DSLである。

主な用途は次の通り。

- REPLでの評価結果表示
- REPLでのバインド変数表示
- `inspect` 結果の色付け
- `:doc` コマンドのドキュメント表示
- `:sig` コマンドのシグネチャ表示
- ユーザコードからのANSI装飾文字列生成

`StyledDoc` は pure Surtr code として実装し、ユーザも標準ライブラリとして利用できるようにする。

---

## 基本方針

### 1. ANSI文字列そのものを中核にしない

`StyledDoc` の中核は ANSI escape sequence ではなく、構造化された表示ドキュメントとする。

```text
StyledDoc::Doc
  -> StyledDoc::Line
    -> StyledDoc::Segment(text, style)
```

ANSI文字列は最終レンダリング結果のひとつにすぎない。

```text
StyledDoc::Doc
  ├─ to_ansi()   -> String
  ├─ plain()     -> String
  └─ TUI adapter -> ratatui Line/Span
```

これにより、REPLではANSI表示、TUIでは `ratatui::Span`、テストではplain textというように、同じ表示モデルを複数の出力先へ流用できる。

### 2. `print(String)` は意味解釈しない

標準出力に出された文字列は、ユーザプログラムの出力である。

そのため、`print("Ok(1)")` のような文字列をREPL側で勝手に `Ok` や数値として色付けしない。

| 出力元 | 扱い |
|---|---|
| return value | `inspect` として色付け |
| bindings value | `inspect` として色付け |
| `print(String)` | plain text |
| `print(non_string)` | `inspect` として色付けしてもよい |
| `eprint(String)` | stderr plain text |
| `eprint(Error)` | error formatとして別扱い |

### 3. `inspect` はトークン化して色付けする

最初の実装では、`inspect(value) -> String` の結果を `InspectTokenizer` に通し、トークン種別ごとに `StyledDoc::Segment` を生成する。

通常のソースparserへ渡す必要はない。  
parserではなく、失敗しない緩いtokenizerとして扱う。

---

## 公開モジュール

```surtr
defmod StyledDoc {
  # records
  # constructors
  # combinators
  # styles
  # renderers
}
```

ユーザは次のように使える。

```surtr
import StyledDoc::*;

def main() {
  let doc =
    StyledDoc::header("Result")
    |> StyledDoc::append(
      StyledDoc::line("success")
      |> StyledDoc::green()
    )

  print(StyledDoc::to_ansi(doc))
}
```

---

## データ型

### `Color`

```surtr
defenum Color {
  Black,
  Red,
  Green,
  Yellow,
  Blue,
  Magenta,
  Cyan,
  White,
  Default,
}
```

初期実装では基本色のみ扱う。

---

### `Style`

```surtr
defrecord Style(
  fg: Option<Color>,
  bg: Option<Color>,
  bold: Bool,
  dim: Bool,
  underline: Bool,
  italic: Bool,
)
```

`None` は未指定を表す。

```surtr
def default_style() -> Style {
  Style(
    fg: None(),
    bg: None(),
    bold: false,
    dim: false,
    underline: false,
    italic: false,
  )
}
```

---

### `Segment`

```surtr
defrecord Segment(
  text: String,
  style: Style,
)
```

`Segment` は同じstyleを持つ連続した文字列を表す。

---

### `Line`

```surtr
defrecord Line(
  segments: List<Segment>,
)
```

`Line` は改行を含まない。  
改行は `Doc.lines` の区切りとして表現する。

---

### `Doc`

```surtr
defrecord Doc(
  lines: List<Line>,
)
```

`Doc` が `StyledDoc` モジュールの中心型である。

---

## 基本コンストラクタ

```surtr
def empty() -> Doc
def text(value: String) -> Doc
def line(value: String) -> Doc
def segment(value: String, style: Style) -> Doc
def newline() -> Doc
```

### 挙動

| 関数 | 意味 |
|---|---|
| `empty()` | 空ドキュメント |
| `text(s)` | 改行なしのplain text |
| `line(s)` | 1行のplain text |
| `segment(s, style)` | style付き1segment |
| `newline()` | 空行 |

---

## 合成API

```surtr
def append(left: Doc, right: Doc) -> Doc
def concat(docs: List<Doc>) -> Doc
def join(docs: List<Doc>, sep: Doc) -> Doc
def append_line(doc: Doc, line: Doc) -> Doc
def inline_concat(docs: List<Doc>) -> Doc
def indent(doc: Doc, width: Int) -> Doc
def pad_left(doc: Doc, width: Int) -> Doc
```

`append(left, right)` は行リストを連結する。

```text
append(
  line("a"),
  line("b")
)

=> 
a
b
```

同一行に続けたい場合は `inline_concat` を使う。

---

## style適用API

```surtr
def style(doc: Doc, style: Style) -> Doc

def bold(doc: Doc) -> Doc
def dim(doc: Doc) -> Doc
def underline(doc: Doc) -> Doc
def italic(doc: Doc) -> Doc

def black(doc: Doc) -> Doc
def red(doc: Doc) -> Doc
def green(doc: Doc) -> Doc
def yellow(doc: Doc) -> Doc
def blue(doc: Doc) -> Doc
def magenta(doc: Doc) -> Doc
def cyan(doc: Doc) -> Doc
def white(doc: Doc) -> Doc
def default_color(doc: Doc) -> Doc
```

style適用は既存segmentへmergeする。

```text
bold(green(text("ok")))

=> fg=Green, bold=true
```

---

## 意味付きstyle helper

REPLやドキュメント表示では色名より意味名を使う方が扱いやすい。

```surtr
def accent(doc: Doc) -> Doc
def muted(doc: Doc) -> Doc
def success(doc: Doc) -> Doc
def warning(doc: Doc) -> Doc
def error(doc: Doc) -> Doc
def code(doc: Doc) -> Doc
def type_name(doc: Doc) -> Doc
def binding_name(doc: Doc) -> Doc
def constructor(doc: Doc) -> Doc
def punctuation(doc: Doc) -> Doc
```

初期マッピングはRust側TOML設定のデフォルト値と揃える。

---

## renderer

### `plain`

```surtr
def plain(doc: Doc) -> String
```

すべてのstyleを捨て、通常の文字列に変換する。

用途:

- テスト
- ログ保存
- 非対応端末
- snapshot比較

---

### `to_ansi`

```surtr
def to_ansi(doc: Doc) -> String
```

`Doc` をANSI escape sequence付きの文字列に変換する。

用途:

- CLI REPL
- 通常のterminal出力
- ユーザコードでの装飾出力

`to_ansi` はpureなString生成であり、I/Oは行わない。

```surtr
print(StyledDoc::to_ansi(doc))
```

---

### `to_segments`

TUIやホスト側連携のため、構造を保持したまま返せるAPIもあるとよい。

```surtr
def lines(doc: Doc) -> List<Line>
```

Rust側はこれを `ratatui::text::Line` / `Span` に変換できる。

---

## inspect表示

### 目的

REPL成功時の表示に色を付ける。

対象:

- return value
- binding value
- `print(non_string)` のinspect表示

対象外:

- `print(String)` の生文字列

---

### API

```surtr
def inspect_doc(text: String) -> Doc
```

`inspect_doc` は、`inspect(value)` の結果文字列を受け取り、色付きDocを返す。

---

### Inspect token

```surtr
defenum InspectTokenKind {
  Number,
  String,
  Bool,
  Unit,
  Constructor,
  TypeName,
  ErrorName,
  Ident,
  Punctuation,
  Operator,
  Plain,
}
```

```surtr
defrecord InspectToken(
  kind: InspectTokenKind,
  text: String,
)
```

---

### token -> style

| token | style |
|---|---|
| `Number` | yellow |
| `String` | green |
| `Bool` | yellow |
| `Unit` | muted |
| `Constructor` | magenta |
| `TypeName` | blue |
| `ErrorName` | red |
| `Ident` | default |
| `Punctuation` | muted |
| `Operator` | muted |
| `Plain` | default |

---

## REPL成功表示

### return value

```surtr
def repl_return_doc(inspected: String) -> Doc {
  StyledDoc::inspect_doc(inspected)
}
```

表示例:

```text
> Ok([1, 2, 3])
```

---

### binding

```surtr
def repl_binding_doc(name: String, ty: String, inspected: String) -> Doc {
  StyledDoc::inline_concat([
    StyledDoc::text(name) |> StyledDoc::binding_name(),
    StyledDoc::text(": ") |> StyledDoc::punctuation(),
    StyledDoc::text(ty) |> StyledDoc::type_name(),
    StyledDoc::text(" = ") |> StyledDoc::punctuation(),
    StyledDoc::inspect_doc(inspected),
  ])
}
```

表示例:

```text
> x: Result<List<Int>, ParseError> = Ok([1, 2, 3])
```

---

### type definition

```surtr
def repl_type_defined_doc(name: String) -> Doc {
  StyledDoc::inline_concat([
    StyledDoc::text("defined type ") |> StyledDoc::muted(),
    StyledDoc::text(name) |> StyledDoc::type_name(),
  ])
}
```

---

## REPL `:doc` 表示

ソースドキュメント表示は、IExの `h Enum.map/2` に近い視認性を優先する。

- 先頭に対象symbol名を表示する
- signature / source head は背景色つきの1行として強調する
- `@spec` 相当の型シグネチャ行を続けて表示する
- 本文は通常テキストとして表示する
- `## Examples` 見出しを強調する
- Examples 内のSurtrコードにも通常のsource token色付けルールを適用する
- CLIではANSI、TUIではsegment構造のまま描画する

### API

```surtr
def doc_entry_doc(entry: DocEntryView) -> Doc
```

`DocEntryView` はREPL側から渡される表示用データである。

```surtr
defrecord DocEntryView(
  symbol: String,
  source_head: Option<String>,
  signature: Option<String>,
  body: Option<String>,
)
```

---

### helper

```surtr
def header(title: String) -> Doc
def section(title: String, body: Doc) -> Doc
def signature_block(signature: String) -> Doc
def documentation_block(body: String) -> Doc
def example_block(code: String) -> Doc
```

---

### IEx風レイアウト

REPLの `:doc` は、raw doc string をそのまま流すのではなく、次の表示ブロックへ
分解してから描画する。

```text
<symbol>

<source head / signature banner>

@spec <signature>

<summary/body>

## Examples

  xldr> <source code>
  <result>
```

`source head / signature banner` は、たとえば `def map(list, f)` や
`defrecord User(...)` のような短い宣言形を表示する。宣言形を復元できない場合は
`signature` を代用する。

`## Examples` 以降のコード断片は `lex_for_display` の対象にする。
本文全体をSurtrコードとして扱わず、example code block / REPL prompt 行だけを
source token色付けする。

---

## REPL `:sig` 表示

`:sig` は `:doc` より短く、シグネチャに集中する。

### 表示例

```text
List.map/2
  map(list: List<A>, f: A -> B) -> List<B>
```

### API

```surtr
def signature_doc(symbol: String, signature: String) -> Doc
```

より細かく色付けしたい場合は、シグネチャ文字列もtokenizerに通す。

```surtr
def signature_highlight(signature: String) -> Doc
```

---

## REPL/TUI統合方針

### CLI REPL

CLI REPLでは `Doc` をANSI文字列に変換して出力する。

### TUI

TUIではANSI文字列をそのまま流さない。

`Doc -> List<Line> -> ratatui::Line/Span` に変換する。

理由:

- ANSI制御文字が表示幅計算を壊す
- スクロール計算が壊れやすい
- 選択・コピー・折り返しが難しくなる
- `ratatui` はstyleを構造として扱える

---

## Rust側トークン化API方針

現時点では、Surtrコードから表示用トークナイザーを公開しない。

`StyledDoc` はPure Surtrの標準ライブラリAPIとして実装する。ただしREPL / TUI
内部表示は、ユーザVM上のSurtr関数を呼ばず、Rust側の表示用APIで処理する。

### 最小公開API

Spireには、構文解析用lexerとは別に、表示用の緩いトークン化経路を設ける。

```rust
pub fn lex_for_display(source: &str) -> Vec<DisplayToken>
```

想定する公開型は次の程度に留める。

```rust
pub struct DisplayToken {
    pub kind: DisplayTokenKind,
    pub span: Span,
    pub text: String,
}

pub enum DisplayTokenKind {
    Number,
    String,
    Bool,
    Keyword,
    TypeName,
    Constructor,
    Ident,
    Operator,
    Punctuation,
    Comment,
    Whitespace,
    Newline,
    Error,
    Plain,
}
```

このAPIはREPL / TUIなど、ホスト側表示のためのAPIであり、
Surtr標準ライブラリ関数としては公開しない。

### 構文解析用lexerとの違い

既存の構文解析用lexerは、parserへ渡すための厳密な経路である。

- `ParseError` を返してよい
- whitespace / comment を捨ててよい
- literalを実行時値へ変換してよい
- parser都合の `Token` 分類でよい

表示用トークン化APIは、色付けと表示継続を目的にする。

- 失敗しない
- 未知文字や壊れた文字列は `Error` / `Plain` に落とす
- whitespace / comment も保持する
- `text` に元の入力断片を保持する
- `span` は入力文字列上の表示範囲として使えるようにする

### REPL / TUI での使い方

REPLの成功表示や `:doc` / `:sig` の色付けは、当面Rust側で
`lex_for_display` または同型のinspect用tokenizerを使って
`StyledDoc`相当の構造へ変換する。

CLIではその構造をANSI文字列へ変換し、TUIではANSIを経由せず
`ratatui::Line` / `ratatui::Span` へ変換する。

REPL表示のために、ユーザVM内で `StyledDoc::inspect_doc` や
`StyledDoc::to_ansi` を実行しない。

将来、Surtrコードからトークン化結果を扱う需要が出た場合は、
別途 `StyledDoc::inspect_tokenize` のPure Surtr実装、または専用builtinの
追加を検討する。

---

## 色付けルール設定

REPL / TUI のホスト側表示色は、プロジェクト内のTOML設定から読み込む。

### 設定ファイル

ファイル名はプロジェクトルート直下の `surtr-doc-style.toml` とする。

REPL起動時に、現在ディレクトリから親ディレクトリへ向かって
`surtr-doc-style.toml` を探索する。見つからない場合は内蔵デフォルトを使う。

読み込み失敗時はREPL起動を止めず、warningを1回だけ表示して内蔵デフォルトへ
フォールバックする。

### 最小schema

初期schemaは、token kind / semantic role から style への対応だけに限定する。

```toml
[token]
keyword = { fg = "magenta", bold = true }
type_name = { fg = "blue" }
constructor = { fg = "magenta" }
number = { fg = "yellow" }
string = { fg = "green" }
bool = { fg = "yellow" }
operator = { fg = "cyan" }
punctuation = { fg = "default", dim = true }
comment = { fg = "default", dim = true }
error = { fg = "red", underline = true }

[role]
doc_header = { fg = "default", bold = true }
doc_signature_banner = { fg = "black", bg = "yellow" }
doc_spec = { fg = "cyan" }
doc_section_heading = { fg = "yellow", bold = true }
doc_body = { fg = "default" }
doc_example_prompt = { fg = "cyan" }
repl_binding_name = { fg = "cyan", bold = true }
repl_type_name = { fg = "blue" }
```

`fg` / `bg` は最初は基本色のみ受け付ける。

```text
black, red, green, yellow, blue, magenta, cyan, white, default
```

未知のkey / 未知の色 / 壊れた値は、その項目だけ無視してデフォルト値を使う。
設定ファイル全体を厳密に失敗させない。

### 適用範囲

TOML設定はRust側 renderer に適用する。

- REPL return value のinspect表示
- REPL binding表示
- `:doc` 表示
- `:sig` 表示
- `:doc` の Examples 内 source code
- TUI docs pane / results pane

Pure Surtrの `StyledDoc::to_ansi` は、現時点ではこのTOMLを自動参照しない。
ユーザコードから使う `StyledDoc` はpureな値変換として扱う。

---

## 実装順序

### Phase 1: 最小StyledDoc

- `Color`
- `Style`
- `Segment`
- `Line`
- `Doc`
- `text`
- `line`
- `concat`
- `inline_concat`
- `indent`
- `plain`
- `to_ansi`
- `red/green/yellow/blue/magenta/cyan/bold/dim`

### Phase 2: inspect色付け

- `InspectTokenKind`
- `InspectToken`
- `inspect_tokenize`
- `inspect_doc`
- REPL return valueへ適用
- REPL binding valueへ適用

### Phase 3: REPL doc/sig

- `header`
- `section`
- `signature_block`
- `doc_entry_doc`
- `signature_doc`
- `:doc` で利用
- `:sig` で利用
- IEx風のsource docレイアウト
- Examples内source codeへのtoken色付け適用

### Phase 4: TUI対応

- `Doc` をRust側で受け取れる構造へ変換
- `StyledDoc::Line` / `Segment` を `ratatui::Line` / `Span` に変換
- ANSI escapeをTUIへ直接流さない

### Phase 5: project style config

- `surtr-doc-style.toml` の探索
- TOML schemaの読み込み
- 壊れた項目だけデフォルトへフォールバック
- REPL / TUI rendererへの適用

---

## 注意点

### 1. `StyledDoc::to_ansi` はI/Oしない

`to_ansi` はpure functionであり、出力は `String` のみ。

```surtr
let rendered = StyledDoc::to_ansi(doc)
print(rendered)
```

### 2. `print(String)` は自動色付けしない

ユーザが明示的に色付けしたい場合は、`StyledDoc::to_ansi` を使う。

```surtr
print(
  StyledDoc::text("hello")
  |> StyledDoc::green()
  |> StyledDoc::to_ansi()
)
```

### 3. tokenizerは失敗しない

未知の文字列は `Plain` / `Error` に落とす。

```surtr
def inspect_tokenize(input: String) -> List<InspectToken>
```

これは `Result` を返さなくてよい。  
表示補助なので、色付け不能でも表示自体は継続する。

### 4. REPL表示のためにVM状態を汚さない

REPL内部の表示処理は、ユーザコードの評価と分離する。  
`StyledDoc` がSurtrコードであっても、REPL表示のたびにユーザVM内で任意関数を実行する必要はない。

初期実装では、Surtr標準ライブラリのAPI設計を正とし、Rust側にも同型のrendererを持つ。
