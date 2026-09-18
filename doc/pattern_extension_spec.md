# Pattern 拡張統合仕様 — Extractor / ExtractorClosure / apply_pattern

## 1. 状態・目的・範囲

- 状態: 未実装の統合仕様。旧 Extractor 返却更改案、Matcher / projection 案、事前引数差分案を本書へ統合した。本書だけで変更後の契約を読めるものとし、削除する原案や添付には依存しない。
- level: 4。構文、型規則、評価規則、SafeBind の失敗返却先、フェーズ間契約を変更する。
- 現行動作の正本: [要件定義v9.md](要件定義v9.md)、[do intrinsic](../docs/dev/Do_intrinsic_spec.md)、[診断](../docs/dev/diagnostics.md)。本書と異なる現行契約は、実装に先立つ仕様反映で本書へ整合させる。文書作成だけで実装済みと扱わない。
- 今回の作業は仕様文書の統合と参照整理まで。製品コード、実行可能なテスト、現行動作の説明は変更しない。

1つの修正タスクとして次を実施する。

1. named Extractor と first-class な ExtractorClosure の共通契約を定める。
2. 両者に事前引数を追加し、最後の入力を照合対象値とする。
3. Extractor の返却型を `Option` から二状態の `MatchResult` へ置換する。
4. 旧設計名 `Matcher` を `ExtractorClosure`、`apply_matcher` を `apply_pattern` に統一する。
5. ExtractorClosure の literal とソース上の型注釈構文を確定する。
6. MatchResult を返す Extractor / ExtractorClosure 本文内で SafeBind を許可する。
7. projection、型注釈、UnitOnly の子 Pattern 省略、consumer / OR / pipe の制約を同じ契約へ統合する。
8. 通常の Result-returning callable から ExtractorClosure を生成する明示的な標準 API `Extractor::from_result` を追加する。

Pattern AST は第一級の値にしない。一般関数の partial application、通常 Closure との暗黙変換、独自の capture / lifetime 規則、MatchResult の Monad 化、専用 Opcode の追加は本タスクの機能要件に含めない。一般の Option / Result API は維持する。

本書のコードは変更後の仕様例であり、現行 REPL で動く例ではない。

## 2. 現状と変更後

現行 Extractor は一入力で `Option<Payload>` を返す。`Some(payload)` は子 Pattern の照合へ進み、`None` は不一致になる。SafeBind の `None` failure は共通 PatternMismatch Error となる。

変更後は、named Extractor と ExtractorClosure の両方を次の契約へ統一する。

```text
(pre_arg1, ..., pre_argN, value) -> MatchResult<Payload, Error>
```

- 入力数は1以上。最後の1引数が照合対象値、それ以前が0個以上の事前引数である。
- named Extractor の実装一意性を維持する。事前引数数による overload は追加しない。
- 成功と失敗は `MatchResult::OK(payload)` / `MatchResult::Err(error)` の二状態である。
- Extractor / ExtractorClosure は常に partial Pattern として扱う。本体が常に OK を返すことを解析して total とみなさない。
- 通常 Bind `=` は total Pattern だけを許可し、Extractor / ExtractorClosure Pattern を拒否する。束縛数0と totality は独立である。

`defextractor` / `@builtin defextractor` の module / impl 配下という宣言位置、qualified head、型 head に付随する `deconstruct` の解決規則は維持する。複数引数の定義でも最後の入力を照合対象にする。たとえば `self` を照合対象とする attached Extractor に事前引数を追加する場合、`self` は最後に置く。

## 3. MatchResult

### 3.1 型・variant・Error

正本定義の配置は `lib/types/special_types.srt` とする。

```surtr
@builtin
defenum MatchResult<$Value> {
    OK($Value),
    Err(Error),
}
```

ソース signature と表示の正規形は `MatchResult<P, Error>` とする。`MatchResult<P>` も短縮入力として受理する。これは通常 nominal 型に対する一般的な省略規則ではなく、MatchResult 固有の compiler-special 規則である。

- 型引数数は1または2だけを受理する。
- 第二引数がある場合は canonical な abstract `Error` だけを受理する。`MatchResult<P, Int>` や具象 error 型を第二引数に指定する形は拒否する。
- 両表記を同一型へ正規化する。内部では成功 payload 型だけを型引数として保持してよい。
- `NoMatch` variant / tag は設けない。不成功はすべて Err であり、Error を保持するか破棄するかは consumer が決める。
- `Result` / `Option` と暗黙変換しない。variant 名や tag の類似を根拠に互換扱いしない。
- constructor は常に qualified な `MatchResult::OK` / `MatchResult::Err` とする。通常 Result の bare `Ok` / `Err` sugar は拡張しない。

