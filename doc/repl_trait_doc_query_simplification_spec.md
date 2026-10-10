# REPLのdoc参照とクエリ簡素化の修正方針

2026-10-10。調査・仕様案。製品コードと実行可能テストは未変更。

## 目的と確定条件

トレイト実装の説明を書くためだけに、deriveで表せる実装を手書きする必要をなくす。
REPLではトレイト定義側の説明を参照し、入力クエリから引数の解析と実装の選択を外す。

- トレイトについてREPLが参照する`@doc`は、定義本体と定義内メソッドに付いたものだけとする。
- 通常の`impl Type`のpublicメンバー、型、module、processなどのdoc参照を維持する。
  ただし、通常関数・ユーザー定義メンバーについて、対象宣言にないdocを別宣言の本文で補うフォールバックは除去する。
  ユーザーがdocを付与できないprocessの生成メンバーは、既存のhidden標準関数docへの対応を維持する。
- トレイト実装と実装メソッドの`@doc`は引き続き付与できる。将来のHTML化では収集するが、HTML化のない現時点では収集しない。
- `:doc`・`:sig`・`:info`は引数による指定を受け付けない。`Facet.User`は最短の参照経路として維持する。
- deriveに移行できる標準実装は移行する。BooleanのEqは、コンパイラがcanonicalな宣言を識別して既存builtinへ接続する。
- `@doc`の付与条件、private拒否、重複拒否、triple-quoted文字列と補間禁止を変更しない。

HTML生成機能、derive対象Traitの追加、型の可視性変更、一般の名前解決・評価規則の変更は対象外。
VM・保存スキーマのバージョンは上げない。

## 現状と根拠

| 対象 | 現行実装と課題 |
|---|---|
| 共通クエリ | `crates/surtr-analysis/src/query.rs:10,132`に`Symbol`・`FacetRootDoc`・`FieldPath`・`TypedCall`・`OperatorTarget`がある。Xldrは`crates/xldr/src/repl/logic/query.rs:1`から再exportする。 |
| REPLの解釈 | `crates/xldr/src/repl/logic/core.rs:2903,4780,4922`で3コマンドが型付き呼出しと演算子の実装指定を処理する。 |
| 実装doc収集 | `crates/sigil/src/semantic_metadata.rs:621`で`TraitImplDef`本体とメソッドのdocを収集し、通常コンパイル・REPL・分析に共有している。現時点でも実装docは収集されている。 |
| 定義側の個別doc | 同ファイル`:480`はTrait本体のdocを各メソッドに複製し、メソッド自身の`attrs.doc`を無視している。Trait本体にdocがない場合はメソッドdocも収集しない。 |
| 個別docの許可 | `crates/spire/src/parser/tests.rs:1088`でTrait本体にdocがなくてもメソッドdocを許可する。`lib/traits/operator/compare.srt:17`以降にも個別docがある。 |
| 通常メンバー | `crates/sigil/src/semantic_metadata.rs:538`の`ImplDef`のメンバーdocはトレイト実装とは別の経路。これは維持する。 |
| メタデータ利用者 | Runeの`crates/rune/src/compile.rs:583`、Xldrのlive/preload収集、分析の`crates/surtr-analysis/src/service.rs:1129`が同じcollectorを使う。 |

これらはソースと既存テストの読み取りによる確認であり、今回の実行検証結果ではない。

## doc収集・表示の変更

### トレイト定義

Trait本体とメソッドの収集を分離し、docを共有しない。
Trait本体は本体自身の`@doc`だけを、メソッドはそのメソッド自身の`@doc`だけを使う。
メソッドに`@doc`がなければ、ドキュメントがないと案内する。
Trait本体や具体的な実装のdocで補う経路は設けない。

`Compare`はTrait全体の説明、`Compare::compare`はメソッド個別の説明を表示する。
bare helperと演算子は現在のcanonicalなTrait・メソッドへの対応を維持する。
通常のスコープ可視性、bindingのshadowing、private宣言の拒否も維持する。

