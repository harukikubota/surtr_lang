# 標準 Extractor 拡充仕様 — scalar guard / String 操作

## 1. 状態・目的・独立した作業範囲

- 状態: 未実装の仕様草案と追加候補一覧。標準定義を調査した結果と、追加する API の提案を区別する。
- 対象: 事前引数を持つ標準 Extractor の拡充、および SRT で表現できる照合・判定処理の標準ソースへの配置。
- 前提機能: 事前引数、二状態 MatchResult、UnitOnly 子 Pattern 省略、as-pattern、型注釈付き projection が利用可能であること。前提の実装タスクは [Pattern 拡張統合仕様](pattern_extension_spec.md) に記載する。本書はそれとは独立した標準定義の修正タスクであり、compiler 構文・型規則の再設計を含めない。
- 今回は仕様作成だけを行う。標準ソース、Rust、実行可能なテストは変更しない。
- 基本は level 1: 標準 SRT に閉じる追加。既存 builtin の SRT 置換を採用する部分は Sindr / Eldr / loader まで影響するため、別の成果単位として影響範囲と検証 level を実装前に確定する。前提機能の level 4 検証を、この標準定義タスクに無条件で重ねない。

必須の拡充領域は次のとおり。

1. Int / Float / String の完全一致、Int / Float の比較。
2. Int の符号判定、odd / even。
3. String の完全一致、先頭一致、後方一致、split、および既存の通常関数から合成できる処理。
4. guard の成功 payload を Unit に統一し、元の入力値は必要な利用者が as-pattern で取得する。
5. 新しい照合判定は可能な限り SRT へ配置し、専用 builtin / Opcode / compiler の名前分岐を増やさない。

Record / struct、Enum、Tuple、List の構造分解 Extractor は追加しない。既存の constructor / structural Pattern を削除する仕様でもない。String 操作の結果として List や tuple を返すことと、それらを入力に取る構造分解 Extractor の追加は区別する。

本書の候補名は提案名であり、既存 API 名ではない。「必須」は機能領域の採用を意味し、「追加候補」は一覧化しただけで実装採用を意味しない。

## 2. 現行標準定義の調査結果

調査対象は `lib/` のテストを除く全63 SRT ファイル。public な通常関数・Trait / impl surface と、既存 Extractor を確認した。

既存 Extractor は次の3件で、現行は Option 返却である。

| 配置 | Extractor | 現行 payload | 本タスクでの扱い |
|---|---|---|---|
| `lib/kernel.srt` | Kernel::uncons | head / tail の tuple | 構造分解拡充の対象外。MatchResult 移行は前提タスク |
| `lib/types/duration.srt` | Duration::deconstruct | Int | 既存機能。移行は前提タスク |
| `lib/types/range.srt` | Range::deconstruct | 境界値の tuple | 構造分解拡充の対象外。移行は前提タスク |

主な合成元は以下。

- [Int](../lib/types/int.srt): sign、is_odd / is_even、safe_mod、bit 判定、各基数の parse。符号・parity は既に SRT に定義され、Eq / Compare と BigInt 演算は builtin を利用する。
- [Float](../lib/types/float.srt): Eq / Compare、floor / ceil / round / trunc、abs / min / max。Eq / Compare と丸めの primitive は builtin。
- [String](../lib/types/string.srt): strip_prefix / strip_suffix / split_once、lines / chars、trim 系、try_to_int / try_to_boolean は SRT。starts_with / ends_with / contains / split は現行 builtin。
- [Regex](../lib/types/regex.srt): compile / is_match / captures / find / split、capture の get / get_name、match の text は既存 builtin。
- [Eq](../lib/traits/operator/eq.srt)、[Compare](../lib/traits/operator/compare.srt): 値比較の既存契約。String には Eq があるが、現行の String Compare 実装はない。
- [Kernel](../lib/kernel.srt): assert は Result<Unit>、ensure は成功時に入力値を返す Result<Self>。標準 guard には assert 型の検証を使い、Self を payload として返す ensure 型を標準化しない。
- Duration / Range / HashMap / Json、Option / Result / Either、Generator、Reader / State / Identity / MonadT、Facet、File / FS / IO / Shell / Process / Random、Project / Config、StyledDoc、Function / 各 Trait、test / bootstrap も一覧を確認した。追加候補または対象外の理由を第7節に記載する。

