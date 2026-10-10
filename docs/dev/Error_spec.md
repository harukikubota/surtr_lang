# Error の実装契約

この文書は、`Error` の宣言、構築、保存 Payload、局所具象型、観測、生成責務、位置と情報保持の正本である。利用者向けの説明は [Error Handling](../site/error-handling.md)、標準 Error ごとの入力・保存フィールド・message は `lib/**/*.srt` の定義と `@doc` を参照する。

Pattern consumer と OR の評価は [Pattern spec](Pattern_spec.md)、failure target は [do intrinsic](Do_intrinsic_spec.md)、VM の表現・命令・実行境界は [EldrVM spec](EldrVM_spec.md)、診断表示は [diagnostics](diagnostics.md) と [エラー表示](display_error.md) に従う。

## 1. 共通 Error と保存 Payload

`Error` は具象 Error 定義の開いた集合を共通の型で扱う。閉じた Enum にせず、呼出し連鎖から発生し得る Error の集合や、具象名による網羅性を推論しない。実行時の値は常に `deferror` 由来であり、宣言 identity、message、保存 Payload と、runtime が管理する location、cause、diagnostic、stack trace を持つ。

Payload はフィールド名・型・宣言順を持つ。空 Payload も長さ 0 の同じ保存表現を使う。異なる型の値を保持する保存列であり、Surtr の `List<T>` を要求しない。

Error は不透明な通常値として引数、戻り値、型注釈、field、container、closure に保持でき、スコープ・関数・module を越えて運べる。静的な型情報の消去、再格納、返却、失敗伝播で runtime Payload を破棄・再構築しない。`Ok(error)` は成功値であり、Error の存在だけでは失敗伝播しない。

抽象 `Error` の直接構築、`&Error`、Error 自体への Trait impl / derive は許可しない。Error 自体に `Eq` や `Show` は提供しない。`Result<T>` の `Err` 同士の Eq は先頭の宣言 kind だけを比較し、message・Payload・cause・位置・診断情報は比較しない。

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

Payload 名は既存のフィールド名規則に従い、追加で `kind` と `message` を禁止する。重複名も拒否する。`message` は共通情報であり、保存 Payload には含めない。Facet と MatchResult を通常値として運搬する能力は追加しない。保存 Payload と入力に Facet を含む型を使う定義や、直接の MatchResult 運搬は既存の拒否規則に従う。

外部 `NegativeValue(input)` とその constructor capture は入力署名を使い、生成値の型は常に共通 `Error` である。生成直後の値に局所具象型は付けない。

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

`match`、`if_let`、`if_let_then` で単一の Error identity への照合に成功した `@ e` は、元 Error を保持する局所具象束縛になる。`Err(e)`、`_ @ e`、異なる具象 identity を束ねた OR 全体の alias は共通 Error である。同名・同型の Payload を持つ別 identity も統合しない。OR の束縛名・canonical 型・個数の Set 比較、重複拒否、評価順は [Pattern spec](Pattern_spec.md) に従う。

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

局所の `alias = e` は具象情報を引き継ぐ。具象型注釈は導出済み束縛との一致検査に限る。共通 Error への具象注釈によるキャストと、具象束縛への `alias: Error = e` は拒否する。通常の引数・戻り値・field・入れ子の型位置へ具象名を公開しない。関数定義の直接の戻り値にある既存 `Result<T, E>` の補助 metadata は維持し、実際の Error 集合を検査する能力へ拡張しない。

成功 branch の結果、通常引数・戻り値、container・tuple・field へ渡る型は共通 Error になる。取り出した値へ具象情報を復元しない。分岐間の型一致はこの結果型で検査する。静的な情報消去で runtime の kind・message・Payload は変えない。

成功スコープで作った通常の closure はレキシカルな束縛と具象情報を捕捉する。同名再束縛は新しい束縛であり、旧束縛を捕捉した closure に影響しない。closure が Error 自体を返せば戻り値は共通 Error、保存フィールドを返せば通常のフィールド型になる。

`is_match` は kind と Payload 子 Pattern の一致・不一致を Boolean で返し、通常 bind と as alias を許可しない。局所具象束縛を作らない。Error 定義 Pattern は通常 Bind、SafeBind、do partial `<-`、`apply_pattern` で拒否する。Error 全体の通常値運搬とユーザー定義 Extractor は既存規則に従う。kind の文字列比較による型絞り込みや一般のキャストは追加しない。

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

`ErrorKind` を受け取る標準 API は具象 `deferror` の宣言 identity を使う。一般の runtime 値や任意の文字列による kind 指定は公開しない。API ごとの marker 位置、静的な種類列、capture と評価規則は [Lazy spec](Lazy_spec.md) と標準宣言に従う。

## 7. 生成位置と情報の保持

Error の主キャプションは生成位置を指す。明示的な構築では外部コンストラクタの呼出式、構文 Pattern の不一致では失敗した子 Pattern、構造自体の不一致ではその構造 Pattern 全体を使う。alias は照合しないため、失敗位置を変更しない。

コンストラクタ実行には生成元の source ID / span を渡し、`deferror` 宣言や内部 `Self` の位置で上書きしない。Source map の確定位置を使い、表示文、Error 名、carrier 名から位置を推測しない。module と REPL でも同じ規則を使い、REPL の後続入力から呼び出した定義は元入力の位置を保持する。

引数渡し、`Err` / `MatchResult::Err` への再格納、返却、SafeBind、MonadFail context の partial `<-` は、元の kind・message・Payload・location・cause・diagnostic・stack trace を再生成しない。Extractor の Err や既存 Error を親の汎用 Pattern Error で上書きしない。Extractor 成功後の子 Pattern が失敗した場合は、その子自身の失敗を使う。

新しい Error で wrap する場合は、新しい構築位置を使い、元 Error を cause に保持する。cause は線形 chain とし、各操作の連結・置換は標準 API の規則に従う。stack trace は呼出し経路を示し、先頭 frame で生成位置を代用しない。

非保持 Pattern consumer と Alternative route は既存規則で Error を破棄する。不要な Error の表示・記録・再生成を追加しない。どの consumer が保持または破棄するかは [Pattern spec](Pattern_spec.md) と [do intrinsic](Do_intrinsic_spec.md) に従う。

## 8. フェーズの責務と検証先

- Spire: Payload ヘッダとコンストラクタ block 引数、App と Pattern の構文を区別する。
- Sigil: 宣言 identity、`Self`、入力と局所束縛のレキシカルな参照、Pattern の宣言 head を解決する。
- Scar: 入力・Payload・終端構築、Pattern 分解、局所具象情報と消去、readonly と capture の境界を検査する。
- Forge: 検査済み宣言 metadata から定義呼出しと構築・照合・観測の命令を生成する。増分コンパイルでも可視 prefix の検査済み型定義を接続し、バイトコードから型を推測しない。
- Eldr: 定義の実行、runtime 入力・保存値のスキーマ検証、生成元位置と既存情報の保持を担う。

直接の成功・拒否境界は `crates/scar/tests/error_payload.rs` と `error_values.rs`、実行時の観測は `lib/tests/language_features/error_payload.srt` と `error_values.srt`、生成・保持・位置は Forge / Sindr / Eldr の crate-local テスト、Rune / Xldr の integration テストで固定する。テストの配置と実行方法は [テスト方針](テスト方針.md) に従う。