### トレイト実装

現行のdoc collectorから`TraitImplDef`本体とメソッドのdoc収集を除去する。
加えて`sigil/src/resolver/declarations.rs:620`のTraitImpl loweringによる
`module_doc: attrs.doc`も除去し、`module_doc`は`None`にする。
この経路を残すと、実装docが通常collectorのModule entryとして収集されてしまう。
ASTの`attrs.doc`は保持し、parserの許可条件は変えない。
将来HTML化を実装する際、そのソース収集経路で実装docを扱う。
今回、未使用のHTML用モードやcollectorは追加しない。

同じcollectorを使う分析・LSP向けメタデータも、実装固有のdocを収集しなくなる。
宣言・実装そのものの名前、型、定義位置、signatureは引き続き必要なので削除しない。
`collect_signature_entries`の`TraitImplDef`収集はdoc収集から独立して維持する。

通常の`impl Type`メンバー自身のdoc、bindingから型docへの参照、process本体・メンバー自身のdocを維持する。
`Boolean::not`、`Duration::new`、`Duration::deconstruct`などの参照先は変えない。
`MyServer::pid`など具体的なprocessの生成メンバーは、hidden標準関数の本文を表示する経路を維持する。

### 通常関数・プロセスの追加調査

通常関数・プロセスについても、doc欠如を別宣言で補う経路がないか調べた。
以下はソースと既存テストによる確認で、実行再現は行っていない。

| 経路 | 現状と修正対象 |
|---|---|
| function capture | `core.rs:4035-4052`の`capture_doc_entry`は、元の`module::name`にdocがないとbare nameでも検索する。`matching_doc_entries`（`:4137`）は宣言identityや可視性を検証せず末尾名で集めるため、別moduleの同名関数docを返し得る。bare nameの救済を削除する。 |
| script preload | `core.rs:3333-3334`は、同じ名前のUIDが存在するだけでもscript由来docを許可する。`:3294`のpreload検索も末尾名を使う。現在選択された宣言とUID／canonical名が一致するdocだけを参照し、同名のpreload docで再定義後の関数や未解決名を補わない。 |
| 具体的なprocessの生成メンバー | `core.rs:3032,3513-3537,3561-3576`は、`Agent`・`GenServer`・`Supervisor`のhidden関数docを使い、表示symbolとsignatureだけ具体的な名前へ差し替える。ユーザーがdocを付与できない生成メンバーの参照経路として維持する。 |

通常`Def`・`BuiltinDecl`（`sigil/semantic_metadata.rs:421`以降）と`impl Type`のメンバー
（同`:538`以降）は、各宣言自身の`attrs.doc`だけを収集している。
module本体・型本体のdocを通常関数へ複製する収集経路は見つからなかった。
プロセス本体のdocはloweringで`module_doc`へ移し、メンバーのdocとは別に扱う。
`@call`・`@get`などのhandlerに付いたdocは、現行loweringで内部handler側に残り、
生成public wrapperへは転写されない（`spire/src/parser/decl.rs:572-588,755-761,841,1354`）。
これは本文フォールバックではない。今回wrapperへdocを新しく転写する変更は加えず、
各lowered宣言自身のdoc収集を維持する。wrapper自身にdocがなければdocなしと案内する。

通常関数の修正は、先に現在のスコープで対象宣言を確定し、その宣言自身のdocだけを引く形とする。
同じcanonical宣言への`Global::`正規化、import経由の参照、preloadで保存された同一宣言の参照は維持する。
名前が未解決なら未解決と案内し、別の関数のdocで成功扱いしない。
対象宣言にdocがなければ、ドキュメントがないと案内する。

captureも元のcanonical宣言のdocだけを参照する。`printer = &print`から`Kernel::print`を引く参照は維持するが、
元宣言にdocがないときに別moduleの同名関数を選ばない。