通常関数と Extractor は同じ owner 内で同名宣言できない。現行 Sigil の `validate_unique_callable_names` は通常 def / builtin def / defextractor をまとめて重複検査する。同名通常関数の Pattern 限定 overload や別 namespace を追加せず、既存 API は維持して新しい名前を使う。

## 3. 共通契約

### 3.1 guard と返却 carrier

ユーザ指定の `Result<Umit>` は Unit と読み、本草案では guard の通常判定処理を `Self -> Result<Unit>`、Pattern で実行する Extractor 版を `Self -> MatchResult<Unit, Error>` とする前提で整理する。Result を Extractor の新しい返却 carrier に変更する案ではない。この解釈が異なる場合は、API 採用前に返却契約を調整する。

事前引数があれば、それらを最初に置く。

```text
guard 判定処理:  (pre_args..., self) -> Result<Unit>
guard Extractor: (pre_args..., self) -> MatchResult<Unit, Error>
変換 Extractor:  (pre_args..., self) -> MatchResult<Converted, Error>
```

成功 payload が UnitOnly の場合だけ、子 Pattern 位置を省略できる。通常 bind、wildcard、projection、型注釈付き projection の明示も許可する。

```surtr
Int::positive() @ num =? 10
# guard は Unit を返し、num は照合対象の Int を受け取る。

Int::literal(10) @ num =? source
Float::literal(1.5) @ value =? source
String::literal("surtr") @ text =? source

apply_pattern(10, Int::positive())
# Ok(())
apply_pattern(10, Int::positive(_1: Unit))
# Ok(())。Unit を明示 projection
apply_pattern(10, Int::positive() @ _1: Int)
# Ok(10)。元の入力を as projection
```

guard は条件成立に必要な情報だけを返す。入力 Self を子 Pattern 用 payload として複製しない。一方、prefix 除去後の String など「変換された結果」は、入力と同じ型でも意味のある payload として返す。

### 3.2 Error・単一評価・totality

- Bool predicate を guard 化する場合は、定義側が明示する具象 deferror を返す。共通の候補名は `StandardExtractorGuardMismatch(condition: String)` とし、条件名・message を SRT 側が決める。
- 既存 Result 関数を利用する場合は、SafeBind で元の Error を保持する。NoneError、ParseIntError、InvalidIntegerLiteral、RegexCompileError 等を一律の mismatch で上書きしない。
- 手書きの MatchResult::Err は具象 deferror の constructor を使う。既存 abstract Error を手書きで再投入する経路を増やさない。
- 事前引数、入力、通常関数への委譲はそれぞれ既存規則に従い一回だけ実行する。guard 判定と元の値の取得のために再評価しない。
- 非保持 consumer は Error を破棄し、SafeBind / apply_pattern の保持経路は元 Error を保持する。標準 Extractor 専用の consumer policy を追加しない。
- 常に成功する split / trim の変換 Extractor でも、静的には partial Pattern のままである。通常 Bind `=` を許可するための本体解析は追加しない。

通常の guard 判定を外部向け新関数として全部公開する必要はない。既存 assert / predicate、必要なら private helper で Result<Unit> 契約を表現する。

### 3.3 SRT での定義例

```surtr
deferror StandardExtractorGuardMismatch(condition: String) {
    "standard extractor guard failed: #{condition}"
}

impl Int {
    defp _check_positive(self: Self) -> Result<Unit> {
        assert(self > 0, StandardExtractorGuardMismatch("Int::positive"))
    }

    defextractor positive(self: Self) -> MatchResult<Unit, Error> {
        _ =? _check_positive(self)
        MatchResult::OK(())
    }
}

impl String {
    defextractor prefix(prefix: String, self: Self) -> MatchResult<String, Error> {
        rest =? String::strip_prefix(self, prefix)
        MatchResult::OK(rest)
    }

    defextractor separated(separator: String, self: Self) -> MatchResult<List<String>, Error> {
        MatchResult::OK(String::split(self, separator))
    }
}
```

これは前提機能実装後の仕様例である。通常関数の入力順を変更せず、Extractor では事前引数を先、照合対象 self を最後に置く。

