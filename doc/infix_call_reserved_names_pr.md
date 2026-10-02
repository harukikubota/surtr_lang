# 予約名とPattern consumerの構文還元規則：変更提案

状態: 予約名・consumer構文と追加指示によるautoimport・grouping修正の実装・検証完了。2026-10-02の指示により、キーワード化に伴うimport制限は追加しない。完全Callのconsumer captureは現行通り許可する。

## 目的

結合優先度や短絡評価に関わる裸の中置Callをキーワードとして予約し、ローカルコンテキストによって構文の意味が変わることを防ぐ。予約する名前と、その名前に適用する構文規則は分けて定義する。

名前解決段階の規則、型規則、関数の呼び出し可能性は現行通りとする。標準関数の固定規則を前置Callやcaptureへ新たに追加しない。

Pattern引数を持つ標準専用consumerも今回の更改に含める。中置Callだけの改修に留めず、通常Call、backtick前置Call、修飾Call、pipe、予約名の使用位置、補完・不完全入力を含めて整合させる。

## 予約する名前と構文規則

優先度に関わる予約名は `on`、`and`、`or`、`eq`、`neq`、`lt`、`lte`、`gt`、`gte` の9個とする。これらと既存のPattern consumer予約名4個は、宣言の適格性を分けて扱う。キーワードであることを理由にimport規則を変更しない。

| 裸の中置Call | 適用する規則 |
|---|---|
| ``left `on` right`` | 現行の `StdOn` 優先度を維持する。`on` に対応する専用演算子はない |
| ``left `and` right`` / ``left `or` right`` | 現行の `AndOr` 優先度とRHSの短絡評価を維持する |
| ``left `eq` right`` / ``left `neq` right`` | 対応する比較演算子と同じ `Compare` 優先度で扱う |
| ``left `lt` right`` / ``left `lte` right`` / ``left `gt` right`` / ``left `gte` right`` | 対応する `<` / `<=` / `>` / `>=` と同じ `Compare` 優先度で扱う |

優先度の順序は現行通り `Bind < StdOn < Apply=Compose < AndOr < Compare < Pair < Expr` とする。既存の結合則も維持し、Compareの4関数には比較層の左結合を適用する。

現行パーサの `comparison_func_literal_name` は `eq` / `neq` のみを比較層に分類している。`lt` / `lte` / `gt` / `gte` を同じ層へ追加する点は変更となる。

## 優先度に関わる9予約名の使用制限

- 通常の変数束縛、関数引数、closure引数、パターン束縛、定数など、変数名として使用できない。
- フィールド名にも使用できない。予約名を使用するためのエスケープ構文は導入しない。
- モジュールやtrait/implの関数名としての定義は許可する。標準の関数定義・trait実装も維持する。
- importの適格性・同名衝突・autoimport規則は現行通りとする。予約名の単一・リスト・全件importに新たな禁止を設けない。

対象SpecialFormsはautoimportモジュールから既に提供される。autoimportは各ファイル先頭での全件importであるため、対象モジュールからの明示importは全件・単一・リストとも重複importとして拒否する。構文規則やキーワード分類では分岐しない。

## 修飾名と名前解決の境界

修飾名の中置Callは現行の規則を維持する。

```surtr
flag `MyMod::and` True
```

この式は通常の名前付き中置Callと同じ `Expr` 優先度・左結合で、通常の関数呼び出しとして引数を評価する。末尾が `and` であることを理由に、短絡評価や `AndOr` 優先度を適用しない。他の予約名についても、末尾の名前だけで特別扱いしない。

現行で特別扱いされる ``left `Function::on` right`` は `StdOn` 優先度を維持する。標準の `Kernel::and` / `Kernel::or` の短絡評価も維持する。

今回の中心はパーサの規則と予約名の使用制限であり、名前解決先を新たに固定する規則を追加しない。変数によるシャドーイングは禁止するが、許可された関数宣言・importの名前解決と呼び出し可能性は現行通りとする。

内側・外側スコープの呼び出し規則も現行通りとする。importが呼び出し可能性を変更する既存の仕組みを維持し、関数の定義を許可したことだけで呼び出し可能性を追加しない。

フェーズ間の依存を一方向に揃えることを目的とした再設計は行わない。言語仕様を保証するために、構文が標準の意味を直接扱う現行の設計を維持する。

## キーワード化しない関数

