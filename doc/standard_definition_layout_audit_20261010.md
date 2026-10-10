# 標準定義ソースの配置・定義順の調査と修正仕様

調査日: 2026-10-10。対象: `ce00e240` の `lib/**/*.srt`（`lib/tests/` を除く）と標準ソースの登録・名前解決経路。

## 確定した修正方針

2026-10-10 の実装依頼で未確定事項3点を確定した。

- 共通 Error に Pattern / Extractor の失敗を含め、下記の16件を `lib/errors.srt` にまとめる。
- 固有定義は定義系の基底モジュールを優先する。`process.srt` は Process → Supervisor → GenServer → Agent → Workers → Task → OutHandler → InHandler → DynamicSupervisor → 補助型の固有 impl の順とする。`types/json.srt` は Json module を JsonValue の固有 impl より先に置く。
- `@hidden def` は通常の公開関数の後、Extractor の前に置く。可視性を変えず、各区分内の相対順を保つ。
- 標準ソース登録の `module_path` は省略可能にする。`errors.srt` と `types/special_types.srt` は単一モジュールを持たない定義バンドルなので `None` とし、存在しない `Errors` / `SpecialTypes` の名前を登録しない。stage 情報でも `Option<String>` を保持し、空文字に変えない。

配置整理は level1。登録情報の省略可能な module path を Sindr・Xldr・Rune・LS の共通登録とキャッシュへ通す修正は複数入口に及ぶため level3 とする。言語仕様・ロードの意味論は変えない。以下の棚卸しと行番号は修正前の調査記録であり、配置規則と受入条件を実装入力とする。

## 結論

指定された順序は、標準定義の構文や型規則を変えずに整理する方針として採用できる。trait 実装は `impl Trait<...> for 対象型` の対象型を所有者とする。この基準では、ファイル間の移動候補は9件である。

共通の具体 Error は新しい `lib/errors.srt` にトップレベルの `deferror` として集約する。抽象型 `Error` とその操作は `lib/types/error.srt` に残す。`deferror` は module member として宣言できないため、`defmod Errors` で包まない。新しいファイルは標準ソースの登録表へ追加する必要がある。

調査時点では読み取りのみを行った。本修正では共通 Error の登録追加、宣言の移動・並べ替え、Error body の折り返しと関連文書の追従を実施する。

## 1. 調査範囲と現状

`@doc` 内のサンプルを除いて宣言を集計した。テスト用標準ソース `lib/test.srt` は対象に含め、テスト入口・support ソースは対象外とした。

| 項目 | 現状 |
|---|---|
| 標準ソース | 64ファイル |
| trait 宣言ファイル | `lib/traits/` 内に21ファイル。具体型への trait 実装は0件 |
| 明示 trait 実装 | `lib/types/` 内に120件。111件は対象型の所有ファイルに配置済み |
| 具体 Error 宣言 | 19ファイルに203件 |
| private 関数 | 7ファイルに89件 |
| 名前付き Extractor | 3件 |
| プロセスのコールバック | 標準実コードには該当なし。`process.srt` の例は `@doc` 内 |

現状の主な不一致は次のとおり。

- `Default` が固有 `impl` より先にある: `types/int.srt:16`、`types/float.srt:10`、`types/string.srt:25`。
- `Eq` が固有 `impl` より先にある: `types/list.srt:73`、`types/result.srt:32`、`types/hash_map.srt:49`。
- `types/tuple.srt` は14行から trait 実装が並び、主要な `defmod Tuple` は286行にある。
- `FileSystem.srt`、`process.srt`、`styled_doc.srt` などは補助型の宣言と固有 `impl` が交互に並び、主要 module が後方にある。
- Error がファイルの先頭や定義間に置かれている。`kernel.srt` は Error の後に `Mapper` / `Predicate` / `Reducer` の alias がある（567–569行）。
- private 関数はすべての該当ファイルで公開関数の前か途中にある。

## 2. ファイル内の配置規則

依頼された5区分を次のように具体化する。これは標準ソースの配置規則であり、言語仕様上の必須構文順ではない。

