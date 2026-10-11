# ParserMonad サンプル

`Parser<A>` に文字列パーサーとコンビネータを実装した、Surtrだけのプロジェクトです。
入力の順次処理には `StateT<ParseInput, Result, A>` を使います。
`Functor`、`Applicative`、`Monad`、`MonadFail`、`MonadRecover` に対応し、
`do::<Parser>` でパーサーを順に合成できます。

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

- `src/parser.srt`: 入力と Error の型、基本パーサー、コンビネータ、Trait実装。
- `src/arithmetic.srt`: BNFに対応する四則演算パーサーと構文木の評価。
- `main.srt` / `run.srt`: 成功例と失敗例を表示する使用例。
- `entry.srt`: 複数ファイルを読み込み、`--entry main` で実行する入口。
- `tests/`: Unicode、合成、Monad則、失敗境界、四則演算のテスト。

## 基本的な使い方

```surtr
include './src/parser.srt'

word = Parser::literal("猫犬")
Parser::parse(word, "猫犬!")
# Ok(("猫犬", ParseInput(rest: "!", offset: 2)))

pair: Parser<(String, String)> = do::<Parser> {
  first <- Parser::item()
  second <- Parser::item()
  Parser::pure((first, second))
}
Parser::parse_all(pair, "ab")
# Ok(("a", "b"))
```

`Parser::parse` は `Result<(A, ParseInput)>` を返します。`Parser::parse_all` は末尾までの
消費を要求し、成功値だけを `Result<A>` で返します。失敗はどちらも元の Error を保持します。
構文不一致は `ParseMismatch(offset, expected)` の Pattern で位置と期待項目を読めます。
`offset` は0始まりのUnicodeスカラー位置です。バイト位置や書記素位置ではありません。

```surtr
match Parser::parse_all(Parser::literal("ab"), "a!") {
  Err(ParseMismatch(offset, expected)) => (offset, expected),
  _ => (0, "success"),
}
# (1, "b")
```

## API

| API | 役割 |
|---|---|
| `pure(value)` / `Monad::return(value)` | 入力を消費せず成功する |
| `reject::<A>(expected)` | 現在位置で ParseMismatch を作る |
| `fail::<A>(error)` / `MonadFail::fail(error)` | 渡された Error を保持して失敗する |
| `recover(parser, handler)` / `MonadRecover::recover` | 失敗したら入口入力から handler の Parser を実行する |
| `MonadRecover::recover_kind(parser, kind, handler)` | 指定 kind の失敗だけを回復する |
| `item()` | Unicodeスカラー1文字を読む |
| `satisfy(predicate, expected)` | 条件を満たす1文字を読む |
| `char(text)` / `literal(text)` | 指定の1文字／文字列を読む |
| `eof()` | 入力末尾を確認する |
| `map(parser, mapper)` / `and_then(parser, mapper)` | 値の変換／値に応じたパーサーの選択 |
| `ap(mapper, value)` | 関数を解析してから引数を解析し、結果に適用 |
| `choice(left, right)` | 左を優先し、範囲内の ParseMismatch なら入口から右を試す |
| `many(parser)` / `some(parser)` | 0回以上／1回以上の反復 |
| `optional(parser)` | 成功をSome、範囲内の ParseMismatch をNoneにする |
| `pair(left, right)` | 順に読み、両方の値をtupleで返す |
| `before(parser, suffix)` / `after(prefix, parser)` | 前側／後側の値を残す |
| `between(open, parser, close)` | 開閉記号で囲まれた値を読む |
| `sep_by(parser, separator)` / `sep_by1(parser, separator)` | 区切り付きの0個以上／1個以上の値を読む |
| `lazy(build)` | 実行時に `build(())` でパーサーを構築する |

`|*>`、`|*|`、`|>=` も標準Trait経由で使えます。
現行Surtrの `Functor::fmap` / `|*>` はListやOptionなどのcontextを返すmapperを拒否します。
そのようなpayloadへの変換には `Parser::map` または `and_then` と `pure` を使います。

`choice` は両方が構文不一致の場合に遠い位置の元 Error を返し、同位置なら左を返します。
`optional` は構文不一致で入口入力に戻し、Noneを返します。
`many` は試行の入口位置での構文不一致で終了し、途中まで消費した失敗を伝播します。
このため `sep_by` は区切り記号の直後に値がない入力を拒否します。
自動回復する構文不一致の位置は、試行入口から入力末尾までに限ります。
範囲外の ParseMismatch と他の Error は、そのまま伝播します。

反復対象が入力を消費せず成功すると `ParserProgressError` を返します。
`many(pure(...))` や `many(optional(...))` はこの条件に当たります。
`char` に0文字・複数文字を渡した場合は `ParserConfigurationError` です。
これらの Error は選択や省略で隠しません。

`recover` は成功時にハンドラ本体を呼びません。失敗時は元 Error を渡し、
ハンドラの Parser を回復境界の入口から一度実行します。ハンドラの再失敗もそのまま返します。
`recover_kind` の kind 引数には、通常値の裸の Error 名を渡します。

```surtr
recovered = MonadRecover::recover_kind(Parser::literal("ab"), ParseMismatch, {|error|
  match error {
    ParseMismatch(_, _) => Parser::literal("ac"),
    _ => MonadFail::fail(error),
  }
})
Parser::parse_all(recovered, "ac")
# Ok("ac")
```

回復境界を bind の外側に置くと全体の入口、内側に置くと後続 Parser の入口に戻ります。
巻き戻せるのは入力だけです。`print` やファイル更新などの外部副作用は戻りません。
引数生成式は通常どおり呼出し時に評価されます。右候補やハンドラの構築と、
その遷移やハンドラ本体の実行は区別してください。

## 四則演算のBNF

```bnf
<input>      ::= <ws> <expression> EOF
<expression> ::= <term> <add-tail>
<add-tail>   ::= "+" <ws> <term> <add-tail>
               | "-" <ws> <term> <add-tail> | epsilon
<term>       ::= <factor> <mul-tail>
<mul-tail>   ::= "*" <ws> <factor> <mul-tail>
               | "/" <ws> <factor> <mul-tail> | epsilon
<factor>     ::= <integer> | "(" <ws> <expression> ")" <ws>
<integer>    ::= <digit> <digits> <ws>
<digits>     ::= <digit> <digits> | epsilon
<digit>      ::= "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"
<ws>         ::= " " <ws> | "\t" <ws> | "\n" <ws> | "\u{d}" <ws> | epsilon
Binary operators fold left. Integer division uses Surtr's Int division.
```

数字はASCIIの非負整数です。単項符号・小数・コメントは扱いません。
乗除算は加減算より優先し、各段の演算子は左結合で構文木にします。
Intの除算は標準の整数除算を使い、ゼロ除算はResultのErrを返します。

`_integer` は `some`、演算子の後続列は `many(pair(...))`、括弧は `between` に対応します。
括弧の内側の式は `lazy` を使い、パーサー構築時の無限再帰を避けています。
`lazy` は左再帰を解消しません。再帰文法では、再帰呼び出しより前に入力を消費してください。

この実装は小さな入力を読む教材用です。ストリーミング、packrat、左再帰、
入力サイズや再帰深度の上限管理は備えていません。
`Parser::new` は `ParseInput -> Result<(A, ParseInput)>` を受け取ります。
成功時の `rest` は入口文字列の suffix とし、`offset` は消費したUnicodeスカラー数だけ進めます。
公開 `run` と合成された遷移は、この契約に違反する成功を `ParserStateError` にします。
負の入口 offset も、遷移を実行する前に拒否します。失敗の Error は作り直しません。
