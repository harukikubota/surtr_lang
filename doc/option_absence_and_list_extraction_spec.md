# 欠如を表す標準 API の Option 化と List 分解エラーの修正仕様

状態: 実装・検証完了。2026-10-24。実装全体は level3。結果は第12節に記録する。

## 1. 目的と優先する方針

MonadExtractor によって Option を SafeBind (`=?`) の右辺から直接分解できるようになったため、値の欠如だけを表す標準 API は Option を返す。
呼び出し側が欠如をエラーとして扱う場合は、通常の `=?`、または追加する `Option::ok_or` で意図を表す。

今回の対象選定では、**ペイロードを持つエラーの維持を優先する**。以前の Option 化候補に含まれていても、欠落したキー・入力・検索条件などをペイロードに保持する API は Result のままにする。
ペイロードがないことだけを理由にすべてのエラーを Option 化せず、値の欠如か、別の失敗かも区別する。

List の値を取得する関数と、非空条件を要求する Extractor / 構造的 Pattern を分ける。
`first` / `last` は Option を返す。List の head-tail 分解が空入力で失敗した場合は `EmptyList` を返す。

本案は、会話で挙がったキャプチャへの置換と `List::_index_out_of_bounds` の削除も含む。既存の private 関数による共通化は維持する。
旧 API の戻り値やエラー名を残す互換経路は追加しない。スキーマ・VM バージョンは変更しない。

## 2. 現行の基盤と変更区分

- `lib/traits/monad_extractor.srt` と `lib/types/option.srt` により、`Some(value)` は `Ok(value)`、`None` は新しい `NoneError` に分解される。
- `=?` は RHS を一度評価し、対応型の extract を一度呼び、その成功値全体を Pattern に渡す。成功値や子 Pattern は再分解しない。
- `<-` は `Monad::bind` が渡す payload を照合し、MonadExtractor を呼ばない。
- `Convert<Result<T>> for Option<T>` は明示的な変換として維持する。MonadExtractor の導入は、Result の pipeline と Option の pipeline の暗黙変換を意味しない。
- Option の MonadExtractor は入力を分解する能力であり、Option に MonadFail を追加する変更ではない。

Option 化、`ok_or`、標準定義内の整理は level1。List 分解のエラー種類と報告位置の修正は Forge / Eldr と診断の観測結果にまたがるため level3 とする。
実装全体は level3 として扱う。型規則、MonadExtractor、SafeBind の評価回数、失敗返却先の選択規則は変更しない。

## 3. Option に変更する API

以下の10個の公開関数と、1個の private 関数を対象とする。表の `$A` / `$B` / `$Item` は既存の総称型引数を表す。

| 定義元 | 関数 | 変更後の戻り値 | 現在の欠如エラー |
|---|---|---|---|
| `lib/types/list.srt` | `first` / `last` | `Option<$A>` | `ListFirstEmpty` / `ListLastEmpty` |
| 同上 | `find` | `Option<$A>` | `ListFindNoMatch` |
| 同上 | `find_map` | `Option<$B>` | `ListFindMapNoValue` |
| 同上 | `min` / `min_by` | `Option<$A>` | `ListMinimumEmpty` |
| 同上 | `max` / `max_by` | `Option<$A>` | `ListMaximumEmpty` |
| 同上 | `min_max` | `Option<($A, $A)>` | `ListMinMaxEmpty` |
| `lib/types/generator.srt` | `next` | `Option<($Item, Generator<$Item>)>` | `GeneratorExhausted` |
| `lib/types/int.srt` | private `_digit_value` | `Option<Int>` | `NoneError` |

### 成功と欠如

- `first` / `last` は非空なら `Some`、空なら `None`。
- `find` は最初の該当要素を `Some` で返す。空入力または該当なしなら `None`。
- `find_map` のコールバック型 `($A -> Option<$B>)` は維持する。最初の `Some` をそのまま返し、空入力または全件 `None` なら `None`。
- 最大・最小系は非空なら `Some`、空なら `None`。比較規則と、同値の場合に先に現れた要素を保持する規則を維持する。
- `Generator::next` は `_step` の Option を返す。正常終了は `None` とする。元の Generator の不変性、同じ値に対する再評価、終端を観測した rest の性質は維持する。
- `_digit_value` は現在認識する ASCII の `0`〜`9`、`A`〜`F`、`a`〜`f` だけを `Some(Int)` にする。それ以外は `None`。公開パーサーが報告するエラーは変えない。