1. **主要な型宣言**: そのファイルを所有する `@builtin type`、`defstruct`、`defenum`、型 alias。主要型が複数なら主要度の順に並べる。
2. **固有定義**: `impl`（trait 実装を除く）、`defmod`、`deftrait`、プロセス定義を定義系の基底モジュールを優先し、主要な型・定義から順に置く。補助型の固有 `impl` もこの区分の後方に置く。
3. **trait 実装**: このファイルが所有する型への実装をまとめる。主要型、補助型の順とし、各型の中では既存の相対順を保つ。
4. **補助宣言など**: 主要でない型宣言、補助 alias、その他のトップレベル定義を置く。
5. **Error**: このファイルの型・APIに固有の `deferror` を最後にまとめる。

主要型宣言がない module ファイルでは区分1を省略する。`Tuple` は `types/tuple.srt` をタプル型群の所有ファイルとして扱い、型宣言を新設せず、`defmod Tuple` の後に trait 実装を置く。trait 専用ファイルでは `deftrait` が主要定義となる。

補助型の固有 `impl` を区分2、宣言を区分4に置くと前方参照になる。同一 stage の事前宣言経路に沿う配置だが、移動後の解決は実装時に検証する。補助型の宣言・固有 `impl` を一組にして区分4へ置く案もあるが、依頼の「impl を主要な定義から順に置く」から外れるため、ここでは採用しない。

`import` とファイル全体の説明は先頭の前置きとして扱う。宣言に付く `@doc`、`@builtin`、`@autoimport`、`@derive`、`@hidden`、コメント、`where`、本体は宣言と一緒に移す。構造体フィールド、enum variant、Payload、引数、式の順序は変更しない。

### 主要定義と補助型の対応

この表は修正対象と主要定義の対応であり、標準ソースのロード一覧の正本ではない。

| ファイル（`lib/` 相対） | 主要な型・定義 | 補助宣言・留意点 |
|---|---|---|
| `bootstrap.srt` | Bootstrap | 共通 Error を切り出す。stage の入口を維持 |
| `kernel.srt` | Kernel | Mapper / Predicate / Reducer を `function.srt` へ移す |
| `function.srt` | Mapper / Predicate / Reducer / Function | シグネチャ alias をトップレベルの区分1へ置き、その後に Function module |
| `extractor.srt` | Extractor | 説明内の Checks は実宣言ではない |
| `facet.srt` | Facet | `@FacetPathKind Type` の宣言・alias を補助宣言として扱う案 |
| `Config.srt` | Config | 構造体と固有 impl |
| `Project.srt` | Project | 構造体と固有 impl |
| `Random.srt` | Random | InvalidRandomRange は末尾 |
| `file.srt` | File | FileHandle / FileMode は補助型 |
| `FileSystem.srt` | FS | FilePath、EntryKind、Permissions、Metadata、Entry、Snapshot の各型 |
| `IO.srt` | IO | 固有 Error を末尾 |
| `Shell.srt` | Shell | CommandResult の宣言は区分4、固有 impl は区分2の後方 |
| `process.srt` | Process と関連するプロセス API | 下記の複数主要定義の扱いを参照 |
| `styled_doc.srt` | StyledDoc | Color / Style / Segment / Line / Doc の各型 |
| `test.srt` | Test | TestEnabled の登録条件を維持 |
| `types/int.srt` | Int | BitWidth / IntBase。Int の impl を IntBase より先に置く |
| `types/float.srt` | Float | Default を trait 実装区分へ |
| `types/string.srt` | String | StringSplit / StringEncoding |
| `types/boolean.srt` | Boolean | derive は型属性のまま |
| `types/list.srt` | List | ReduceStep |
| `types/hash_map.srt` | HashMap | Eq を固有 impl の後へ |
| `types/option.srt` | Option | for Result の変換実装を移す |
| `types/result.srt` | Result | Eq を固有 impl の後へ。for Either の変換実装を移す |
| `types/either.srt` | Either | for Either の変換実装を受け入れる |
| `types/identity.srt` | Identity | 既存の型→固有 impl→trait 実装を基本にする |
| `types/reader.srt` | Reader | 同上 |
| `types/state.srt` | State | 同上 |
| `types/monad_transformer/option_t.srt` | OptionT | 同上 |
| `types/monad_transformer/result_t.srt` | ResultT | for EitherT の変換実装を移す |
| `types/monad_transformer/either_t.srt` | EitherT | 変換実装を受け入れる |
| `types/monad_transformer/reader_t.srt` | ReaderT | 既存配置を基本にする |
| `types/monad_transformer/state_t.srt` | StateT | 同上 |
| `types/duration.srt` | Duration | for Int の変換実装を移す。固有 Error を末尾 |
| `types/range.srt` | Range | new と Extractor の順序は現状に沿う |
| `types/ordering.srt` | Ordering | 型と derive のみ |
| `types/monoid.srt` | Monoid | Semigroup alias を区分4へ |
| `types/generator.srt` | Generator | private と Error を各末尾区分へ |
| `types/infinite_generator.srt` | InfiniteGenerator | 固有 Error を末尾 |
| `types/regex.srt` | Regex | RegexCaptures / RegexMatch は補助型 |
| `types/json.srt` | JsonValue / Json | 主型、Json module、JsonValue 固有 impl、対象型の trait 実装の順 |
| `types/tuple.srt` | Tuple | タプル型群の実装所有ファイル。型宣言を追加しない |
| `types/error.srt` | 抽象 Error | 具体 Error の共通ファイルと役割を分ける |
| `types/special_types.srt` | compiler-facing 型群 | 既存の宣言群の相対順を保ち、Unit 実装をその後にまとめる |
| `traits/**/*.srt`（21ファイル） | 各ファイルの deftrait | 具体型の impl を追加しない |

