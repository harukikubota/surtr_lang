# Error Payload 拡張・生成経路統一の修正仕様

## 状態と目的

本書は未実装の修正仕様である。入力仕様と調査後の指定を反映し、Error の Payload 保存、局所的な具象型、ランタイムでの生成経路の統一、および標準 Error 定義の段階移行を定める。型・評価・フェーズ間契約を変更するため level4 とする。

`Error` を具象 Error 定義の開いた集合として扱う。Error 集合を閉じた Enum にしたり、呼出し連鎖から発生し得る Error を推論したりしない。通常値としての運搬と既存の失敗伝播を維持し、照合成功の局所束縛から必要な Payload を読めるようにする。

構造体・レコードの構築規則、一般関数の引数規則、Enum の構築・分解規則の変更は本書の対象外である。Error の引数正規化は本書に記載した規則だけで完結する。

## 現状と変更点

| 項目 | 現行 | 変更後 |
|---|---|---|
| Error の運搬 | 引数・戻り値・field・container・closure で通常値として運搬 | 維持し、Payload も同じ値に保持 |
| `deferror` ヘッダ | コンストラクタ引数 | 保存する Payload のスキーマ |
| コンストラクタ入力 | ヘッダから生成 | 本体先頭のブロック引数 |
| 本体の結果 | String | 空 Payload の String、または末尾の直接内部構築 |
| Error Pattern | kind 照合と `Kind @ err: Error` | kind 照合、Payload 分解、限定された局所型絞り込み |
| Payload の観測 | 保存・分解を公開しない | Pattern 分解と readonly FacetPath |
| OR の束縛整合性 | 名前・型・個数・順序が一致 | 名前・型・個数の Set が一致。順序は問わない |
| 実行時の内部 Error 生成 | `.srt` 定義と Rust の直接生成がある | ランタイムで発生する言語レベルの Error はすべて `.srt` 定義を通す |

現状の確認先は `lib/types/error.srt`、`lib/types/result.srt`、`lib/bootstrap.srt`、`crates/scar/src/checker/{definitions,matching,patterns,types}.rs`、`crates/sindr/src/runtime.rs`、`crates/forge/src/codegen.rs`、`crates/eldr/src/{builtin,vm}.rs` である。既存の成功・拒否境界は `crates/scar/tests/error_values.rs` と `lib/tests/language_features/error_values.srt` にある。

## 1. 共通 Error と Payload

- 各 `deferror` は宣言 identity、フィールド名・型・宣言順を含む固有の Payload スキーマ、および唯一のコンストラクタを定義する。
- Payload は空の場合も長さ 0 の同じ保存表現とする。Payload の有無を `Option` や別の旧 Error 表現で分岐させない。
- 内部の Payload 列は異なる型の値を保持する概念モデルであり、Surtr の `List<T>` を要求しない。
- 共通 Error は kind、message、Payload と、既存の location、cause、diagnostic、stack trace を保持する。
- 型情報の消去、引数渡し、格納、返却、失敗伝播で Payload を破棄・再構築しない。
- 抽象 `Error` の直接構築、Error 自体への Trait impl / derive、一般値としての ErrorKind は引き続き許可しない。
- Result の `Err` 同士の Eq は先頭の宣言 kind だけを比較する。Payload や message による構造的な Error 比較は追加しない。

## 2. `deferror` の宣言と内部構築

### 2.1 宣言

```surtr
deferror NoValueError { "no value" }
deferror FixedError() { Self(message: "fixed error") }

deferror NegativeIntError(num: Int) {
  |value: Int|
  num = value
  Self(message: "negative: #{value}", num)
}
```

ヘッダの括弧内は Payload の Record 宣言であり、外部コンストラクタの引数ではない。空ヘッダは `Name()` と括弧を省略した `Name` の両方を受理する。