`find` / `find_map` は最初の成功後にコールバックを呼ばない。空入力ではコールバック・比較関数を呼ばない。戻り値の変更によって走査順や評価回数を変えない。

`_digit_value` の呼び出し元は `_parse_digit_for_base`、`_parse_literal_digit_for_base`、`_parse_literal_unsigned` の3か所で、Some / None の照合へ変更する。
`IntParseInvalidDigit`、`IntLiteralInvalidDigit`、`IntLiteralUnknownPrefix` など、公開のエラー種類とペイロードを維持する。

対象関数の Result 戻り値、戻り値に書かれた旧エラー注釈、および上表の欠如専用エラー定義を削除する。ただし `NoneError` は削除しない。

## 4. Result とペイロードを維持する API

| 対象 | 維持する理由・エラー |
|---|---|
| `List::at` | `ListIndexOutOfBounds(index, length)` |
| `HashMap::map_get` | `HashMapKeyMissing(key)` |
| `String::strip_prefix` / `strip_suffix` / `split_once` | 入力と prefix / suffix / separator を保持するエラー |
| `Regex::find` / `captures` | pattern と input を保持するエラー |
| `RegexCaptures::get` / `get_name` | index / name のペイロードと、存在しない場合・未マッチの場合の区別 |
| `FS::parent` / `name` | `FileSystemParentMissing(path)` / `FileSystemNameMissing(path)` |
| `Json::get` / `at` / `as_*` | 欠落情報、期待する型、実際の型などの診断 |
| `Generator::take_exact` と range の検証 | 不正な要求数、要求数と実際の数、範囲指定の不正 |
| 数値パース・デコード・文字コード変換 | 不正入力とその詳細 |
| IO / File / FS の入出力・操作 | EOF 以外の I/O 失敗などを含む契約 |
| Facet API | index / key / variant 不一致の情報と、入力 Result のエラー伝播 |
| `require` / `ensure`、Result の合成関数 | 呼び出し側が指定する任意の Error |

String・Regex・FS は以前の候補から除外する。この方針により、これらの専用エラーと利用側の Result パターンも維持する。
HashMap Pattern が使う `map_get`、`Json::get` / `at` の変換処理、Facet の失敗契約も変更しない。
`Int::min/max` と `Float::min/max` は2値から通常値を返す既存の契約を維持する。

## 5. Option::ok_or の追加

`lib/types/option.srt` の Option 固有関数として、次を追加する。新しい builtin は不要。

```surtr
def ok_or(value: Option<$T>, err: Lazy<Error>) -> Result<$T> {
  match value {
    Option::Some(inner) => Ok(inner),
    Option::None => Err(err()),
  }
}
```

- `Some` では成功値を保持し、エラー式を評価しない。
- `None` ではエラー式を一度だけ評価し、その Error を返す。
- 既存 Error を渡した場合は、その情報を保持する。NoneError を先に作ったり、cause に追加したりしない。
- Lazy 引数は既存の `Result::map_err`、`require`、`ensure` と同じ契約に従う。Lazy placeholder の関数型への正規化も既存規則を使う。
- `Option::to_result` などの別名・互換 API は追加しない。

以下は変更後の API の例であり、現行実装での実行結果ではない。

```surtr
deferror MissingHead { "head was required" }

def first_or_error(values: List<Int>) -> Result<Int> {
  Option::ok_or(List::first(values), EmptyList())
}

def first_or_custom_error(values: List<Int>) -> Result<Int> {
  Option::ok_or(List::first(values), MissingHead())
}

def first_with_default_failure(values: List<Int>) -> Result<Int> {
  value =? List::first(values)
  Ok(value)
}
```