`process.srt` は一つの主要型に絞れない。区分2は Process → Supervisor → GenServer → Agent → Workers → Task → OutHandler → InHandler → DynamicSupervisor の順とし、その後に SupervisorStatus / WorkerStrategy の固有 impl を置く。これは実行順や依存順を表すものではない。区分1には Workers / WorkerLease / TaskHandle の handle 型を、区分4には SupervisorStatus、WorkerScale、WorkerStrategy、CallResult、StopReply、CastResult、StopReason を置く案とする。

`special_types.srt` は分割を要求しない。現在の先頭コメントは「type heads と Extractor result variants のみ」と説明しているが、実際には Pending / Ready 関数と Unit の trait 実装もある（157行以降）。整理時にこの説明を実構成に合わせる。

### シグネチャ alias の移動方針（ユーザ指定）

`kernel.srt:567–569` の次の3宣言は、`lib/function.srt` のトップレベルへ移す。`defmod Function` の中には入れず、区分1として module より前に置く。宣言の相対順と署名を維持し、Kernel 側から削除する。

```surtr
type Mapper<$A, $B> = ($A -> $B)
type Predicate<$A> = ($A -> Boolean)
type Reducer<$Acc, $A> = ($Acc, $A -> $Acc)
```

この配置方針に沿って3宣言を移動し、Kernel 側の宣言を削除した。

## 3. trait 実装の移動候補

`Convert<$To>` / `TryConvert<$To>` / `Encode<$To>` / `Decode<$To>` は Self から `$To` への操作である（各 `lib/traits/*.srt:3`）。変換先を配置所有者にせず、`for` 側で統一する。双方向変換も片方向ずつ分ける。

| 現在地・行 | 実装 | 移動先（`lib/` 相対） |
|---|---|---|
| `types/duration.srt:103` | TryConvert<Duration> for Int | `types/int.srt` |
| `types/json.srt:228` | Decode<JsonValue> for String | `types/string.srt` |
| `types/json.srt:282` | Encode<JsonValue> for String | `types/string.srt` |
| `types/json.srt:291` | Encode<JsonValue> for Int | `types/int.srt` |
| `types/json.srt:300` | Encode<JsonValue> for Float | `types/float.srt` |
| `types/json.srt:309` | Encode<JsonValue> for Boolean | `types/boolean.srt` |
| `types/option.srt:226` | Convert<Option<$T>> for Result<$T> | `types/result.srt` |
| `types/result.srt:442` | Convert<Result<$A>> for Either<Error, $A> | `types/either.srt` |
| `types/monad_transformer/result_t.srt:166` | Convert<ResultT<$M, $A>> for EitherT<Error, $M, $A> | `types/monad_transformer/either_t.srt` |