外部コンストラクタの入力は本体先頭の `|args...|` から確定する。0 引数は `||` を省略できる。引数・式の型検査は通常のブロック引数と式の機構を使う。Payload と入力が同じ名前・型であっても、別々の宣言として検査する。

Payload フィールド名は既存のフィールド名規則に従い、追加で `kind` と `message` を禁止する。`message` は内部構築の共通フィールドであり、保存 Payload のフィールドにはならない。重複名は拒否する。

### 2.2 外部コンストラクタ App と内部構築 App

外部の `NegativeIntError(...)` はブロック引数を受け取り、戻り値は常に共通 `Error` である。生成直後に具象型を与える規則は追加しない。

`deferror` 本体の `Self(...)` と宣言中の Error 名による App は、内部構築として区別する。宣言名の同定は解決済みの宣言 identity に基づき、名前の綴りから推測しない。`Self` はこの identity に正規化する。

内部構築に指定できる項目は `message: String` と宣言済み Payload フィールドだけである。kind、location、cause、diagnostic、stack trace はコンパイラ・ランタイムの管理情報として統合する。内部構築は外部コンストラクタの再帰呼出しではない。

内部構築 App は、宣言本体のトップレベルかつ末尾の式に限って許可する。途中の式、束縛 RHS、引数、入れ子の block、closure、branch の内部に置いた同じ内部構築は拒否する。末尾に構築値を持つ変数を返すことも許可しない。

### 2.3 本体の最終結果

- トップレベルの末尾が当該宣言の内部構築 App なら、フィールドを検査して共通 Error を生成する。
- Payload が空の場合だけ、末尾の String 式を受理する。式の評価結果を message とし、長さ 0 の Payload を持つ同じ Error 表現へ正規化する。
- Payload がある定義で String を返した場合は TypeError とする。
- 内部構築以外の Error 値や、String 以外の値を返す場合は拒否する。

通常の分岐式の型一致は維持する。Payload がある定義では、分岐で通常のフィールド値を計算してから、末尾で一度だけ内部構築する。branch 内の内部構築を許す特例は設けない。

```surtr
deferror NegativeIntError(num: Int) {
  |value: Int|
  num = value
  message = if(value < 0, "negative: #{value}", "unexpected nonnegative: #{value}")
  Self(message: message, num: num)
}
```

拒否する形は次のとおりである。

```surtr
deferror MissingPayload(num: Int) { |value: Int| "negative: #{value}" }
deferror MissingField(num: Int) { |value: Int| Self(message: "negative") }
deferror StoredConstruction(num: Int) {
  |value: Int|
  built = Self(message: "negative", num: value)
  built
}
```

## 3. 引数とフィールド指定の正規化

外部 Error コンストラクタ、内部構築、Payload の名前付き Pattern に次の分類を適用する。裸の項目の名前から位置指定とキーワード指定を推測して切り替えない。

1. 明示的なキーワード指定が一つもない場合、全項目を位置指定として扱う。
2. 明示的なキーワード指定を含む場合、裸の変数名を `name: name` へ正規化する。全項目を名前指定として検査する。
3. 名前指定を含む列に、名前を省略した任意の式・literal を置いても、未指定の位置へ自動割当てしない。名前で対応できない項目は拒否する。

外部 App はブロック引数の宣言に対応させる。内部構築の位置順は `message`、続いて Payload の宣言順とする。Payload Pattern の位置順は Payload の宣言順であり、message を含めない。

```surtr
deferror PairError(left: Int, right: Int) {
  |a: Int, b: Int|
  left = a
  right = b
  Self(message: "invalid pair", left, right)
}

a = 1
b = 2
PairError(b, a)       # 位置指定。a に b、b に a の値を渡す
PairError(a: a, b)    # a: a, b: b に正規化
# PairError(a: a, 2)  # 拒否。残った位置を推測しない
```