手書きの `MatchResult::Err(...)` の引数は具象 `deferror` 値に限定する。抽象 Error の直接構築、観測済み abstract Error の手書き constructor への再投入、裸の Error 値の一般保持、String 等の任意値による代用は許可しない。SafeBind の compiler-owned な伝播では取得済みの Error をそのまま保持する。consumer が既存 Error を保持する内部経路と、利用者の明示 constructor の入力制約を区別する。

Error の kind / message / location / cause は既存 Error / RichError 契約に従う。定義側が返した Error を compiler が共通 PatternMismatch で上書きしたり、Extractor 名や型名から message を再構成したりしない。builtin Extractor も同じ契約を持つ。空の list / string に対する `uncons` の Error は標準定義 / builtin の契約が選び、Forge の名前判定に置かない。

### 3.2 利用位置

MatchResult は一般のユーザ値ではない。許可する型位置は次に限る。

- `defextractor` / `@builtin defextractor` の戻り型。
- `ExtractorClosure<Signature>` 内の Signature の戻り型。
- これらの本文で照合結果を返す経路を型検査するための型位置。これを通常 local binding / field / container への保持許可と解釈しない。

`MatchResult::OK(...)` / `MatchResult::Err(...)` は Extractor / ExtractorClosure 自身の本文でのみ構築できる。`if` / `match` の各返却経路は同じ MatchResult expected type へ一致させる。

通常の変数、引数、field、collection 要素、通常関数 / 通常 Closure の戻り値として MatchResult を保持・受け渡しできない。MatchResult 自体を通常 Pattern で分解すること、constructor を capture すること、Trait / operator / From / TryFrom の対象にすることも禁止する。

Extractor の本文内にあるというだけで、内側の通常 Closure へ MatchResult 利用権限を継承しない。各 callable が通常関数 / 通常 Closure / Extractor / ExtractorClosure のどれかを明示的に管理する。一般値の型候補・completion には MatchResult を提示せず、許可された signature の表示は維持する。

## 4. ExtractorClosure

### 4.1 literal・型構文

literal は `*{|params...| body}` とし、入力は1個以上必要である。各引数の型注釈は任意で、通常推論が成立すれば省略できる。

```surtr
limit = 10
greater_than = *{|value: Int|
    True =? value > limit
    MatchResult::OK(value)
}
```

ソース上の型注釈構文は次とする。

```surtr
ExtractorClosure<(Int -> MatchResult<Int, Error>)>
ExtractorClosure<(Int, Int, Int -> MatchResult<Int, Error>)>
```

`ExtractorClosure` は1個の関数 signature 型引数を取る compiler-special 型である。signature 内部は既存の `(A, B -> R)` 関数型構文を利用し、入力数1以上、返却型 MatchResult という専用制約を検査する。一般の関数型への alias ではない。

多引数と単一 tuple 入力を区別する。

```surtr
# 入力2個: 事前引数1個、照合対象値1個
ExtractorClosure<(Int, Int -> MatchResult<Int, Error>)>

# 入力1個: tuple 全体が照合対象値
ExtractorClosure<((Int, Int) -> MatchResult<Int, Error>)>
```

`Matcher(...)`、`ExtractorClosure(...)`、`*{expr}`、`*{|| expr}` は採用しない。入力なしの ExtractorClosure、非 signature 型引数、Option / Result を返す signature は静的拒否とする。

### 4.2 推論・capture・受け渡し

ExtractorClosure は通常 Closure と同じ lexical capture 規則に従う第一級の値である。通常の変数、関数引数、戻り値として受け渡し、同じ signature の値を通常の `if` / `match` で選択できる。capture 内容の違いを専用型の不一致理由にしない。通常 Closure に適用される既存の escape / Facet 制約は維持する。

型注釈、expected type、consumer input、projection を利用する側の型要求を通常推論へ接続する。定義位置で全型を確定しなければならないという専用制約は設けない。一方、Pattern application の引数領域は確定した callable signature の入力数と payload shape から静的に決める。generic の通常の型統一と、入力数 / 子 Pattern 数の推測は区別する。

通常推論後も境界や必要な型が未確定なら compile error とする。定義の実装候補数や consumer 名から推測しない。

### 4.3 適用位置・名前解決

named Extractor と ExtractorClosure の本体実行は Pattern 位置に限る。ExtractorClosure 値の生成・選択・受け渡しは通常 Expr で行えるが、`closure(value)` で MatchResult を取得する通常 call は許可しない。

```surtr
greater_than(number) =? value
apply_pattern(value, greater_than(_1: Int))
```

literal や生成式を Pattern head へ直接埋め込まず、一度変数へ bind するか helper 引数名を利用する。