`compare` は通常の関数呼び出しであり、予約対象に含めない。

`pipe` / `fmap` / `bind` と、関数合成演算子の関数インターフェースも予約対象に含めない。これらの名前付き中置Callは通常関数と同じ `Expr` 優先度・左結合とする。

対応する演算子が持つ結合優先度やAST操作を、関数インターフェースの中置Callへ引き継がない。RHSへのLHS注入も行わず、通常の2引数Callとして扱う。演算子自体の規則は変更しない。

`if` など、引数を通常Exprとして解析でき、Lazy評価を呼び出し先解決後に処理できる関数は、その評価規則だけを理由に今回キーワード化しない。

## Pattern consumer全体の更改

### 対象と既存の意味論

Pattern引数を受け取る関数は標準専用とする。既存の `PatternConsumer` 契約を正本として、次の4consumerの予約構文をパーサで認識する。

| consumer | 完全Callの引数文法 | OR Pattern | projection |
|---|---|---|---|
| `is_match` | Expr, Pattern | root / nestedとも許可。通常bindとas aliasは禁止 | 禁止 |
| `apply_pattern` | Expr, Pattern | root / nestedとも禁止 | `_1`〜`_16`を許可 |
| `if_let` | Expr, Pattern, Expr, Expr | root / nestedとも許可。各alternativeのbind契約は現行通り | 禁止 |
| `if_let_then` | Expr, Pattern, Expr | root / nestedとも許可。各alternativeのbind契約は現行通り | 禁止 |

branch引数は構文上Exprであり、Lazy評価とPatternのbindスコープは既存の意味論を維持する。`if_let`系の成功branchだけがPatternのbindを参照でき、失敗branchやconsumerの外へbindを公開しない。入力は一度だけ評価する。Extractor Errorの扱い、`apply_pattern`のResult・projection形状、ORのbinding整合、入力Resultを自動unwrapしない規則も変更しない。

4consumerは既存通り、通常の宣言名・引数名・local bind・user member名として使えない。今回フィールド名も予約名として拒否する。標準の正規builtin宣言を維持し、9予約名の「ユーザーのmember関数として定義可能」という規則をconsumerには広げない。consumer名にもキーワード化を理由とするimport禁止を追加しない。Kernel consumerの明示importは、Kernelがautoimport済みであることを理由に拒否する。別identityへ解決されたconsumer構文はcanonical identity不整合として拒否し、通常Callへ再解釈しない。

### パーサと後段の責務

パーサは裸の予約consumerと正規の `Kernel::consumer` 表記から引数文法を選ぶ。関数のロード済みメタデータをParserContextへ供給する仕組みや、一般関数にPattern引数を追加する仕組みは導入しない。4consumerの引数位置・引数数・OR許可は共有の構文契約へ集約し、各Call経路で名前と数字を再列挙しない。

内部canonical名の `Global::` と利用者のソース表記を混同しない。正規表記の判定は既存のroot namespace・namespace規則に従い、今回 `Global::Kernel::...` を明示記述できるようにする変更は行わない。

consumerの外側では、Pattern位置を通常Exprとして解析してから再解釈せず、その位置でPatternParserへ切り替える。Expr位置にPatternしか成立しない構文があればエラーにする。consumer表記と正規identityの不整合もエラーとし、通常Callへの再解釈で隠さない。

Sigilは既存の名前解決とcanonical consumer identityの検証を担当する。Scarは照合対象の型を使ってconstructorの子、Extractor適用、projectionの型などを確定する。パーサへ完全な名前解決や型解決を移さない。

Pattern内部の `head(args...)` は別の境界である。named ExtractorやExtractorClosureでは、宣言・型情報により事前Expr引数と子Patternの役割が決まるため、現行の候補保持と後段での選択を維持する。consumer外枠の二重解析を整理することを理由に、`parse_pattern_argument` や `AstPatternArgument` を一律に削除しない。

### 呼び出し表記をまたぐ契約