名前指定の検査では、対象の宣言名・型と、値を参照するローカル変数の存在・型を確認する。未知名、不足、重複、型不一致は拒否する。全項目が裸の変数であれば、その変数名が宣言名と違っても位置指定として検査する。

Pattern の名前省略は、フィールドに対応する裸の束縛名を名前付き子 Pattern に正規化する。wildcard、literal、複合 Pattern を名前なしで混ぜてもフィールドを推測しない。その場合は `field: child` を明記する。Pattern の通常の型注釈・名前付き子の区別は既存の構文規則を維持する。

## 4. Error Pattern と局所具象型

```surtr
def describe(error: Error) -> String {
  match error {
    NegativeIntError(num) @ e => "#{num}: #{e.message}",
    NoValueError @ e => e.message,
    other => other.message,
  }
}
```

- 定義名だけの Pattern は、Payload の有無にかかわらず kind だけを照合する。
- `NegativeIntError(num)` は kind 一致を確認してから Payload を宣言順で分解する。Pattern が参照するのは外部コンストラクタの入力ではなく、保存 Payload のスキーマである。
- 名前指定と正規化後のフィールド対応にも同じスキーマを使う。未宣言名・不足・重複・型不一致を拒否する。
- ダウンキャスト対象式で単一の Error 定義への照合に成功した `@ e` は、元 Error を保持する局所具象型の変数束縛となる。
- `Err(e)` や `_ @ e` は共通 Error の束縛であり、型を絞り込まない。
- 具象 Error 名の型注釈は、具象型が導出済みの局所変数に対する一致検査に限る。共通 Error からのキャストには使わない。

照合成功スコープ内の `x = e` は具象情報を引き継ぐ。`x: NegativeIntError = e` も許可する。一方、共通 Error に対する具象注釈と、具象束縛への `x: Error = e` は一致しないため拒否する。

通常の値型の引数・戻り値・フィールド・入れ子の型位置に具象名を記載して、局所具象型を公開することは許可しない。関数定義の直接の戻り値に書ける既存の `Result<T, E>` の Error 契約は宣言 metadata として維持する。実際の Error 集合や網羅性を検査せず、複数名の列挙構文は追加しない。

### 4.1 ダウンキャスト対象

利用者が参照できる変数へ具象 Error として束縛するダウンキャストは、`match`、`if_let`、`if_let_then` の3種に固定する。

`is_match` でも Error の Payload に対する Pattern を実行できる。定義名だけなら kind を照合し、子 Pattern を指定した場合は kind 一致後に保存 Payload を照合する。結果は Boolean であり、具象 Error の変数束縛や局所型情報を利用者へ公開しない。通常 bind と as alias を拒否する既存の制限を維持する。Payload の照合自体を、変数へのダウンキャストと同一視しない。

```surtr
is_match(NegativeIntError(-1), NegativeIntError)     # True: kind のみ
is_match(NegativeIntError(-1), NegativeIntError(-1)) # True: Payload も一致
is_match(NegativeIntError(-1), NegativeIntError(0))  # False: Payload が不一致
# is_match(NegativeIntError(-1), NegativeIntError(num)) # 拒否: 通常 bind
# is_match(NegativeIntError(-1), NegativeIntError @ e)  # 拒否: as alias
```

SafeBind による Error 定義 Pattern の直接ダウンキャストは禁止する。不一致時に別の Pattern Error が生成され、元 Error の意味が置換されるためである。他の式を新しいダウンキャスト対象に追加しない。通常 Bind、do の partial `<-`、`apply_pattern` についても、Error 定義 Pattern を拒否する現行の境界を維持する。通常のユーザー定義 Extractor と Error 全体の値運搬には従来の規則を適用する。

`if` の kind 比較からの型絞り込み、一般のダウンキャスト、`as` キャスト構文は追加しない。

### 4.2 スコープと型情報の消去

具象情報は、照合に成功した不変の変数束縛に付く。スコープを抜ける値は共通 Error として扱い、型絞り込みの対象外へ情報を持ち出す特例は設けない。脱出経路ごとのホワイトリストは作らない。