```surtr
selected = get_extractor_closure()
apply_pattern(value, selected(_1))

# 以下は拒否
apply_pattern(value, get_extractor_closure()(_1))
apply_pattern(value, (get_extractor_closure())(_1))
apply_pattern(value, *{|v: Int| MatchResult::OK(v)}(_1))
```

`apply_pattern(value, get_extractor_closure())` の第2引数を通常 Expr とみなし、payload 全体を暗黙に返す別モードも追加しない。

Pattern の `name(...)` は既存 lexical scope の名前解決に従う。local ExtractorClosure は同名 Extractor を shadow できる。選択された local が ExtractorClosure でなくても named Extractor を探し直さない。型名 / constructor / qualified Extractor の既存 head 解決を維持する。

Sigil は symbol / lexical binding の identity を解決し、Scar は local の ExtractorClosure 型を検査する。Sigil が Scar の内部型推論に依存する名前解決を追加しない。named Extractor の通常 capture / 値化は禁止のままとする。

### 4.4 明示的な生成 API: Extractor::from_result

`Extractor::from_result` を採用する。単一入力の通常 callable `($A -> Result<$B>)` を受け取り、`ExtractorClosure<($A -> MatchResult<$B, Error>)>` を返す通常の標準関数とする。新しい builtin、special form、暗黙変換は追加しない。

```surtr
defmod Extractor {
    def from_result(f: ($A -> Result<$B>)) -> ExtractorClosure<($A -> MatchResult<$B, Error>)> {
        *{|value: $A|
            converted =? f(value)
            MatchResult::OK(converted)
        }
    }
}

decimal = Extractor::from_result(&String::try_to_int)
apply_pattern("123", decimal(_1: Int))
# Ok(123)
```

- 生成時には callable 値を受け取って capture し、`f` の本体は実行しない。生成した ExtractorClosure の occurrence に到達した時点で、照合対象値を渡して `f` を一回実行する。
- Result の外側一段だけを SafeBind で射影し、Ok payload をそのまま MatchResult::OK へ渡す。Err は kind / message / location / cause を保持して MatchResult::Err へ伝播する。payload 自体が Result でも再帰的に unwrap しない。
- `$A` / `$B` は入力 callable と通常推論から定まる。成功 payload を Unit や固定した共通型へ変更せず、単値 / tuple / Unit の子 Pattern 規則をそのまま適用する。
- 生成結果は事前引数なしの ExtractorClosure であり、通常の変数または helper 引数へ束縛して Pattern head に使う。生成式を Pattern head に直接埋め込むことや、生成結果の通常 call は許可しない。
- 複数入力の通常関数を使う場合、必要な値を capture した単一入力の通常 Closure を利用者が明示的に渡す。from_result に可変 arity や暗黙の partial application を追加しない。
- Option や raw payload を返す callable は受理しない。`Extractor::from_function` は採用対象に含めない。

本 API の実装と @doc は標準 SRT に置く。前提となる ExtractorClosure / MatchResult 本文の SafeBind 契約を利用し、MatchResult 自体を通常関数の戻り値として公開しない。

## 5. 事前引数・payload shape・UnitOnly

### 5.1 引数領域

```text
head(pre_args..., payload_patterns...)
```

照合対象値を head の引数列へ記述せず、consumer が与える値を本体の最後の入力へ渡す。

```text
pre_arity = definition_input_count - 1
caller.args[0 .. pre_arity] = Expr
caller.args[pre_arity ..] = payload Pattern
```

事前引数の型は対応する input slot、照合対象値の型は最後の input slot に照合する。payload の子 Pattern は成功 payload の shape と各型に照合する。

構文上の区切りや別の PreArgs 型 / metadata は追加しない。Spire は signature を知らない段階で Expr / Pattern 境界を決め打ちしない。解決後に signature から分類して各領域を検査できる表現と source span を保持する。通常 Expr に見える表記を理由に Pattern と推測したり、arity 不一致を暗黙補正したりしない。

### 5.2 payload の子 Pattern 数

成功 payload 型と shape は適用先 Extractor の戻り型に依存し、各適用箇所で signature の型が確定する際に決まる。入力型に応じて出力型が定まる Function::curry / Applicative::ap と同様に、型に依存する契約として扱う。PayloadShape を利用者が渡す引数や共通の値型にせず、異なる Extractor の payload を一つの Union へ閉じ込める規則も追加しない。内部の typed contract が確定した shape を保持することとは区別する。

| 成功 payload shape | 必要な子 Pattern 数 |
|---|---|
| 非 tuple・非 Unit の一値 | 1 |
| tuple `(A, B, ...)` | tuple 要素数 |
| UnitOnly: payload 自体が canonical Unit | 0、または1 |

