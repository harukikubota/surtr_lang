# PR: 標準 Eq・トレイト実装制限・Test の等価性判定の整理

状態: 実装入力。2026-09-30 に未確定の実装範囲を確定。
変更レベル: 4（トレイト能力、実装権限、標準比較、derive 診断の契約変更）。

## 目的

`Test::assert_eq` の成否を `inspect` の表示文字列から切り離し、型ごとに定めた
`Eq` によって判定する。コンパイラ管理型のトレイト実装可否は Sindr を正本として
管理し、比較できない型は compile error として利用者へ説明する。

この文書を実装の契約とする。

## 1. 確定方針

- Eq は同じ静的型同士だけに適用する。異なる型の暗黙変換や異種比較は追加しない。
- 通常の値は immutable・deep copy を前提とし、格納アドレスを値の等価性にしない。
- 標準型は、仕様が確定し Eq の対象となる型に Eq を提供する。
  明示的な対象外型と、仕様未確定の保留型は分ける。
- Duration のように実装内部で特別扱いしていても、ユーザコード上で一般の値である型は
  通常のユーザ定義型の規則で扱う。コンパイラによる特別扱いだけを理由に制限しない。
- Result / List / Tuple / HashMap も通常のユーザ定義型レベルで扱う。
  標準の実装をソースで提供し、重複・制約の検査は通常のトレイト規則を使う。
- 任意のユーザ定義型へ Eq を無条件に自動付与する変更ではない。
  通常型の明示的な impl / derive と、既存の重複・整合性検査を維持する。
- Error と関数値はトレイト実装対象外。標準実装も例外にしない。
- FacetPath は利用者が操作する通常のデータ型ではなく、マーカーとして扱う
  インスタンス型であり、トレイト実装対象外とする。Compose トレイトの廃止を今回に含める。
- Show はユーザが表示を実装するためのトレイトとし、プリミティブの既存実装は維持する。
  暗黙の Show 提供を廃止し、derive Show はフィールド・payload の Show を再帰的に要求する。
- 関数演算子トレイトの移行を今回含める。移行後に関数型の全トレイト禁止を有効化する。
- `Test::assert_eq` は Eq を要求し、Eq 未実装時に表示比較へ戻らない。
- PartialEq の新設、通常値のコピー方式変更、比較用の個体 ID 追加は今回の対象外。

## 2. Error と Result

### Error

Error は抽象的な、コンパイラ管理の観測・伝播用オブジェクトである。
具体的な失敗は `deferror` により構築し、保存・受け渡しは Result の失敗枝を通す。
既存の Error の利用位置制約を緩めない。コンパイラ管理の構築・伝播経路と、
ユーザが裸の Error を一般データとして保持することは区別する。

- Eq / Show / Convert を含め、Error へのトレイト実装を禁止する。
- `lib/types/error.srt` の `impl Show for Error` と `impl Convert<String> for Error` を削除対象とする。
  現行の Convert は Error → String の変換である。
- `inspect` / `eprint` と必要な公開プロパティ API を観測経路として残す。
  Error の観測処理は Show に依存させない。
- 一般 API が失敗を返すときは Result を返す。呼び出し側が毎回裸の Error を
  Err に包み直す API は設けない。
- 元の constructor 引数列を比較用に保持しない。現行 `RichError` に引数列は保存されていない。
- 呼び出し先が返しうる具体 error の集合を静的に列挙させない。
  具象 kind の一致判定は runtime で行う。名前解決や constructor 引数の型検査は維持する。

### Result の Eq

同じ `Result<T>` 同士を比較し、`T: Eq` を要求する。

| 左 | 右 | 判定 |
|---|---|---|
| Ok(a) | Ok(b) | T の Eq |
| Ok(_) | Err(_) | False |
| Err(_) | Ok(_) | False |
| Err(a) | Err(b) | head の具象 kind が同じなら True |

Err の message・cause・元引数・発生地点・スタック・診断情報は比較しない。
同じ kind であれば、これらが異なっても等しい。デバッグ情報の有無で結果を変えない。
Error の Eq を呼ぶ経路は作らず、標準 Result の match と kind 比較で表す。
Eq によって保証するのは失敗の種類の一致であり、同じ発生一件や詳細の一致ではない。

kind は具象宣言を識別し、match と Result の比較で同じ意味を持たせる。
現在 `Error::kind` は `surface_path_name` を通すため、内部名と公開名の対応を検証する。
表示の偶然の一致に依存せず、新しい数値 tag を無条件に導入しない。
runtime の壊れた Error 表現は False に隠さず invariant failure とする。