9件の本体には移動元に固有の private helper 呼び出しは見つからなかった。`Json::decode`、JsonValue variant、`Duration::new`、`ResultT::new`、`Functor::fmap` などの参照は移動後も解決を確認する。

`types/json.srt:214` の `JsonValue::encode(value)` は変換先固定の固有関数であり、trait 実装と一緒には移さない。`@derive` は独立した手書き impl ではないため型宣言に付けたままにする。タプル arity 2〜8 の Eq / Compare と Unit の実装も、既存の所有ファイルに残す。

## 4. 関数順の具体化

各 `impl` / `defmod` / `deftrait` / プロセス定義の中で、次の順序を適用する。

1. 構造体の `new`。
2. プロセス定義内で `@init` / `@get` / `@set` / `@call` / `@cast` などの属性により指定されたコールバック。名前だけでは判定しない（`docs/dev/ProcessRuntime_spec.md:451–470`）。
3. 通常の公開関数 `def`（builtin / intrinsic / trait の body なし宣言も含む）。
4. `@hidden def`。
5. `defextractor`。
6. private 関数 `defp`（builtin private も含む）。

同一区分では既存の相対順を保つ。公開関数を名前順に並べたり、用途のまとまりを分割したりする追加規則は設けない。`@hidden def` は可視性を `defp` に変えず、通常の公開関数の後、Extractor の前に置く。名前が `__` や `_` で始まるだけでは private と判定しない。

`new` という名前だけで優先しない。対象型が構造体であることを確認する。通常の構築関数を new に改名したり、enum / builtin 型に new を増やしたりしない。

| 対象 | 現状と整理内容 |
|---|---|
| `types/string.srt` | private 16件。116行から helper が先行し、公開関数の途中にも helper がある。全件を同じ impl の末尾へ |
| `types/int.srt` | private 19件。258行以降の parse / bit helper を impl 末尾へ |
| `types/list.srt` | private 15件。139行以降の再帰 helper を impl 末尾へ |
| `types/result.srt` | private 4件。52 / 59 / 296 / 314行の helper を impl 末尾へ |
| `types/generator.srt` | private 6件。80行の builtin `_step` を含め impl 末尾へ |
| `styled_doc.srt` | private 22件。155行以降の helper を StyledDoc module の末尾へ |
| `test.srt` | private 7件。203行以降の helper を Test module の末尾へ。`__test_*` の def を defp に変更しない |
| `kernel.srt` | uncons Extractor（388行）が公開関数の途中にある。公開関数群の後へ |
| `types/duration.srt` | deconstruct Extractor（73行）は固有 impl の末尾にあり、順序に沿う |
| `types/range.srt` | deconstruct Extractor（78行）も固有 impl の末尾にあり、順序に沿う |

標準実コードには `@init` / `@call` / `@cast` などのコールバックはない。`process.srt` の ImageWorkerPool 等を実宣言として数えない。`DynamicSupervisor`（722行）は実際の `defsupervisor` だが callback を持たない。今後のコールバック追加に適用できる規則として残す。

## 5. 共通 Error と固有 Error

共通性は参照ファイルの数だけでは決めない。言語の共通制御・Pattern 契約、複数の型で共有する失敗を `errors.srt` に置き、型や API に固有の失敗は所有ファイルの末尾に残す。

### 共通ファイルへ集約する Error

| 元ファイル・行 | 候補 | 理由 |
|---|---|---|
| `bootstrap.srt:1,361,370,379,389` | ZeroModuloError、NoneError、ZeroDivisionError、EmptyHeadTailListPattern、NotImplemented | 標準の欠損・未実装、共通 Pattern、Int / Float で共有する演算失敗 |
| `kernel.srt:1,5` | UnconsEmptyList、UnconsEmptyString | 共通 Extractor の入力種別ごとの失敗 |
| `kernel.srt:476–506` | IntLiteralPatternMismatch、StringLiteralPatternMismatch、BooleanLiteralPatternMismatch、DurationLiteralPatternMismatch、PinnedValuePatternMismatch、ResultVariantPatternMismatch、EnumVariantPatternMismatch | 特定の型の API よりも言語 Pattern の失敗契約 |
| `kernel.srt:533,538` | ListPatternTooShort、ListPatternTooLong | 言語の構造 Pattern の失敗契約 |