| 表記 | 更改後の扱い |
|---|---|
| `consumer(value, pattern, ...)` | 完全Callの引数文法を直接選ぶ |
| `Kernel::consumer(value, pattern, ...)` | 同じconsumer処理へ接続する |
| backtick前置Call | 裸名・正規修飾名とも通常前置Callと同じ引数文法へ接続する |
| 2引数consumerの中置Call | LHSを第1 Expr引数、RHSを第2 Pattern引数とし、前置Callと同じconsumer処理へ接続する |
| `if_let` / `if_let_then` の中置Call | 2引数では足りないため拒否する。tupleによる引数包装や新しい部分適用構文は導入しない |
| `Regex::is_match` の通常・backtick前置・中置Call | 通常のExpr引数Call。末尾の名前だけでconsumerに分類しない |
| consumerの値参照・capture・placeholder capture | 値参照・bare capture・Pattern位置の直接placeholderは禁止。Patternを記述した完全Callのcaptureは現行通り許可する |
| `Regex::is_match` の値取得・capture | 現行の標準builtinの許可範囲を維持する |

Pattern consumerの中置Callは通常の名前付き中置Callと同じ `Expr` 層・左結合とする。ただしRHSはPattern文法で読む。PatternOrの `|` はPattern領域のみの演算子であり、Expr全体の優先度表には追加しない。Pattern領域を抜けたら外側のExpr文法へ戻る。

Patternの終端はPattern文法からパーサが判定する。`+` などのExpr演算子をPattern/Extractorのhead演算として追加せず、constructor・list・tuple・Extractor引数の区切りも既存文法に従う。PatternParserが受け取らない後続tokenは外側のExpr解析へ戻す。

`is_match`のRHSでは `Pattern (| Pattern)*` をPatternOrとして還元するため、裸のORを囲む追加括弧は要求しない。例えば ``value `is_match` Ok(_) | Err(_) `and` flag`` は ``is_match(value, Ok(_) | Err(_)) `and` flag`` の構造となる。後続の `+` などもPattern内部へ取り込まない。外側のExprとして型が合わなければ通常の型エラーとし、Patternへ再解釈しない。

### PatternOrとas-patternの結合

Patternの構文は次の形とする。ここで `App` は `PatternExpr` を指し、通常Exprの関数Callと同じ文法を意味しない。

```text
App = PatternExpr
PATTERN = (App (PATTERNOR App)*) (@ Var)?
```

as-patternはPatternOrより低優先度である。`p1 | p2 @ whole` は `(p1 | p2) @ whole` の構造となり、ORの各branchへ `@` を分配しない。文法の各階層でasは任意の1個とし、同じ階層で複数の `@` を連続させる規則は設けない。

最外のasは照合対象となるExprの値全体を束縛する。ORの各branchも同じ入力値を受け取り、いずれかのbranchが成功した後で、その入力値を共通のaliasへ束縛する。branchごとに同じaliasを記述する必要はない。ここでExprを受け取るとは照合対象の値を指し、`@` の右辺を一般Exprへ拡張する意味ではない。右辺は `Var` とする。

入れ子のPatternでは同じ規則を再帰的に適用する。子Patternのasは、その子ノード位置に渡された値を束縛する。例えば `Some((p1 | p2) @ child) @ whole` は `child` にSomeのpayloadを、`whole` に照合対象全体を束縛する。既存のalias型注釈を扱う場合も、この結合範囲を変えない。

consumerごとの禁止は維持する。`is_match` は最外・入れ子とも通常のas aliasを拒否する。`apply_pattern` のprojection aliasは子位置または全体の値を選ぶ既存の規則を使い、OR禁止を解除しない。一般のPatternでは成功前のaliasを外部へ公開しない。

PatternOrと中置Callの改行規則は通常演算子に揃える。演算子の前後で改行を受理する位置と文の継続・終端は通常演算子と同じ規則に従い、PatternOrやconsumer固有の改行継続は設けない。pipe演算子系列の既存の改行に関する特別扱いだけを維持し、PatternOrや中置Callへ転用しない。前置Callの括弧内の改行など、既存の区切り内の規則は維持する。

### pipeと構文コンテキスト

pipeによる引数注入は既存の演算子にだけ適用する。通常関数の `pipe` / `fmap` / `bind` へ注入コンテキストを渡さない。

引数注入を持つ `|>` / `|*>` / `|>=` の責務はRHSの最外Callへの共通の引数操作だけとする。`|*|` は文脈内callableとvalueの適用であり、引数注入しない。直接引数にpipe placeholderがあればその位置へ、なければ先頭へLHSを挿入する。呼び出し先の引数数や、パラメータがExpr・Pattern・LazyのどれであるかによってAST操作を分岐させない。引数数から省略形・全引数形を選び直す経路も設けない。