総引数数は `pre_arity + 選択した子 Pattern 数` と一致させる。arity 検査後にだけ各 input / 子 Pattern を対応付け、先行する zip で不足・余剰を消さない。

**payload が UnitOnly なら、子 Pattern 位置を省略できる。** これを UnitOnly に関する優先規則とする。省略は任意であり、明示する場合は通常の変数 bind、wildcard、projection、およびそれらの適切な型注釈を許可する。Unit は束縛可能な値なので、Unit 位置の projection も通常の slot として数える。

```surtr
# check は事前引数なし、成功 payload が Unit
apply_pattern(value, check())
apply_pattern(value, check(unit: Unit))
apply_pattern(value, check(_))
apply_pattern(value, check(_1))
apply_pattern(value, check(_1: Unit))
# すべて成功時は Ok(())、型は Result<Unit>
```

事前引数がある UnitOnly head では `check(pre_args...)` が子 Pattern 省略形、`check(pre_args..., _1: Unit)` が明示形になる。省略された位置に projection を自動生成しない。

tuple の Unit 要素も bind / projection できるが、tuple 自体を UnitOnly とみなして要素を省略しない。Unit 値の保持・射影と、Unit 値の検査 Pattern は別の規則である。`check(())`、`() = ()`、`() =? Ok(())` は引き続き ParseError とする。

### 5.3 単一評価・短絡

- consumer input / RHS は一回だけ評価する。
- 各 occurrence へ到達した時点で事前引数を通常関数引数の評価順に各一回だけ評価し、consumer の値を最後の入力として本体を一回だけ実行する。
- lexical capture は通常 Closure と同じ生成時の規則に従う。事前引数は occurrence 到達時の評価であり、capture と同一視しない。
- OK 後は同じ payload で子 Pattern を検査する。成功検査、束縛、projection のために本体を再実行しない。
- 不一致 / Err 後は後続の子 Pattern を評価しない。到達しなかった occurrence の事前引数も評価しない。
- `match` が次の arm へ進む場合、その arm の occurrence は独立した照合として扱う。

## 6. apply_pattern・projection

### 6.1 consumer の外部契約

```surtr
@builtin
def apply_pattern(value: $Value, pattern: $Pattern) -> Result<$Return>
```

これは compiler-known consumer の外部 signature 表示である。`$Pattern` を一般値型として追加せず、第2引数を Pattern として解析・解決・型検査する。正規 identity は `Kernel::apply_pattern` とする。

- 第1引数の値全体を照合する。Result input も自動 unwrap しない。必要なら `Ok(...)` / `Err(...)` を Pattern に明記する。
- 全照合成功時だけ projection 結果を Result::Ok へ入れる。
- Extractor / ExtractorClosure の Err は元の Error を Result::Err へ保持する。通常 Pattern 不一致は既存規則の Error を返す。
- enclosing callable の Result 戻り値を要求せず、早期 return を発生させない。
- 静的 Pattern エラーは compile error とする。利用者の Err や runtime 不一致へ変換しない。

apply_pattern は通常の Expr ブロックへ Pattern / Extractor の実行を組み込み、結果を既存の Result / Monad 操作へ接続する境界である。適用後の変換・逐次合成・失敗処理はそれらの既存 API を使う。ExtractorClosure 側に対応する map / then / both 等の合成 API を重複して設けず、別 consumer の apply_extractor も追加しない。

異なる payload を扱う場合は、match の各 branch で Extractor の結果を使った式を実行し、branch の最終結果を同じ型へ揃えられる。各 Extractor の payload 型を揃える必要はない。異なる signature の ExtractorClosure 値を直接一つの値として選択する規則や、Extractor 同士の並列合成のための Union / 新しい型制約は本仕様に追加しない。

### 6.2 slot と出力型

projection は `_1`, `_2`, ... とし、lexical variable を導入しない。出力順は traversal 順ではなく番号順とする。

| slot 数 | 成功値 | 戻り値型 |
|---|---|---|
| 0 | `()` | `Result<Unit>` |
| 1 | `_1` の値 | `Result<T1>` |
| 2以上 | `(_1, ..., _N)` | `Result<(T1, ..., TN)>` |

```surtr
apply_pattern([1, 2, 3], [_1: Int, .._2: List<Int>])
# Ok((1, [2, 3]))

apply_pattern([(1, 3)], [(_1: Int, 3) @ _3] @ _2)
# Ok((1, [(1, 3)], (1, 3)))
```

