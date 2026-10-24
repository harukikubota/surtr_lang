# Error の実装契約

この文書は、`Error` の宣言、構築、保存 Payload、局所具象型、観測、生成責務、位置と情報保持の正本である。利用者向けの説明は [Error Handling](../site/error-handling.md)、標準 Error ごとの入力・保存フィールド・message は `lib/**/*.srt` の定義と `@doc` を参照する。

Pattern consumer と OR の評価は [Pattern spec](Pattern_spec.md)、failure target は [do intrinsic](Do_intrinsic_spec.md)、VM の表現・命令・実行境界は [EldrVM spec](EldrVM_spec.md)、診断表示は [diagnostics](diagnostics.md) と [エラー表示](display_error.md) に従う。

## 1. 共通 Error と保存 Payload

抽象 `Error` の宣言と操作は `lib/types/error.srt`、共通の具象 Error は `lib/errors.srt` に置く。共通の具象 Error には制御・演算・Pattern・Extractor の失敗を含め、shared standard stage に登録する。型・API 固有の Error はその所有ファイルの末尾に置く。具象 Error はトップレベルの `deferror` とし、ファイルの移動で module member に変えない。

`Error` は具象 Error 定義の開いた集合を共通の型で扱う。閉じた Enum にせず、呼出し連鎖から発生し得る Error の集合や、具象名による網羅性を推論しない。実行時の値は常に `deferror` 由来であり、宣言 identity、message、保存 Payload と、runtime が管理する location、cause、diagnostic、stack trace を持つ。

Payload はフィールド名・型・宣言順を持つ。空 Payload も長さ 0 の同じ保存表現を使う。異なる型の値を保持する保存列であり、Surtr の `List<T>` を要求しない。

Error は不透明な通常値として引数、戻り値、型注釈、field、container、closure に保持でき、スコープ・関数・module を越えて運べる。静的な型情報の消去、再格納、返却、失敗伝播で runtime Payload を破棄・再構築しない。`Ok(error)` は成功値であり、Error の存在だけでは失敗伝播しない。

抽象 `Error` の直接構築、`&Error`、Error 自体への Trait impl / derive は許可しない。Error 自体に `Eq` や `Show` は提供しない。`Result<T>` の `Err` 同士の Eq は先頭の宣言 kind だけを比較し、message・Payload・cause・位置・診断情報は比較しない。

## EnumError と Result の Error 位置

`defenumerror` は、宣言済みの具象 Error を列挙する名義型である。同じ項目を持つ別の集合も別型とする。共通 `Error` の開いた集合と区別し、呼出し連鎖の発生し得る Error 集合は推論しない。EnumError の扱いは関数スコープで決まる。呼出し先から集合を連鎖的に伝える経路は前提としない。

```surtr
defenumerror Failures {
  NoneError,
  Other(Error)
}
selected: Failures = NoneError()
named = Failures::NoneError()
```

同じ declaration stage では、集合型を参照する field・署名と集合宣言の記述順に依存しない。分類情報は field の型解決前に登録する。

各項目は canonical な `deferror` identity を参照する。具象 Error の保存 Payload を再宣言しない。Payload を持てる項目は任意の `Other(Error)` だけとし、未宣言名、具象 Error 以外の参照、identity の重複、その他の Payload、idx 指定を拒否する。`Other` の手動構築は許可しない。集合自身への Trait impl と Trait を生成する derive は禁止する。固有関数の `impl Failures` は許可する。Other は定義側が集合の閉じ具合を選ぶ設計パラメータである。Other のない集合は閉集合として網羅性を保証し、Other のある集合は利用者が追加した Error も保持できる。検査強度が共通 Error に近づくことは定義側の選択として許容する。

集合と具象 Error の名前空間は独立に解決する。省略した名前空間はそれぞれ Global とする。`Selected::Failures::Concrete::Failure()` が明示構築の正規形であり、`Failures::Failure()` から登録済みの別名前空間を探索しない。Other を理由に未宣言・不可視の名前を受理しない。

| 入力 | EnumError への格納 |
|---|---|
| 直接の具象 Error 生成、照合済みの局所具象束縛 | 列挙済みの項目へ格納。列挙外なら Other が必要 |
| 共通 Error、異なる EnumError | 変換先に Other が必要。元 Error の canonical kind により分類 |
| 同じ EnumError | 通常の型一致。Other は不要 |