Result の明示的な標準 Show を提供する場合も通常の impl として定義し、成功値に Show を
要求する。失敗枝は明示的な Error 観測経路を使い、Error の Show を要求しない。
inspect(Result) の表示は、Result の Show の有無とは独立して提供する。
`lib/tests/error.srt` の Show / Convert 成功テストは新しい観測 API と拒否境界へ移行する。

## 3. 型ごとの Eq 方針

| 分類・型 | 方針 |
|---|---|
| Int / String / Boolean | 標準の値比較 |
| Float | 現行の finite-only と数値比較に従う |
| Unit | 常に等しい |
| Tuple | 2〜8 要素の標準 Eq を提供し、各要素の Eq による対応位置の比較 |
| List<T> | T: Eq。長さ・順序・要素が一致 |
| HashMap<V> | V: Eq。キー集合・対応する値が一致。挿入・格納順は無関係 |
| Result<T> | 前節の専用契約 |
| worker の PID<T> | 同じ process type の PID 同士で ID が一致 |
| singleton の PID<T> | 同じ process type の singleton は一意なので等しい |
| Duration / Range / Option / Either 等の通常型 | 通常の impl / derive 規則。必要な要素・payload の Eq を要求 |
| Error / 関数値 | 全トレイト実装禁止 |
| FacetPath | Eq 対象外 |
| MatchResult / MatchArms / CondClauses / DoBlock / BulkUpdateEntries / Lazy / StandbyInit / Hole | 一般の値比較を提供しない。利用者のトレイト実装を拒否する |
| Generator | 比較仕様未確定。今回 Eq を追加せず、利用者の実装も拒否する |
| Regex / RegexMatch / RegexCaptures / RandomGenerator | 比較範囲未確定。今回 Eq を追加せず、利用者の実装も拒否する |
| FileHandle / TaskHandle / Workers / WorkerLease | 個体の意味・寿命を含む比較仕様未確定。今回 Eq を追加せず、利用者の実装も拒否する |

コンテナの条件は型単位で検査する。空 List や両辺 Err を理由に要素型の Eq 制約を外さない。
通常の enum の構造比較は variant identity と payload の比較とし、Result の kind 比較を
通常 enum へ一般化しない。内部の Rust `PartialEq` を言語の Eq として無条件に公開しない。

PID は異なる process type 間の比較を拒否する。再起動・失効後の handle の操作可否と
比較を混同しない。handler capability の PID は singleton / worker の方針から推測せず
保留する。既存の `doc/pid_eq_spec.md` は今回編集せず、採用時に本方針との整合を取る。

## 4. Sindr を正本とするトレイト実装管理

PR の主目的は Eq と Test の整理だが、Sindr の関心事はトレイト全般である。
Eq 専用の許可フラグを増やす方式にはしない。型別監査では、会話で確定した通常型扱い、
Error・関数値・Facet の禁止、PID の Eq と、意味論未確定の管理型への実装禁止を区別する。

「トレイトを実装できるか」と「実装が実際に提供されているか」を分離する。
許可メタデータだけで trait obligation を満たしたことにしない。

| 実装方針 | 意味 |
|---|---|
| 通常実装可能 | ユーザ・標準が通常の整合性規則に従って実装できる |
| 標準管理 | 指定された標準またはコンパイラ生成の実装だけを認める |
| 実装禁止 | ユーザ・標準・自動生成を含め実装できない |

型 identity / 型 family、トレイト identity と型引数、実装元の権限から判定する。
Error・関数型の全トレイト禁止と、特定型の Eq 禁止を区別する。
未確定型の Eq も、仕様確定まではユーザ実装で先取りできないようにする。
通常型に対する独自トレイトまで一律禁止しない。

- Sindr: 共有する型・トレイト実装ポリシーの正本。現行 `SymbolCapabilities::impl_target`
  の単一フラグだけでは表せない権限を、inherent impl の可否と区別して定義する。
- Sigil: canonical identity と信頼できる宣言の由来を保持する。
- Scar: 解決済みの型・トレイト・型引数を用い、実装の登録・適用前に検査する。
- derive / generic impl / compiler-generated capability も同じ規則を通す。
  既存の標準ソースや生成経路であることだけを理由に禁止を回避しない。
