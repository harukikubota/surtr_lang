# SR 呼出し解決・構文境界の調査と実装計画案

記録日: 2026-10-04。入力: [SR 改修方針](sr_revision_notes.md)。

SR-01・07・10・11について、現行コード・正本・既存テストを読み取った結果を整理する。SR-01の確定範囲とSR-09は今回の実施結果も記録する。未確定の構文案・予約範囲は現行仕様や実装完了を意味しない。入力文書の方針と、実装上の選択が必要な箇所を分ける。

## 1. 現行の分類

以下の「予約」は現行実装の記録であり、今回すべてを予約語へ変更する提案ではない。トップレベル関数定義をモジュールとして読み込む現行仕様も維持する。

| 対象 | 現行の構文・呼出し先 | 名前とシャドーイングの境界 | 通常呼出し・パイプの差 |
|---|---|---|---|
| `if` / `if_then` / `require` / `ensure` | 通常Identを解決し、canonical `Kernel`関数なら特殊処理 | lexer上の専用キーワードではない。通常symbolとして解決する | 通常呼出しには解決失敗を綴りで救済する経路がある。パイプ準備は解決エラーを返す |
| `map_err` / `cause` / `recover_kind` | canonical `Result`関数なら特殊処理 | 通常Ident。local / parameterの`map_err`遮蔽を既存テストが許可 | 上と同じ非対称がある |
| `assert_err_kind` / `assert_cause_chain` | canonical `Test`関数なら特殊処理 | 通常Ident。新たな予約は入力方針で確定していない | 通常呼出しの綴りfallback一覧には含まれない |
| `and` / `or` | `ReservedCallName` token。canonical `Kernel`関数の処理に接続 | 変数・引数・フィールド位置などで予約名制約がある。member宣言とimportの可否は別契約 | `&&` / `||`には構文から直接論理演算へ接続する経路もある |
| `if_let` / `if_let_then` / `is_match` / `apply_pattern` | `PatternConsumer` token。裸名と`Kernel::name`を専用ASTへ変換 | 通常宣言、引数、local bind、user member、fieldで予約。既存標準`Regex::is_match`の修飾呼出しは通常関数 | パイプの最外呼出しだけを扱い、Pattern / Lazy位置への注入を拒否 |
| `match` / `cond` / `do` | 専用キーワードとAST | 通常の関数名解決を経由しない | 通常関数の引数待ち受け呼出しとは別の構文 |
| `dbg!` | `Ident("dbg")`と`Bang`から`Ast::Dbg`へ変換 | 変数・通常関数の`dbg`とは別。symbol lookupを行わない | パイプ対応の追加は今回の確定事項ではない |
| `Facet::bulk_update` | Spireが修飾名の綴りから専用ASTへ変換 | bare `bulk_update`は現状キーワードではない | 現行専用構文は`Facet::bulk_update(source) { entries }` |

分類の正本・実装:

- `crates/sindr/src/names.rs`: `ReservedCallName`、`is_reserved_value_name`。
- `crates/sindr/src/pattern.rs`: `PatternConsumer`。引数位置・個数・OR・Lazy引数の共有定義。
- `crates/spire/src/lexer.rs`: `tokenize`。`match` / `cond` / `do`、予約名、consumer tokenの分類。
- `crates/sigil/src/resolver/expr.rs`: `canonical_special_form_from_qname`、`classify_canonical_special_form_callee`、`fallback_special_form_from_surface`、`resolve_node`の`Ast::App`、`prepare_pipe_rhs`。
- `lib/kernel.srt`、`lib/types/result.srt`、`lib/test.srt`: 各標準関数の宣言・`@doc`。
- `docs/dev/Pattern_spec.md`の「予約語・OR・pipe」、`docs/dev/Lazy_spec.md`。

`on`も現行`ReservedCallName`に含まれる。入力SR-06の「中置以外はシャドーイングされる」と整合させる際は、通常member呼出しの解決と、変数名・引数名の予約制約を別に確認する。本計画では予約範囲を変更しない。

## 2. SR-01: 新規予約の判断に依存しない改修

### 確定している方向

標準環境で呼出し先を解決し、そのcanonical identityから特殊処理を決定する。名前解決が失敗した入力を、綴りだけで別の呼出し先へ救済しない。通常関数呼出しとパイプで、同じ名前が異なる対象を指す仕様にしない。

現行`resolve_node`の`Ast::App`は、calleeの解決失敗後に`fallback_special_form_from_surface`を呼ぶ。`prepare_pipe_rhs`はcallee解決のエラーをそのまま返すため、入口が非対称である。また`classify_canonical_special_form_callee`には、`entry.auto_import`と名前から特殊処理へ分類する経路がある。autoimportは可視性の設定であり、それだけでcanonical標準関数のidentityを与えるものではない。