ユーザーがdocを付与できないprocessの生成メンバーには、既存のhidden標準関数docを表示する。
対象は`MyServer::pid`、workerの`spawn`、Supervisor系の`spawn`・`adopt`・`status`・`workers`である。
表示symbolとsignatureは具体的なprocess名に揃える。docを付ける新しい構文は設けない。
この対応を通常関数やユーザー定義メンバーへ広げず、それらにdocがなければドキュメントがないと案内する。
`GenServer::spawn`などhidden標準関数を明示名で引く場合は、その関数自身のdocを表示する。
具体的なprocessのdoc alias、signature生成・参照、`:sig`の既存機能は維持する。

closureから`Closure`、extractor closureから`ExtractorClosure`、non-callable bindingからその型のdocを引く参照は、
宣言のdoc欠如を補う処理ではなく、bindingの種別・型の説明として維持する。
通常のcaptureを、docがないという理由でこれら別の説明へ流す救済は設けない。

## クエリの形

共通parserは式や型引数を解析せず、次の字句形だけを受け付ける。

| 形 | 例 | 扱い |
|---|---|---|
| 名前・修飾名 | `Compare`, `compare`, `Compare::compare`, `Boolean::not`, `User::new` | 維持 |
| callable名のsuffix | `predicate?`, `dbg!`, `User!` | 現行の名前規則とowner extractor参照を維持 |
| 公開演算子・特殊形式の固定symbol | `==`, `/`, `|*>`, `(,)`, `=`, `=?`, `Kernel::=?`など | 現行の公開symbolの集合を維持。一般式にはしない |
| Facet root | `Facet.User` | 維持。`:doc`はroot説明、`:sig`は既存の案内、`:info`は既存の型照会 |
| field path | `User.field` | 現行の案内を維持。`:info`でもfield情報の新規表示はせず、`:facet User.field`へ案内 |

型・bindingの引数指定はすべて拒否する。
`compare(Int, Int)`、`Compare(Int, Int)`、`Boolean::not(Boolean)`、`|*> Option`、
generic型のクエリ、literal、capture、一般式をsymbol扱いして継続しない。

空引数の`User()`・`User!()`もcall形式として廃止する案とする。
型docは`User`、constructor docは`User::new`、extractor docは`User!`へ統一する。
既存の`:doc User()`と`:doc User`は参照先が違うので、前者を単純に`User`へ置換しない。

`TypedCall`・`OperatorTarget`とその専用構造体・引数分割・型指定解析・診断理由を削除する。
`FacetRootDoc`・`FieldPath`は引数解析なしで扱えるため維持する。
現行の「空白のない任意文字列をSymbolにする」判定は、名前の字句検証へ置き換える。
文字単位の診断spanは維持する。

`User!`を内部で空引数`TypedCallQuery`に変換する経路も削除し、owner extractorへ直接接続する。
`:sig`の定義signature、Trait family、process owner一覧などは維持し、
入力の引数型からのspecializationと実装選択だけを廃止する。
`:info`の通常の定義・binding照会も維持する。

`parse_signature_type`、`parse_binding_query_type`、`format_query_ty`はsignature表示や分析でも使う。
クエリ引数解析の廃止に巻き込んで削除しない。

## 標準定義のderive移行

| 型 | 移行するTrait | 根拠・条件 |
|---|---|---|
| `Duration` | `Eq`, `Compare` | `lib/types/duration.srt:83-109`の唯一の`millis: Int`に対する比較は構造比較・宣言順比較と一致する。private fieldを扱うderiveの既存権限を使う。 |
| `Range<$A>` | `Eq`, `Compare` | `lib/types/range.srt:80-111`の境界比較を生成実装へ移す。必要な要素Trait条件は既存deriveが生成する。 |
| `Boolean` | `Eq` | `lib/types/boolean.srt:14,88-96`。payloadなしenumの等値比較。現行builtinを使うコンパイラ経路を維持する。 |