空入力では順に EmptyList、MissingHead、NoneError を返す。関数の Option 戻り値と、呼び出し側が選ぶエラーを分ける。
`to::<Result>` と `MonadExtractor::extract` は引き続き NoneError を使う。

## 6. List 分解の EmptyList と失敗位置

### エラー種類

`lib/errors.srt` に、ペイロードを持たない共有エラー `EmptyList` を定義する。メッセージは提案した `empty list` を採用する。

| 経路 | 現行 | 変更後 |
|---|---|---|
| 空 List に対する構造的 `[head, ..tail]` | `EmptyHeadTailListPattern` | `EmptyList` |
| 空 List に対する named `uncons(head, tail)` | `UnconsEmptyList` | `EmptyList` |
| 空 String に対する uncons / head-tail | `UnconsEmptyString` | 維持 |
| 固定長 List の長さ不一致 | `ListPatternTooShort` / `ListPatternTooLong` | ペイロードを含めて維持 |

置換後の `EmptyHeadTailListPattern` と `UnconsEmptyList` は削除する。両経路の実装責務は維持し、エラーの canonical identity を共有する。
分岐 consumer が不一致を破棄する規則と、SafeBind / apply_pattern などが失敗を保持する規則は変更しない。

### 報告位置

構造的 head-tail Pattern が空 List に到達したときは、その段の head Pattern のソース位置を使う。単純な変数束縛なら変数名を指す。

| 入力例 | 失敗種類 | 報告位置 |
|---|---|---|
| `[h, ..tail] =? empty`（empty は空の List） | `EmptyList` | `h` |
| `[h1, ..[h2, ..tail]] =? [1]` | `EmptyList` | `h2` |
| `[h1, ..[h2, ..tail]] =? empty` | `EmptyList` | `h1` |

空入力のため head 自体を取得できなかったという位置であり、内側の head-tail 全体、外側 Pattern 全体、RHS の位置には広げない。
head が単純変数以外なら、その head Pattern の既存 span を使う。診断表示と Error が保持する発生位置を整合させる。

直接の named `uncons` は今回エラー種類を統一するが、既存の Extractor application の報告位置は維持する。
空入力以外の子 Pattern の不一致は、その子のエラーと発生位置を保持する。
`Option::ok_or` で明示した `EmptyList()` は通常の Error 生成位置を保持し、List Pattern の位置に付け替えない。

## 7. Result の do にある呼び出し元の追従

変更前の調査で見つかった対象は、4ファイル・10か所。行番号は本案作成時の目印であり、実装時に再検索する。
Result のアサーションを合成する do は Result のまま保ち、Option を取り出す箇所を `<-` から `=?` に変更する。

| ファイル | 現在の行 | 対象 |
|---|---|---|
| `lib/tests/monads/list.srt` | 587 / 596 / 605 | `List::min_max` の3か所 |
| `lib/tests/basic_types/project.srt` | 11 / 20 / 30 | `List::first` の3か所 |
| `lib/tests/basic_types/regex.srt` | 87 / 88 | 検索結果 List の `first` / `last` |
| `lib/tests/modules/generator.srt` | 78 / 92 | `Generator::next` の capture 経由と直接呼び出し |

Generator テストの75行目の capture 型も、`Generator<Int> -> Result<(Int, Generator<Int>)>` から `Generator<Int> -> Option<(Int, Generator<Int>)>` へ変更する。

```surtr
do {
  (minimum, maximum) =? List::min_max([3, 1, 4, 2])
  assert_eq(1, minimum)
  assert_eq(4, maximum)
}
```

String / Regex / FS の Result 呼び出し9か所は変更しない。Regex テスト内でも、`Regex::find/captures` の `<-` と `List::first/last` の分解を区別する。
対象 API を `assert_ok_eq` / `assert_err_kind`、`match Ok/Err`、`|>=`、関数型注釈から使う箇所も検索し、Option の契約に合わせる。
一律に `to::<Result>` を挿入して古いテストを通すことはせず、取得結果そのものの検証には Some / None のアサーションを使う。
失敗伝播を検証するケースでは Result の成功・失敗と後続の短絡を確認する。

## 8. 単純なクロージャのキャプチャ化と指定ラッパーの削除