EnumError から共通 Error へは元 Error を取り出して昇格する。構築先は受け取り側の期待型または明示 owner が決める。明示 owner は期待型で上書きしない。入れ子の container・field・Result にも Error 位置まで期待型を渡す。共通 Error へ消去済みの変数や関数結果から生成元を復元しない。集合の部分集合関係を理由に変換を許可しない。

構築、分類、運搬、分解では、元 Error の kind・message・保存 Payload・location・cause・diagnostic・stack trace を保持する。具象コンストラクタは一度だけ呼び、集合への格納で Error を再構築したり cause を追加したりしない。

`Result<T>` は `Result<T, Error>` に相当する。引数、変数、field、関数型、入れ子の Error 位置にも Error または EnumError を指定できる。具象 Error 名は関数定義の直接の戻り値でのみ説明用 metadata として記述でき、実際に返す kind を制限しない。EnumError を指定した戻り値は実型として検査し、呼出し側へも集合型を渡す。型変数の Error 位置も具体化時に Error / EnumError を検査する。

`Result<T>` は `Either<Error, T>` に相当する。省略した Error 位置は内部で共通 Error を導出し、`Result<T, Error>` として扱う。この導出は内部規則であり、ソースに `Result<T, _>` は書けない。変数、field、引数、戻り値、関数型、入れ子、call-site ReturnTypeArgument のいずれでも Error 位置の明示 `_` を拒否する。入力に集合を持つ Result は共通 Error へのキャストを適用して受理する。`Result<T, Errors>` は明示された EnumError を保持する。通常の標準 helper は `Result<T>` を受け取り、`Result<U>` の共通 Error を返す。入力の集合型を返却シグネチャへ自動継承しない。

利用者が定義した Trait impl の呼出しは、その実装のシグネチャに従う。Trait 名だけを理由に利用者実装の集合型を消去しない。受け取り側に集合を指定した場合は通常の変換規則を適用する。Result 全体の変換は成功型と外枠を保ち、Err の Error 位置だけを変換する。

EnumError の match は外側の列挙項目と Other の網羅性を検査する。Other は列挙済み項目を覆わない。列挙項目の具象 Error Pattern に成功した alias は、元 Error の局所具象束縛となる。Other からの取り出しは共通 Error であり、子 Pattern による絞り込みは既存の Error Pattern 規則に従う。

値を関数・クロージャ・field・container に渡す地点と返却地点では、受け取り側の期待型に対して上記のキャストルールを適用する。EnumError の期待型があれば、消去前の具象 identity の所属または Other の存在を検査して集合型として渡す。Error の期待型または期待型の指定がない場合は共通 Error として扱う。標準定義の `Result<T>` は常に共通 Error と解釈する。各 branch・クロージャの返却もこの規則に従う。共通 Error を返す関数・クロージャのシグネチャを期待型で読み替えない。Error モジュールの操作と Result の Err の Eq は Error / EnumError の両方を受理し、元 Error の canonical kind を使う。

`ResultT<M, T, E>` は `M<Result<T, E>>` を保存する。保存型の E と標準関数の返却契約は区別し、`new` / `run` も共通 Error の契約を使う。明示した EnumError は field と、その型を指定する利用者関数のシグネチャで保持する。本機能は未リリースであり、旧二引数の互換経路や未確定 E を Error に補う経路は設けない。`ResultT<M, T, _>` は拒否し、共通 Error を使う場合も `ResultT<M, T, Error>` と明示する。Error 位置の型変数は具体化時に Error / EnumError を検査する。`MonadFail::fail(error: Error)` は E が Error なら直接保持、Other を持つ EnumError なら分類して保持し、Other のない EnumError ならコンパイルエラーとする。SafeBind と partial `<-` も同じ条件を使う。ResultT の失敗は内側 Result に保持し、base の失敗へ移さない。

Error の操作は他の層で抽象化し、Error 自体への Trait 実装は提供しない。具象 Error は Payload を持てるが、共通 Error はその Payload を直接操作する型ではなく、局所的な具象型へ実装を増やさない。具象 Error への impl も禁止する。

`impl Ty` は特定の型 Ty の `defmod Ty` として扱い、型ごとに一度だけ定義できる。重複はエラーとし、標準定義が `impl Error` を提供しているため、利用者の `impl Error` は重複エラーになる。この一度限りの規則と Trait impl 禁止は別の契約である。

## 2. 宣言と内部構築