共通 Error は上記の計16件とする。Bootstrap の5件と Kernel の Pattern / Extractor 関連11件を移す。

### 所有ファイルへ残す・戻す Error

- `ListIndexOutOfBounds`（`kernel.srt:518`）は `types/list.srt` の末尾へ移す。List の値アクセスと共有する型固有の失敗として扱う。
- FacetListIndexOutOfBounds / FacetListRangeReversed / FacetKeyNotFound / FacetReadVariantMismatch / FacetUpdateVariantMismatch（`kernel.srt:523,528,548,557,562`）は `facet.srt` の末尾へ移す。
- `HashMapKeyMissing`（`types/hash_map.srt:2`）は構造 Pattern にも使われるが、HashMap のキー取得契約を表すため同ファイルに残す。
- Int / String / Regex / Json / Generator 等の変換・検証・取得エラーは、その型・APIのファイルに残す。
- File / FS / IO / Shell / Process / StyledDoc / Test のエラーも、各 API のファイルの末尾に残す。全203件を共通ファイルへ集める必要はない。

抽象 `Error` の canonical 宣言・固有 impl（`types/error.srt:35,37`）は移動しない。Error コンストラクタの入力署名、Payload、メッセージ、cause、kind を変えず、既存の拒否条件やエラー種別を保持する。

### Error 定義の折り返し（ユーザ指定）

移動・並べ替えに合わせて、長い body を持つ `deferror` も適切に折り返す。長いコンストラクタ呼び出しや引数列は、既存の構文に沿って複数行に分け、インデントをそろえる。共通 Error の移設と、固有 Error を各ファイルの末尾へまとめる作業の両方に含める。

折り返しはソース上の整形に限定し、文字列の内容や補間、式、引数順、評価順を維持する。メッセージ文字列に改行を挿入したり、整形のために文字列連結や helper を追加したりしない。文字列自体が長い場合は周囲の呼び出し・引数列を折り返し、文字列のトークンはそのまま保持する。

## 6. ロード・名前解決上の制約

### 標準ソース登録

登録と stage の正本は `crates/sindr/src/stdlib.rs` の `STDLIB_MODULE_SPECS`（73行以降）。Bootstrap が compile stage 0、Main / TestExtension が共有の compile stage 1（69–72行）となる。

`lib/errors.srt` はファイルを作るだけでは埋込標準の共有 stage 1 に登録されない。ローカルの `lib/` を探索できる入口では、未登録ファイルも追加の標準 stage として拾われる場合がある（`crates/xldr/src/loader.rs:490–505`、`crates/xldr/src/lib.rs:535–539`）。全入口へ確実に供給し、共有 stage 1 から参照できるよう、`file_name: "errors.srt"`、`module_path: None`、`include_str!`、Main / Default の登録を追加する。`Bootstrap` の先頭 anchor は維持し、ファイルを include 経由で二重ロードしない。

Bootstrap の実コードは builtin / intrinsic 宣言で、共通 Error のコンストラクタを呼ぶ本体はない。共通 Error の参照は宣言自身か `@doc` の例にある。Sigil は全 stage の解決結果を連結し、Scar は全体を事前宣言してから一回の `check_program` で検査する。このため調査時点では5件を Main の errors.srt へ移す案が成立する見込みと判断した。修正後の実行検証は第8節に記録する。

同一 stage の import 可視性は「前 stage + 同一 stage」で、ファイルの登録順によらず参照できる。後 stage 参照を救済する経路は追加しない。根拠: `docs/dev/Xldr_spec.md:75–84`、`crates/sigil/src/resolver/mod.rs:464–486`、`crates/scar/src/checker/mod.rs:1539,5140–5161`。

### 宣言 identity と import

トップレベルの `deferror` は Global の宣言になるため、物理ファイルを移してもトップレベルのままなら canonical identity は維持できる（`crates/sigil/src/resolver/declarations.rs:629–636,3051–3074`）。`NoneError` 等を新たに `Errors::NoneError` と書き換える必要はない。`deferror` は module member の許可リストに含まれず、`defmod Errors` へ入れる構文は使えない（`crates/spire/src/parser/context.rs:152–170`）。登録表の `module_path` は `Option<&'static str>`、ロード後の stage 情報は `Option<String>` とし、バンドルの名前なしを `None` のまま保持する。ソースの出典はファイル名と SourceId で表す。