実装では次を分ける。

1. 通常の名前解決でcalleeを確定する。
2. 確定したidentityがcanonical special formなら、その契約へ接続する。
3. 通常関数なら通常呼出しへ接続する。
4. 解決失敗なら、その診断を保持して終了する。別名の探索、綴りによる救済、特殊処理への降格は行わない。

未定義calleeの診断を呼出し用に整える既存`map_undefined_callable_error`まで機械的に削除する趣旨ではない。元の失敗理由・source spanを保ち、同じ入力を成功へ変える経路を除く。

### 実施結果（2026-10-04）

綴りfallbackとautoimport属性による誤分類を削除した。分類元は選択済み `ResolvedId.qualified_name` のcanonical identityのみとする。生成closure内部でもこのidentityを保持し、宣言表を再検索しない。未解決calleeと非canonical autoimport関数の内部契約テストをRedからGreenへ進め、Sigil全315件を確認した。全体検証の結果は[入力文書](sr_revision_notes.md)を参照。

### 未確定事項

各関数名を新たに予約するか、どの宣言・引数・field位置で禁止するかはSR-01の未確定事項である。現行の通常Identを一括でキーワード化しない。綴りfallback除去と新規予約の決定を同じ変更にまとめる必要はない。

### 受入条件とテスト

- 通常の標準環境で、裸名・canonical修飾名の呼出しとパイプが同じ対象を選ぶ。
- 現行で認めるlocal / parameterの`map_err`遮蔽は、通常呼出し・パイプともユーザー値を選ぶ。
- user moduleの同名関数は、autoimport属性だけで標準special formにならない。
- callee解決に失敗した場合は、特殊処理へ進まず解決エラーを返す。
- Lazy / ErrorKindの評価・marker規則を維持する。パイプ両辺の新たな一律評価順は約束しない。

既存の`test_pipeline_partial_special_form_does_not_trigger_for_shadowed_local_binding`、`test_pipeline_partial_special_form_does_not_trigger_for_shadowed_parameter`を基点に、通常呼出しとの対応とidentity不一致の拒否をSigilで固定する。blankslate環境で標準名が成功することを期待するテストは追加しない。必要な標準・import環境の整備はSR-02〜04と連携する。

名前解決の入口と複数フェーズの契約に関わるためlevel4として扱う。

## 3. SR-07: Pattern consumerの構文選択

### 現行正本との差

`docs/dev/Pattern_spec.md`は、裸名と`Kernel::name`の第2引数をPattern文法で直接読み、`Regex::is_match`の引数をExprとして読むと定める。Sigilはconsumer表記とcanonical identityの一致を検証し、不一致を通常関数呼出しへ再解釈しない。

実装もこの契約に沿っている。

- Spire `parser/expr.rs`の`parse_pattern_consumer_call`: consumer引数をExprまたはPatternの片方だけとして保存する。
- Sigil `resolver/pattern_consumers.rs`の`resolve_pattern_consumer_identity`: canonical Kernel以外を拒否する。
- 同ファイルの`consumer_syntax_rejects_an_ordinary_builtin_identity`: Regex identityをconsumer構文として受理しないことを固定する。

入力方針の「Patternで保持し、後続の呼出し先に従ってExprへ変換」は、この正本と拒否テストの変更を含む。先にSR-01の予約・名前解決方針を確定する必要がある。

### 変換候補と曖昧性

以下は検討表であり、すべての変換を認める決定ではない。

| 構文候補 | Exprへ移す場合の論点 |
|---|---|
| 文字列・数値などのliteral | literal値とsource spanを保つ一対一変換の候補 |
| tuple / list | 子要素がすべて変換可能なら再帰変換できるか。listのtail分解と通常list式の差を確認する |
| `name` | Patternではbind、Exprでは値参照になる。新規bindingを公開する前にcalleeに基づいて役割を決める必要がある |
| `Name(...)` / `name(...)` | constructor / Extractor / 通常関数で意味が異なる。calleeや引数の再探索による救済にしない |
| `_` / `_N` / pin / as / OR | 通常Exprへの一律変換はできない。許可対象を明記し、非対応形は診断する |
| 演算式・通常ブロックなどExpr専用構文 | 最初のPattern解析だけで受理できるとは限らない。parserが保持する範囲の設計が必要 |