挿入位置の適格性検査は、この共通操作とは分ける。挿入先がLazyやPatternなどの注入禁止位置なら拒否する。先頭挿入で引数が過剰になった場合や、placeholder置換後も引数が不足する場合は引数Mismatchとして拒否し、呼び出し先に合わせて別の挿入方法へ切り替えない。

consumerのパーサは共通の挿入位置から引数の対応を認識し、自身の構文契約に従ってPattern位置を解析する。例えば `value |> apply_pattern([_, _1])` はplaceholderのない最外Callなので先頭挿入となり、記載されたlistは第2引数のPatternとなる。`value |> if_let(Ok(x), x, fallback)` も同じ共通規則で先頭挿入する。ここでPattern文法を選ぶ責務はconsumerのCall解析にあり、pipeによるパラメータ別のAST操作ではない。

この操作と挿入位置の情報はRHSの最外Callだけに適用し、入れ子のCallへ伝播しない。次の例では先頭挿入後に `Boolean::eqv` の2引数が揃い、内側の `is_match` は完全Callとして解析する。

```surtr
True |> Boolean::eqv(is_match(Ok(1), Ok(_)))
# Boolean::eqv(True, is_match(Ok(1), Ok(_)))
```

placeholderがある場合のslot数・番号の既存制約は維持する。Pattern内部のprojection `_1` は最外Callの直接引数slotではないため探索しない。入れ子のCall、Pattern内部、Extractor事前Expr引数へslot探索を広げない。直接引数のplaceholderの挿入先がPatternまたはLazyなら拒否する。欠けたPattern引数の補完も行わない。

Patternコンテキストは領域に入って抜ける状態とする。consumerの入力・branch、Pattern内部の事前Expr引数、通常Call、後続Exprへ漏らさない。OR許可やprojection許可も対象領域だけに適用し、Expr内の別の `match` のarmなどへ外側consumerの禁止を伝播させない。

consumer自体のcapture禁止と、外側の通常関数のplaceholder captureがconsumer内部のExprを含むことを区別する。後者の既存の `&1`〜`&16` 探索は維持し、Patternのprojection `_1` と混同しない。

## 受入条件

以下の受入条件は、parserテストとscript/module fixtureで検証した。

| 入力・条件 | 期待する結果 |
|---|---|
| ``1 `lt` 2 + 3`` | `lt(1, 2 + 3)` の構造となる。`lte` / `gt` / `gte` も同じ比較層に分類する |
| ``False `and` rhs`` / ``True `or` rhs`` | 実行時にRHSを評価しない。RHSの名前解決・型検査は省略しない |
| ``flag `MyMod::and` True`` | 呼び出し可能な場合、通常の中置Callとして評価する |
| ``left `on` right`` / ``left `Function::on` right`` | 現行の低優先度を維持する |
| `on = value`、closureの引数 `and`、パターン束縛 `eq`、フィールド名 `lt` | 予約名の使用として拒否する。残りの予約名にも同じ制限を適用する |
| `MyMod` 内の関数 `and` の定義 | 定義を許可する |
| 予約名の単一・リスト・全件import | 既存のimport規則を維持する。キーワード化による禁止を追加しない |
| `compare`、pipe族、関数合成の関数インターフェース | 今回の予約による名前使用・import制限を受けない |

9予約名の前置Call、backtick前置Call、capture、修飾Callについては、今回の予約制限以外の解決・評価規則が変わらないことも確認する。

Pattern consumerについては、さらに次を固定する。

- 通常・正規修飾・backtick前置Callで、4consumerのPattern位置と入力・branchのExpr位置が一致する。
- ``value `is_match` Ok(_) | Err(_)`` はRHS全体をOR Patternとして扱う。``value `apply_pattern` [_1, .._]`` はprojectionのResultを返す。
- `apply_pattern`のroot / nested OR拒否、`is_match`の通常bind禁止、`if_let`系のOR binding整合を維持する。
- `p1 | p2 @ whole` はOR全体へのasとする。ORの各branchは同じ入力を受け取り、最外asは入力全体、子Patternのasはその子位置の値を束縛する。同一階層の連続asは拒否する。
- `Regex::is_match`の全Call表記はExpr引数のまま。予約名の末尾一致によるPattern化をしない。
- placeholderなしの先頭挿入、直接引数placeholderへの挿入、修飾・backtick前置CallをRHSとするpipeで、共通操作後の引数対応に従ってPattern位置を選ぶ。Pattern/Lazyへの注入、Pattern部分適用、既存制約に反するslotを拒否する。引数数が合わなくても別の挿入方法へ切り替えない。
- consumerの完全Call captureと標準Regex captureを許可し、bare consumer capture・値参照・Patternの直接placeholderを拒否する。consumerのユーザー宣言・field拒否、標準宣言の許可を固定する。
- Pattern内のExtractor事前Expr引数とpayload子Patternの役割を、既存の宣言・型情報で選択できる。
- 通常・tolerant・不完全入力の解析、補完、spanの付け替え、ASTの走査・変換、REPL・LSP経路が同じ構文契約を使う。入力途中にPatternコンテキストが残って後続へ漏れない。
- PatternOrと中置Callの演算子前後の改行は、通常演算子と同じ継続・終端規則で検証する。pipe系列の特別扱いをこれらへ広げない。

