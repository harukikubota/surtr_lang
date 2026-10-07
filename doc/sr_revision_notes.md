# SR-01〜13への回答に基づく改修案・後段タスクメモ

記録日: 2026-10-04。

利用者の回答を起点に、現行コード・正本文書・実行結果を照合した。各項目の冒頭は回答時の方針を保持し、「現行確認」「実施記録」で調査結果と実装状況を区別する。

| 項目 | 状況 | 次の作業・根拠 |
|---|---|---|
| SR-01 | 確定範囲を実装・検証済み | canonical identity と元エラー保持を統一。新規予約の範囲は未確定 |
| SR-02〜04 | 実装・検証済み | [標準環境の統一計画](sr_standard_environment_plan.md)にAPI・移行順・受入条件を整理 |
| SR-05 | 実装・局所検証済み | 未登録builtin metadataで内部エラー |
| SR-06 | 既存挙動確認・回帰テスト追加済み | 関数宣言のshadowと中置固定先を区別 |
| SR-07 | 詳細仕様待ち | Pattern→Expr変換範囲と対象表記の確定が必要 |
| SR-08 | 現行実装・既存テスト確認 | Result / Booleanの実宣言に基づくaliasを維持 |
| SR-09 | 既存挙動確認・回帰テスト追加済み | 通常dbgと専用dbg!の共存 |
| SR-10 | 実装・検証済み | matchは3形式を同等に扱う。bulk_updateは引数の括弧必須、condは括弧有無を許容 |
| SR-11 | 現行処理の責務を確認 | callee UIDは一意。型に依存する引数役割の遅延と区別 |
| SR-12 | 可視性・拒否条件を確認 | public Constはimportなし、private Constはファイル内 |
| SR-13 | 正本文書の整合済み | newの定義必須によるコンパイル時の呼出し先解決。実行時のResult値とは独立 |

呼出しと予約名の現行仕様は[関数名と呼出し構文](../docs/site/callable-names.md)、Pattern consumer の現行契約は[Pattern / Extractor 実装契約](../docs/dev/Pattern_spec.md)を参照する。SR-01・07・10・11の当時の判断と残件は、本書の各節に記録する。

## SR-01: special formの呼出し先とスコープ解決

トップレベル関数定義はレキシカルスコープではなく、通常のモジュールモードでの読み込みと同様に扱う。この仕様自体を問題として扱わない。

`map_err`などはautoimportされるため、どの位置でもモジュール名なしで呼べる。この仕様と、名前解決エラーを綴りで救済する経路の課題を分ける。

special formも呼出し先が決まった時点でスコープ解決が完了する。特別扱いによって通常関数コールとパイプの動作が変わらないようにする。

キーワードのシャドーイング禁止／許可は関数ごとに異なる。後段タスクで各関数の振る舞いを整理し、仕様を詰めてから修正する。今回、この境界の仕様は確定しない。

### 確定範囲の実施記録（2026-10-04、level4）

`fallback_special_form_from_surface` と、通常関数呼出しのcallee解決失敗を特殊形式へ救済する分岐を削除した。分類は、名前解決で選択した `ResolvedId.qualified_name` のcanonical identityだけを使う。autoimport属性と裸名による分類、module名とmember名からの重複判定、宣言表の再検索は行わない。生成closureにも選択済みidentityが保持されるので、宣言表を持たない内部resolverでも同じ対象を扱える。

未解決の `if` が `Resolved::If` として成功するケースと、`User::map_err` がautoimport属性だけで標準special formへ誤分類されるケースを内部契約テストでRedとして確認した。修正後は両テストとSigil全315件が成功した。通常呼出し用の未定義callee診断整形は保持する。`docs/dev/Lazy_spec.md` に分類の契約を記載した。

関数ごとの新規予約・シャドーイング範囲は変更していない。SR-07のPattern→Expr変換やSR-10の構文選択も、この確定範囲へ含めない。全体検証・独立レビューは末尾の記録を参照。

## SR-02: autoimportを通常環境の前提にする

autoimportは任意に切り替えて挙動を制御するものではない。通常実行では必ず`auto_import=true`とする。

デフォルトでautoimportが有効であることを検証するテストは必要。標準環境を除いたblankslate環境での実行テストは不要とする。各テストにautoimportの切替フラグを意識させる構成にしない。