trait 実装の lowering は対象型の owner を使う一方、import 環境は元のファイルから引き継ぐ（同ファイル596–617行）。移動時は名前解決に必要な明示 import / autoimport と helper 参照を確認する。import を無条件に全コピーしたり、解決失敗を別名への fallback で隠したりしない。

ファイル名・グループが変わると、REPL の宣言表示・doc の出典表示も変わり得る。`Bootstrap::NoneError` 表示を扱うテスト内サンプルは `crates/xldr/src/lib.rs:1702–1734` にある。これは実際の標準ファイルを読むテストではないため、移動だけで期待値変更が必須になるわけではない。宣言 identity の維持と実際の表示所属を分けて確認する。同様に `crates/sigil/src/resolver/tests.rs:1775–1795` はテスト内で NoneError を stage 0 に置く汎用テストであり、標準配置の移動を理由に stage 判定の契約を書き換えない。

### 変更しない内部契約

Surtr ファイルの並べ替えは `BUILTIN_METAS` の定義順や Eldr の builtin 実装対応を変える理由にはならない。スキーマ・VM バージョンも上げない。source / function / declaration index は変わり得る。標準 cache key はファイル名、module path の有無と値、stage、variant、本文と登録順を含む（`crates/xldr/src/loader.rs:605–625`）ため、既存の指紋・再生成経路で検証する。

## 7. 実施手順と受入条件

1. 冒頭の確定した修正方針を適用する。
2. errors.srt と標準登録表を整合させ、共通 Error を元ファイルから削除して移す。Bootstrap anchor と Global identity を確認する。
3. 9件の trait 実装を対象型のファイルへ移し、移動元の実装を削除する。
4. Mapper / Predicate / Reducer を `function.srt` のトップレベルへ移し、主要型→固有定義→trait 実装→補助宣言→Error の順序にそろえる。各所有者内の関数順もそろえ、長い Error body は上記の方針で折り返す。
5. 既存の正本文書・出典表示を必要な箇所だけ追従する。標準 API の説明内容を配置整理に合わせて書き直す必要はない。

受入条件は次のとおり。

- 64ファイルと新規 errors.srt について、対象宣言を取りこぼさず配置基準を満たす。
- trait 実装9件がそれぞれ移動先に一度だけ存在する。型属性の derive は保持する。
- Mapper / Predicate / Reducer が `function.srt` のトップレベルに一度だけ存在し、`defmod Function` より前に指定順で並ぶ。Kernel 側に旧宣言を残さない。
- 共通 Error の選定一覧と固有 Error の所有者が仕様と一致し、元の宣言が残らない。
- 長い Error body が適切に折り返され、文字列の内容・補間、式、引数順、評価順を保持している。
- 89件の defp は各所有者の末尾にあり、3件の Extractor は公開関数の後にある。
- new / callbacks / def / hidden def / defextractor / defp の分類で、可視性・属性・署名・本体を変更していない。
- 同一区分の相対順、フィールド・variant・Payload の順序、公開 API、エラーの kind と内容を保持する。
- Default / TestEnabled、script / REPL / LSP が同じ登録表から新規ファイルを取得できる。重複登録や欠落がない。
- 定義バンドルの module path は登録・stage・parser・LS で `None` のまま扱い、仮名や空文字で補完しない。`None` と名前付きのソースで cache key を区別する。worker 起動失敗の診断も、名前なしならソースの失敗としてファイル名と SourceId を保持する。
- Error の観測・構築・Pattern、双方向変換、Json の Encode / Decode、公開関数から private helper への参照が従来どおり動作する。

実装時は影響する既存の標準テストから確認する。主な対象は `lib/tests/basic_types/{error,int,float,string,boolean,json,duration}.srt`、`lib/tests/traits/{convert,try_convert,encode,decode}.srt`、`lib/tests/monads/{option,result,either,result_t,either_t}.srt` と整理した各 module のテスト。その後、level1 の全体検証として `rtk proxy cargo run -- test --quiet --all` を実行する。