```surtr
deferror NegativeValue(number: Int) {
  |input: Int|
  Self(message: "negative: #{input}", number: input)
}
deferror MissingValue { "missing value" }
deferror FixedValue() { Self(message: "fixed value") }
```

ヘッダは保存する Payload スキーマ、本体先頭の `|args...|` は外部コンストラクタの入力である。両者は名前・型・個数が同じでも別に検査する。ヘッダのフィールドを本体の引数として自動束縛しない。空ヘッダは括弧の省略と `Name()` の両形式を受理し、0 引数の `||` は省略できる。コンストラクタ入力には型注釈が必要であり、式と引数は既存の型規則で検査する。

具象 Error 定義では、保存 Payload とメッセージコンストラクタ引数を総称型で宣言できない。`$T` は `List<$T>`、tuple、関数型などの内部でも禁止し、`GenericTypeInErrorDefinition` で型変数の位置を示す。`List<Int>` のように型引数が具体化済みの型は許可する。総称値の処理は呼び出し元で行い、具体型の値をコンストラクタに渡す。

Payload 名は既存のフィールド名規則に従い、追加で `kind` と `message` を禁止する。重複名も拒否する。`message` は共通情報であり、保存 Payload には含めない。Facet と MatchResult を通常値として運搬する能力は追加しない。保存 Payload と入力に Facet を含む型を使う定義や、直接の MatchResult 運搬は既存の拒否規則に従う。

外部 `NegativeValue(input)` とその constructor capture は入力署名を使い、型注釈も明示 owner もない生成値の型は共通 `Error` である。EnumError の期待型または明示 owner がある場合は、以下の所属規則で集合へ格納する。生成直後の値に局所具象型は付けない。

本体内の `Self(...)` と、当該宣言 identity へ解決した宣言名 App は内部構築である。`Self` を同じ identity に正規化し、名前の綴りから判定しない。内部構築は外部コンストラクタの再帰呼出しではない。

内部構築で指定できる項目は `message: String` と宣言済み Payload だけである。kind、location、cause、diagnostic、stack trace はコンパイラ・runtime が管理する。内部構築 App は宣言本体のトップレベル末尾に限る。途中の式、束縛 RHS、引数、入れ子 block、closure、branch の内部に置く形と、構築値を変数経由で返す形は拒否する。

Payload が空の場合は、末尾の String 式を message として同じ Error 構築へ正規化できる。String を計算する `if` / `match` 等の分岐も許可する。トップレベル末尾という内部構築 App の制限を、String 式の内部へ適用しない。

```surtr
deferror SignDescription {
  |value: Int|
  if(value < 0, "negative", "nonnegative")
}
```

非空 Payload の String 終端、別の Error 値、String 以外の値を本体の最終結果にする形は拒否する。非空 Payload で分岐が必要なら、通常の値を計算してから末尾で一度だけ内部構築する。通常の分岐の型一致は維持する。

## 3. 引数と名前付きフィールド

外部 Error コンストラクタ、内部構築、Error Payload Pattern に次の正規化を適用する。

1. 明示的キーワードがなければ、すべて位置指定として検査する。裸変数の名前が宣言名と異なっても位置指定である。
2. キーワードが一つでもあれば、裸変数を `name: name` に正規化し、すべて名前指定として検査する。
3. 名前指定の列へ混在した literal・任意式を、空いている位置へ自動割当てしない。未知名、不足、重複、型不一致は拒否する。

外部 App は入力署名に対応させる。内部構築の位置順は message、続いて Payload 宣言順である。Pattern の位置順は Payload 宣言順であり、message を含めない。

```surtr
deferror PairError(left: Int, right: Int) {
  |a: Int, b: Int|
  left = a
  right = b
  Self(message: "pair", left, right)
}
a = 1
b = 2
PairError(b, a)    # 位置指定
PairError(a: a, b) # a: a, b: b
# PairError(a: a, 2) は拒否
```

名前付き Payload Pattern の名前省略は裸の束縛名に限る。wildcard・literal・複合 Pattern は `field: child` を明記する。名前付き子と位置指定の型注釈の区別は既存の構文規則に従う。この正規化を Record、Enum、一般関数の引数規則へ拡張しない。

## 4. Error Pattern と局所具象型

Error 名だけの Pattern は、Payload の有無にかかわらず kind だけを照合する。`NegativeValue(number)` は kind 一致後に保存 Payload を分解する。外部入力署名を分解しない。名前指定も同じ保存スキーマに対応させる。

