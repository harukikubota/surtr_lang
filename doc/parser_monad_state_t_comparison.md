# ParserMonad の再設計方針 — StateT と Result

作成日: 2026-10-11。対象ブランチ: `parser-monad-example`。
対象: `examples/parser_monad/`。この文書は旧実装の構造を前提にせず、入力の遷移と Error の回復から組み直した設計案である。今回は仕様作成までとし、サンプルの実装・テスト・README は変更していない。

## 1. 採用する構成

`Parser<A>` を `StateT<ParseInput, Result, A>` の名目的なラッパーにする。
独自の成功・失敗 Enum を廃止し、成功は `Ok((value, next_input))`、失敗は `Err(error)` で表す。
構文の不一致には位置と期待項目を保存した具体 Error を使う。既存 Error の伝播には `MonadFail`、回復には `MonadRecover` を使う。

```text
Parser<A>
  = StateT<ParseInput, Result, A> を保持する型
  = 実行時には ParseInput -> Result<(A, ParseInput)>

bind:
Parser<A> -> (A -> Parser<B>) -> Parser<B>

recover:
Parser<A> -> (Error -> Parser<A>) -> Parser<A>
```

`bind` は成功したときだけ次の Parser に値と更新後の入力を渡す。
`recover` は失敗したときだけ、回復境界の入口の入力でハンドラが返した Parser を実行する。

```text
s0 -- left --> Ok((a, s1)) -- continuation(a) --> s2
             Err(e)       -- handler(e) を s0 から実行 --> 結果
```

`Result` の失敗には更新後の入力状態がない。構文不一致の発生位置は Error の Payload に保持する。
入力の復元と診断位置の保持を、それぞれ状態遷移と Error の責務にする。

## 2. 前提と現在の状態

確認したサンプルの HEAD は `b70f0775`。`doc/example_parser_monad.md` と `examples/parser_monad/` は旧構成の仕様・実装として読む。
サンプルブランチには、この案が必要とする最新の標準拡張がまだ揃っていない。

| 機能 | 確認先と状態 | この案での扱い |
|---|---|---|
| Error の保存 Payload と具象 Pattern | main `9f756199` の `docs/dev/Error_spec.md`、`crates/scar/tests/error_payload.rs` | 利用する |
| `MonadFail::fail(Error)` と StateT の実装 | main の `lib/traits/monad_fail.srt`、`lib/types/monad_transformer/state_t.srt` | 利用する |
| `MonadRecover`、Error を受ける `Result::recover`、通常値 ErrorKind | `monad-recover-errorkind` ワークツリーの作業中コード | 完了・統合を実装の前提にする |
| StateT / ResultT の標準 MonadRecover | 調査時点では実装なし。標準の実装は Result のみ | 追加を要求せず、Parser 自身に実装する |

入力として指定されたレビュー文書は、削除された a8a3 から
`/Users/haruca/.codex/worktrees/monad-recover-errorkind/surtr/doc/monad_recover_errorkind_review.md`
へ引き継がれている。この文書の過去の調査結果と、進行中コードの状態は区別する。
本案の例は拡張完了後の受入例であり、現在のサンプルブランチで実行済みの例ではない。

Parser の実装範囲は既存の言語機能・標準 Trait を使う **level1**。コンパイラ、標準 Trait の契約、標準 Transformer の実装は変更しない。
前提の ErrorKind / MonadRecover 拡張の level4 検証は、その拡張側で完了させる。

## 3. 層順序の比較

| 構成 | 実行結果 | 失敗したときの状態 | 判断 |
|---|---|---|---|
| 独自 `ParseReply` | `Success(A, Input)` / `Failure(ParseIssue)` | 独自の失敗型で管理 | 廃止。Result と Error の機能を重複させる必要がない |
| 関数を直接保持する Parser | `Input -> Result<(A, Input)>` | Err に更新後状態を含まない | 意味論は適合するが、標準 StateT と同じ bind / ap を再実装する |
| **Parser が StateT<Input, Result, A> を保持** | `Input -> Result<(A, Input)>` | Err に更新後状態を含まない | **採用**。順次処理を標準 StateT に委譲し、Parser 固有の回復だけを定義する |
| ResultT<State<Input>, A> | `Input -> (Result<A>, Input)` | 失敗後の更新状態を保持する | 今回は採用しない。回復時の入力復元を追加操作として管理することになる |