## 4. 必須候補 — scalar 一致・比較・Int guard

以下は優先して API を確定する機能領域。すべて成功 payload は Unit、Extractor signature の戻り型は MatchResult<Unit, Error> とする。

| 提案 head | 事前引数 | 成立条件 / 既存の合成元 |
|---|---|---|
| Int::literal | expected: Int | self == expected / Eq |
| Float::literal | expected: Float | self == expected / Eq |
| String::literal | expected: String | self == expected / Eq。完全一致 |
| Int::less_than / Float::less_than | limit: Self | self < limit |
| Int::at_most / Float::at_most | limit: Self | self <= limit |
| Int::greater_than / Float::greater_than | limit: Self | self > limit |
| Int::at_least / Float::at_least | limit: Self | self >= limit |
| Int::positive | なし | self > 0 / sign |
| Int::negative | なし | self < 0 / sign |
| Int::zero | なし | self == 0 |
| Int::non_negative | なし | self >= 0 |
| Int::non_positive | なし | self <= 0 |
| Int::odd | なし | self mod 2 != 0 / safe_mod・is_odd |
| Int::even | なし | self mod 2 == 0 / safe_mod・is_even |

既存の eq / lt / lte / gt / gte / sign / is_odd / is_even を改名しない。Extractor のための Eq / Compare Trait 拡張は不要である。

literal の事前引数は通常 Expr なので、定数リテラルだけでなく既存の型が確定した値も渡せる。名称 literal を理由に compile-time 定数制約を追加しない。本タスクはこの named Extractor の追加を扱い、裸の Float literal Pattern 等の新しい parser 構文を追加しない。

Int は BigInt のままとし、fixed-width 整数へ狭めない。odd / even は負の整数でも成立条件どおり扱う。失敗する通常関数を合成する場合に Err を False / 成功へ隠す新しい fallback を設けない。

Float は既存 finite-only と Eq / Compare の契約をそのまま使う。完全一致に暗黙の許容誤差を導入せず、NaN / infinity を matcher 側で例外扱いする経路も追加しない。

## 5. 必須候補 — String の既存関数版 Extractor

| 提案 head | 事前引数 | 成功 payload | 合成元 / 意味 |
|---|---|---|---|
| String::literal | expected: String | Unit | Eq。完全一致。第4節と同じ1件 |
| String::has_prefix | prefix: String | Unit | starts_with。先頭条件だけを検査 |
| String::has_suffix | suffix: String | Unit | ends_with。後方条件だけを検査 |
| String::prefix | prefix: String | String | strip_prefix。先頭一致後の残り |
| String::suffix | suffix: String | String | strip_suffix。後方一致前の残り |
| String::separated | separator: String | List<String> | split。通常関数と同じ全分割 |
| String::separated_once | separator: String | (String, String) | split_once。最初の一致の前後 |

has_prefix / has_suffix は guard、prefix / suffix は除去後の値を返す変換として責務を分ける。子 Pattern を省略できるのは UnitOnly guard だけである。

```surtr
String::has_prefix("pre") @ original =? "prefix"
# original = "prefix"

String::prefix("pre", rest) @ original =? "prefix"
# rest = "fix"、original = "prefix"

String::suffix(".srt", stem) =? "main.srt"
# stem = "main"

String::separated(",", parts) =? "a,,b,"
# parts = ["a", "", "b", ""]

String::separated_once("=", key, value) =? "key=value=tail"
# key = "key"、value = "value=tail"
```

separated_once は通常関数の StringSplit::Split を SRT 内で取り出し、成功 payload の tuple として返す。これを StringSplit Enum 自体の構造分解 Extractor を追加する根拠にはしない。既存 Pattern を標準関数内部で利用することまで禁止する仕様ではない。

既存の意味をそのまま引き継ぐ。

- 空 prefix / suffix は成功する。除去型の結果は入力全体である。
- prefix / suffix / split_once の不成立は既存 NoneError を保持する。
- split の空 separator は chars と同じ結果になる。空入力なら []。
- 非空 separator の split は不在でも成功し、入力全体を1要素として返す。空入力なら [""]。
- 連続 separator と末尾 separator の空要素を保持する。split_once は先頭の一致だけを使う。
- split_once の空 separator は ("", self) として成功する。
- Unicode は既存 String 操作の surface character / substring 契約に従い、正規化・大文字小文字変換・新しい grapheme 規則を暗黙に導入しない。