各 arm / 成功 branch の局所束縛では具象情報を保持し、その結果が branch を抜ける時点で Error に戻る。分岐間の型一致は、この結果型で検査する。関数へ渡す値や container / field に格納する値も共通 Error であり、取り出した値には具象情報を復元しない。

```surtr
def relay(error: Error) -> Error { error }

def normalize(error: Error) -> Error {
  match error {
    NegativeIntError @ e => e,
    NoValueError @ e => e,
    other => other,
  }
}
```

`normalize` の各 arm から受ける型と戻り値は Error になる。局所で `alias = e` とした場合の alias と、タプル等へ格納した後に取り出した Error は区別する。静的情報の消去は runtime の kind・Payload の変更を伴わない。

クロージャはレキシカルな束縛を捕捉する。成功スコープで作ったクロージャ本体では、捕捉した具象情報を維持する。同名の再束縛は新しい束縛であり、旧束縛を捕捉したクロージャへ影響しない。クロージャが Error 自体を返す場合の戻り値は共通 Error、Payload の値を返す場合は通常の Payload フィールド型とする。

## 5. OR Pattern の束縛契約

アズパターンは OR 全体の外側に適用する。既存の `A | B @ e` は `(A | B) @ e` の構造として扱う。

OR の許可対象と左から順に照合する評価規則は維持する。今回変更する束縛整合性は、Error Pattern だけの特例にせず、既存の OR Pattern 検査に統一する。

各候補から収集する束縛は、変数名と canonical な解決済み型の組の Set として比較する。名前、型、個数が一致すれば受理し、構造の走査順・変数への代入順・収集順の一致は要求しない。同一候補内の同名重複束縛は拒否し、Set 化によって重複を隠さない。

成功した候補の値を名前により共有 branch の束縛へ対応付ける。最初の成功で停止し、部分的な束縛を公開せず、guard / branch は一度だけ評価する。外側 alias は候補内の Set 比較から除く。

```surtr
deferror FirstPair(x: Int, y: Int) {
  |a: Int, b: Int|
  Self(message: "first", x: a, y: b)
}
deferror SecondPair(x: Int, y: Int) {
  |a: Int, b: Int|
  Self(message: "second", x: a, y: b)
}

def sum_pair(error: Error) -> Int {
  match error {
    FirstPair(x, y) | SecondPair(y, x) => x + y,
    _ => 0,
  }
}
```

異なる具象 Error を束ねた OR 全体の alias は共通 Error とする。同名・同型の Payload があっても、その alias から具象 Payload を参照しない。型を絞れる根拠は単一 identity への照合成功に限る。

## 6. readonly FacetPath

共通 `kind`・`message` の読み取りと readonly 性を維持する。具象束縛に対してもこの共通情報は読めるようにする。これは共通観測であり、一般の型変換・Payload 公開へ拡張しない。

具象 Payload の field から readonly FacetPath を導出する。パスの宣言・束縛と対象値への消費を区別し、パスの作成時には Error 値の照合を要求しない。

```surtr
def read_num(error: Error) -> Int {
  path = NegativeIntError.num
  match error {
    NegativeIntError @ e => Facet::view(path, e),
    _ => 0,
  }
}
```

Facet API が具象パスを消費する時点で、対象が対応する具象情報を持つ変数束縛であることを検査する。捕捉した束縛も同じ規則に従う。`e.num` は同じ読み取りの糖衣構文である。Facet API 自身のこの検査に渡しただけで、対象の局所情報を先に消去してしまわない。

共通 Error の変数、field / tuple / container から取り出した共通 Error、外部コンストラクタの生成値に具象パスを適用することは拒否する。

パスは同じレキシカルスコープと内側のクロージャで消費する。一般関数へパス自体を渡すこと、戻り値や container として運搬することは許可しない。