StateT と ResultT は、同じ名前の失敗操作を持っていても状態を保持する層が異なる。
今回は Parser の回復を入口への入力復元として定めるので、状態遷移の結果を Result で包む。
`Parser<A>` は型別名ではなく独立した型にし、Parser 固有の MonadRecover を他の StateT に適用しない。

## 4. 型と失敗の境界

以下は採用する宣言案である。Error の入力引数と保存 Payload を別々に宣言する現行規則に従う。

```surtr
@derive Eq
defstruct ParseInput {
  rest: String,
  offset: Int,
}

defstruct Parser<$A> {
  transition: StateT<ParseInput, Result, $A>,
}

deferror ParseMismatch(offset: Int, expected: String) {
  |position: Int, expectation: String|
  Self(message: "at #{position}: expected #{expectation}",
       offset: position, expected: expectation)
}

deferror ParserConfigurationError(reason: String) {
  |detail: String|
  Self(message: detail, reason: detail)
}

deferror ParserProgressError(offset: Int) {
  |position: Int|
  Self(message: "repeated parser must consume input", offset: position)
}

deferror ParserStateError(reason: String) {
  |detail: String|
  Self(message: detail, reason: detail)
}
```

`ParseMismatch` だけを、構文コンビネータが自動回復してよい失敗とする。
`ParserConfigurationError` は `char` に0文字・複数文字を渡した場合など、パーサーの構成が不正なことを表す。
`ParserProgressError` は反復対象が入力を消費せず成功したことを表す。
`ParserStateError` は低水準の自作 Parser が入力遷移の契約を破ったことを表す。
これらを `fatal: Boolean` のフラグにまとめない。

その他の Error は、そのまま Parser の失敗として運べる。汎用 Pattern Error、Extractor の Error、変換失敗などを ParseMismatch に変換しない。
`MonadFail::fail(error)` は渡された Error を保持する操作であり、位置付き構文不一致を新しく作る操作とは分ける。

`ParseReply`、`ParseIssue`、`fatal`、サンプル側の `ExampleSyntaxError` は削除する。
古い Enum への変換 API、旧 `fail(expected: String)` の互換 API、Error の message 文字列による種別判定は残さない。

## 5. 入力の契約

`offset` は0始まりの Unicode スカラー位置とする。バイト位置・書記素位置ではない。
`parse` は `ParseInput(rest: text, offset: 0)` から実行する。
公開 run に負の offset を渡した場合は、遷移を実行する前に ParserStateError を返す。
以下の値保持・回復の法則は、有効な入口入力に対して定める。
成功時の次状態は、入力の末尾から文字を残した状態であり、次の条件を満たす。

- `next.rest` は `input.rest` の suffix。
- `next.offset = input.offset + String::len(input.rest) - String::len(next.rest)`。
- 位置は非負。読み取り以外の `pure` などは同じ入力を返す。

`Parser::new` は通常関数 `ParseInput -> Result<(A, ParseInput)>` を StateT に包む低水準入口として残す。
`Parser::run` は成功時の状態契約を一か所で検査し、不正な返却を `ParserStateError` にする。
失敗は常に元 Error を返す。既存 ParseMismatch が現在の入力範囲外の位置を持っていても、別の Error に置き換えない。

実行用の取り出しは private な `_checked_transition(Parser<A>) -> StateT<ParseInput, Result, A>` に統一する。
この helper が入口と成功結果を検査する遷移を返す。公開 run と bind / ap / recover は必ずこの遷移を使い、生の transition を直接実行しない。
StateT を Parser に保存する private helper は保存だけを行い、生成時に別の検査 wrapper を積み重ねない。
bind / ap はそれぞれの入力 Parser から検査付き遷移を取り出し、中間の不正状態を次の処理へ渡さない。
owner impl 内の struct literal による保存や、Facet による transition の差替えも、run / bind の検査を迂回する理由にしない。
owner impl 外の利用者は new を使い、struct literal の既存の構築制限を維持する。
生のフィールドを実行できる低水準入口は公開しない。利用者が StateT を直接実行する操作は Parser API の契約の対象外である。
正常な Error を検査のために作り直さず、不正な成功状態だけを新しい契約違反として扱う。
String の長さと suffix 検査のコストは教材用サンプルとして受け入れる。独自のチェック省略経路は作らない。