- Forge / Eldr: 受理された比較契約を実行する。型名の再解釈や独自の許可表を持たない。

コンパイラ管理型の未登録ケースを通常型へフォールバックさせない。
診断用の表示名・ソース上の別名・宣言順ではなく、解決済み identity を使用する。

### 4.1 型単位の基本方針と、必要なトレイト別設定

全 compiler-managed type を一律に許可／禁止する方式では既存の契約を表せない。
型ごとの基本方針と、必要な型・トレイトの組に対する明示的な設定を同じ正本に置く。

| 型・family | 全トレイトの基本方針案 | 個別設定・確認結果 |
|---|---|---|
| Int / Float / String / Boolean / Unit | 通常実装可能 | 標準 Eq 等の既定実装は置換不可。既存の Show・Convert・数値演算等を維持。String へのユーザ Convert<Slug> のテストもあり、全ユーザ impl 禁止にはしない |
| Tuple | 通常実装可能（確定） | ユーザ定義型レベル。Eq / Compare の要素制約・標準対応 arity を通常の実装で表す |
| List / HashMap | 通常実装可能（確定） | ユーザ定義型レベル。標準 Eq を通常の impl として提供。List の Functor / Applicative / Monad / Alternative を維持 |
| Result | 通常実装可能（確定） | ユーザ定義型レベル。Eq の失敗枝規則は標準ソースの実装で表す。Functor / Applicative / Monad と Convert<Option<T>> 等を維持 |
| Duration、通常 struct / record / enum、Option 等 | 通常実装可能 | コンパイラ内の名前・表現上の特別扱いから権限を推測しない。既存の通常 impl / derive / coherence に従う |
| Error | 全トレイト禁止（確定） | 標準 Show / Convert を削除。観測 API とトレイトを分離 |
| 関数型 / Closure marker / ExtractorClosure | 全トレイト禁止（確定） | inspect はトレイト能力ではない。既存の関数合成 impl の移行は第7節で扱う |
| MatchArms / CondClauses / DoBlock / BulkUpdateEntries / Lazy / Hole | 全トレイト禁止 | 構文・評価用マーカー。inherent / signature の既存使用制約と別に設定 |
| MatchResult / StandbyInit | 全トレイト禁止 | Extractor / process のプロトコル用 carrier。Result と見た目が似ていても同じ能力を自動付与しない |
| Facet（FacetPath の型表現） | 全トレイト禁止 | Compose を今回廃止し、`/` は Facet::chain に対応する固定構文へ移す。標準 Compose の許可例外は作らない |
| PID | Eq の compiler-owned capability だけ許可 | singleton / worker の Eq を提供する。Show・Convert・Compare 等は拒否する。handler capability は Eq の対象に含めない |
| Regex / RegexMatch / RegexCaptures / RandomGenerator / Generator | 全トレイト禁止 | 比較意味論は保留。未確定能力は利用者から見れば実装禁止であり、暗黙 Show も提供しない |
| FileHandle / Workers / WorkerLease / TaskHandle | 全トレイト禁止 | 資源・実行主体の handle。比較意味論は保留し、既存 helper や handle 操作は維持する |

調査対象は `BUILTIN_TYPE_METAS`、`TypeName`、Tuple と実際の関数型 family。
型 head の一覧にない PID や function signature、通常宣言である Duration も区別した。
表の「原則禁止」は標準からの任意 impl を許す意味ではなく、列挙した組だけを許可する。
新しいトレイトを追加しても、閉じた管理型に自動的な許可は生じない。
通常型の標準実装との重複拒否は既存 coherence を使い、すべてを個別表へ複製しない。

### 4.2 宣言以外の能力提供も同じ管理を通す

現行 `crates/scar/src/checker/predeclare.rs` の `compiler_trait_impl_exists` と
`compiler_trait_dispatch_target` は、Show を「型変数・primitive・Error 以外」へ広く
暗黙提供する。さらに enum の Eq も compiler-owned capability として提供する。
標準 `.srt` の impl 一覧だけでは、実際のトレイト能力を把握できない。

- Show の暗黙提供経路は型を問わず廃止する。プリミティブの明示実装は維持する。
  Show がない場合に inspect へ委譲する fallback は設けない。
- 明示 impl、derive、enum Eq、今後の PID 能力が同じ許可判定を通る。
- トレイト証明と実行先の選択で別の許可判定を持たない。証明できても dispatch できない、
  あるいは禁止した能力が dispatch だけに残る状態を拒否する。