## SR-03: 単独Sigil入口も必要なimport環境を使う

autoimportと明示importを含め、呼出し解決に必要な環境を用意する。

その標準環境を前提として、明示importの有無によって呼出しを解決できるかをテストする。import宣言をno-opとして捨てる別経路を前提にしない。

## SR-04: 特定の標準symbolだけを先に置かない

特定のsymbolだけを初期scopeへ先に置く構成を改修する。

コンパイラが型特有の情報を持ち、stdを読み込んだ結果とマージする構成は許容する。テストも標準環境をベースに実施する。

### SR-02〜04の実施記録（2026-10-04、level4）

3項目は宣言UID・初期scope・import状態を共有するため、一つの移行として実装した。

- 標準source inventoryを `crates/sindr/src/stdlib.rs` へ移し、Xldr・Sigilテスト・Scar / Forge helper・解析側が同じ順序と宣言を使うようにした。属性の `auto_import` を一律にtrueへ変更していない。
- `ResolveEnvironment` を必須とする単独名前解決APIとsession初期化へ統一した。内部の `resolve_program` はimport検査済みの型だけを受け取り、未処理importを無条件に捨てる公開入口を削除した。
- REPL / preload の独自import処理を撤去し、Sigilが導入済み状態・scope・表示用の適用結果を管理する。失敗時は同じcheckpointへ戻す。`import Add::add` がREPLだけで使えた差も、canonical trait ownerのmember解決へ統一した。修正後はscriptでも `add(1, 2)` が3を返すことを確認した。
- `print`、`to_string`、`inspect`、`eprint`、`set_exit_code` の初期登録と、その登録をautoimportで上書きする旧分岐を削除した。型固有情報とcompiler生成runtime関数は維持した。
- 標準なしの通常ソース用テスト入口を標準helperへ移行した。標準と同名のfixtureは目的を保って改名し、標準の模造や重複挿入を削除した。内部scopeや宣言契約の直接テストは残した。
- 単独文書・project解析も標準stage付きに統一した。固定標準だけの解析ではprocess内でprefixを再利用し、project stage追加時・標準文書編集時は全stageを解析する。異なる宣言表のcheckpointを合成しない。

Redで確認した境界は、単独入口による未知importの誤受理、初期scopeの5関数、解析側の標準autoimport、trait memberの明示importである。独立レビューで見つかった解析cacheのUID衝突は、32個のproject宣言を追加すると標準 `Function::on` の引数とproject関数がUID961を共有する例で再現した。追加stageに標準のみのcheckpointを使わない設計へ修正し、UID一意性のテストを通した。

局所検証はSigil315件、Forge95件、XldrのREPL12bucketが成功。Scarは全体検証で残った標準名衝突等を修正し、該当46件を再確認した。解析側は全144件と追加の標準文書編集・UID境界テストが成功した。workspace全体の最終結果は末尾に記録する。

## SR-05: builtin lookup失敗を明確な内部エラーにする

lookup失敗はコンパイラバグとして、明確なエラーで処理を止める。`u16::MAX`などの代替値へ降格して処理を続けない。

### 実装方針（2026-10-04、level2）

対象は `crates/sindr/src/builtin.rs` の `BuiltinMeta::runtime_id()`。現行のポインタ検索→名前検索→`u16::MAX` の経路を、正本 `BUILTIN_METAS` の名前検索に一本化する。登録済み metadata のコピーも同じ ID に対応させる。登録のない metadata は利用者の入力エラーではなくコンパイラ内部契約違反であり、builtin 名を含む `internal compiler error` の invariant panic で即時停止する。既存の非 fallible API を保ち、代替 ID は生成しない。一般の検索 API が未知名を `None` として返す契約は維持する。

受入条件は、全登録 builtin の ID が定義順を保つこと、コピーした metadata も同じ ID を得ること、未登録 metadata の ID 要求が明確に停止すること。Sindr の回帰テストで旧 sentinel 成功を Red として確認し、修正後に対象 crate と CI profile の全体テストを検証する。正本の builtin 単一テーブル契約も更新する。

### 実施記録（2026-10-04）

