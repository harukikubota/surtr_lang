# Lazy special formの実装契約

`Lazy<T>` は標準定義が宣言する引数契約のマーカーであり、通常の値型ではない。標準 special form に加えて、標準 Trait メソッドの引数にも宣言できる。

Sigilは通常の名前解決でcallee UIDを確定し、対応する標準宣言のcanonical identityからspecial formを選ぶ。autoimport属性や呼出しの綴りだけでspecial formへ分類しない。callee解決が失敗した場合は元の診断を返し、通常呼出し・パイプのどちらでも綴りによる救済を行わない。`and` / `or` は予約名で、利用者の独立した関数・member 定義も拒否する。`if`、`if_then`、`require`、`ensure`、`map_err`、`cause` は通常名で、同名の利用者関数・関数値変数は自身の署名と通常の引数評価に従う。Pattern consumer の予約規則は別に維持する。
利用者の独立した関数・Trait は Lazy 引数契約を宣言できない。標準 Trait のユーザー impl は、対応するメソッドの引数位置に限って Lazy 契約を継承できる。宣言の identity で対応付け、メソッド名の綴りや実装元では許可を決めない。default method と override の呼出しは同じ宣言契約に従い、実装本体へ通常の0引数 callable を渡す。戻り値・型注釈・field・container の値型としては公開しない。
builtin の signature は `crates/sindr/src/builtin.rs` の `BUILTIN_METAS` と対応する標準定義を正本とする。Trait メソッドの Lazy 引数位置は、解決された標準 Trait 宣言を正本とする。
利用者向けの規則と関数別の例は[Lazy evaluation](../site/lazy-evaluation.md)、標準APIの説明は`lib/kernel.srt`・`lib/types/result.srt`の`@doc`に置く。

## フェーズの責務

| フェーズ | 責務 |
|---|---|
| Spire | callの引数領域、grouping、capture placeholderを保持する。Patternを通常Exprへ解析し直して救済しない |
| Sigil | canonical callee・Pattern binding・deferrorのidentityとlexical scopeを確定する。builtin の裸 Lazy capture を拒否し、診断用の由来を生成parameterへ保持する |
| Scar | Lazy入力の型とclosure shellを正規化し、生成parameterの要求型・callの戻り値型を確定する。Trait の宣言権限・override の対応と裸 capture を検査し、通常の型関係を維持する |
| Forge | 照合・branch選択前のeager入力を一回評価し、選択されたbranchの正規化済みshellを一回consumeする。tail位置でも同じ順序を守る |
| Eldr | 通常のcall frame・closureを実行する。Lazy値は公開しない。ErrorKindは独立した通常値として運搬する |

概念上のwrapとruntime closure allocationは別である。Forgeは確定した評価順と型を実行し、すべてのbranchにclosure allocationを要求しない。
実行式のbranchへLazyマーカーを残したり、正規化済みparameterへ再びLazy処理を適用したりしない。

## 入力とclosure depth

Scarの`LazyInput`は、型検査済みのnode、直接placeholderかどうか、裸callの概念的なshellを保持する。

- 裸callは戻り値型によらず一段包む。callが0引数関数を返しても、callの実行はbranch選択まで遅延する。
- Lazy引数位置の`(EXPR)`はeager境界である。式を選択前に一度評価し、その結果を正規化する。関数値の取得と本体の実行を区別する。
- 直接placeholderとそのgrouping（`&N`・`(&N)`）は同じ仮引数参照である。固定式のeager境界を再適用せず、追加のwrap・callも行わない。
- `match` arm内の括弧はそのarm内のgroupingであり、Lazy引数のeager境界ではない。

depthは型の先頭に連続する0引数関数の段数である。引数付き関数型は基底型として扱う。

| 入力型 | depth | 基底型 |
|---|---:|---|
| `Int` | 0 | `Int` |
| `(-> Int)` | 1 | `Int` |
| `(-> (-> Int))` | 2 | `Int` |
| `(Int -> String)` | 0 | `(Int -> String)` |

裸callの概念的なshellは、上記の戻り値型のdepthに加えて扱う。
例えば`make()`が`(-> Int)`を返す場合、Lazy位置の`make()`はdepth 2、`(make())`の評価結果はdepth 1となる。

## 正規化と一回のconsume

既知の両branchは次の規則で共通型へ揃える。

1. 同型かつdepth 0なら、両方を一段wrapする。
2. 同型かつdepth 1以上なら、そのまま使う。
3. depthが異なる場合は、浅い側だけを一段wrapする。
4. その後は通常の型関係で整合を検査する。二段以上の差や基底型の不一致を追加wrap・暗黙変換で隠さない。

正規化後の共通型が`(-> R)`なら、選択branchを一回呼び、callの戻り値を`R`とする。
`R`がさらに関数型でも追加で呼ばない。浅い側を一段wrapした場合は、その一回のconsumeで元の値を返す。
単branchの`if_then`と、bindingを作らない`if_let_then`の成功branchは`(-> Unit)`を要求する。bindingを作る成功branchはDirectExpressionとして`Unit`を要求する。
`require`・`ensure`・`Result::map_err`・`Result::cause`のerrorは`(-> Error)`を要求する。
`and`・`or`の右辺は`(-> Boolean)`を要求し、それぞれ左辺が`True`・`False`のときだけ実行する。
個別の評価条件は標準 API の契約に従う。Error は通常の値として受け渡す。