標準登録表を変更した場合は Sindr の登録契約、Rune の標準ロード、Xldr の doc / signature、LS の共通登録を直接検証する既存テストを選ぶ。merge 前の全体検証は `rtk cargo nextest run --profile ci --workspace` を使う。本修正では新規テストを追加せず、既存テストを追従して検証する。

追従先は `lib/bootstrap.srt` の bootstrap Error の説明、`lib/kernel.srt:475` の配置説明、`types/special_types.srt` の構成説明、`docs/dev/Xldr_spec.md` の標準ソース説明、`docs/dev/Error_spec.md`、`docs/dev/テスト方針.md`、必要なら `docs/dev/README.md` とする。`docs/site/standard-library.md:78–80` は trait 実装を型ファイルへ置く既存方針を説明しているため、今回の規則との整合を確認する。完全なロード一覧は `STDLIB_MODULE_SPECS` に一本化する。

## 8. 検証結果

未確定事項3点は冒頭の方針で確定した。

- 宣言・所有者内のメンバーを修正前後で照合し、属性、署名、本体、文字列トークン、フィールド・variant・Payload・引数の順序を保持していることを確認した。文書・コメント・空白と呼び出し末尾の任意のカンマは比較から除外した。
- 65ファイルの配置、trait 実装120件の所有者、具体 Error 203件の保持を確認した。89件の private 関数と3件の Extractor も指定の位置にある。
- ユーザ指定により仕様範囲の新規テストは追加しない。重複定義は既存の宣言検査で拒否されるため、共通 Error の重複を再検査するテストも追加しない。既存テストの期待値と対象範囲のみ追従する。
- `rtk proxy cargo run -- test --quiet lib/tests/basic_types/error.srt` と `rtk proxy cargo run -- test --quiet --all` は成功した。
- Xldr の既存標準ロードテスト16件が成功した。登録順序と stage 境界を保ち、モジュール名の一覧は名前を持つソースだけを検査する形に追従した。
- CI 全体検証の初回は2,445件成功・2件失敗だった。失敗した Scar の2件は Process / Agent の検証範囲を隣接モジュールとの文字列位置で切り出していた。AST の module span から対象定義だけを取得するよう追従し、旧配置順への依存を除いた。
- Astra に会話履歴を渡さず、仕様と最終差分を読み取りレビューしてもらった。製品コードの新規問題・デグレの指摘はなかった。登録から parser / LS までの `None` の保持、cache key の区別、Global Error の identity と出典、worker 起動失敗の SourceId を確認した。
- Kernel 全置換で拒否を検査する既存テストは、移動した Error が使う `inspect` も削除し、意図しない解決失敗を先に観測していた。Astra と相談し、元の最小 Kernel fixture だけを独立した stage で解決する形へ追従した。公開関数・署名・未知 builtin の診断期待値はそのまま保ち、標準依存や autoimport 衝突を混ぜない。
- `Option` 化後の `rtk cargo nextest run -p scar --test typecheck_surface typecheck_surface_bucket_` は12件成功、`rtk proxy cargo run -- test --quiet --all` も再実行して成功した。
- Astra は最終の最小 stage による既存テスト追従も確認し、追加指摘なしとした。新規テストや余分な製品コード変更が残っていないことも確認した。
- 新規テストを除いた全体 CI は2,445件成功・1件失敗だった。既存の `process_worker_reply_later_outer_timeout.srt` が `Ok(2)` に対して `Ok(1)` を返した。対象 bucket の単独再実行は成功した。
- この失敗も Astra に独立した読み取り調査を依頼した。fixture は handler の開始を確認せず1msの期限を設定しており、既存の未開始 callback の取消仕様では状態が更新されず `Ok(1)` になり得る。VM・continuation・製品側 Forge に今回の差分はなく、配置変更によるデグレを示す証拠はなかった。ただし失敗時の開始 trace がないため、この機序が今回の原因かは未確認。fixture と runtime は変更していない。
- 最終の `rtk cargo nextest run --profile ci --workspace` は2,446件すべて成功した（67 binaries、106.027秒）。