as-pattern の alias 位置にも projection と型注釈を許可する。root / nested のいずれもその位置の Pattern 全体の値を射影する。alias の注釈は `pattern @ _N: T` と書き、その alias が受ける値の型を検査する。

slot index は1始まり、最大16。`_0`、17以上、巨大 index、重複、欠番を静的拒否する。`_01` は `_1` と同じ整数 index に正規化し、正規化後に重複を検査する。`_`, `_name`, `_foo1` は WildPattern のままとする。

`_N: T` を、通常の projection、list tail、as alias に許可する。注釈型は対応位置の型と通常の型関係規則で検査し、cast / 暗黙変換 / runtime 型検査を追加しない。型不一致は compile error とする。ソースコードをドキュメントとして扱うため、推論結果と同じ冗長な注釈も受理する。注釈なしでも通常推論が成立すれば受理する。

projection の許可範囲は canonical apply_pattern の Pattern 領域だけである。事前引数 Expr 内に書いた `_N` を projection と解釈しない。他 consumer の Pattern にある数字だけの `_N` を旧 wildcard に fallback しない。tuple path の `._0` や capture の `&1` とは別の構文要素として扱う。

数字だけの `_N` は通常の変数名として宣言・bind・shadow できず、通常 Expr の値参照にも使えない。Spire は Expr 内の `_N` を文脈検査が必要な placeholder 候補として保持し、後段で許可位置を確定する。canonical apply_pattern の Pattern projection、または既存 pipe の最外 call の direct Expr argument slot 以外に候補が残れば compile error とする。通常 Pattern 文法全体に projection の許可を広げず、事前引数内の候補も wildcard / 通常変数へ fallback しない。

capture placeholder の番号解釈と範囲も1〜16へ揃える。現行 capture parser の上限なしという経路は置換し、数値変換の切り捨て / wraparound は許可しない。

### 6.3 bind・pin・scope

`=` / `=?` は全照合成功後に既存の binding 順で変数へ代入する。apply_pattern の通常 bind は照合内部で完結し、projection だけを外部結果へ公開する。失敗時は部分的な bind / projection を公開しない。

```surtr
result = apply_pattern([10, 20], [temporary, _1: Int])
# Ok(20)。temporary は外へ導入しない。
```

事前引数と pin が参照できるのは、同じ Pattern の外側で事前に束縛された値だけである。同じ Pattern 内で導入した名前を後続の事前引数や pin から参照しない。外側に同名値があればそれを参照し、存在しなければ ResolveError にする。

## 7. consumer の成功・失敗 policy

| consumer | OK 後に子 Pattern も成功 | Extractor の Err / 子 Pattern の不一致 |
|---|---|---|
| `match` arm | arm body | Error を破棄し、次の alternative / arm |
| `if_let` | then branch | Error を破棄し、else branch |
| `if_let_then` | branch | Error を破棄し、branch 未評価で Unit |
| `is_match` | True | Error を破棄し、False |
| SafeBind `=?` | bind して続行 | 現在の failure target の preserve / discard policy |
| `apply_pattern` | projection の Result::Ok | 元の Error を保持した Result::Err |
| do の partial `<-` | payload の bind と continuation | do-local failure target の preserve / discard policy |

非保持 consumer は Error の kind / message / location / cause を表示・伝播・記録しない。OK 後の literal / constructor / list shape 等の子 Pattern 不一致は既存の Pattern failure である。nested Extractor が Err を返した場合、保持 consumer は実際に失敗した nested Extractor の Error を使う。

Error を返す consumer policy と Extractor の返却 carrier 解釈は共通 Pattern engine の責務として接続する。apply_pattern や do が独自の Extractor tag / 名前判定を再実装しない。

## 8. MatchResult 本文内の SafeBind

### 8.1 failure target の追加

do 外の Extractor / ExtractorClosure 本文では、その本文の MatchResult 戻り値を compiler-owned な SafeBind failure target として許可する。

| failure の起点 | 本文からの早期 return |
|---|---|
| RHS の canonical Result::Err(error) | MatchResult::Err(error) |
| LHS 内の Extractor / ExtractorClosure の Err(error) | MatchResult::Err(error) |
| literal / constructor / list shape 等の通常不一致 | 既存 Pattern Error を持つ MatchResult::Err |

RHS が canonical Result の場合は外側一段だけ射影し、Ok payload を通常 Pattern checker へ渡す。non-Result RHS は値全体を渡し、通常型検査を通った partial Pattern だけを許可する。total non-Result の既存拒否分類、通常 Pattern エラーの優先順位は維持する。

`=?` 自体の結果型は Unit であり、成功時は全照合成功後に束縛して続行する。failure では後続の式を評価しない。RHS / Pattern の Error 値をそのまま保持し、message や cause を再構成しない。