`lib/types/string.srt` の現行 split @doc には Ok 付き表示があるが、実 signature、Eldr 実装、lib/tests/string.srt は List<String> の直接返却を示す。Extractor の設計はこの実契約に合わせ、実装時に @doc の不整合も修正する。

## 6. SRT 配置と builtin の境界

### 6.1 標準 Extractor の実装

新しい Extractor は原則として通常の `defextractor` で各型の SRT に置く。literal / 比較 / 符号 / parity / String guard の判定・Error 構築、prefix / suffix / split の Result から MatchResult への接続を Rust の新しい builtin にしない。

既存の BigInt / finite Float / String の値表現、Eq / Compare / 算術、String len、既存 uncons、Regex engine 等の runtime primitive は必要な境界として利用する。新しい標準 head の名前や payload 型を Forge / Eldr で特別判定しない。

配置の候補は以下。

- `lib/types/int.srt`: Int guard / literal / 比較。
- `lib/types/float.srt`: Float literal / 比較、採用した追加 guard。
- `lib/types/string.srt`: String guard・除去・分割・採用した変換。
- `lib/types/regex.srt` / `duration.srt`: 採用した追加候補だけ。
- 共通 guard Error は、loader の先行型登録と stage 制約を確認して standard source の1か所に定義する。候補は既存 Error 定義と同じ `lib/types/error.srt`。新規 shared file の導入を前提にしない。

### 6.2 既存通常 builtin を SRT へ移せる候補

「Extractor を SRT で書く」だけでなく、既存通常関数にも移せる処理がある。以下は置換候補として列挙し、Extractor 追加の採用と別に判断する。

| 現行 builtin | SRT の合成候補 | 留意点 |
|---|---|---|
| String::starts_with | strip_prefix の成否 | 空 prefix の成功、既存 Error の成否観測 |
| String::ends_with | strip_suffix の成否 | 空 suffix、Unicode / reverse コスト |
| String::contains | split_once の成否 | 空 needle の成功、判定のために前後値を作るコスト |
| String::split | split_once の反復 + chars | 空 separator を先に分岐し、反復が進まない経路を作らない |
| String::replace | 既存 private _replace_go + split_once | 空 from は入力保持。すべての非重複一致を置換 |
| Int / Float の lt / lte / gt / gte | 既存 Compare default の再利用 | Compare trait に SRT default がある。type-specific builtin が不要か性能とdispatchを確認 |

これらの合成元には、既存 String / list の操作や通常 Pattern が使われる。新しい Record / Enum / Tuple / List の Pattern head を追加する作業とは区別する。

置換を採用する場合は public 名・signature・空値 / Unicode / Error の意味を維持し、旧 builtin entry、BUILTIN_IMPLS、未使用 Rust body、compiler 専用分岐を削除する。SRT 実装と旧 builtin を互換経路として並存させない。BUILTIN_METAS の順序による ID、compiled cache、bootstrap / loader stage の影響を確認する。

BigInt 演算・値比較、finite Float の primitive、native String 表現の最小操作、Regex engine まで SRT 化する仕様ではない。SRT 化が再帰の深さ・中間値・走査回数を増やす場合は計測して採用判断し、意味が同じことだけで runtime primitive を一律に削除しない。

## 7. 標準全体から見た追加候補・対象外

### 7.1 scalar の追加候補

以下もすべて guard で、成功 payload は Unit。