構文コンビネータが自動回復する ParseMismatch は、試行入口以上かつ元入力の末尾以下の位置を持つものに限る。
範囲判定は一つの private helper に置き、choice / optional で共有する。
範囲外なら元 Error を伝播し、過去位置の失敗を未消費失敗に丸めない。
これは失敗値の生成・運搬を制限する型規則ではなく、自動回復に使える診断かどうかの実行時判定である。
`fail(ParseMismatch(9, "x"))` を空入力で実行しても同じ Error が残る。

Parser 値や入力を複数の候補へ渡すことはできるが、復元できるのは不変な ParseInput だけである。
`print`、ファイル更新などの外部副作用は巻き戻らない。

## 6. 基本操作と Trait の責務

公開する中心的な署名は次のとおり。

```text
new       : (ParseInput -> Result<(A, ParseInput)>) -> Parser<A>
run       : Parser<A> -> ParseInput -> Result<(A, ParseInput)>
parse     : Parser<A> -> String -> Result<(A, ParseInput)>
parse_all : Parser<A> -> String -> Result<A>

pure      : A -> Parser<A>
map       : Parser<A> -> (A -> B) -> Parser<B>
and_then  : Parser<A> -> (A -> Parser<B>) -> Parser<B>
reject    : String -> Parser<A>
fail      : Error -> Parser<A>       # MonadFail の操作
recover   : Parser<A> -> (Error -> Parser<A>) -> Parser<A>
```

`parse_all` は `before(parser, eof())` を実行し、成功 pair の値だけを返す。
残余入力が必要な利用者は `parse` を使う。全入力消費後の空入力を毎回返す必要はない。

| Trait / API | 実装責務 |
|---|---|
| Applicative::pure / Monad::return | StateT の pure に包み、入力を変えない |
| Monad::bind / and_then | 入力と mapper が返す Parser の遷移を _checked_transition で取り出し、StateT の bind に委譲する |
| Applicative::ap | 両入力の遷移を _checked_transition で取り出し、StateT の ap に委譲。関数側を先に実行し、成功時だけ引数側を実行する |
| map | bind と pure の合成。入力を保ちながら通常の mapper を使う |
| Functor::fmap | map に委譲。呼出側の現行 Functor 型検査は維持する |
| MonadFail::fail | StateT の MonadFail に委譲し、元 Error を保持する |
| MonadRecover::recover | Parser の入口入力を捕捉し、Result の recover でハンドラを実行する |
| reject(expected) | 実行時の現在位置で ParseMismatch を構築する |

`map` は List / Option なども通常の成功値として生成できる形にする。
期待型のない Functor mapper 推論における contextual output の拒否を、Parser のために緩和しない。
通常の `map` を StateT / Result の Functor 呼出しだけに依存させず、bind と pure で定義する。

回復処理の核は次の通常関数の合成である。

```text
recover(p, handler).run(s0)
  = Result::recover(
      p.run(s0),
      error -> handler(error).run(s0)
    )
```

成功時は同じ値と次入力を返し、ハンドラ本体を呼ばない。
失敗時は元 Error を渡し、ハンドラの Parser を入口入力から一度実行する。
ハンドラの再失敗は、そのまま返す。再回復ループは作らない。
`recover(p, error -> fail(error))` は、値・状態・元 Error の観測について元の Parser と同じになる。

回復境界は、recover に渡した Parser 全体の入口である。
`recover(bind(item(), failing), handler)` は item より前の入力から回復する。
`bind(item(), recover(failing, handler))` は item の後の入力から回復する。
入れ子の recover に最外側の parse 開始位置を使う実装にはしない。