```surtr
deferror OutOfRange(value: Int) { "outside range" }

defmod Bounds {
    defextractor between(min: Int, max: Int, value: Int) -> MatchResult<Int, Error> {
        if(
            min <= value && value <= max,
            MatchResult::OK(value),
            MatchResult::Err(OutOfRange(value))
        )
    }
}

# source は Result<Int> を返す通常 Closure
checked = *{|source: (-> Result<Int>), value: Int|
    parsed =? source()
    accepted =? apply_pattern(parsed, Bounds::between(0, value, _1: Int))
    MatchResult::OK(accepted)
}
```

上の例では source の Result::Err と apply_pattern の Result::Err を、checked 自身の MatchResult::Err へ返す。LHS に直接 Extractor を使う場合も同じ target へ返す。

```surtr
direct = *{|value: Int|
    Bounds::between(0, 10, accepted) =? value
    MatchResult::OK(accepted)
}
```

成功終端は引き続き明示的な `MatchResult::OK(payload)`、明示失敗は `MatchResult::Err(error)` が基本である。raw payload、Option、Result を暗黙包装しない。MatchResult を SafeBind RHS の自動 unwrap 対象にする変更でもない。

### 8.2 callable / do の境界

失敗返却先は最も近い callable または現在の do-local target とする。

- 内側の通常 Closure は外側 Extractor の MatchResult target を借りず、自身の合法な戻り値文脈で検査する。
- 内側の ExtractorClosure は独立した Extractor 本文であり、自身の MatchResult target を持つ。
- Extractor 内の do でも、failure はその do 自身の carrier へ接続する。外側 MatchResult への直接 return を追加しない。
- do は引き続き Monad を要求し、failure target は ResultEffect > Alternative > Monad の順に解決する。Result-preserving route は元 Error を保持し、Alternative route は明示的に破棄して resolved empty へ進む。
- `do::<MatchResult>`、MatchResult への Monad / Alternative / @result_effect の導入は対象外である。
- REPL top-level SafeBind は既存の Error 表示とセッション継続境界を使う。MatchResult を通常 REPL 値として公開しない。

## 9. 予約語・OR・pipe

`if_let`、`if_let_then`、`is_match`、`apply_pattern` を Pattern 引数を持つ予約 consumer surface とする。通常の宣言名、引数名、local bind、user member 名への利用・shadowing は Spire で拒否する。標準 @builtin 宣言と正規の Kernel::name call は明示的に許可し、canonical consumer identity へ確定する。

予約語 token は qualified member / capture 構文でも解析できるようにする。既存の canonical 標準 builtin の qualified 通常 call / capture は維持する。たとえば `Regex::is_match(re, input)` とその capture は通常の Regex builtin であり、第2引数は Expr のままである。Pattern consumer と判定するのは canonical な Kernel consumer identity だけとし、member の綴りが `is_match` であることでは判定しない。この許可を新規 user member 宣言や予約 consumer の shadowing に広げず、Regex API の改名や表示名による fallback は追加しない。

旧名 apply_matcher は consumer 名 / 予約語 / alias として残さない。旧 builtin entry や名前による特殊処理があれば削除する。同名の通常 user function があっても、その引数を Pattern と解釈する compatibility route は設けない。

OR Pattern `p1 | p2` は match arm 内だけに許可する。apply_pattern、if_let、if_let_then、is_match、Bind、SafeBind、do binding では nested OR も拒否する。match には既存の arm body 型と bind scope 規則を適用する。apply_pattern に OR の projection slot 統一規則は追加しない。

```surtr
value |> apply_pattern(Bounds::between(0, 10, _1: Int))
# apply_pattern(value, Bounds::between(0, 10, _1: Int))
```

pipe は既存の第1 Expr 引数注入を使う。最外 call の direct Expr argument だけを pipe slot として扱い、Pattern 引数やその子 Pattern の `_N` と混同しない。事前引数 Expr 内への pipe slot 探索も追加しない。

Pattern 位置への explicit / implicit pipe 注入は禁止する。Pattern 引数が欠けた call を partial call として補完しない。

```surtr
get_extractor_closure() |> apply_pattern(value, _1)
# 拒否: Pattern 位置へ pipe 注入できない

def run_pattern(ext: ExtractorClosure<(Int -> MatchResult<Int, Error>)>, value: Int) -> Result<Int> {
    apply_pattern(value, ext(_1: Int))
}
get_extractor_closure() |> run_pattern(value)
# 通常 Expr 引数への pipe は許可
```

is_match の通常 bind 禁止、通常 Bind の totality、consumer ごとの continuation は、明示した変更以外維持する。