- builtin 宣言の signature 検査は実装権限の検査と別に維持する。
  builtin metadata に関数が載っているだけで、その型へのトレイト実装を許可しない。
- トレイトの前提能力も検証する。Applicative → Functor、Monad → Applicative のような
  依存を満たせない設定を、許可表の登録だけで成立させない。

### 4.3 実装対象・引数・構造依存を分ける

「関数型へのトレイト実装禁止」は、トレイトメソッドが関数値を受け取ることや、
List / Result が関数値を保持することを禁止しない。
例えば Applicative の `Self<(A -> B)>` と mapper 引数は既存の正当な用途である。
普通の外側型の手書きトレイトまで、内部に関数があるだけで一律禁止にはしない。

一方、Eq の構造比較や derive は、その操作が必要とする内部型の能力を検査する。
関数を含む型の derive 拒否は第5節の依存能力検査で説明する。
Show の手書き実装による明示的な inspect 使用は許可し、コンパイラの fallback と区別する。
Result の Err に含まれる Error は Result 自身の専用契約で扱い、Error の Eq / Show
を再帰的に要求してはならない。

Convert<T> の実装先 Self と変換先 T も分ける。例えば通常型から裸の Error を返す
宣言は、Error が Self ではなくても既存の Error 利用位置制約で拒否する。
任意の型引数に禁止型が現れたら一律拒否する規則は作らない。

### 4.4 追加の受入条件

- Error / 関数型は Eq 以外の Show / Convert / ユーザ独自トレイトも拒否する。
- 構文・プロトコル用マーカーの基本方針を採用した場合、同じ全トレイト禁止を検証する。
- Facet の全トレイト実装を拒否し、Compose 廃止後も `/` と Facet::chain の型・可視性規則が一致する。
- PID の Eq 許可から Compare / Show / Convert 等の能力が暗黙に生じない。
- 通常型のユーザトレイト、String の Convert<ユーザ型>、List / Result の高階操作を維持する。
- 関数値を保持する List / Result の Applicative 操作と、Eq / derive の拒否を区別する。
- 暗黙 Show は存在せず、derive・trait obligation・dispatch が同じ管理規則を使う。
- Result / List / Tuple の通常 impl と要素制約が働き、Sindr の専用許可リストを要求しない。

## 5. 関数値と derive の拒否診断

named function capture、closure、partial application、ExtractorClosure を含む関数値は
トレイト実装対象外とする。通常の呼出しや capture を禁止する意味ではない。
表示・観測には inspect を用い、Show / Convert / Eq などの実装は認めない。

関数値を直接のフィールド・payload に持つ場合、その関数型に必要なトレイトがないため
構造に沿った derive は成立しない。Tuple / List / Result / 通常の型を介する場合も、
各実装が要求する能力の依存をたどって検査する。
外側の通常型に対する手書き impl は、関数型自身への実装禁止とは区別する。

### Show の明示実装と derive

- Show はユーザ実装用のトレイト。プリミティブの既存の明示実装を維持する。
- derive Show は各フィールド・payload に Show を要求し、その制約を再帰的に満たすか
  検査する。生成する表示処理も各値の Show を使う。現行の InspectShow による
  `inspect(self)` への無条件委譲は置き換える。
- generic derive は必要な Show bound を持ち、具体型適用時にも検査する。
  未充足なら compile error とし、inspect で救済しない。
- 手書き Show は、各部分を Show で表示するか、明示的に inspect で観測するかを選べる。
  関数フィールドを持つ外側型の Show を手書きし、そのフィールドに inspect を使うことは許可する。
- derive の検査は明示実装の契約に従う。内側の通常型が手書き Show を持つ場合はそれを利用し、
  その内部表現をさらに展開して拒否しない。禁止型の再帰的な包含だけで判断する規則は設けない。
- inspect は Show の有無と独立する。ユーザによる明示呼出しとコンパイラの暗黙 fallback を混同しない。

### ルートから原因を説明する

例として、Root.callbacks が List<(Int -> Int)> のとき、`@derive Eq` の失敗は
次のような情報を含める。以下は診断の概念例であり、固定文言ではない。

```text
Root の Eq を derive できません。
Root.callbacks -> List の要素 -> (Int -> Int)
関数型はトレイト実装対象外です。
```

