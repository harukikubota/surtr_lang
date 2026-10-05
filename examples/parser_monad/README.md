# ParserMonad サンプル

`Parser<A>` に文字列パーサーとコンビネータを実装した、Surtrだけのプロジェクトです。
`Functor`、`Applicative`、`Monad`に対応し、`do::<Parser>` でパーサーを順に合成できます。

## 実行とテスト

リポジトリルートで実行します。

```sh
cargo run -p rune -- run examples/parser_monad/run.srt
cargo run -p rune -- run examples/parser_monad/entry.srt --entry main
cargo run -p rune -- test examples/parser_monad/tests/parser.srt
cargo run -p rune -- test examples/parser_monad/tests/arithmetic.srt
```

使用例は構文木と評価結果を表示します。`2 + 3 * 4` は `Ok(14)`、
`(2 + 3) * 4` は `Ok(20)`、`20 / 2 / 5` は `Ok(2)` になります。
`1 +` は構文エラー、`1 / 0` は解析成功後の評価エラーです。

## ファイル

- `src/parser.srt`: 入力・結果の型、基本パーサー、コンビネータ、Trait実装。
- `src/arithmetic.srt`: BNFに対応する四則演算パーサーと構文木の評価。
- `main.srt` / `run.srt`: 成功例と失敗例を表示する使用例。
- `entry.srt`: 複数ファイルを読み込み、`--entry main` で実行する入口。
- `tests/`: Unicode、合成、Monad則、失敗境界、四則演算のテスト。

## 基本的な使い方

```surtr
include './src/parser.srt'

word = Parser::literal("猫犬")
Parser::parse(word, "猫犬!")
# ParseReply::Success("猫犬", ParseInput(rest: "!", offset: 2))

pair: Parser<(String, String)> = do::<Parser> {
  first <- Parser::item()
  second <- Parser::item()
  Parser::pure((first, second))
}
Parser::parse_all(pair, "ab")
# ParseReply::Success(("a", "b"), ParseInput(rest: "", offset: 2))
```

`Parser::parse` は残りの入力を返します。`Parser::parse_all` は末尾までの消費を要求します。
成功は `ParseReply::Success(value, input)`、失敗は `ParseReply::Failure(issue)` です。
`issue.offset` は0始まりのUnicodeスカラー位置です。バイト位置や書記素位置ではありません。

## API

| API | 役割 |
|---|---|
| `pure(value)` / `Monad::return(value)` | 入力を消費せず成功する |
| `fail::<A>(expected)` | 現在位置で通常の失敗を返す |
| `item()` | Unicodeスカラー1文字を読む |
| `satisfy(predicate, expected)` | 条件を満たす1文字を読む |
| `char(text)` / `literal(text)` | 指定の1文字／文字列を読む |
| `eof()` | 入力末尾を確認する |
| `map(parser, mapper)` / `and_then(parser, mapper)` | 値の変換／値に応じたパーサーの選択 |
| `choice(left, right)` | 左を優先し、通常の失敗なら元の位置から右を試す |
| `many(parser)` / `some(parser)` | 0回以上／1回以上の反復 |
| `optional(parser)` | 成功をSome、通常の失敗をNoneにする |
| `pair(left, right)` | 順に読み、両方の値をtupleで返す |
| `before(parser, suffix)` / `after(prefix, parser)` | 前側／後側の値を残す |
| `between(open, parser, close)` | 開閉記号で囲まれた値を読む |
| `sep_by(parser, separator)` / `sep_by1(parser, separator)` | 区切り付きの0個以上／1個以上の値を読む |
| `lazy(build)` | 実行時に `build(())` でパーサーを構築する |

`|*>`、`|*|`、`|>=` も標準Trait経由で使えます。
現行Surtrの `Functor::fmap` / `|*>` はListやOptionなどのcontextを返すmapperを拒否します。
そのようなpayloadへの変換には `Parser::map` または `and_then` と `pure` を使います。

`choice` は両方が失敗した場合に遠い位置の診断を返し、同位置なら左の診断を返します。
`optional` も通常の失敗では消費を巻き戻します。
`many` は未消費の通常失敗で終了しますが、途中まで消費した失敗はそのまま返します。
このため `sep_by` は区切り記号の直後に値がない入力を拒否します。

反復対象が入力を消費せず成功すると、`fatal: True` の構成エラーを返します。
`many(pure(...))` や `many(optional(...))` はこの条件に当たります。
構成エラーは選択や省略でも隠しません。`char` に0文字・複数文字を渡した場合も構成エラーです。

## 四則演算のBNF

```bnf
<input>      ::= <ws> <expression> EOF
<expression> ::= <term> <add-tail>
<add-tail>   ::= "+" <ws> <term> <add-tail>
               | "-" <ws> <term> <add-tail> | ε
<term>       ::= <factor> <mul-tail>
<mul-tail>   ::= "*" <ws> <factor> <mul-tail>
               | "/" <ws> <factor> <mul-tail> | ε
<factor>     ::= <integer> | "(" <ws> <expression> ")" <ws>
<integer>    ::= <digit> <digits> <ws>
<digits>     ::= <digit> <digits> | ε
<digit>      ::= "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"
<ws>         ::= " " <ws> | TAB <ws> | LF <ws> | CR <ws> | ε
```

数字はASCIIの非負整数です。単項符号・小数・コメントは扱いません。
乗除算は加減算より優先し、各段の演算子は左結合で構文木にします。
Intの除算は標準の整数除算を使い、ゼロ除算はResultのErrを返します。

`_integer` は `some`、演算子の後続列は `many(pair(...))`、括弧は `between` に対応します。
括弧の内側の式は `lazy` を使い、パーサー構築時の無限再帰を避けています。
`lazy` は左再帰を解消しません。再帰文法では、再帰呼び出しより前に入力を消費してください。

この実装は小さな入力を読む教材用です。ストリーミング、packrat、左再帰、
入力サイズや再帰深度の上限管理は備えていません。
`Parser::new` で独自のパーサーを作る場合、成功時の `rest` と `offset` を一致させる必要があります。