## 10. フェーズ間契約・移行・実装順序

### 10.1 責務

| 対象 | 主な責務 |
|---|---|
| Spire | Extractor 複数入力、ExtractorClosure literal / 型注釈、projection 注釈・alias、予約語、OR / pipe 文脈、source span |
| Sigil | signature と head identity、lexical shadowing、引数領域の保持、capture / scope、source origin |
| Scar | canonical MatchResult / ExtractorClosure 型、通常推論、入力数・引数境界・payload shape / UnitOnly、projection 型と index、consumer policy、SafeBind target |
| Forge | 共通 Pattern lowering、事前引数と本体の単一評価、短絡、番号順 projection、Error の保持 / 破棄と早期 return |
| Sindr / Eldr / loader | canonical 型・variant・builtin / callable metadata、既存 Closure capture 表現、builtin Extractor の MatchResult 表現、metadata / chunk rebase |
| Rune / Xldr | script / module / REPL の診断・継続、signature / completion、capture 付き値の受け渡しと表示 |
| 標準定義 | special_types の型正本、kernel の consumer 宣言、user-defined / builtin Extractor、@doc |

typed contract は callable identity / 確定 signature、型検査済み事前引数、照合対象型、成功 payload shape、canonical OK / Err tags、projection slot / 注釈型、consumer failure policy、effective failure target、元の source origins を明示する。Forge で表示名や raw signature を再解析して復元しない。pending な型 / dispatch / 引数境界は実行境界までに拒否する。

builtin の正本は `crates/sindr/src/builtin.rs` の BUILTIN_METAS とし、Eldr の BUILTIN_IMPLS と対応させる。各フェーズへ builtin ID / 表示名を直書きしない。専用 Opcode を前提にせず、既存 Closure / call / branch / return の表現を利用する。

未知 tag / variant、不正 field 数、壊れた callable metadata、payload representation 不一致は内部契約違反として即時 failure にする。利用者の Err、PatternMismatch、次の match arm、Alternative::empty へ fallback しない。

### 10.2 実装順序

各段階は同じ修正タスクの依存順序であり、旧 Option 契約と新 MatchResult 契約を並存させる公開段階を設けない。

1. 正本の仕様を本書へ整合させ、canonical type / variant / consumer metadata と旧経路の削除対象を確定する。
2. named Extractor の複数入力、MatchResult 制約、ExtractorClosure の構文・名前解決・通常推論・capture を共通 typed contract へ接続する。
3. payload shape / UnitOnly、事前引数 Expr と子 Pattern、単一評価・短絡を共通 engine へ接続し、標準 / builtin Extractor を移行する。
4. apply_pattern、型注釈付き projection、scope / pin、予約語・OR・pipe の制約を接続する。
5. MatchResult 本文の SafeBind target、既存 Result-effect / Alternative / REPL の境界、診断 producer の source facts を整合させ、標準 SRT の Extractor::from_result と @doc を追加する。
6. 旧 Option Extractor 専用 lowering、三状態 MatchResult の NoMatch 経路、旧 consumer 名 / 互換経路、未使用 metadata を削除する。
7. 成功・拒否境界を検証し、最終差分の独立レビューと level 4 の全体検証を行う。

現行にない MatchResult の過去実装は必要なら Git 履歴を比較資料にできるが、現行基盤へ機械的に復元する仕様ではない。

### 10.3 正本への反映

実装着手時に、少なくとも次を同じ契約へ整合させる。

- `doc/要件定義v9.md`: builtin / special type、Error、Extractor、Closure、Bind / SafeBind、Pattern consumer、pipe。
- `docs/dev/Do_intrinsic_spec.md`: Extractor の Option 前提、partial <- / SafeBind failure、do-local target。
- `docs/dev/diagnostics.md`: MatchResult / ExtractorClosure 利用位置、arity / 型注釈、予約語、projection、SafeBind target、内部契約違反の producer-owned reason / data。
- `docs/dev/Xldr_spec.md`、`docs/dev/テスト方針.md`: signature / completion と受入境界。
- `docs/site/extractors.md`、`pattern-matching.md`、`language-reference.md`、`language-guide.md`、do / SafeBind / Closure / consumer の利用者説明。
- 標準 Extractor / consumer / special type の @doc と REPL サンプル。

現行動作の説明だけを先に将来契約へ書き換えない。未実装仕様である本書と、仕様反映後もまだ未実装の箇所を明確に区別する。

## 11. 受入条件と検証

### 11.1 必須の成功・拒否境界