具象 Error を source とするパスキャプチャは、`&NegativeIntError.num`、`&Facet::view(NegativeIntError.num, &1)` 等の形を含めて禁止する。既に具象情報を持つ変数をクロージャリテラルで捕捉する `{|| e.num}` は許可する。

Payload を通る `put`・`set`・`over`・`bulk_update` 等の更新は拒否する。深い path や compose 後も readonly 属性を失わず、宣言元や標準コードにも更新権限を設けない。取り出した Payload 値自体は通常値であり、その値に対する Facet 操作には従来の規則を適用する。

## 7. Error 生成経路とソース位置

### 7.1 `.srt` 定義への統一

統一の対象はランタイムで発生する言語レベルの Error である。利用者・標準関数・builtin・VM と、コンパイラが出力したコードの実行時に生成する Error は、すべて `.srt` にある `deferror` 定義を通す。定義から解決したコンストラクタを使用し、引数と Payload のスキーマを検査する。Rust 側に別の message テンプレートや Payload の組立てを重複させない。

最終統一後は、生成側が発生条件に対応する意味のある入力値を渡し、message のテンプレートと Payload を決める責務は定義側に置く。`message`・`detail` 等の文字列引数を完全削除することは要求しない。OS、外部 parser、利用者の説明等に由来する文字列は入力として受け取り、必要なら Payload に保存できる。発生条件を示す固定文、文脈情報との組合せ、表示形式は `.srt` 定義側で決める。呼出し側が完成 message を組み立て、定義がそのまま転送することを標準の生成方式にしない。

コンストラクタの引数・型・Payload を変更した場合、生成側の不整合をコンパイル時の静的契約検査または runtime 契約の検証時に検出できることを必須とする。静的な署名・スキーマの照合は、コンパイル中に Surtr ランタイムを実行することを意味しない。Forge は実行時に定義を呼ぶコードを生成し、コンパイルフェーズではコンストラクタを評価しない。未解決の定義や引数不整合を、空 Payload や手組み Error へフォールバックしない。

コンパイルフェーズの ParseError / ResolveError / TypeError / CodegenError 等は本移行の対象外であり、Surtr ランタイムや `.srt` の Error コンストラクタを参照・実行する必要はない。VM の内部不整合を表す Rust の RuntimeError も言語レベルの `Error` と区別する。これらまで `.srt` の recoverable Error に変換する変更は行わない。

### 7.2 生成位置・伝播・スタック

Error の主キャプションは発生元のソース位置を保持する。`deferror` の宣言位置や本体の `Self(...)` の位置を、生成位置として参照しない。

- 明示的な Error 生成は、外部コンストラクタの呼出し位置を使う。
- コンパイラが出力したコードの実行時に生成する Error は、不一致が発生した Pattern 等の既存の生成元 span をコンストラクタへ渡す。
- builtin / runtime による生成も、対応する発生元 context を保ち、標準定義の実行位置で上書きしない。
- 移送・再格納・伝播で location を更新しない。wrap は新しい生成位置を持ち、元 Error を cause に残す。

生成元の source ID と span をフェーズ間で保持し、REPL の後続入力や module 境界でも別ソースの位置に置き換えない。定義を呼び出すための内部 frame と生成元 context を区別し、既存の呼出し経路・cause・diagnostic・stack trace の情報を保持する。スタック操作は新しい生成契約に統一し、旧生成経路を残さない。

runtime は kind と Payload の長さ・型・順序の契約を信頼して実行する。内部不整合は RuntimeError とし、値の欠損や別の Error として曖昧に救済しない。

## 8. フェーズの責務と診断