実装と対象検証を完了した。未知 metadata の ID 要求が旧実装では停止しないことを `test did not panic as expected` で確認し、正本検索への一本化後は `rtk cargo nextest run -p sindr` が108件成功（exit 0）。登録順・metadata のコピー・未知名検索の `None` 契約も検証した。`docs/dev/EldrVM_spec.md` の builtin 登録契約を更新した。全体検証の結果は末尾へ追記する。



## SR-06: 裸の中置onはFunction::onに固定する

現行の固定先は仕様通り。中置記法では優先度が絡むため、bare名の`on`は`Function::on`を指す。

ただし`on`はキーワードレベルではない。他の呼出し形式ではシャドーイングされる。この区別を維持する。

### 現行確認と実施記録（2026-10-04）

`crates/spire/src/parser/expr.rs` の `canonical_infix_callee` は裸の中置 `on` だけを `Function::on` へ変換する。通常呼出し・キャプチャ・パイプ右辺は通常の名前解決を使う。同名の `def on` を置いた source を実行し、通常呼出しが `42`、キャプチャ経由が `12`、パイプ経由が `3` を返す一方、中置の比較関数が標準 `Function::on` として動作することを確認した。

この境界を既存 `tests/fixtures/script/pass/functions/reserved_infix_precedence.srt` と `.expected` に追加し、出力一致・exit 0 を確認した。`docs/site/callables.md` も説明を補った。処理系は変更していないため、意図的な Red は作っていない。

注意する区別: `on` は専用 keyword token ではないが、現行の `ReservedCallName` に含まれ、変数・引数・field の束縛名としては予約される。今回確認した shadow は関数宣言によるもの。変数名としての予約解除までをこの項目の実施済みに含めない。


## SR-07: Pattern consumerのフローを見直す

他への影響が大きいため、フロー自体を修正する。SR-01でキーワードごとの振る舞いを決めた後に取り組む。

`is_match`は正規のKernel consumerとしてPatternを解析し、解決後に通常Exprへ再解釈しない。Regexの通常関数は`Regex::matches`へ改名し、同名宣言の特権や通常callへのフォールバックを設けない。

## SR-08: Result／Booleanの裸alias

改修で仕様を確定済みとの回答。今回、新たな仕様変更案は追加しない。

### 現行確認（2026-10-04）

Sigil の `special_variants_require_a_real_enum_declaration`、`canonical_special_variants_share_alias_and_capture_targets`、`reserved_special_variant_aliases_reject_programmatic_binding_and_argument_shadowing` が、実enum宣言の必要性、canonical UIDの共有、予約aliasの束縛拒否を固定している。初期scopeに架空constructorを置く仕様へ戻さない。新たな仕様変更は不要。

## SR-09: dbg!とdbgを区別する

`dbg!`と変数・関数の`dbg`を区別するように改修する。具体的な名前解決・シャドーイングの境界は、後段の仕様整理で扱う。

### 現行確認と実施記録（2026-10-04）

現行 parser は `dbg` と `!` を組み合わせた専用構文を `Ast::Dbg` にし、通常の `dbg` は通常の名前解決へ渡す。Bootstrap の `dbg!` 宣言は文書・signature の参照先として保持し、通常関数の symbol にはしない。処理系の変更は不要だった。

`tests/fixtures/script/pass/functions/dbg_name_is_distinct_from_special_form.srt` で同名関数と同名引数の通常呼出し、および `dbg!(dbg(value))` の共存を固定した。Xldr の既存 `core_dbg_docs_and_signatures_resolve_from_bootstrap_source` にはローカル `dbg` の呼出しを追加し、`dbg!` の出力と Bootstrap 文書・signature が両立することを確認した。script fixture bucket 7 と、CI profile の REPL bucket 1 はそれぞれ成功した。既存挙動の回帰テストであり、意図的な Red は作っていない。

## SR-10: bulk_updateの専用構文と呼出し形式

### 元の指摘と今回の判断

元の監査では、Spireがソース上の完全修飾名 `Facet::bulk_update` を認識して専用ASTを作り、外側のcalleeを通常の関数として解決しない点を確認対象としていた。Sigilはsourceを `Ok(source)` で包み、entryを `Facet::set` / `Facet::over` / `Facet::over_result` / `Facet::case_set` / `Facet::case_over` の逐次合成へlowerする。生成した各関数呼出しは通常の名前解決・型検査を通る。