## キャプチャの要求型

直接placeholderには、正規化済みのbranch型を要求する。placeholder自体を一段wrapして型不一致を救済しない。

| branchの組み合わせ | placeholderの要求型 | callの戻り値型 |
|---|---|---|
| `&1, 1` | `(-> Int)` | `Int` |
| `&1, {|| 1}` | `(-> Int)` | `Int` |
| `&1, {|| {|| 1}}` | `(-> (-> Int))` | `(-> Int)` |

既知branchの型とdepthが優先する。外側の注釈は型の整合に使い、既知branchの評価方法を変更しない。
両branchが未知なら期待される関数型から確定する。local binding・REPL行の完了時に未確定signatureを保存せず、後のcallから決め直さない。
外側の宣言済みrigid genericを保持することと、未確定型をlocal polymorphic schemeへ一般化することは区別する。

固定式の事前評価境界 `(EXPR)` では、キャプチャの生成引数が導入される前のローカルコンテキストを参照する。この境界内から今回の `&N` を参照した場合は拒否する。型不一致や追加の thunk 化で救済しない。直接置換の `&N`・`(&N)` はこの境界ではない。Lazy 引数位置以外の grouping と Pattern の事前 Expr に、この禁止を広げない。
実行時の評価はキャプチャの呼び出しごとに分岐選択前に一回行い、キャプチャ生成時へ移動しない。

同じ番号のplaceholderは一つのparameterであり、通常Expr内の使用を含むすべての要求型を統一する。
`&and(&1, &1)`の`Boolean`と`(-> Boolean)`は競合する。番号の並べ替えは生成parameterの順序へ反映する。
裸の標準Lazy capture（`&and`など）と標準 Trait の Lazy メソッドの裸 capture は拒否し、引数を記述したcaptureへ案内する。暗黙wrapperは生成しない。
既存の関数値に対する`&f`は同じ値の参照であり、通常のidentity規則を維持する。

生成された関数は通常のcall frameを使う。引数は評価済みの値として渡され、branch用の関数値を取得したことはその本体を実行したことを意味しない。
利用者の wrapper も通常の `(-> T)` を受け取る。標準 Trait の対応する override 以外では `Lazy<T>` を利用者の signature に書けない。
`(-> Error)` を含む生成関数も通常の callable として受け渡せる。Error を返すことだけを理由とする制限はない。

## PatternとErrorKindの境界

Error の宣言 identity と通常値の制約は [Error spec](Error_spec.md) に従う。本節は Pattern と marker を受け取る API の構文・capture 境界を定める。

Pattern consumerの完全call capture、固定Pattern、bindingを伴う成功branchのDirectExpression、照合前scopeのeager式は[Pattern spec](Pattern_spec.md#pattern-consumerのキャプチャと成功scope)に従う。
Pattern自体・DirectExpressionBlock自体をplaceholderで置き換えず、特殊ブロック内への新しいplaceholder許可やpipeの引数注入規則を追加しない。
`&N`、projectionの`_N`、pipeの`_N`はそれぞれの構文・役割を維持する。

`Result::recover_kind`、`Test::assert_err_kind`、`Test::assert_cause_chain` の ErrorKind 入力は通常値であり、Lazy 入力ではない。動的な値・List・spread・capture placeholder は通常の型規則で扱う。値生成と Error 構築の境界は [Error spec](Error_spec.md) に従う。回復関数の handler 生成式は通常どおり eager に評価し、handler 本体は失敗時（kind 指定なら一致時）だけ呼び出す。
Sigil は修飾名を含む裸の canonical `deferror` 参照を ErrorKind 値生成へ解決する。入力 arity と保存 Payload arity は値生成に関係しない。Forge は ErrorKind 定数を生成し、Eldr は専用の通常値として運搬する。constructor call と Error instance は Error 型、文字列は String 型であり、ErrorKind とは通常の型検査で区別する。値表現と検証の詳細は [EldrVM spec](EldrVM_spec.md) に従う。

## 診断と検証

関数別の修正案、由来の保存、通常のreason・構造化入力の保持は[診断契約](diagnostics.md#lazy・patternキャプチャとerrorkind)に従う。
型と評価の境界は`crates/scar/tests/lazy_capture_revision.rs`、関数別案内は`crates/scar/tests/lazy_capture_diagnostics.rs`で検証する。
評価回数・grouping・短絡は`tests/fixtures/script/pass/functions/lazy_capture_normalization.srt`、consumer captureは`tests/fixtures/script/pass/patterns/consumer_capture.srt`、ErrorKindの許可・拒否は対応するscript/module fixtureで検証する。
REPLのsignature確定と診断由来の継続は`crates/xldr/tests/repl_core.rs`で確認する。