通常型の内部は、その型自身の明示 impl / derive を能力の境界とする。例えば
`Job.callback` が関数型なら `Job` の derive 失敗は `Job.callback` を根から説明する。
外側の `Root.jobs: List<Job>` は、`Job: Eq` の不足を示す。手書きの `Job: Eq` が
存在すれば関数フィールドを比較しない実装も正当なので、`Root` の診断から
`Job.callback` へ無条件に潜らない。

- root の derive 指定と、原因となる field / payload の宣言位置を source label で示す。
- nested type、enum variant、tuple position、container の型引数を依存経路として示す。
- generic 型は、宣言時の未充足 bound と、具体型適用後の禁止型を区別する。
  `Box<T>` を宣言できても `Box<(Int -> Int)>` に必要な能力は成立しない。
- `assert_eq` で初めて能力を要求した場合は、その呼出し位置から必要な型と原因まで案内する。
- 再帰型の診断走査は循環を検出し、同じ型を無限展開しない。
  最初に示す原因は宣言順等の決定的な規則で選ぶ。
- 本当に禁止された型に対して「Eq を実装してください」と案内しない。
  help は derive の削除、比較したいデータの明示的な抽出、表示目的なら inspect の使用を示す。
  Show の場合、通常型の未実装なら明示 impl を案内し、関数型の禁止なら外側型の手書き Show で
  関数部分を inspect する方法を案内する。

`docs/dev/diagnostics.md` に従い、root・経路・末端型・禁止理由・source fact を構造化して
保持し、Human / JSON の両方を同じ情報から生成する。文字列の再解析で原因を推測しない。
単なるトレイト未実装と、ポリシー上の実装禁止を識別できる診断にする。

## 6. Test の移行

- `assert_eq(expected: A, actual: A)` に A: Eq を要求する。
- 合否判定は Eq::eq のみ。neq / != は Eq の否定として整合させる。
- inspect は失敗時の説明にのみ使用できる。Show は要求しない。
- `assert_ok_eq` 等の委譲 helper と、必要な呼出し元にも Eq 制約を伝える。
- 旧テストは「値の等価性」と「表示の一致」に分けて移行する。
  表示契約のテストでは inspect(value) 等を明示的に呼び、その String を比較する。
- 関数値や Eq 未確定型のテストは、公開 API の結果等、実際に検証したい値を比較する。
- 異なる値が同じ inspect 表示になっても、それだけでテストが成功してはならない。

## 7. 実装範囲と既存経路の移行

### 今回の対象: Compose の廃止

演算子トレイトは、ユーザが型ごとの振る舞いを定義できるポリモーフィズムの入口として
ソースコードに置く。FacetPath の合成は処理系が意味を固定するためトレイトを介さない。

- `lib/traits/operator/compose.srt` と `lib/facet.srt` の Compose impl を削除する。
- 合成の説明・型の接続条件・可視性規則・使用例を Facet::chain の @doc に集約する。
  Compose::compose に依存する説明を残さず、`/` が chain に対応する固定構文であることを示す。
- `/` は維持し、Facet::chain と同じ型検査・可視性検査・lowering 契約へ接続する。
  trait lookup、Compose 用の builtin surface metadata、読込・補完・文書参照を整理する。
- 旧 Compose 経路や失敗時の trait fallback は残さない。他型の `/` は受理しない。

演算子全体の分類は [演算子整理タスク](./flow_operator_dispatch_and_repl_lookup_plan.md) と
共有する。Compose 廃止は本 PR の担当とし、他の演算子トレイト移行を取り込まない。

### 今回含める関数合成・パイプ適用の移行

現行標準には以下の関数型へのトレイト実装が存在する。

- `lib/traits/operator/composable.srt`: Composable、演算子 >>
- `lib/traits/operator/pipe_apply.srt`: PipeApply、演算子 |>
- `lib/types/{result,list,option}.srt`: LiftComposable / KleisliComposable、演算子 >* / >=>

これらの宣言・実装の移行を今回含める。[演算子整理タスク](./flow_operator_dispatch_and_repl_lookup_plan.md)
に示した Bootstrap builtin operator へ置き換えた後で、関数型の全トレイト禁止を有効化する。
REPL の検索入口は演算子そのもの、または対応する `fun_name` とする。
標準 impl への暫定例外や新旧二経路は設けない。

### 実装対象の棚卸し

実装範囲は次のとおり確定した。現行ソースとの対応を示す。