今回の判断は、この専用構文としての契約を維持したうえで表記を追加すること。元の監査の「利用者分類: 未選択」は、この項目について選択済みとなる。`Facet::bulk_update(source) { ... }` も維持し、裸の `bulk_update` は予約キーワードとして扱う。外側の `bulk_update` を通常の関数値として解決する経路は追加しない。必須の専用ブロックがなければparse errorとし、生成したFacet関数の解決・型検査・実行時の失敗を、成功値の生成や別経路で救済しない。

### 追加する表記

通常関数呼出しの形でも記述できるようにする構文上の拡張とする。各形式は同じ専用ASTへ接続し、既存の意味論とブロック内の文法を維持する。

`match`は次の3形式を等しく扱う。

```surtr
match ARG1 { ... }
match(ARG1) { ... }
match(ARG1, { ... })
```

`bulk_update`は値の引数を括弧で囲む。次の2形式を扱い、`bulk_update value { ... }`は禁止する。

```surtr
bulk_update(value) { ... }
bulk_update(value, { ... })
```

`cond`は次の括弧有無だけを許容する。

```surtr
cond { ... }
cond({ ... })
```

`{ ... }`は各専用ブロックの内容を省略した表記。空ブロックの受理や、ブロックを一般の値として扱う機能を追加する意味ではない。named argument、パイプ注入、ブロック内区切りなどの新しい規則は今回の構文追加に含めない。

以前の「一つの正規形を選び、外置きブロックを拒否する」という提案は撤回する。`bulk_update`のキーワード化・シャドーイング禁止という既存方針は維持する。既存の`Facet::bulk_update(source) { ... }`も、同じ専用構文として維持する。

### 実施記録（2026-10-04、level4）

Spireで指定の3形式の`match`、2形式の`cond`と`bulk_update`を同じ専用ASTへ接続した。`bulk_update`をキーワードにし、同名の通常宣言・引数・束縛を拒否する。標準のintrinsic宣言と既存の`Facet::bulk_update(source) { ... }`は維持し、修飾表記でも括弧内に更新ブロックを置ける。tolerant parserのkeyword分類も整合させた。

`match`は既存のタプル対象・括弧後の演算式・タプル内クロージャを維持する。括弧内の専用arm blockはtokenの括弧深さとarmの矢印で判別し、失敗後に別文法で再解析する経路は設けていない。外側のcallee解決、Sigilのlower、型・評価規則は変更していない。

新構文・予約境界のテストで旧実装の失敗を確認してから実装した。Spire全504件が成功。既存の`cond_provenance_runtime`と`facet_dynamic_container_path_expr`を新表記へ変更し、対応するscript fixture bucket 1・5の2件が成功した。既存の期待出力は変更していない。独立レビューは未解決の指摘なし。最終のworkspace CIは2300件・56バイナリがすべて成功し、標準Surtrテストもexit 0だった。

正本の言語リファレンス・Facetガイド・BootstrapとFacetの`@doc`・テスト方針を更新した。元の監査で未選択だったcalleeの扱いも、専用構文として維持する判断を記録した。

## SR-11: Extractorの呼出し先は一意

Extractorも通常関数と同じく、呼出し先が一意であることを保証する。再走査などは不要とする。

この前提でExpr／Patternの候補保持と後続処理を整理する。今回、新たな探索・再試行経路は提案しない。

## SR-12: importと型・Constの可視性を分ける

importは関数・Extractor専用のキーワードとして全体で統一する。型・Constの可視性と話を混ぜない。

型とConstはどのスコープからも同じ見え方とする。ただしprivate constはファイルスコープに閉じる。

### 現行確認（2026-10-04）

Sigil `build_global_scope`はpublic Constを裸名・修飾名で導入し、private Constはそのファイルのscopeだけに導入する。`is_importable_declaration`はConst・Struct・builtin型・`new`をimport対象から除く。既存module fixture `public_const_cross_file`、`private_const_visibility_forbidden`、`duplicate_public_const`が可視性・衝突を固定している。