`recover_kind` は標準 Trait のデフォルト実装を継承する。Parser に同じ kind 判定を再実装しない。
一般の `recover` / `recover_kind` は、利用者が明示したハンドラに回復を委ねる。
次節の自動回復の制限は `choice`、`optional`、反復などの構文コンビネータに適用する。

## 7. 選択・反復・省略

既存サンプルのバックトラック規則を、具体 Error に対して定め直す。
消費後の失敗を一般に commit する新規則や `try` / `cut` は追加しない。

### choice(left, right)

1. 左を入口入力から実行する。成功ならそのまま返し、右を実行しない。
2. 左の失敗が入力範囲内の ParseMismatch なら、右を同じ入口入力から実行する。
3. 右が成功したら右の結果を返す。
4. 両方が入力範囲内の ParseMismatch なら、offset が大きい側の **元 Error** を返す。同位置なら左を返す。
5. 左または実行した右の失敗が他の Error または範囲外の ParseMismatch なら、その Error を返す。

`literal("ab")` と `literal("ac")` の選択に `"ac!"` を渡すと、左が位置1で失敗した後、右が位置0から読み直して成功する。
`"a!"` では同位置なので、左の期待項目 `"b"` を保持する。
期待項目の集合への統合や Error の再構築は行わない。

kind と Payload は `match error { ParseMismatch(...) @ original => ... , _ => ... }` で読む。
`recover_kind` の handler 引数は共通 Error のままであり、kind 一致だけで `.offset` を読めるとは扱わない。

標準 StateT の Alternative::choose は両方の遷移を実行するので、Parser::choice の実装には使わない。
Parser に Alternative は追加しない。構文選択と、Error を捨てる `empty` の契約を混ぜない。

### many(parser) / some(parser)

各試行の開始入力を `si` とする。

| 試行結果 | 動作 |
|---|---|
| 成功し、入力を消費した | 値を蓄え、次入力から反復する |
| 成功し、入力を消費しない | ParserProgressError を返す |
| ParseMismatch の offset が `si.offset` | 正常終了。蓄えた List と `si` を返す |
| 入力範囲内の ParseMismatch の offset が `si.offset` より大きい | 消費後の失敗として元 Error を伝播する |
| 範囲外の位置を持つ ParseMismatch | 元 Error を伝播する |
| その他の Error | 元 Error を伝播する |

正常終了で戻る入力は、直前までの成功した反復の次入力である。反復全体の入口に戻さず、停止した試行だけを戻す。
offset が入口より小さい失敗は元 Error のまま伝播する。rest と offset が矛盾した成功は、前節の入力検査で ParserStateError になる。
`many(pure(value))`、`many(optional(parser))` の未消費成功を空列へ変換しない。
`some` は1回目を bind し、残りに many を使う。

### optional(parser)

成功なら Some を返す。入力範囲内の ParseMismatch なら、消費の有無にかかわらず入口に戻して None を返す。
その他の Error と範囲外の ParseMismatch は元値を伝播する。通常の構文不一致については既存サンプルと同じバックトラック規則である。
many の終了規則とは異なるので、optional を many の停止判定として利用しない。

### sep_by / sep_by1

`sep_by1` は最初の要素と `many(after(separator, element))` を合成する。
`sep_by` は入口位置の ParseMismatch だけを空列へ回復する。消費後の ParseMismatch と他の Error は伝播する。
そのため `"a,a!"` は成功し、`"a,!"` は区切りの後の欠落として失敗する。
sep_by を optional と単純に合成して、区切り後の失敗まで空列へ戻す実装にはしない。

## 8. 残す API と利用例

`item`、`satisfy`、`char`、`literal`、`eof` は StateT の入力遷移として作る。
`pair`、`before`、`after`、`between`、`some`、`sep_by1` は bind / pure と既存コンビネータから導出する。
`lazy(build: Unit -> Parser<A>)` は実行時に build を呼ぶ通常の Parser として残す。
新しい Lazy 引数の宣言や、コンパイラによる Parser 専用遅延は追加しない。左再帰は対象外である。