| フェーズ | 責務 |
|---|---|
| Spire | 空ヘッダ、本体先頭の引数、内部構築のトップレベル末尾制限、キーワード・Pattern の構造を解析 |
| Sigil | 宣言 identity、`Self` と当該宣言名の対応、レキシカル束縛、標準 Error 定義参照を解決 |
| Scar | 入力署名・Payload スキーマ・フィールド正規化・局所型・境界消去・OR Set 一致・Facet 消費権限を検査 |
| Forge | 検査済みコンストラクタ呼出し、Payload 保存、生成元 context と readonly field 読取りを命令へ変換 |
| Sindr | スキーマと生成元を受け渡す公開表現、Payload を持つ共通 runtime Error を定義 |
| Eldr | 型検査済み定義を実行し、Error 生成・保存・伝播・内部契約検証を行う |
| Rune / Xldr | script / module / REPL の生成位置・表示・定義照会を追従 |

不足・重複・未知の Payload field、禁止名、型不一致、構築位置違反、引数不整合、許可外の最終型、局所型の不一致、許可外のダウンキャスト、具象パスキャプチャ、readonly 更新を compile error とする。可能な場合は宣言位置と指定位置を併記する。生成した内部ノードの位置だけを診断に使わず、利用者の発生位置を保持する。

## 9. 標準 Error の段階移行

### 9.1 移行原則

定義と生成・観測側を同じ契約変更の単位で移行する。段階化は作業と検証の分割であり、旧仕様の常設サポート、旧構文との二重解釈、schema / VM version の引上げを意味しない。

新しい構文へ切り替える段階では、読み込む標準ソースと既存テストの宣言を一括して新形式へそろえる。旧ヘッダをコンストラクタ引数として読む fallback を作って、未移行ファイルを動かさない。その後、標準定義ごとに保存 Payload と Error の細分化を完成させる。まだ個別の Payload 設計を適用していない定義も新構文・同じ内部表現を使う。

保存 Payload と入力引数は別であるため、移行途中に空 Payload と入力引数を持つ新形式を使うことはできる。ただしそれを移行済みの完成契約とは扱わず、段階0で定める対象別の最終スキーマと照合して残件を管理する。

既存 Error は発生条件に応じて細分化する。なるべく使い回しを減らし、定義・発生条件・入力・Payload・message テンプレートの凝集度を高める。同じ名前や似た文言だけを理由に、別 API や別の失敗条件を共通 Error へ寄せない。完成済みの定義では、入力情報から定義側のテンプレートで message を生成する。文字列引数の存続は許可し、文字列を受け取ること自体を移行残件とは扱わない。

段階1では現行定義への実行時の生成入口の集約を先に完了させる。`IndexOutOfBounds(detail) { detail }` 等の現行入力契約を使う場合、生成側が所有する完成 message のテンプレートを移行残件として記録し、段階3〜6の該当定義群で定義側へ移す。この一時状態は新たな旧構文サポートや直接 Error 生成の fallback ではなく、定義と呼出し側の責務移行が未完了であることを示す。段階7で生成側に重複したテンプレートの残存を許可しない。

現在 `.srt` 定義を持たない実行時 kind も段階1の対象に含める。言語 Error として残すものは定義を追加してから生成入口を接続し、内部不整合は Rust の RuntimeError に分離する。未定義の kind だけ直接生成を残す例外は設けない。定義追加時に使う一時入力契約も棚卸しで追跡し、対応する段階で最終の用途別定義へ移す。

### 9.2 手順と完了条件

個別の移行先は `doc/error_definition_migration_inventory.md` に定める。標準51定義、Rust・VMの実行時生成72用途行、Forgeの不一致生成を照合し、現行入力、最終入力、保存 Payload、用途別の分割、定義側のテンプレートを対応付けた。検索で生成利用が見つからない定義は、削除せず利用確認の対象に残す。