入力の`is_match._1`が引数位置の説明か実際のソース表記かは未確定。また入力の`Regex::match`に対応する現行関連APIは`Regex::is_match`である。名称を推測で置き換えて実装しない。

推奨案は、calleeを一度確定した後、仕様で列挙した構文だけを一方向に変換する契約である。Pattern解決の失敗後にExprとして試し直す経路は設けない。非採用の構文候補がscopeへbindingを残さないことも必要になる。

### 受入条件とテスト

仕様確定後、通常呼出し・backtick前置／中置・パイプの最外呼出しで同じ引数役割を選ぶことをSpire / Sigilの直接的な層で検証する。canonical consumerのPattern bind・OR・projection、Regexの通常Expr、Pattern / Lazyへの注入拒否、入れ子にconsumer文脈を漏らさない境界を維持する。

既存Spireテスト`pattern_consumers_choose_argument_grammar_directly_in_all_call_forms`、`pattern_consumer_pipe_context_applies_only_to_outer_call`、`pattern_consumer_pipe_recognizes_every_direct_slot_without_projection_confusion`を変更後の契約に照らす。必要な拒否テストは削除せず、拒否する条件と診断を新契約へ合わせる。

構文とscopeの契約変更なのでlevel4。未確定事項が残る間は実装しない。

## 4. SR-10: 括弧内の専用ブロック

### 確定事項と現行

入力ではbare `bulk_update`のキーワード化とshadow禁止を指定している。現在はSpire `parser/expr.rs`の`parse_ident_continuation`が`Facet::bulk_update(source)`の閉じ括弧後に更新ブロックを要求し、`parse_bulk_update_expr`が専用entryを読む。正本は`docs/site/facet.md`と`lib/facet.srt`の`@doc`。

`parse_match_expr`は`match source { arms }`、`parse_cond_expr`は`cond { clauses }`を読む。各ブロックは通常Exprと同じ文法ではない。

### 確定した構文（2026-10-04の補足）

通常関数呼出しの形でも記述できるようにする。構文ごとの専用AST・意味論は共通にする。

| 構文 | 受理する形式 | 拒否する形式 |
|---|---|---|
| `match` | `match ARG1 { ... }`、`match(ARG1) { ... }`、`match(ARG1, { ... })` | 今回の補足による既存形式の削除なし |
| `bulk_update` | `bulk_update(value) { ... }`、`bulk_update(value, { ... })` | `bulk_update value { ... }` |
| `cond` | `cond { ... }`、`cond({ ... })` | 括弧有無の追加以外に新しい呼出し形式を設けない |

`{ ... }`は専用ブロックの内容を省略した表記である。一般ブロックの第一級値化や空ブロックの受理を意味しない。named argument、末尾カンマ、ブロック内区切り、パイプ注入の規則は今回の構文追加を理由に変更しない。

一つの正規形に限定して外置き形式を拒否する従来の提案は撤回する。現行の`Facet::bulk_update`という修飾表記の存廃は、上記の括弧の規則と分ける。今回の補足は、その表記の削除を指定していない。

### 受入条件とテスト

- `match`の3形式は同じ対象式・arm列を持つASTとなり、同じ値・binding・診断を生成する。
- `bulk_update`の2形式は同じASTとなる。sourceを一度評価し、更新entryの順序・失敗伝播・Facet能力検査を維持する。値の引数に括弧がない形式はparse errorにする。
- `cond`の2形式は同じ条件節列を持つASTとなり、評価順序を維持する。
- 括弧内専用ブロック、ネスト、通常Exprを含むsourceの境界を解析できる。
- 各ブロックの既存文法を維持し、通常Exprとして任意の位置へ持ち出す機能は追加しない。
- `bulk_update`のキーワード化・シャドーイング禁止という既存方針を維持する。

既存Spireテスト`test_facet_bulk_update_special_form_parses`、`test_facet_bulk_update_rejects_commas_between_entries`、`test_facet_bulk_update_rejects_non_whitelisted_leaf_call`を基点にする。runtimeの更新順序をparserテストへ重複させず、既存Facet実行テストで確認する。

構文・予約範囲を変えるためlevel4。括弧の形式は確定済みであり、実装時はこの受入条件に沿って正本とparserを更新する。本項目の処理系への実装はまだ行っていない。

## 5. SR-11: calleeの一意性と引数役割の遅延

### 現行の処理

呼出し先が一意でも、その関数値の型がScarで確定する場合、引数のExpr / Patternの役割はSigilだけで決められない。