### キャプチャへの置換

関数・演算子・constructor の呼び出しだけを行うクロージャは、既存の capture 構文で表す。
引数の順序、固定式の評価時点、評価回数、Lazy の扱いを維持する。

| 箇所 | 置換後の形 |
|---|---|
| `List::sum` の加算 | ``&`+` `` |
| `List::min/max/sort` の Compare 呼び出し | `&Compare::compare` |
| `OptionT::lift` の Some 包装 | `&Option::Some` |
| `EitherT::lift` の Right 包装 | `&Either::Right` |
| `ResultT::lift` の Ok 包装 | `&Ok` |
| `String::_lines_non_empty` の map | `&_strip_trailing_cr` |
| `List::sort_by` の reduce | `&_insert_sorted(&2, &1, cmp)` |
| `List` の Applicative の map 呼び出し | `&List::map(value, &1)` |
| StyledDoc の style 適用 | `&_style_line(&1, next)` など |

複数の文、分岐、値の構築を含む処理は、呼び出しを含むことだけを理由に機械的に置換しない。
Reader / State 系の引数を無視するクロージャを、引数数の異なる裸 capture に置き換えない。

### 指定ラッパーの削除と private の共通化の維持

`List::_index_out_of_bounds` は削除し、2か所の呼び出しを `Err(ListIndexOutOfBounds(index, length))` の直接記述にする。
エラー定義と index / length のペイロードは維持する。通常の Error 生成に従い、発生位置は直接記述した生成式の位置になる。

他モジュールで同じ `Err(Error(...))` を返すだけの private 関数は、現時点の調査では見つかっていない。
既存の private 関数による共通化は維持する。`Int::_pow2`、および StyledDoc の `_single_line_doc`、`_prefix_segment`、`_indent_line`、`_style_segment`、`_style_with_fg/bg` は削除・インライン化しない。
これらの補助関数を呼ぶクロージャのキャプチャ化では、呼び出し先の共通化を維持する。
`_digit_value` の Option 化は本案の対象として維持するが、関数自体の共通化は解消しない。

## 9. 変更する正本と実装順序

| 段階 | 対象 | 完了条件 |
|---|---|---|
| 1 | 対象 API の `@doc`、`docs/site/error-handling.md`、`docs/site/generator.md`、`docs/site/language-guide.md`、`lib/errors.srt` の説明 | Option と Result の用途、削除エラー、新 API の変更後の契約を明記 |
| 2 | `docs/dev/Pattern_spec.md`、`Error_spec.md`、`diagnostics.md`、`EldrVM_spec.md`、関連する `docs/site/{pattern-matching,extractors,kernel}.md` と Kernel の `@doc` | EmptyList と報告位置を整合。MonadExtractor の既存規則を維持 |
| 3 | `lib/types/{option,list,generator,int}.srt` と標準テスト | 対象境界の Red を確認後に実装し、Option 化・ok_or・呼び出し元の追従を検証 |
| 4 | `lib/errors.srt`、Forge、Eldr、既存の Pattern / ソース位置テスト | EmptyList の種類と head の位置を直接検証し、旧エラー経路を削除 |
| 5 | capture 化、`_index_out_of_bounds` の削除、全体検索 | 評価規則、既存 private 関数の共通化、対象外のペイロード契約を維持 |
| 6 | 標準テストと workspace の統合検証 | 最終ソースについて全件 Green を1回確認 |

`docs/site/monad.md`、`docs/site/function-operators.md`、`docs/dev/Do_intrinsic_spec.md` は呼び出し例・記述に追従が必要な範囲だけ更新する。
`docs/site/regex.md` や FS / String の Result 契約を Option に書き換えない。
`lib/types/option.srt` の explicit Result pipeline の例は、通常の Monad 合成に変換が必要な場面を説明しているため、SafeBind の利用と分けて記述する。
`docs/dev/テスト方針.md` は、変更するエラーと位置の検証責務を必要な範囲で更新する。

## 10. 受入条件