`match`、`if_let`、`if_let_then`、`Test::assert_error_matches` で単一の Error identity への照合に成功した `@ e` は、元 Error を保持する局所具象束縛になる。`Err(e)`、`_ @ e`、異なる具象 identity を束ねた OR 全体の alias は共通 Error である。同名・同型の Payload を持つ別 identity も統合しない。OR の束縛名・canonical 型・個数の Set 比較、重複拒否、評価順は [Pattern spec](Pattern_spec.md) に従う。

```surtr
def read_number(error: Error) -> Int {
  match error {
    NegativeValue(number) @ e => {
      alias: NegativeValue = e
      alias.number
    },
    _ => 0,
  }
}
```

局所の `alias = e` は具象情報を引き継ぐ。具象型注釈は導出済み束縛との一致検査に限る。共通 Error への具象注釈によるキャストと、具象束縛への `alias: Error = e` は拒否する。通常の引数・戻り値・field・入れ子の型位置へ具象名を公開しない。関数定義の直接の戻り値にある既存 `Result<T, E>` の補助 metadata はドキュメント用に維持する。指定したエラー名の存在と記述位置は検査するが、実際に返す kind の静的制限・網羅検査は行わず、指定と異なる kind を返すことだけでは拒否しない。成功型と引数型は通常どおり検査する。

EnumError の期待型がない場合、成功 branch の結果、通常引数・戻り値、container・tuple・field へ渡る型は共通 Error になる。取り出した値へ具象情報を復元しない。分岐間の型一致はこの結果型で検査する。静的な情報消去で runtime の kind・message・Payload は変えない。

成功スコープで作った通常の closure はレキシカルな束縛と具象情報を捕捉する。同名再束縛は新しい束縛であり、旧束縛を捕捉した closure に影響しない。closure が Error 自体を返せば戻り値は共通 Error、保存フィールドを返せば通常のフィールド型になる。

`is_match` は kind と Payload 子 Pattern の一致・不一致を Boolean で返し、通常 bind と as alias を許可しない。局所具象束縛を作らない。Error 定義 Pattern は通常 Bind、SafeBind、do partial `<-`、`apply_pattern` で拒否する。Error 全体の通常値運搬とユーザー定義 Extractor は既存規則に従う。kind の文字列比較による型絞り込みや一般のキャストは追加しない。