`Range`の現在の`neq`は要素側の`Eq::neq`を呼ぶが、deriveの`neq`は生成した`eq`本体の反転になる
（`crates/sigil/src/resolver/derive.rs:690-711`）。要素型が独自の`neq`をoverrideすると結果が変わり得る。
ユーザーのderive移行了承を受け、この差を移行後の契約として明記する。derive側へ特例は追加しない。

削除する手書き実装の型固有の説明・例は、各型の既存`@doc`へ統合する。
新しくderiveへdocを付ける構文は設けない。

次は手書きを維持する。

- `Duration`と`Boolean`の`Show`：現行の`"250ms"`・`"True"`はderiveの構造表示と異なる。
- `Result`の`Eq`：`Err`を`Error::same_kind`で比較する契約があり、payloadの構造比較では代替できない。
- `@builtin type`やTupleなど、現行deriveの対象となる型宣言を持たないもの。
- derive registryの`Eq`・`Compare`・`Show`・`Default`以外の実装。

### Booleanのbuiltin接続

deriveで実装を宣言したことと、実行先をbuiltinへ最適化することを分離する。
標準の手書き`impl Eq for Boolean`は削除し、canonicalなBoolean・Eqの生成実装だけを
既存のTrait dispatch overrideへ接続する。

現行の接続入口は`crates/scar/src/checker/predeclare.rs:3845`の`impl_method.is_builtin`条件、
共通の選択とsignature検証は同ファイル`:2415`の`trait_dispatch_override`にある。
`crates/sindr/src/builtin.rs:188,192`が既存Boolean `eq`・`neq`の対応を持つ。
これを使い、他クレートにbuiltin名やIDを直書きしない。

推奨する追加条件は、既存の生成由来情報`generated_derive`、canonical Eqを示す
`trait_info.compiler_owned_equality`、canonical target headの`TypeName::Boolean`の組である。
`eq`と`neq`の両方を既存metadataの対応へ接続する。
Sigilの未解決ASTに型名の綴りで`builtin`属性を付ける方法は採らない。

型やTraitの綴りだけで選択しない。通常のユーザー型、別のTrait、他のderiveには適用しない。
必要なmetadataがない・signatureが合わない内部状態ではエラーにし、生成関数への救済を設けない。
既存のBoolean比較builtin、`EqBool`・`NeqBool`は維持する。新規builtin・Opcodeは不要。
`eq`・`neq`、演算子、qualified call、function captureで同じ能力と実行先を使うことを検証する。

## 実装順序と正本の追従

Boolean生成実装のdispatch契約が処理系に及ぶため、実装時はlevel 4とする。
当初のREPL中心のlevel 3から、この追加条件に合わせて引き上げる。

1. `docs/dev/Xldr_spec.md`のdoc参照・許可クエリ・specialization例を整合させる。
   具体的なprocessの生成メンバーへのhidden本文流用規則（`:197,252`）は維持する。
   `docs/dev/Trait_system_spec.md`へBooleanのderiveとbuiltin接続、標準derive移行の契約を反映する。
   `docs/site/function-operators.md`の実装指定クエリ例も追従する。
2. fixture内の短いdocで、定義側と実装側の識別・本体とメソッドのdocの独立性・実装doc非収集をテストする。
   標準doc本文や有無を固定するテストは作らない。parserの付与条件テストは維持する。
3. 共通query parserと3コマンドの旧引数経路を削除する。
   実装docのrank・本文文字列からの推定・型単一化・specializationなど、専用resolverも削除する。
   help・補完・分析の公開APIテスト・CLIのクエリ例を追従する。
   通常関数のcapture・preloadの別宣言検索を除去する。
   processの生成メンバーのdoc・signatureに必要なalias解決は維持する。
   `crates/xldr/tests/repl_core.rs:5685`と`tests/integration/repl.rs:1348`の
   生成メンバーへのhidden本文流用を期待するテストは維持する。
4. Booleanのcanonical dispatchを整備してから、対象の標準定義をderiveへ移行する。
   各型のdocと既存の比較テストを追従する。