| 提案 head | 事前引数 / 条件 | 既存の合成元 |
|---|---|---|
| Int::different_from / Float::different_from / String::different_from | expected と不一致 | Eq::neq |
| Int::between / Float::between | min <= self <= max | Compare。境界を逆転させて自動補正しない |
| Int::multiple_of | divisor で割り切れる | safe_mod。divisor=0 の ZeroDivisionError を保持 |
| Int::remainder_is | divisor, expected remainder | safe_mod の既存符号規則をそのまま利用 |
| Int::bit_set / Int::bit_clear | index 番目の bit | test_bit。NegativeBitIndex を保持 |
| Float::positive / negative / zero / non_negative / non_positive | 0.0 との比較 | Eq / Compare |
| Float::integral | self == trunc(self) | trunc と Eq。Int への cast ではない |
| String::empty / String::not_empty | 空 / 非空 | is_empty / non_empty。通常関数との名前衝突を回避 |
| String::containing | needle を含む | contains。空 needle も成立 |
| String::length_is / String::length_between | surface character 数 | len + Int の比較 |
| Duration::between / at_least / at_most | scalar な時間量の比較 | 既存 Eq / Compare。millis の構造分解は追加しない |

String の大小比較は現行 Compare 実装がないため初期候補に含めない。追加するなら文字列順序の独立した仕様が必要である。Float の approximate match も許容誤差の新契約を伴うため対象外とする。

### 7.2 String 変換・解析の追加候補

| 提案 head | 事前引数 | 成功 payload | 合成元 |
|---|---|---|---|
| String::line_parts | なし | List<String> | lines。改行・末尾空行・CR の既存規則 |
| String::characters | なし | List<String> | chars |
| String::trimmed / left_trimmed / right_trimmed | なし | String | trim / trim_start / trim_end。既存 ASCII whitespace |
| String::decimal | なし | Int | try_to_int。InvalidIntegerLiteral を保持 |
| String::integer_literal | なし | Int | Int::parse_literal。基数 prefix 等は通常関数の契約 |
| String::hexadecimal / octal / binary | なし | Int | Int::parse_hex / parse_oct / parse_bin |
| String::boolean_text | なし | Boolean | try_to_boolean。受理する綴りを変更しない |
| String::encoded | encoding: StringEncoding | List<Int> | codepoints。InvalidStringEncoding を保持 |
| String::compiled_regex | なし | Regex | Regex::compile。RegexCompileError を保持 |
| String::json_text | なし | JsonValue | Json::parse。JsonParseError を保持。Enum payload の構造分解は追加しない |

trim / chars / lines のような常に成功する変換は、通常関数で先に変換する表現と比較して採用する。通常関数の全APIを機械的に Extractor 化せず、Pattern の段階で変換結果を取り出す用途がある候補を優先する。

### 7.3 Regex と汎用 guard の追加候補

| 提案 head | 事前引数 | 照合対象 | 成功 payload | 合成元 |
|---|---|---|---|---|
| String::regex_match | regex: Regex | String | Unit | Regex::is_match。部分一致の現行契約 |
| String::regex_captures | regex: Regex | String | RegexCaptures | Regex::captures |
| String::regex_find | regex: Regex | String | RegexMatch | Regex::find |
| String::regex_separated | regex: Regex | String | List<String> | Regex::split |
| RegexCaptures::group | index: Int | RegexCaptures | String | get。欠落 / 不参加 group の NoneError を保持 |
| RegexCaptures::named_group | name: String | RegexCaptures | String | get_name。同上 |
| Pattern::checked | check: ($A -> Result<Unit>) | $A | Unit | check(self) を SafeBind して OK(()) |

Regex は runtime engine の既存 builtin に委譲し、新しい regex Pattern 文法や String head ごとの native builtin は作らない。RegexCaptures 候補は既存の opaque API から単一値を取得するもので、Record 分解ではない。

汎用 guard は事前引数に通常 callable を受け取る SRT Extractor の候補である。新しい Guard Trait / compiler guard syntax / Trait への defextractor 宣言は追加しない。Kernel::guard は既存 Alternative の関数なので改名・上書きせず、Pattern module の導入もこの候補を採用した場合だけ検討する。

### 7.4 見送る領域