1. named / builtin Extractor と ExtractorClosure が MatchResult の OK / Err だけを返す。手書き Err の具象 deferror 引数を受理し、観測済み abstract Error の手書き再投入を拒否する。旧 Option、一般 Result、NoMatch、非 Error payload も拒否する。
2. 両者の事前引数0 / 1 / 複数が成立し、総 arity、事前引数型、末尾の consumer input 型、子 Pattern 型・arity の不一致を静的拒否する。
3. 単値 / tuple / UnitOnly の shape が同じ規則で扱われる。UnitOnly の省略、通常 bind、wildcard、Unit projection、型注釈付き Unit projection が成功する。事前引数付き省略形と tuple の Unit slot も検証する。
4. projection 0 / 1 / 複数、nested / root alias、list tail、番号と traversal 順が異なる例が所定の値・型になる。最大16、欠番、重複、_01 正規化、不正利用位置を検証する。
5. 冗長な projection 型注釈と注釈なし推論の両方を受理し、通常位置 / tail / alias / Unit の型不一致は compile error とする。型注釈を runtime 不一致へ変換しない。
6. capture、helper 引数 / 戻り値、同型値の if / match 選択、Extractors の shadowing、generic の通常統一が成立する。選択した non-ExtractorClosure local からの解決し直しを拒否する。
7. 通常 call、literal / 生成式の Pattern head 直接適用、通常 Closure との暗黙変換、MatchResult の一般値保持・分解・capture、入力0の ExtractorClosure、partial Pattern の通常 Bind を拒否する。
8. input / RHS、到達した事前引数、本体の一回評価、到達しない occurrence の未評価、nested failure 後の短絡、全成功後だけの bind / projection 公開を確認する。
9. match / if_let / if_let_then / is_match が Err を破棄し、apply_pattern が実際に失敗した Extractor の Error を保持する。OK 後の子 Pattern 不一致は既存 Pattern Error になる。
10. 通常 Result target と MatchResult 本文 target の SafeBind が RHS Result.Err、LHS Extractor.Err、nested Err、通常 Pattern Error の kind / message / location / cause を保持して早期 return する。失敗後の本文は未評価で、成功終端には明示 constructor を必要とする。
11. Result RHS は外側一段だけ射影する。non-Result partial pass-through、total non-Result 拒否と型エラー優先を維持する。apply_pattern は Result input を自動射影しない。
12. nested 通常 Closure / ExtractorClosure / do の failure target が最も近い正しい境界を指す。do の Result-effect 保存、Alternative empty、Monad 単独拒否、REPL Error 表示と継続を維持する。
13. 通常 bind が外へ漏れず、事前引数 / pin は同じ Pattern 内の新規 bind を参照しない。OR の match 限定、予約語 shadowing 拒否、既存 Regex::is_match の qualified 通常 call / capture、pipe と projection の分離、通常 Expr 内の _N 残存と宣言 / bind / shadow の拒否が成立する。
14. list / string uncons の成功と空入力 Error、builtin / user-defined / local の consumer 一貫性、不正 tag / metadata の内部 failure を検証する。
15. 旧専用経路と互換 fallback が残らず、正本・標準 @doc・実装・テストが同じ契約を示す。
16. Extractor::from_result が単一入力の Result-returning callable を受理し、単値 / tuple / Unit payload を維持する。生成時は本体未評価、各 occurrence 到達時は一回評価とし、元 Error の保持、Result payload の追加 unwrap なし、通常 Closure の capture、Option / raw payload / 入力 arity 不一致の静的拒否を確認する。
17. 異なる payload 型の Extractor を match の別 branch で使い、各 branch の式が同じ型を返す場合に成立する。apply_pattern の結果が既存 Result / Monad 操作へ接続でき、Pattern 内の bind が外へ漏れない。

### 11.2 検証方法

構文・型・内部契約は直接責務を持つ crate-local テスト、PureSurtr の成功例は lib/tests、静的拒否は script fixture、scope / import は module fixture、REPL 継続・signature / completion は Xldr の既存 test inventory に置く。diagnostics の human / JSON は元の reason / span / source facts を比較する。同じ契約を各層で重複検証しない。

実装時は各段階の最小境界で TDD を行い、関連フェーズへ必要な範囲だけ広げる。局所コマンドと fixture 配置の詳細は [テスト方針](../docs/dev/テスト方針.md) に従う。level 4 の実装完了には、最終 revision の次の全件 Green と独立レビューを必要とする。

```sh
rtk cargo nextest run --profile ci --workspace
cargo run -- test --quiet --all
```

仕様文書の統合だけを行うターンでは compiler テストを実行せず、原案・ユーザ決定の網羅性、削除文書への依存なし、ローカルリンク、Markdown / whitespace、現行契約との区別を検証する。実装用テストの成功を本書の作成完了条件と混同しない。