`Test::assert_error_matches` は照合成功 scope の局所具象情報と保存 Payload を同じ規則で扱う。Error identity の不一致は空 Payload の `TestErrorPatternKindMismatch` とし、子 Pattern・Extractor の失敗は元 Error と失敗位置を保持する。成功側の Lazy callable は Result<Unit> を返す。詳細は [Pattern spec](Pattern_spec.md#testassert_error_matches) に従う。

`MonadExtractor::extract_error` と `Test::expect_error` は extract を一度だけ呼び、Err 内の Error を全情報を保持した Ok に移す。一段だけを対象とし、成功値の内側は調べない。成功入力にはそれぞれ空 Payload の `ErrorExtractionFailed`（固定 message `error extraction failed`）、`TestExpectedError`（`expected an error, got <元入力の inspect>`）を返す。共通側は inspect せず、Test 側は失敗分岐だけで元入力を一度 inspect する。

## 5. 共通観測と readonly Facet

共通 `err.kind` / `err.message` は、共通 Error にも局所具象束縛にも許可する。対応する `Error::kind` / `Error::message` と同じ読み取りである。cause や location を一般の field として公開しない。

| 形 | 契約 |
|---|---|
| `err.message` / `err.kind` | Error 型で常に読める |
| `Error.message` / `Error.kind` | 共通情報の readonly path |
| `&Error.message` / `&Error.kind` | 共通 Error を読む通常の unary capture |
| `MyError.value` | 保存 Payload の readonly path。作成時には値の照合は不要 |
| `e.value` / `Facet::view(MyError.value, e)` | 同じ identity への照合に成功した局所具象束縛に限り読める |
| `&MyError.message` / `&MyError.value` | 具象 Error root の path capture として拒否 |
| `{|| e.value}` | 成功スコープの局所具象束縛を通常 closure で捕捉できる |

Payload path を消費する時点で対象の局所具象 identity を検査する。Facet API の検査へ渡すだけで、その情報を先に消去しない。共通 Error、外部コンストラクタの生成値、container・tuple・field から取り出した Error への直接適用は拒否する。

Payload path は同じレキシカルスコープと内側 closure の Facet API で消費する。一般の引数、戻り値、container へ path 自体を運搬しない。具象 root と具象束縛の path capture、`&Facet::view(MyError.value, &1)` 等の capture で局所照合を置き換えない。共通 `&Error.message` の capture はこの制限の対象ではない。

共通 kind / message と Payload を通る更新は readonly として拒否する。`put`、`set`、`over`、bulk update 等の直接・深い・合成パスでも同じで、宣言元や標準コードに更新権限を設けない。取り出した Payload 値自体は通常値であり、その値への Facet 操作は既存規則に従う。

## 6. 生成責務と宣言契約の検証

利用者、標準関数、builtin、VM、Forge 出力が実行時に生成する言語レベルの Error は、解決済み `.srt` の `deferror` コンストラクタを通す。生成側は発生条件に必要な型付き入力を渡し、message の文型と保存 Payload は定義本文が構築する。Rust 側に独立した message テンプレートや Payload の組立てを重複させない。

OS・外部 parser の原文 detail、利用者の説明、任意型の `inspect` 結果は必要な String 入力として渡せる。完成 message を標準の入力方式として単に転送したり、その文章を解析して kind・入力値を復元したりしない。各 API 固有の失敗条件・入力・保存フィールドは標準定義を正本とし、存在しない値を架空の入力や空文字で補わない。

生成入口は canonical な宣言 identity と、入力署名・型・保存スキーマを接続する。宣言や生成側を変更した際の不整合は静的契約検査または runtime 契約検証で検出する。未解決 identity、署名・スキーマ不一致を手組み Error、空 Payload、別の汎用 Error へフォールバックしない。保存値の長さ・型・宣言順も検査する。

コンパイルフェーズはコンストラクタを評価しない。Forge は実行時に定義を呼ぶ命令を出力し、builtin / VM からの生成も既存の Call / Resume と継続処理へ接続する。Rust helper から VM を同期再帰駆動しない。バイトコードの metadata と値検証の詳細は [EldrVM spec](EldrVM_spec.md) に従う。

ParseError、ResolveError、TypeError、CodegenError と VM 内部不整合は、recoverable な言語 Error へ変換しない。型検査済み表現の破損、未知 tag、欠損 metadata、未知 MatchResult tag 等は内部契約違反として扱い、利用者の通常の失敗値と区別する。

`ErrorKind` は具象 `deferror` の canonical declaration identity を保持する不透明な通常値である。値式での裸名は、期待型によらず ErrorKind を生成する。`NoneError` と `(NoneError)` は ErrorKind、`NoneError()` は Error であり、Error 期待位置でも裸名を暗黙に呼び出さない。入力を持つ宣言の裸名も ErrorKind となり、コンストラクタの入力 arity に依存しない。最初に選ばれた参照が通常値なら、同名の外側 deferror へ再解決しない。

ErrorKind は変数・引数・戻り値・Tuple・List・field・closure で運搬できる。公開 field、constructor、Eq / Show、利用者 Trait impl、String や Error からの変換、ErrorKind からの動的 Error 構築は提供しない。`inspect` は `ErrorKind(canonical qualified name)` と表示し、container 内も同じ規則に従う。`Error::is_kind(error, kind)` は canonical identity の一致を Boolean で返し、局所具象情報を付けない。Pattern head、型位置、readonly Facet root、`&NoneError` の constructor capture はそれぞれの既存規則を維持する。

`MonadRecover: MonadFail` は保持した Error による回復を表す。`recover(self: Self, handler: (Error -> Self)) -> Self` は失敗時だけ handler 本体を呼び、成功値と handler の再失敗をそのまま返す。Trait の `recover_kind` は recover、is_kind、fail の合成であり、不一致時は元 Error を再構築せず伝播する。通常の `Result::recover` / `Result::recover_kind` は直接の match で共通 Error を返し、入力 Error 位置の Trait 能力を要求しない。初回の標準実装は Result のみ。引数の kind / handler を生成する式は通常の eager 評価であり、SafeBind の failure target と MonadExtractor による RHS 分解は独立した契約として扱う。

## 7. 生成位置と情報の保持

Error の主キャプションは生成位置を指す。明示的な構築では外部コンストラクタの呼出式、構文 Pattern の不一致では失敗した子 Pattern、構造自体の不一致ではその構造 Pattern 全体を使う。alias は照合しないため、失敗位置を変更しない。

コンストラクタ実行には生成元の source ID / span を渡し、`deferror` 宣言や内部 `Self` の位置で上書きしない。Source map の確定位置を使い、表示文、Error 名、carrier 名から位置を推測しない。module と REPL でも同じ規則を使い、REPL の後続入力から呼び出した定義は元入力の位置を保持する。

引数渡し、`Err` / `MatchResult::Err` への再格納、返却、および Error を保持する失敗先への SafeBind・partial `<-`・文末 `?` は、元の kind・message・Payload・location・cause・diagnostic・stack trace を再生成しない。Extractor の Err や既存 Error を親の汎用 Pattern Error で上書きしない。Extractor 成功後の子 Pattern が失敗した場合は、その子自身の失敗を使う。

SafeBind は canonical MonadExtractor の通常呼出しが返した Result を外側一段だけ処理する。RHS が Result でも標準 impl を迂回しない。成功値に含まれる失敗や子 Pattern に自動分解を再適用しない。

文末 `?` は、異なる MonadExtractor 実装型の入れ子も含め、成功値が Unit に至るまで各層の extract を一度ずつ呼ぶ。どの層でも返却 Result が Err なら、その元 Error を同じ現在の失敗先へ渡し、内側の分解と後続を実行しない。全層が Ok で Unit に到達した場合だけ後続へ進む。各層の分解能力と有限段で Unit に至ることは静的に確認する。

標準の Result / Either<Error, A> の extract は、既存 Error の kind・message・Payload・location・cause・diagnostic・stack trace を保持する。文末 `?` の各層から失敗先へ渡す際も元 Error を再生成しない。失敗先が Alternative の場合は、下記の規則に従って破棄する。Option の None は `NoneError()` を通常どおり生成し、その後の伝播で生成位置を上書きしない。

MonadExtractor は値から Result への分解、MonadFail は Error を受け取る失敗返却先であり、相互に能力を付与しない。ユーザー型は Error 以外の値から具体 Error を生成する extract を明示実装できるが、既存 Error を受け取る fail の保持契約とは区別する。入力分解の不足と失敗返却先の不足は別の診断にする。

新しい Error で wrap する場合は、新しい構築位置を使い、元 Error を cause に保持する。cause は線形 chain とし、各操作の連結・置換は標準 API の規則に従う。stack trace は呼出し経路を示し、先頭 frame で生成位置を代用しない。

非保持 Pattern consumer と Alternative route は既存規則で Error を破棄する。不要な Error の表示・記録・再生成を追加しない。どの consumer が保持または破棄するかは [Pattern spec](Pattern_spec.md) と [do intrinsic](Do_intrinsic_spec.md) に従う。

do の Alternative route は、その do が所有する失敗処理に限って適用する。Error の発生場所による区別ではなく、呼出し先から返された Err も、その do の SafeBind や文末 `?` で処理すれば対象となる。失敗先は carrier の能力で決まり、`List<Result<Unit>>` の payload に Result があることを理由に Error の保持経路を選ばない。通常の値として保持した Error / Result や、別 callable・nested do 自身の失敗処理先には、この route を適用しない。

## 8. フェーズの責務と検証先

- Spire: Payload ヘッダとコンストラクタ block 引数、App と Pattern の構文を区別する。
- Sigil: 宣言 identity、`Self`、入力と局所束縛のレキシカルな参照、Pattern の宣言 head を解決する。
- Scar: 入力・Payload・終端構築、Pattern 分解、局所具象情報と消去、readonly と capture の境界を検査する。
- Forge: 検査済み宣言 metadata から定義呼出しと構築・照合・観測の命令を生成する。増分コンパイルでも可視 prefix の検査済み型定義を接続し、バイトコードから型を推測しない。
- Eldr: 定義の実行、runtime 入力・保存値のスキーマ検証、生成元位置と既存情報の保持を担う。

直接の成功・拒否境界は `crates/scar/tests/error_payload.rs` と `error_values.rs`、実行時の観測は `lib/tests/language_features/error_payload.srt` と `error_values.srt`、生成・保持・位置は Forge / Sindr / Eldr の crate-local テスト、Rune / Xldr の integration テストで固定する。テストの配置と実行方法は [テスト方針](テスト方針.md) に従う。