追加の複数ファイルprobeではpublic `VALUE`をimportなしで参照できた。同じmoduleの関数importは成功し、Constのimportは `Import target not importable`、型のmember importは `Unknown import member`、enum型をmoduleとするvariant importは `Unknown module import` で停止した。宣言kindだけでimport可否を判断せず、実際のnamespace経路も含めて確認した。型・Constの可視性変更は不要。

## SR-13: Structのnewとdeconstructは明確な言語仕様

`new`の定義を必須にすることで、コンストラクタ表記から呼出し先をコンパイル時に解決できることを保証する。「確実に成功する」はこの呼出し先解決を指す。実行時に返す`Result`値とは関係しない。

Pattern側のExtractorは、定義されていなければコンパイル時のエラーで処理を止める。

この仕様を維持し、未定義Extractorを別の分解経路で救済しない。

### 現行確認（2026-10-04）

`docs/site/structs.md`は `new -> Result<Self, Error>` を許可し、Scar の `struct_new_accepts_result_self_return_type` / `struct_constructor_call_accepts_result_return_type` がこの境界を検証する。実際に `new(value)` が負数で `Err(NoneError)` を返す Struct を実行し、constructor呼出しが `Err(NoneError("None Value."))` を返すことを確認した。

利用者の補足により、「確実に成功する」はコンパイル時の呼出し先解決を意味すると確認した。前回の調査では実行時の成功保証との解釈を未確定事項にしたが、その扱いを訂正する。上記の`Result`を返す契約と矛盾せず、実行時の戻り値や成功・失敗の規則を変更する必要はない。

一方、`new`を定義し`deconstruct`を定義しないStructをPattern headに使うと、Sigilは `requires attached extractor ... but it is not defined` で停止した。構造分解への救済は追加しない。

### 文書整合の実施記録（2026-10-04）

`docs/site/structs.md` と `docs/dev/Pattern_spec.md` に、`new` の必須条件がコンパイル時の呼出し先解決を保証すること、実行時の `Result` の成否とは独立することを記載した。`deconstruct` 未定義時は名前解決エラーとし、フィールドの直接分解や別Extractorで救済しない点も明記した。処理系・テストは変更していない。文書差分を確認し、`git diff --check` は成功した。

## 後段タスクへの引継ぎ

最初にSR-01の関数ごとのキーワード・シャドーイング・呼出し先解決の仕様を詰める。SR-07はその決定後にフローを設計する。SR-09の区別は確認済み。SR-10は上記の構文方針に沿って実装した。

SR-02〜04は標準環境を前提に入口とテスト構成を統一し、SR-05は内部lookup失敗を明確なエラーにする。SR-06／08／12／13は回答で示された仕様を前提に扱う。SR-11は呼出し先の一意性を前提とし、再走査を加えない。

完了の判断は各項目の実施記録による。構文・予約範囲の未確定事項と、実装の内部選択だけで進められる項目を区別し、未確定仕様を実装済みとして扱わない。

## 最終検証（2026-10-04）

- `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace --test-threads 4`: 2292件成功、56バイナリ、exit 0。
- `SURTR_TEST_CACHE=1 rtk proxy cargo run -- test --quiet --all`: exit 0。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。
- 独立レビュー: canonical identity、環境・import共通化、rollback、標準helper、解析cacheの最終コード差分について未解決の指摘なし。

最初の全体検証で見つかったLSPテストの標準 `print` との名前衝突は、テスト用symbolを `fixture_print` へ変えて修正した。同時実行時の15秒timeoutは並列数を4へ下げて全件を再実行し、成功を確認した。テストの除外・ignored化・時間上限の変更は行っていない。

## SR-10の最終検証（2026-10-04）

- `SURTR_TEST_CACHE=1 rtk cargo nextest run -p spire`: 504件成功。
- `SURTR_TEST_CACHE=1 rtk cargo nextest run -p rune --test integration -E 'test(run_srt::script_fixtures_bucket_1) | test(run_srt::script_fixtures_bucket_5)'`: 対象2件成功。
- `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace --test-threads 4`: 2300件成功、56バイナリ、exit 0。失敗・除外なし。
- `SURTR_TEST_CACHE=1 rtk proxy cargo run -- test --quiet --all`: exit 0。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。
- 最終コード差分の独立レビュー: 未解決の指摘なし。