| フェーズ | 現行の責務と根拠 |
|---|---|
| Spire | `parser/pattern.rs::parse_pattern_argument`がExpr / Pattern / named Pattern候補と診断を保存する。境界の不一致を拒否する |
| Sigil: 宣言済みExtractor | `resolver/patterns.rs::select_pattern_argument_roles`が宣言signatureの事前引数数で役割を選び、非採用候補を捨てる |
| Sigil: local head | `resolve_pattern_inner`が選択済みhead UIDを維持し、各引数のExpr / Pattern解決結果を`ResolvedPatternArgument`に保持する |
| Scar | `checker/patterns.rs::select_extractor_application`が同UIDの型を検査し、`ExtractorClosure`signatureから役割を選ぶ。non-ExtractorClosureなら拒否する |

`docs/dev/Pattern_spec.md`はlocal ExtractorClosureによるshadowingを認め、選択したlocalがExtractorClosureでなくてもnamed Extractorを探し直さないと定める。現行の候補保持は、必ずしもcalleeを探し直す処理ではない。

### 推奨方針・受入条件

callee identityを確定した後は変更しない。引数役割の決定だけを必要なフェーズまで遅延する。宣言済みExtractorで既に分かる役割までScarへ持ち越す必要はない。

- local headがnamed Extractorを遮蔽した場合、localのUIDだけを使う。
- localの型が通常Closureなどなら拒否し、named Extractorへ再探索しない。
- 選択した役割の診断を採用し、非採用候補のエラーで正常入力を拒否しない。
- 事前引数は外側scopeで解決し、同じPattern内の新規bindを参照させない。
- payload側だけでbindingを公開する。候補の解決で生成したbindingを外へ漏らさない。
- generic signature、nested Extractor、Record named Patternを維持する。

既存`Pattern_spec.md`のshadowing・事前引数scope・consumer成功scopeの受入条件を再利用する。Spireの再parseを減らす場合も、曖昧なASTを一つの意味に決め打ちして正常入力を失わないことを先に確認する。具体的な候補表現の変更はSR-07の確定後に設計する。

候補表現がフェーズ間契約を変える場合はlevel4。読み取り上、callee再探索を追加する必要はない。

## 6. SR-09: 現行分離の受入確認

製品コードの変更は不要だった。`dbg!`は専用AST、通常の`dbg`は名前解決する値・関数である。`parse_intrinsic_decl`も宣言名を`dbg!`として保存する。

今回の追加・拡張:

- `tests/fixtures/script/pass/functions/dbg_name_is_distinct_from_special_form.srt`と`.expected`: `def dbg`、引数`dbg`、`dbg!`の共存を検証する。
- `crates/xldr/tests/repl_core.rs::core_dbg_docs_and_signatures_resolve_from_bootstrap_source`: local callable `dbg`を束縛し、`dbg!(dbg(3))`の診断値が4であることと、`:doc dbg!` / `:sig dbg!`がBootstrap intrinsicを参照することを確認する。

対象fixtureを含む`rtk cargo nextest run -p rune --test integration run_srt::script_fixtures_bucket_7`、対象REPL caseを含む`rtk cargo nextest run --profile ci -p xldr --test repl_core repl_core_bucket_1`は各1 passed、exit 0。bucket番号は今回のworktreeとテスト配置に対する値であり、移動後は再確認する。全体検証は親作業で行う。

これは既存契約のテスト補強であり、`Bootstrap::dbg!`やパイプ対応を追加していない。

## 7. 実施順と完了条件

1. SR-02〜04と連携して標準・import環境を整え、SR-01のcallee identityと元エラー伝播を統一する。新規予約の判断とは分離できる。
2. SR-01で関数別の予約・shadowing境界を決定する。
3. SR-07の変換対象・拒否範囲を確定し、Pattern正本を更新してから実装する。
4. SR-11の候補保持を、UIDの一意性と型確定時期に合わせて整理する。
5. SR-10は確定した括弧の形式に沿って実装する。SR-07の変換実装に依存しない。`bulk_update`の予約名と修飾表記の扱いは区別して整合させる。

SR-01・07・10・11の実装では、変更契約を直接検証する最小テストからTDDで進める。対象Spire / Sigil / Scar、script fixture、必要なREPL境界を確認した後、level4の全体検証として`rtk cargo nextest run --profile ci --workspace`と`rtk proxy cargo run -- test --quiet --all`を実行し、別エージェントによる最終差分レビューを行う。

SR-01の上記確定範囲とSR-09の確認を実施した。SR-07の未確定構文・予約範囲、および今回確定したSR-10の構文追加は未実装。SR-11はcalleeの一意性と引数役割の遅延を区別する方針を記録し、候補表現の変更は仕様確定後に判断する。