1. 対象の List 9関数と Generator::next は Option を返す。空入力・不一致・正常終了は None であり、旧欠如エラーを生成しない。
2. find / find_map の早期終了、空入力での callback 非実行、比較の同値規則、Generator の評価契約を維持する。
3. Int の private digit 判定を Option 化し、公開パーサーの成功値・エラー種類・ペイロードを維持する。
4. ok_or は Some のときエラー式を呼ばず、None のとき一度だけ呼ぶ。EmptyList とユーザー定義エラーを返せ、既存 Error の情報も保持する。Lazy 引数の capture を既存規則で使える。
5. Result の do 内で対象 Option を `=?` から分解できる。成功時は続行し、None の場合は NoneError を返して後続を実行しない。Option の `<-` を Result の bind として暗黙変換しない。
6. 構造的 head-tail / named uncons の空 List は同じ EmptyList を返す。String の空入力、固定長 Pattern、子 Pattern のエラー契約を維持する。
7. `[h1, ..[h2, ..tail]] =? [1]` の Error と診断の主位置は h2。空の外側 List なら h1。単純な head-tail の空入力なら head の位置になる。apply_pattern で保持した Error の位置も一致する。
8. ペイロードを持つ API の Result と Error を維持する。String / Regex / FS の以前の Option 候補を変更しない。
9. 削除対象の旧エラー名・古い Result 戻り値への依存を残さない。必要な旧仕様例は拒否テストとして現行の診断条件へ更新し、互換 alias やフォールバックを追加しない。
10. capture 化と `_index_out_of_bounds` の削除後も評価回数と順序、Lazy、再帰、ペイロードの契約を維持する。既存 private 関数の共通化は維持する。

## 11. 修正案作成時の検証計画と確認範囲

実装時は `surtr-development`、`.srt` 更新には `surtr-source-update`、テスト選択には `surtr-test-strategy` を適用する。
各段階で対象の契約を固定するテストから始める。意味のある成功・欠如・拒否境界を選び、同じ契約を各層で重複検証しない。

### 標準 API

- `lib/tests/monads/{option,list}.srt`、`lib/tests/modules/generator.srt`、既存の Int パーステストを中心に、Some / None、Lazy の評価回数、短絡と不変性を検証する。
- project / regex のテスト、Transformer / String / StyledDoc の既存テストは呼び出し追従と capture 整理の影響範囲として確認する。
- 最小範囲はリポジトリルートから `cargo run -- test --quiet lib/tests/monads/option.srt` など、対象ファイルのパスを指定する。現行 runner の単体指定は起動時 CWD 基準であり、lib 相対への付け替えを行わない。統合時に `cargo run -- test --quiet --all` を実行する。
- 古い Result 型注釈の使用や Result の do での誤った `<-` は、必要な拒否境界として fixture の最も直接的な箇所で確認する。

### List 分解と位置

- Forge の crate-local テスト、Eldr の uncons テスト、`lib/tests/language_features/apply_pattern.srt` を拡張する。
- `tests/integration/error_source_locations.rs` で、単純な空入力と入れ子の h2 の source span、種類、保持した位置を確認する。非 ASCII 文字が前にある場合も既存の位置規約に従う。
- `tests/integration/language_features/safebind_and_errors.rs` などの旧エラー期待値を更新する。
- `rtk cargo nextest run -p forge`、`rtk cargo nextest run -p eldr` から関連するテストを選ぶ。script fixture は `rtk cargo nextest run -p rune --test integration run_srt`。
- source location の CLI 境界は default profile の除外対象なので、`rtk cargo nextest run --profile cold -p rune --test integration error_source_locations` を使う。
- 複数フェーズに及ぶ最終差分の全体検証は `rtk cargo nextest run --profile ci --workspace`。標準テスト全件と合わせて、最終ソースで全件 Green を1回確認する。

本案の作成時はソース・仕様・既存テストを読み取り、対象と呼び出し元を照合した。
製品コードとテストは変更しておらず、ビルド・実行テストは行っていない。上記は今後の検証計画であり、検証済み結果ではない。

EmptyList のメッセージは `empty list` を採用し、既存 private 関数の共通化は維持する。Option 化対象、ペイロードを維持する対象、first / last と Extractor の境界も本案に記載した方針で確定している。