| 判断事項 | 現行状態と選択肢 |
|---|---|
| tuple の標準 Eq arity | `lib/types/tuple.srt` の 2 要素を 2〜8 要素へ拡張し、Compare と提供範囲を揃える |
| 管理型の権限 | PID は Eq のみ。その他の内部型・opaque 型・handle 型には未確定能力を公開せず、利用者のトレイト実装を拒否する |
| 関数型の全禁止 | 現存する Composable / PipeApply / LiftComposable / KleisliComposable の関数型 impl を今回移行してから有効化する |

暗黙 Show の廃止、通常型扱い、Compose の廃止も今回実施する。
意味論未確定の能力は、将来仕様が定まるまで利用者からの実装を拒否する。

標準の通常型を含む Eq の不足箇所、利用可能な tuple arity、enum の compiler-owned
capability と通常 impl の境界を確認し、対応漏れをなくす。現行 `lib/types/tuple.srt` の
Eq は pair に限られ、Compare の 2〜8 要素対応とは範囲が異なる。
tuple の標準 Eq は 2〜8 要素へ提供する。
Regex 系・RandomGenerator・各 handle 型の比較意味論は別途確定するまで保留する。

generic derive Show は第5節の Show bound による検査へ移行する。
既存の型制約処理に接続し、宣言と具体型適用のどちらで失敗したかを診断に保持する。

本ターンは最初の変更を戻す依頼に従い、`doc/open-issues.md` は復元した。
今回採用しない各 handle 等の比較意味論は、将来の個別設計に残す。

## 8. 実装計画と受入条件

1. Eq 対象範囲と関数演算子の移行・禁止の依存順を正本へ反映する。
2. Sindr のトレイト全般のポリシーと Sigil / Scar の検査を実装し、直接・generic・derive・
   compiler-owned capability の迂回を拒否する。型別の基本方針と必要な標準能力を検証する。
3. ルートから原因へ至る構造化診断を実装し、直下・nested・generic・再帰型を確認する。
4. Error の Show / Convert と暗黙 Show を削除し、derive Show を再帰的な能力検査と表示へ移行する。
5. Compose を削除し、`/` を Facet::chain の固定規則へ接続する。コメント・文書を集約する。
6. 通常の標準 Eq と Result / PID の比較を追加・整合し、Test を Eq 判定へ変更する。
7. 関数演算子を Bootstrap builtin operator へ移行してから、関数型の全禁止を有効化する。
8. 既存テストと正本文書を移行し、全体検証と独立レビューを行う。

正本の更新先は `docs/dev/Trait_system_spec.md`、`docs/dev/diagnostics.md`、
`docs/dev/EldrVM_spec.md`、`docs/dev/ProcessRuntime_spec.md`、対応する `docs/site/` と
`lib/**/*.srt` の @doc とする。@doc のサンプルは実際に実行して確認する。

受入条件:

- 同型比較、条件付き Eq、Result の全枝、PID の singleton / worker 境界が成立する。
- Err 同士は kind だけで判定し、message / cause / 場所 / debug 情報の差は影響しない。
- Error・関数型の禁止実装を標準・ユーザ・生成経路で拒否し、旧成功テストを現行の拒否へ移す。
- 必要なトレイトを満たさない derive の失敗に root から末端までの経路が表示される。
- Show 未実装を暗黙に補わない。プリミティブの明示 Show と通常型の手書き Show は利用できる。
- derive Show は各要素の Show を呼ぶ。nested / generic の制約不足を拒否する。
- 関数フィールドへの明示的な inspect を使う外側型の手書き Show は成功し、その型を含む
  外側の derive Show も当該明示実装を利用できる。
- 機能が未実装なのか、実装禁止なのか、generic bound が不足しているのかを区別する。
- Eq と inspect が異なる結論になるケースで、assert_eq は Eq の結果を採用する。
- Eq を持たない型は assert_eq で compile error。文字列化による救済を設けない。
- デバッグ情報あり／なしの同一ソースで assertion の判定が一致する。
- Compose 宣言・impl・dispatch を除去し、`/` と Facet::chain の型・可視性規則が一致する。
- 関数演算子を移行し、旧トレイト impl を削除した後に関数型の全禁止を有効化する。

実装時は変更契約を直接検証する層で TDD を行い、最終的に
`rtk cargo nextest run --profile ci --workspace` と
`cargo run -- test --quiet --all`、独立レビューを実施する。

実装の完了時に実行コマンド・結果・残件を報告する。