```surtr
pair: Parser<(String, String)> = do::<Parser> {
  first <- Parser::item()
  second <- Parser::item()
  Parser::pure((first, second))
}
Parser::parse(pair, "猫犬!")
# Ok((("猫", "犬"), ParseInput(rest: "!", offset: 2)))

Parser::parse_all(pair, "猫犬")
# Ok(("猫", "犬"))

match Parser::parse_all(pair, "猫") {
  Err(ParseMismatch(offset, expected)) => (offset, expected),
  _ => (0, "success"),
}
# (1, "character")
```

既存 Error を扱うハンドラでは、Pattern 成功スコープで Payload を取り出す。

```surtr
recovered = MonadRecover::recover(parser, {|error|
  match error {
    ParseMismatch(_, _) => Parser::pure(fallback),
    _ => MonadFail::fail(error),
  }
})
```

Error 生成は `ParseMismatch(position, expected)` の明示 call、kind 指定は裸の `ParseMismatch` とする。
ErrorKind 値からの動的 Error 構築や、共通 Error への具象型注釈によるキャストは追加しない。

`do::<Parser>` の partial `<-` と SafeBind は既存の MonadFail を使う。
生成された通常の Pattern Error も Parser に保持し、choice などで構文不一致として回復しない。
SafeBind の RHS で暗黙アンラップされるのは canonical Result の外側一段だけである。
Parser や StateT を `=?` で実行・アンラップする経路は作らない。
Error の kind / Payload の分岐は match で行い、ErrorDownCast を SafeBind へ持ち込まない。

普通の引数生成式は eager に評価する。右 Parser や handler を作る式の評価と、その Parser の run / handler 本体の実行は区別する。
例えば choice の右 Parser の構築式は呼出し時に評価されるが、左成功時に右の遷移は実行されない。

## 9. 四則演算例の再構成

BNF、構文木、優先順位、左結合、ASCII 空白、BigInt の評価は維持する。
`parse_all(Arithmetic::parser(), text)` は `Result<ArithmeticExpr>` を返すので、評価まで次の通常関数で繋げられる。

```surtr
def evaluate_example(text: String) -> Result<Int> {
  tree =? Parser::parse_all(Arithmetic::parser(), text)
  Arithmetic::evaluate(tree)
}
```

構文失敗を ExampleSyntaxError に作り直す変換は削除する。
表示例は Error::format または具象 Pattern で offset / expected を読む。
`1 +` は ParseMismatch、`1 / 0` は解析成功後の元の評価 Error を返す。

数字列の `String::try_to_int` が失敗した場合も、元 Error を MonadFail で伝播する。
変換失敗を `reject("integer")` に置き換えて別候補へバックトラックする旧処理は削除する。
「入力が数字列だから成功するはず」という理由で変換失敗を曖昧に扱わない。

README と `src/arithmetic.srt` の BNF は同じ内容に揃える。
`run.srt` と `entry.srt --entry main` を残し、明示 entry の戻り値は既存の `Result<()>` 契約に合わせる。

## 10. 受入条件

旧テストの観測対象を、成功値・状態と、失敗の kind / Payload に分けて移行する。
Result の Err の Eq は kind だけを見るので、`assert_eq(Err(...), actual)` で offset / expected の検証を済ませない。
失敗は具象 Pattern で Payload を取り出して比較し、異なる kind なら Test の失敗として返す。