| 段階 | 作業単位 | 完了条件 |
|---|---|---|
| 0: 定義・生成箇所の棚卸し | 標準 `deferror`、コンパイラ出力コードの実行時生成、builtin、process、fixture・test の定義と呼出しを分類 | 各発生条件について、最終定義、入力署名、保存 Payload、定義側の message テンプレート、生成元 span、関連テストを対応付ける。細分化や引数変更を実装前に確定 |
| 1: 生成入口の集約 | コンパイラ出力コード・builtin・VM の実行時 Error を `.srt` 定義へ接続 | 定義の実行を通り、Rust の独立した Error 組立てを削除。署名変更の検知と生成元 span 保持を固定。message テンプレートの移行残件を記録し、Payload 拡張前でも定義参照の検査が成立 |
| 2: 構文・表現の切替 | Parser、署名、内部構築、共通保存表現と全標準宣言の構文追従 | 旧ヘッダ解釈・旧内部表現を削除。全標準ソースが新形式で読み込め、空 Payload を含む単一表現になる。各生成入口も同じ契約へ追従 |
| 3: bootstrap・共通制御 | `lib/bootstrap.srt`、List / Option / HashMap の欠落、Uncons、Pattern mismatch・Facet・失敗伝播に関係する定義と生成側 | 基本 Error の最終 Payload・入力・message・位置を完成。各既存 Pattern / SafeBind / MonadFail の意味を保持 |
| 4: 基本データ・変換 | `lib/types/{int,string,duration,generator,regex,json}.srt` と対応 builtin | 数値・範囲・変換・parse / decode の発生条件ごとに移行。message テンプレートを定義側に置き、必要な入力を保存 |
| 5: I/O・外部境界 | `lib/{IO,file,FileSystem,Shell,Random}.srt` と対応 runtime | path、入力、外部失敗等の最終スキーマを適用。外部入力の失敗と内部不整合を区別し、発生元を保持 |
| 6: process・test・残件 | process の生成経路、`lib/test.srt`、標準外の fixture・example を含む残件 | 全対象を棚卸し表へ照合。旧生成 helper、生成側の独立テンプレート、未移行定義、古い期待値を除去 |
| 7: 全体統一 | 局所具象型、Pattern、Facet、OR、表示・REPL・文書を含む最終統合 | 本書の受入条件、全件検証、別エージェントレビューが完了。途中形式や未移行の契約が残らない |

段階3〜6は対象群ごとに、定義・呼出し・必要な回帰テスト・正本の説明を一つの変更単位として完了させる。棚卸しで共通生成箇所が判明した場合は、依存する定義群より先にその生成箇所を移す。同じ target へのビルド・テストや最終検証中の編集を並行させない。

## 10. 正本の追従

実装時は、適用する変更段階に合わせて既存正本を先に整合させる。本書の未実装の内容を、仕様作成時点で現行挙動として書き込まない。

- `lib/**/*.srt` の対象 Error と `@doc`、`lib/types/error.srt`、`lib/types/result.srt`、`lib/facet.srt`、Pattern consumer の説明。
- `docs/dev/Pattern_spec.md` の Error Pattern、束縛 Set、consumer とスコープ。
- `docs/dev/EldrVM_spec.md` の Payload 保存・生成契約・ソース位置と `docs/dev/display_error.md` の表示契約。
- `docs/dev/Do_intrinsic_spec.md`、`docs/dev/Lazy_spec.md`、`docs/dev/diagnostics.md` の関連契約。
- `docs/dev/Xldr_spec.md`、`docs/dev/Surtr_LSP_spec.md` の定義・署名・field の照会。
- `docs/site/error-handling.md`、`docs/site/pattern-matching.md`、`docs/site/facet.md`、`docs/site/capture-operator.md` と対象標準 API の利用説明。

利用者向けの例は適用段階の処理系で実行できる形にし、旧挙動の成功例は必要に応じて現行条件に沿う拒否テストへ変更する。

## 11. 受入条件と検証

実装は契約を直接検査する最小範囲のテストから開始する。段階ごとの定義移行では、変更した Error の生成、保存、読み取り、失敗伝播と位置を検査する。文言比較だけで Payload の正しさを代用しない。