現行のtolerant lexerは4consumerを通常識別子として分類しており、strict lexerの専用token分類と一致していない。今回、予約名分類とsyntax token・補完の扱いを揃える。外側のplaceholder captureがconsumer内部のExprを走査するケースも回帰検証に含める。

### 実装前に確定する境界

Pattern位置の認識、PatternOrとas-patternの結合・入力値、終端判定、通常演算子に揃えた改行規則は確定している。中置Patternの終端やasの接続を未確定事項として扱わない。括弧の既存の意味を変えず、ExprとPatternを混ぜた共通の優先度表や解析失敗後の再解釈は追加しない。

キーワード化はimport規則を変更しない。対象SpecialFormsは既存autoimportで提供され、同じ宣言の明示importは構文規則に影響しない。別identityへ解決されたconsumer表記はSigilで拒否する。`Regex::is_match` の修飾Call・captureは通常Expr引数Callのまま維持する。

## 実装・検証計画

影響levelは **4**。予約名と構文優先度の言語契約を変更するため、実装時は全体検証と別エージェントによる最終レビューを行う。

1. import規則を維持し、`docs/dev/Pattern_spec.md`、`docs/site/{language-reference,callables,pattern-matching,extractors}.md`、関連する `lib/function.srt`、`lib/kernel.srt`、`lib/traits/operator/{eq,compare}.srt` の `@doc` を仕様に整合させる。診断の分類は `docs/dev/diagnostics.md` に合わせる。
2. 共有の予約名・consumer構文契約を整理し、Spireで宣言・束縛・field制限、通常・修飾・backtick前置Callの引数文法、pipeの共通挿入位置とCall引数の対応を検証する。9予約名と標準専用consumerの宣言制限を混同しない。pipeのAST操作はパラメータ種別や引数数で分岐させず、注入禁止位置の検査を分ける。
3. 中置consumerを同じ処理へ接続し、OR・後続Expr・不足引数・nested文法切替の境界を固定する。consumer外枠の旧候補選択経路を置換し、Extractor内部の型依存候補保持は維持する。AST走査・変換と補完・tolerant・REPL/LSP経路を揃える。
4. Sigilのconsumer canonical identity検証を明示し、通常Callへの再解釈を削除する。標準autoimport、明示importと既存の名前解決・型解決は維持する。
5. script/module fixtureで短絡評価、修飾名の通常評価、比較の優先度、4consumerの全Call表記・pipe・binding・projection・失敗Error・評価回数を検証する。置き換えた旧経路は削除し、失敗をフォールバックで隠さない。
6. 対象検証後に `rtk cargo nextest run --profile ci --workspace` と `cargo run -- test --quiet --all` を実行し、仕様・最終差分・検証結果を別エージェントにレビューさせる。

2026-10-02の実装では、予約名の共有token分類、consumer外枠の直接Expr/Pattern解析、PatternOr/as、中置Call、pipeの共通挿入規則、canonical identity検証、tolerant・不完全入力・補完を更改した。Extractor内部の候補保持、完全Callのconsumer capture、既存import規則は維持した。

独立レビューで検出したpipeの構文コンテキスト漏れは、Grouped/Tupleと後続演算子による外側の式を含めて修正し、AST・実行fixtureで再発を検証した。調査中に確認した既存のKernel全件importとgrouped Hole callableの課題は、`doc/open-issues.md` のOI-038・OI-039へ記録した。

追加修正前の検証結果:

- `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace`: 2054件成功、失敗・除外なし。
- `cargo run -- test --quiet --all`: 終了コード0、標準Surtrテスト全件成功。新規worktreeには既存Fileテストの前提である `tmp/sandbox` を作成した。
- `cargo fmt --all -- --check` / `git diff --check`: 成功。
- 最終差分の独立レビュー: 未解決の実装指摘なし。デグレと新規問題はAstraの独立コンテキストで確認した。

変更と検証は専用worktreeで実施した。

## 2026-10-02の追加修正指示

ユーザの追加仕様を入力として、OI-038・OI-039を修正する。import規則とgroupingを介する期待型の契約に及ぶためlevel 4とする。

- autoimportは各ファイル先頭で対象モジュールを全件importする。既に有効なautoimport元の全件・単一member・リストの明示importは重複importとして拒否する。Kernel、Function、autoimport trait・impl ownerにも同じ規則を適用する。stageで未導入のモジュールを導入済みとして扱わない。異なるモジュールからの同名member importと既存のshadowing規則は維持する。autoimport trait自身とhelperも既に導入済みのため、namespaceの親モジュールからそのtraitを選択する全件・単一・リストimportを重複として拒否する。親モジュールの他のmemberだけを選択するimportは維持する。
- pipe RHSのgroupingは、括弧内を評価して得た関数値を入力先とする。最外Callへの引数注入を括弧内へ広げず、map/bindの空・非空carrierでも生成式を一度だけ評価する。
- groupingは期待型を内側へ伝える。通常引数、注釈、返り値、分岐、入れ子のgroupingでも同じ契約とする。入力を使わないcallableの既存契約を期待callable文脈でも維持し、引数数・使用する入力型・返り型の不一致は拒否する。Holeを一般の型比較のwildcardにしない。解析・型検査失敗後の再解釈や別経路へのfallbackは追加しない。Lazy引数のgroupingによるeager境界も維持する。

実装はSigilのimport状態とScarの期待callable型制約に分担し、対象テストのRedを確認してから修正する。正本はlanguage-reference、Pattern_spec、callables、pipe-operatorsへ配備する。成功fixtureの冗長なautoimportを除き、拒否fixtureに全件・単一・リストの境界を固定する。groupingは通常引数・注釈・返り値・分岐とpipe実行fixtureで推論と評価回数を検証する。Astraの独立相談と最終差分の独立レビューを行い、CI profileのworkspace全件と標準Surtrテストを実行する。解決時にopen-issuesの2件を削除する。

追加修正の実装結果:

- Sigilは各autoimport元のstageを記録し、既に導入済みの元への明示importをmember走査前に拒否する。namespaceの親モジュールからautoimport traitを選択する3経路にも同じ検査を適用する。冗長な標準ソースのimportは除去し、拒否fixtureへ移行した。
- Scarは期待する値と実際の値の適合を厳密な型一致から分け、単項callableの実際の入力がHoleの場合だけ期待入力を受け入れる。返り型は厳密に照合する。既知の期待型を持つ分岐は両枝を期待型へ照合し、枝自身のHoleを保持する。期待型のない分岐、trait signature、集約型内部の規則は維持した。
- 型の由来情報を比較前に消す回帰を全Scar検証で検出し、元の型を比較へ渡すよう修正した。既存bare occurrenceテストと内部contractテストで固定した。
- grouped RHSは評価済みcallableを入力先とする。plain applyのRHS生成→LHS評価→適用という既存順序も維持する。空List・None・Errを含むmap/bindで生成式を一度だけ評価する。
- 対象検証はSigil全289件、Scar全331件が成功。OI-038・OI-039は仕様が確定して実装・対象検証が完了したためopen-issuesから削除した。

追加修正後の最終検証結果:

- 全体CIの初回で、旧Bootstrap import成功期待と旧Kernel member衝突診断期待のcoldテスト2件が失敗した。両方を導入済みautoimportの重複拒否へ移行し、対象coldテスト2件の成功を確認した。
- `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace`: 2066件成功、50 binaries、60.553秒。失敗・除外なし。
- `cargo run -- test --quiet --all`: 終了コード0、標準Surtrテスト全件成功。
- `cargo fmt --all -- --check` / `git diff --check`: 成功。
- 修正中の回帰・新規問題はAstraの独立コンテキストで確認した。変更と検証は専用worktreeで実施した。
- 追加修正を含む最終差分の独立レビュー: 未解決の指摘なし。