## 12. 実装と検証結果（2026-10-24）

### 実装した内容

- List の対象9関数、Generator::next、Int::_digit_value を Option 化し、対象の欠如専用エラーを削除した。走査順、短絡、比較の同値保持、Generator の再評価と終端、公開 Int パーサーのエラーとペイロードを維持した。
- Option::ok_or を通常の標準定義として追加した。Some はエラー式を評価せず、None は一度評価する。既存 Error の Payload・location・cause・stack trace などを再構築せず保持する。
- 空 List の構造的 head-tail と named uncons は EmptyList（message: `empty list`）を共有する。構造的分解は失敗した段の head span を使い、named uncons は適用位置を維持する。保持 consumer で head の位置情報が欠けた場合は CodegenError とし、Pattern 全体へのフォールバックは設けない。
- 指定 capture 化と List::_index_out_of_bounds の削除を行い、Int / StyledDoc の private 関数による共通化を維持した。
- Result の do 内の呼び出し、Random テスト、麻雀サンプル、関連正本文書と @doc を追従した。旧 Result 型の関数値注釈と Result do での誤った `<-` は拒否 fixture に追加した。
- ペイロードを持つ API の Result、String の空分解、固定長 List と子 Pattern のエラー、型規則と MonadExtractor の契約、スキーマ・VM バージョンは変更していない。

### 検証

List の契約テストは `expected Option<Int>, got Result<Int>`、ok_or は未定義、Forge の位置テストは head span の不一致で Red を確認した後に実装した。

| 検証 | 結果 |
|---|---|
| List / Generator / Int / Option と関連する標準テスト入口 | 終了コード0 |
| `rtk cargo nextest run -p forge` | 103件成功。追加した Error assertion の head span / 位置欠落テストは別途 `-p forge empty_list` で2件成功 |
| `rtk cargo nextest run -p eldr uncons_builtin` | 3件成功 |
| `rtk cargo nextest run -p rune --test integration run_srt` | 9件成功（成功・拒否 fixture の bucket） |
| `rtk cargo nextest run --profile cold -p rune --test integration error_source_locations` | 14件成功。入れ子・外側空・非ASCII・apply_pattern・named uncons・ok_or と既存 Error の metadata 保持を確認 |
| `rtk proxy cargo run -- test --quiet --all` | 終了コード0 |
| 麻雀サンプルの normalize / yaku_pure | 終了コード0、4件 / 14件成功 |
| 変更した API の @doc サンプル | REPL で実測し、Option::Some の表示と失敗例を整合 |
| `rtk cargo nextest run --profile ci --workspace` | 2,765件成功、75 binaries、除外・失敗なし |
| `git diff --check` | 成功 |

サブエージェントで標準 API と List 分解を相互レビューし、製品実装の未解決指摘はない。レビューで挙がった位置保持の観測不足は、既存の Error metadata・source location テストと Forge の Error assertion 経路テストで補った。

最終ソースの workspace 全件と標準テスト全件は成功した。未検証の受入条件・残件はない。


## 13. main への追従と統合（2026-10-24）

EnumError を含む main（`db5b524c`）へ追従し、競合2件を解消した。
`ResultT::lift` は main の3型引数（Error を含む）を維持し、本体だけを `&Ok` に変更した。
`docs/dev/diagnostics.md` は EnumError の診断方針と EmptyList の head span の説明を両方残した。

一時統合版で、標準テスト全件が終了コード0、`rtk cargo nextest run --profile ci --workspace -j 4` が2,833件成功（79 binaries、除外なし）だった。
既定の並列数では Scar の既存 `bootstrap_source_does_not_see_standard_nominals_from_native_inventory` が15秒でタイムアウトしたが、単独実行は約8秒で成功した。制限時間とテストは変更せず、並列数4で全件を確認した。
EnumError 値を ok_or に渡す場合、Some でエラー式を評価しない場合、Other(Error) を持つ集合への返却も成功した。

競合解消後の全追跡ファイルが検証済み一時統合版と一致することを確認した。その後の変更はこの統合記録だけである。