必須の成功・拒否境界は次のとおりである。

1. 空ヘッダの両形式、0 引数省略、Payload と入力が異なる宣言、空 Payload の String 正規化、宣言名による末尾構築が成功する。
2. 非末尾・入れ子・束縛 RHS の内部構築、非空 Payload の String、不足・未知・重複・禁止名・型不一致を拒否する。
3. キーワードなしは位置指定、キーワードありは裸変数の名前指定になる。同型の引数でも値の割当てを一意に検査でき、literal 混在で位置を推測しない。
4. kind 照合、Payload 宣言順の分解、名前付き正規化、局所具象注釈、共通情報と readonly field の読み取りが成功する。`is_match` は kind のみの判定と Payload の子 Pattern 判定を行え、一致・不一致を Boolean で返す。通常 bind・as alias を拒否し、具象束縛や局所型情報を公開しない。
5. 共通 Error の直接具象注釈、許可外のダウンキャスト、具象パスキャプチャ、readonly の直接・合成・深い更新を拒否する。
6. branch 結果・格納・通常呼出しでは共通 Error となり、Payload は保持する。別名と捕捉は局所情報を維持し、同名再束縛の影響を受けない。
7. OR の走査順が異なっても名前・型・個数の Set が同じなら成功し、名前で正しい値を共有 branch に渡す。型・名前・個数不一致と候補内重複は拒否する。
8. コンパイラ出力コード・builtin・VM の実行時 Error 生成は `.srt` 定義を実行し、入力署名変更を検知する。内部生成の Payload を通常の照合で観測できる。コンパイル中の診断で Surtr ランタイムを参照・実行しない。
9. 明示呼出し、Pattern 不一致、builtin、module、REPL、constructor capture、cause のいずれも生成元 span を保持する。`deferror` の位置を主キャプションにしない。
10. 同名の別 module の Error は宣言 identity とスキーマを混同しない。Payload 欠損等の内部不整合は RuntimeError になる。
11. Result 専用の SafeBind 一段射影、MonadFail の失敗先、ResultT の失敗層、kind のみの Result Eq、ErrorKind の限定用途を維持する。
12. 全標準定義と実行経路が新仕様へ統一され、旧経路・fallback・生成側の独立 message テンプレート・未移行の棚卸し項目が残らない。必要な文字列引数は維持できる。schema / VM version は上げない。

| 対象 | 実装時の検証 |
|---|---|
| 宣言・型・Pattern | `rtk cargo nextest run -p spire`、`rtk cargo nextest run -p sigil`、`rtk cargo nextest run -p scar` から変更契約に関係するテストを選択 |
| 命令と Error 保存 | `rtk cargo nextest run -p forge`、`rtk cargo nextest run -p sindr`、`rtk cargo nextest run -p eldr` の対象テスト |
| script / module | `rtk cargo nextest run -p rune --test integration run_srt`、`rtk cargo nextest run -p rune --test integration module_import_fixtures` |
| CLI / REPL / 外部境界 | cold integration と対象 Xldr テスト。選択名は既存 runner を確認する |
| 標準定義の移行単位 | `cargo run -- test --quiet <lib-relative-name>` で対象の生成・Payload・伝播を確認 |
| 最終全体検証 | `rtk cargo nextest run --profile ci --workspace` と `cargo run -- test --quiet --all` |

全件成功後、仕様・最終差分・検証結果を別エージェントへ渡し、意味論、旧生成経路、スコープ、Payload 保持、位置、受入条件の不足をレビューする。修正後は影響範囲を再検証し、最終変更が複数フェーズに及ぶ場合は全体検証を完了させる。

本書作成時点では製品コード・実行可能テストを変更せず、コンパイラのテストも実行していない。個々の Error の最終スキーマと細分化は棚卸し文書の修正方針を段階0の実装入力とする。実装・実行結果として扱わず、後続段階の完了条件から省略しない。