| 標準領域 | 本タスクで見送るもの / 理由 |
|---|---|
| Record / struct / Tuple / List / Range | field / 要素 / head-tail 等の構造分解。既存 Pattern と重複して複雑さを増やす |
| Enum / Boolean / Ordering / Option / Result / Either | variant 判定・payload 分解 Extractor。既存 constructor Pattern を使う |
| JsonValue の as_* / get / at | Enum / object / array の構造検査・投影。JSONテキスト解析候補と区別する |
| HashMap | キー / entry / 全要素の構造抽出。map_get / map_contains_key は既存通常 API を使う |
| Generator | next / unfold 等の進行・状態付き分解。単純な scalar guard ではない |
| Reader / State / Identity / MonadT / Monoid | carrier 実行・state 更新・representation 分解。標準 Pattern API の追加対象ではない |
| Facet | 型検査・source scope を持つ既存 compiler API。汎用の投影 Extractor へ重ねない |
| IO / File / FS / Shell / Process / Task / Random | 入出力、外部状態、乱数、非同期・副作用。arm の試行に副作用を持ち込まない |
| Project / Config / StyledDoc | builder / 表示データの構造分解。新しい guard 条件が必要になった時に独立検討 |
| Function / composition / encode-decode Trait / test / bootstrap | callable 実行・変換一般・tooling / 制御構文。全関数の Extractor 化や新しい special form はしない |

## 8. 実装成果単位・正本・受入条件

### 8.1 成果単位

1. 必須 API の名前、guard / 変換の分類、元の Error の保持と新規 guard Error を確定する。通常関数・予約語・owner identity との衝突を確認する。
2. Int / Float / String literal、比較、Int 符号・parity の SRT Extractor を追加する。
3. String の guard、prefix / suffix、split / split_once を SRT Extractor へ接続する。
4. 第7節の追加候補を必要性に応じて採用する。候補一覧は採用済み API の正本ではない。
5. 第6.2節の通常 builtin 置換を採用する場合だけ、その責務・計測・Rust metadata 削除・検証を別成果単位として実施する。

標準 API の正本は各 `lib/*.srt` の @doc とし、実装後は REPL で動く成功・失敗例を記載する。候補一覧を実装済みと扱わない。ユーザ向け Extractor 説明、Int / Float / String / Regex の各説明、テスト方針は採用した API と実際の builtin 境界へ整合させる。

### 8.2 受入条件

- 各 guard の成功 payload は Unit で、子 Pattern 省略、通常 Unit bind、Unit projection / 注釈、as-pattern での元 Self 取得が成立する。guard payload と as alias の型を混同しない。
- literal は完全一致、比較は self を左側・事前引数を右側として評価し、Int は BigInt のまま、Float は既存 finite-only のまま扱う。
- 正 / 負 / 0、境界値、負数を含む odd / even、BigInt の大きい値で条件が一致する。
- String の空 prefix / suffix / separator、空入力、delimiter 不在、連続 / 末尾 delimiter、Unicode を通常関数と同じ結果・Errorで扱う。
- split List payload は単一の子 Pattern、split_once tuple payload は2個の子 Pattern として扱う。変換 payload を guard と誤認して省略しない。
- SafeBind / apply_pattern は guard mismatch または委譲元 Result の元 Error を保持し、match / if_let / is_match は不成立として破棄する。保持 / 破棄のために委譲処理を二回実行しない。
- 事前引数や子 Pattern の数・型の不一致は前提機能の静的拒否規則に従う。標準 head ごとの特例を加えない。
- 新規 structural head、同名 overload、Result-returning Extractor、guard が Self を返す暗黙モード、新しい native matcher / Opcode を追加しない。
- 新たな通常 builtin の SRT 置換を採用した場合は、意味・Error・evaluation が一致し、旧 native entry / body / metadata と互換 fallback を削除できている。

### 8.3 検証

SRT だけの追加は、Int / Float / String、採用した Regex / Duration 等の既存 lib/tests を最小範囲で拡張し、通常関数と Extractor の成功・失敗境界を確認する。guard の全件を各 consumer で重複検証せず、代表例で共有 consumer policy と as / projection の接続を固定する。

```sh
cargo run -- test --quiet int
cargo run -- test --quiet float
cargo run -- test --quiet string
cargo run -- test --quiet --all
```

runner の selector は `int` / `int.srt` または `lib/tests/int.srt` とし、`tests/int` は使わない。Rust に触れない追加に workspace nextest を要求しない。builtin 置換では関係 crate の責務と cache / loader / REPL 境界を確認し、変更範囲に応じた gate を実行する。文書作成ターンは compiler テストを実行せず、候補の根拠、リンク、未実装との区別、前提機能との整合を検証する。