5. 対象検証、全体検証、別エージェントによる最終レビューを行う。

`DocEntry`や`.eldr`の形式変更、schema/VM version変更は必要ない。
stdlib cacheはcompiler build keyと標準ソースを材料にしている
（`crates/xldr/src/lib.rs:1007`、`crates/xldr/build.rs:101,162`）。
実装後はcold/warm・live/preload経路で新しい収集条件が一致することを確認する。

## 受入条件と検証

- `:doc Compare`はTrait本体のdocを返す。`:doc Compare::compare`・`:doc compare`・
  対応演算子は対象メソッド自身のdocを返す。実装側だけにdocがあっても、その本文をREPLへ出さない。
- Trait本体のdocがない場合でも、定義内メソッドの個別docを参照できる。
- メソッドにdocがなければ、Trait本体や実装側にdocがあっても、ドキュメントがないと案内する。
  メソッドのdocもTrait本体のdocとして使わない。
- 通常の型メンバー・型・processのdocと`Facet.User`の既存参照を維持する。
- docなし関数のcaptureに対し、別moduleの同名関数だけにdocがあっても、その本文を返さない。
- preloadのdoc付き関数をdocなし関数でshadowした場合、古い本文を返さない。
  同一宣言をbare名・修飾名・import経由で引く成功例は維持する。
- process本体だけにdocがある場合、docなしユーザー定義メンバーにはドキュメントがないと案内する。
  本体・各loweredメンバーのdoc収集は独立させ、handlerの本文をwrapperの本文として補わない。
- `MyServer::pid`・`MySup::status`など、ユーザーがdocを付与できない生成メンバーはhidden標準関数docを表示する。
  hidden関数を明示名で引くdoc参照、具体的なsymbol・signature表示も維持する。
  ユーザー定義メンバーへこの本文流用を広げない。
- 型指定付きTraitクエリ・通常関数クエリ・演算子の実装指定を3コマンドとも拒否する。
  空引数call形式も拒否し、constructorとextractorの名前による参照を成功させる。
- `@doc`の付与条件テストは従来どおり通る。実装docはASTにはあり、通常doc metadataにはない。
- `Duration`・`Range`の成功比較と必要な要素Traitの拒否境界を既存テストで固定する。
  Rangeの独自`neq`境界は移行後の生成契約を直接検証する。
- Booleanの等値・不等値がbuiltinへ接続し、通常型のderiveを同じ特例で処理しない。
- 補完・binding・signature表示に必要な型parserは残り、旧クエリの専用経路は残らない。

最小検証は変更箇所に合わせて次から選ぶ。

```sh
rtk cargo nextest run -p surtr-analysis
rtk cargo nextest run -p xldr
rtk cargo nextest run -p scar
rtk cargo nextest run -p forge
rtk cargo nextest run --profile cold -p rune --test integration repl
```

Spireは付与条件を変更しないため既存のdoc境界テストを選ぶ。
標準比較は`lib/tests/basic_types/`の対応テストから検証する。
最終的に`rtk cargo nextest run --profile ci --workspace`と
`rtk proxy cargo run -- test --quiet --all`を直列で実行し、最終差分を別エージェントがレビューする。

今回行ったのは主担当と2サブエージェントによるソース・既存テスト・正本文書の読み取り調査。
テスト実行・性能測定・HTML生成の検証は行っていない。
クエリの形は調査担当が再点検した。その後、ユーザー指示によりTrait本体とメソッドのdoc共有を禁止し、
メソッド自身にdocがない場合の案内を仕様と受入条件に反映した。
続いて通常関数・プロセスを2サブエージェントで調べ、通常関数の別宣言docへのフォールバックの除去を修正対象へ追加した。
プロセスの生成メンバーはユーザーがdocを付与できないため、ユーザー指示により既存のhidden標準関数doc参照を維持する。
この追加調査でTraitImplの`module_doc`経由の収集も確認し、実装doc非収集の修正対象へ追加した。