| 契約 | 必須の境界 |
|---|---|
| 基本遷移 | Unicode、pure / eof の未消費、残余、literal の途中失敗位置、parse_all の値と末尾拒否 |
| 型と順次処理 | A→B の bind、Tuple / List / Option 成功値、ap の関数側優先、mapper / 後続の失敗時未実行 |
| MonadFail | 手作り Error の Payload / message 保持、現在入力の範囲外の ParseMismatch の保持、汎用 Pattern Error の保持、partial do、Result RHS の SafeBind |
| MonadRecover | 成功維持、handler 本体の呼出し回数、入口入力の復元、bind の外側・内側で回復入口が異なること、handler の再失敗、元 Error を fail で戻す法則 |
| recover_kind | 一致・不一致、Pattern による Payload 取得、不一致時の元情報保持、引数生成式の eager 評価 |
| choice | 左成功時の右未実行、消費後の巻戻し、遠い位置・同位置左優先、左右の他 Error / 範囲外 ParseMismatch の伝播 |
| 反復 | 順序、入口不一致で終了、消費後の失敗保持、未消費成功の拒否、構成・進捗 Error を choice / optional で隠さない |
| 省略と区切り | optional の消費後巻戻し、sep_by の入口空列、区切り後の要素欠落拒否。範囲外 ParseMismatch を optional / many / sep_by で隠さない |
| 入力契約 | 負の入口 offset の実行前拒否と遷移の未実行。offset だけ進む成功、suffix でない成功、offset / rest の不一致を ParserStateError にする。owner 内で保存した遷移や Facet 差替えも検査を通り、不正な中間成功の後に bind の mapper / ap の引数側を実行しない。入口以前・末尾以後の ParseMismatch は自動回復せず元値を伝播 |
| 評価順 | Parser 構築と実行の区別、回復しても外部副作用を巻き戻さないこと |
| 四則演算 | 既存 BNF の成功・拒否、parse→evaluate の元 Error 伝播、ゼロ除算、使用例・明示 entry |
| 法則 | 成功・失敗に対する Monad 三法則と recover の再伝播法則。Error 自体への Eq は要求しない |

Error の情報保持は message と Payload の比較に加え、位置・cause のある Error の再伝播で観測する。
不一致時や choice の診断選択時に、同じ kind の新しい Error を構築して済ませない。
コンパイラの既存拒否境界を各サンプルテストで重複検証する必要はない。

## 11. 実装順序と変更対象

1. 前提の標準拡張を検証済みの状態で統合し、サンプルブランチで署名と型検査の差を確認する。途中状態の別ワークツリーから製品コードをコピーしない。
2. `examples/parser_monad/tests/parser.srt` の成功結果・位置付き不一致・元 Error 伝播を先に新契約へ変更し、旧実装では意図した理由で失敗することを確認する。
3. `src/parser.srt` の型、入力検査、StateT 委譲、Parser の MonadFail / MonadRecover を実装する。旧 ParseReply / ParseIssue とその分岐を削除する。
4. choice / many / optional / sep_by の境界テストを移し、通常の Surtr 関数で実装する。状態検査の重複や他 Error を隠す分岐を残さない。
5. `src/arithmetic.srt`、`tests/arithmetic.srt`、`main.srt` を Result 接続へ変更し、使用例と entry を確認する。
6. README と `doc/example_parser_monad.md` を新しい API・結果・検証事実に追従させる。古い実装結果を新実装の検証として流用しない。
7. 最終差分を別エージェントがレビューし、元 Error の保持、状態復元、旧経路の削除、テスト不足を確認する。

入力の正本は `docs/dev/Error_spec.md`、`Lazy_spec.md`、`Trait_system_spec.md`、標準 Trait と Transformer の `@doc`。
このサンプルのために正本の言語規則や標準 Transformer の責務を変えない。
スキーマ・VM バージョンも変更しない。

level1 の実装検証は、リポジトリルートで次を直列に実行する。

```sh
rtk proxy cargo run -p rune -- test --quiet examples/parser_monad/tests/parser.srt
rtk proxy cargo run -p rune -- test --quiet examples/parser_monad/tests/arithmetic.srt
rtk proxy cargo run -p rune -- run examples/parser_monad/run.srt
rtk proxy cargo run -p rune -- run examples/parser_monad/entry.srt --entry main
rtk proxy cargo run -- test --quiet --all
```

Rust に変更がなければ、このサンプルのための nextest は不要。
前提の拡張に不備が見つかった場合は、サンプルに互換処理を足さず、拡張側の修正範囲・level・検証を改めて定める。

## 12. 今回の検証範囲

2名のサブエージェントで既存 Parser と標準 Error / Transformer の読み取り調査を分担し、別の1名が設計文書を独立にレビューした。
指摘された MonadFail の元 Error 保持と入力範囲検査の衝突、遷移差替えによる検査の迂回、回復境界と中間遷移の受入条件を修正した。
今回の成果はこの変更方針だけである。実装、実行可能なテストの変更、サンプル実行、全標準テスト、全体 CI は行っていない。
前提の拡張の実行成功も、この設計調査で確認したとは扱わない。
