# docs 横断調査 — バグ・曖昧性・改修対応項目

調査日: 2026-10-05。基準コミット: `0ef9260806b1c5fa1c765e8c0a70e2f8b2ce5ac5`。

## 目的と調査方法

利用者向け文書、開発仕様、作業メモを横断し、現行実装・標準定義・テストと合わない記述と、公開契約が曖昧な箇所を探した。言語、runtime・標準定義、CLI・REPL・LSPの3領域をサブエージェントに分担し、主担当が文書の導線・残件管理と各結果を確認した。

今回は調査と対応条件の整理まで。製品コード、実行可能テスト、既存文書は変更していない。変更levelは後続作業の目安であり、この報告を確定済みの実装仕様として扱わない。

「実装バグ」は既存の契約に反する現行挙動、「文書不整合」は現行契約への追随漏れ、「曖昧性」は公開境界の説明不足を表す。利用者による把握済み／未把握の分類は行わない。優先度はデータ損失、通常の例の失敗、残件判断への影響から付けた。

直近の更新は次のように扱った。

- `0ef92608`: REPLのResult-effectエラー保持の回帰境界を追加。削除された追補は残件へ戻さない。
- `31c553a7` / `bebb8fec` / `c36c1dbd` / `ccf2c882`: 標準callableの宣言identity、予約名、中置呼出し、利用者向け説明を更新。これ以前のshadowing記録は現行の根拠にしない。
- `39f1dfa3`: 標準環境・importを共通化。古い独立入口の調査記録を、そのまま現行バグとして再掲しない。
- `de1f700a`: `List::map` / `filter` を共有 `flat_map` builtin経由へ移行。

## 項目一覧

| ID | 優先度 | 分類 | 対応対象 |
|---|---|---|---|
| DA-01 | 高 | 実装バグ | JSON巨大整数のFloat降格・精度損失 |
| DA-02 | 中 | 実装バグ | FSの巨大負depthがRuntimeErrorになる |
| DA-03 | 中 | 実装バグ | LSP file URIのauthorityが相対pathになる |
| DA-04 | 中 | 文書不整合 | JSONの旧decode API・旧impl署名 |
| DA-05 | 中 | 文書不整合 | Fileの旧closure・mode表記 |
| DA-06 | 中 | 文書不整合 | File例の戻り値型 |
| DA-07 | 中 | 文書不整合・契約整理 | FileHandleの保存禁止という説明 |
| DA-08 | 中 | 文書不整合 | `\|*>` のcontextual mapper結果の受理条件 |
| DA-09 | 低 | 対応済み（文書） | 存在しないNeq traitの列挙 |
| DA-10 | 低 | 対応済み（文書） | `T?` と `Option<T>` の機能差を示す説明 |
| DA-11 | 中 | 対応済み（文書） | 同じdirect TypeConstructor traitのwitness共有 |
| DA-12 | 中 | 文書不整合 | 比較Boolean helperが非公開という説明 |
| DA-13 | 低 | 文書不整合 | Docs chunk・標準型の構成説明 |
| DA-14 | 低 | 文書不整合 | 削除された計画へのリンク |
| DA-15 | 中 | 文書不整合 | SRメモの予約名・shadowing状況 |
| DA-16 | 低 | 残件管理 | OI-036の未確定扱い |
| DA-17 | 低 | 残件管理 | ListのRT-6の対象記述 |
| DA-18 | 低 | 残件管理 | 存在しないOI-038への案内 |
| DA-19 | 低 | 文書配置 | docs内の過去の実装計画 |
| DA-20 | 中 | 曖昧性 | Shellのsignal終了時のexit_code |

計20件。実装バグ3件、文書不整合・契約整理12件、曖昧性1件、残件管理3件、文書配置1件。既存監査の残件は後述し、新規件数へ加えていない。

## 実装の対応項目

### DA-01 JSON巨大整数が成功値のまま精度を失う

- 根拠: `docs/dev/Json_spec.md:42-43`、`docs/site/json.md:32-34` は整数literalを `JsonValue::Int`、小数・指数literalを `JsonValue::Float` と定める。
- 原因: `crates/eldr/src/builtin.rs:4245-4260` はi64/u64へ入らないnumberをf64へ降格する。`crates/eldr/Cargo.toml:17` のserde_jsonには `arbitrary_precision` 指定がない。
- 実測: 現行ソースのビルド後に次の入力を実行し、exit 0。

```surtr
print(inspect(Json::parse("18446744073709551617")))
print(inspect(Json::stringify(JsonValue::Int(18446744073709551617))))
```

```text
Ok(JsonValue::Float(18446744073709552000.0))
Err(JsonEncodeError("json encode error: JsonValue::Int cannot be represented as a JSON number: 18446744073709551617"))
```

- 対応: 整数literalの分類・値をBigIntとして保つ。巨大整数のstringifyが `JsonEncodeError` になること自体は、stringify不能値の拒否契約と分けて扱う。stringifyの公開範囲も整合確認するが、今回の確定バグはparseが丸めた値を成功として返す点。
- 受入条件: i64/u64境界とその外側の正負の整数を、Floatに変えず正確にparseする。decimal/exponentの現行分類を保つ。旧降格経路は残さない。
- テスト境界: `crates/eldr/src/builtin.rs:5484` 付近の既存JSON分類テストは通常サイズを確認する。数値境界はruntime層へ置き、JSON文書の分類例も確認する。目安level3。

### DA-02 FS::tree_depthの巨大負数がResultの外へ落ちる

- 根拠: `docs/dev/FS_Shell_spec.md:218-219` は `depth < 0` を `Err(FileSystemInvalidDepth(...))` と定める。
- 原因: `crates/eldr/src/builtin.rs:3261-3271` が負数判定前にi64へ変換し、`:3951-3961` が変換失敗をRuntimeErrorにする。
- 実測: 次の入力でexit 1、`filesystem_tree_depth Int argument depth is out of range for i64: -18446744073709551617`。

```surtr
root =? FS::path(".")
print(inspect(FS::tree_depth(root, -18446744073709551617)))
```

- 対応: BigIntの符号を先に判定し、すべての負数を公開APIの `Err` として返す。非負の巨大depthについてもOS側の有限表現との境界を明記する。
- 受入条件: `-1`、i64最小値より小さい負数、`0` を区別して検証する。正常な型の公開入力を内部契約違反へ変えない。目安level2、公開範囲の新規決定が必要ならlevel4。

### DA-03 LSP file URIのauthorityをpathの一部にしてしまう

- 根拠: `crates/surtr-lsp/src/lib.rs:350-353` の `file_uri_to_path` が `file://` と文字列prefix `localhost` を取り除く。`:131-140` のdocument登録がその結果を使う。
- 公開APIの実測: `file://localhostevil/repo/main.srt` は `Some("evil/repo/main.srt")`、`file://remote/repo/main.srt` は `Some("remote/repo/main.srt")`。query / fragmentもファイル名に残る。
- 問題: authority付きURIが作業ディレクトリ相対の別ファイルへ対応する。特にlocalhostのprefix判定はホスト境界を区別していない。
- 対応: authorityとpathを分けて解釈する。ローカルURIだけを扱う場合、空authorityと正確なlocalhost以外を明示拒否する。query / fragmentの受理条件も固定する。
- 受入条件: 非対応URIを相対pathとして登録しない。`crates/surtr-lsp/tests/adapter.rs:15` の空白・日本語・percent encodingのround-tripを保ち、authority境界を公開API層で検証する。目安level2。

## 文書の対応項目

### DA-04 JSONに旧decode経路とReturnTypeArgumentのないimplが残る

- 根拠: `docs/site/json.md:14,41-42,72,80,93-94` の `JsonValue::decode(T)`、`:92,100` の `def decode(self...)` / `def encode(self...)`。`docs/dev/Json_spec.md:57-65,152,164` にも旧trait署名・テスト基準が残る。
- 現行: `lib/traits/decode.srt:15` は `def decode::<$To>`。`lib/types/json.srt:171-183` にdecode memberはない。`tests/fixtures/script/pass/json/decode_pipeline.srt:2`、`custom_config_decode.srt:4-8` が現行例。
- 実測: `JsonValue::decode(Int)` は `Undefined function JsonValue::decode/1`。旧impl宣言は `Trait impl method decode has incompatible ReturnTypeArgument arity: expected 1, got 0`。いずれもcheck exit 1。
- 対応・受入条件: 呼出しを `Decode::decode::<T>`、impl宣言を現行RTA付きへ揃える。旧APIを復活させない。site・dev・標準 `@doc` の独立コード例をcheckし、想定出力まで確認する。

### DA-05 Fileのclosure・mode例が現行構文では動かない

- 根拠: `docs/site/file-io.md:96,143,155`、`lib/file.srt:191,210,227` の `fn(file) { ... }` と裸の `Write` / `Read` / `Append`。
- 実測: `import File` の例は `Undefined variable: Write`。modeだけを `FileMode::Write` に直しても `Undefined function fn/2`。
- 現行例: `lib/tests/modules/file.srt:27,46` の `FileMode::Read` / `FileMode::Write` と `{|handle| ...}`。
- 対応・受入条件: siteと正本 `@doc` を現行closure・Enum variant表記へ揃える。必要な一時ファイル・ディレクトリを示し、各例を単独でcheck/runできる形にする。

### DA-06 Fileのwrite/read例が宣言した戻り値型と違う

- 根拠: `docs/site/file-io.md:197-200` は `Result<()>` の関数から `File::read(path)` を返す。`lib/file.srt:119` のreadは `Result<String>`。
- 実測: `Return type mismatch: expected Result<Unit>, got Result<String>`、check exit 1。
- 対応・受入条件: 読んだ文字列を返す例なら `Result<String>` にする。読み書きの検査例なら値を検査して `Ok(())` を返す。例の目的と結果を揃え、check/runする。

### DA-07 FileHandleの「保存できない」が現行の資源管理と違う

- 根拠: `lib/file.srt:46-48` は利用者がhandleをstoreできないと説明する。一方 `:198` のwith_openは `Result<$A>` を返す。
- 実測: 存在するUTF-8ファイルを `path` として次のコードが成功し、exit 0、`Err(FileClosed("file is already closed"))`。

```surtr
import File
handle =? File::with_open(path, FileMode::Read, {|file| Ok(file)})
print(inspect(File::read_chunk(handle, 1)))
```

- 境界: `docs/dev/EldrVM_spec.md:191,193` はcallback終了・失敗時のcloseとcheckpoint復元を定め、静的保存禁止を定めていない。`crates/scar/src/checker/types.rs:1068` は通常のbuiltin型として扱う。
- 対応・受入条件: 現行契約へ追随するなら、保存は可能だがcallback外では閉じたhandleとなりFileClosedを返すと明記する。静的escape禁止を選ぶ場合は別途level4の仕様整理が必要。今回、実装漏れとは断定しない。

### DA-08 `|*>` のmapper結果を一律に拒否すると読める

- 根拠: `docs/site/function-operators.md:106-107` は `A -> Result<B>` を受けないと断言する。`docs/site/language-reference.md:367-368` にも同じ拒否説明がある。
- 現行テスト: `crates/scar/tests/common_constructor_invocation.rs:121-153` は、期待型が明示されたnested carrierなら成功し、未注釈ならCallableShapeMismatchになる境界を固定する。
- 現行処理系の実測: 次の入力はexit 0、`Ok(Ok(1))`。

```surtr
value: Result<Result<Int>> = Ok(1) |*> {|x: Int| Ok(x)}
print(inspect(value))
```

- 対応・受入条件: plain mapperの推論規則と、明示されたnested expected typeによる受理を分けて説明する。成功・拒否の両例を置き、`|>=` との違いも値の入れ子／flattenで示す。既存の受理条件を文書だけで禁止へ変えない。

### DA-09 存在しないNeq traitをoperator dispatchに列挙する

- 根拠: `docs/site/trait-impls.md:13` がNeqを列挙。
- 現行: `lib/traits/operator/eq.srt:16-23` のEqがneqを持ち、`crates/scar/src/checker/expr.rs:13337` 付近の `!=` はEqへ接続する。
- 対応・受入条件: `!=` / `neq` はEqの契約であることを示す。独立Neq traitが必要だと読める説明を除く。静的照合のみ。

#### 対応記録（2026-10-07）

`docs/site/trait-impls.md` から独立した `Neq` の列挙を除き、`==` / `eq` と `!=` / `neq` が `Eq` の契約であることを明記した。`lib/traits/operator/eq.srt` の宣言と Scar の演算子解決を静的に照合した。文書だけの変更で、実行テストは行っていない。`git diff --check` で差分を確認した。

### DA-10 `T?` と `Option<T>` の記法に機能差があるように説明する

- 根拠: `docs/site/structs.md:227-257` は `T?` の方がFacet更新パイプを短く保てると説明し、`Option<T>` の例にだけ変換を要求する。
- 現行: `crates/spire/src/parser/ty.rs:95-104` は `T?` をOptionのgeneric型へ変換。同ページ`:256` も同一型と明記する。
- 対応・受入条件: パイプの短さは、値変換とFacetのSome selectorによる操作の違いとして説明する。両方の型表記で同じFacet操作が使えることを示す。静的照合のみ。

#### 対応記録（2026-10-07）

`structs.md` と `standard-library.md` で、`T?` と `Option<T>` が同じ型であることを前提に説明を揃えた。値を取り出して変換する例と、Facet で構造体を更新する例の結果を区別し、どちらの型表記でも同じ操作を使えると明記した。Spire の型構文と `lib/facet.srt` の契約を静的に照合し、リンク先と `git diff --check` を確認した。実行例の内容は変更せず、実行テストは行っていない。

### DA-11 direct parameterのwitness共有条件が食い違う

- 根拠: `docs/site/trait-impls.md:164-166` は別々のdirect parameterを同じfamilyでも独立と説明する。
- 併読対象: `docs/dev/Trait_system_spec.md:239-242`、`docs/site/trait-system.md:146` は同じdirect Trait名なら共有、異なるTrait名なら独立と説明する。`crates/scar/tests/typecheck_surface.rs:4987-5022` の `same_trait_constructor_parameters_share_one_witness` が同名Traitに対する別carrierの拒否を固定する。
- 対応・受入条件: same familyという能力分類と、同じTrait identity、名前付きconstructor variable、trait methodのSelfを区別する。独立になる条件と共有する条件を成功／拒否例で揃える。現行のwitness関係を変更する提案ではない。

#### 対応記録（2026-10-07）

`trait-impls.md` を、同じ direct Trait 名を使う引数・戻り値は同じ型コンストラクタを要求し、異なる Trait 名は独立する説明へ修正した。名前付き変数と Trait method の `Self` も区別した。`Trait_system_spec.md`、`trait-system.md`、Scar の `same_trait_constructor_parameters_share_one_witness` を静的に照合し、リンク先と `git diff --check` を確認した。実行例の追加や実行テストは行っていない。

### DA-12 比較Boolean helperは現行では公開されている

- 根拠: `docs/site/standard-library.md:53` は専用Boolean helper名を公開しないと説明する。
- 現行: `lib/traits/operator/compare.srt:4-7,34,49,64,79` はautoimportされる `lt` / `lte` / `gt` / `gte` を定義し、中置優先順位と予約名も説明する。
- 実測: `lt(1,2)`、`lte(1,1)`、`gt(2,1)`、`gte(1,1)` はすべてTrue、run exit 0。
- 対応・受入条件: 標準ライブラリの説明をCompareの公開surfaceへ揃える。callable名の予約・中置規則は `docs/site/callable-names.md` へつなぐ。

### DA-13 標準ライブラリ構成の説明が古い

- 根拠1: `docs/site/standard-library.md:6` はDocs chunkからの参照を将来扱いにする。現行の `crates/rune/src/compile.rs:604,752` はdocをbytecodeへ格納し、`crates/sindr/src/ir.rs:1440-1441,1511` はDocsを保存・復元する。
- 根拠2: 同ページ`:82-87` は各type moduleを `@builtin type` と `defmod` の2層に一律分類する。`lib/types/result.srt:27`、`option.srt:29`、`either.srt:13` は通常Enum、`range.srt:13` はStruct。
- 対応・受入条件: 実装済みのdoc保持と今後の利用機能を区別し、builtin型とsource定義型を分けて説明する。ソースから得られる完全inventoryを手書きで複製しない。静的照合のみ。

## 導線と残件管理

### DA-14 削除された仕様・計画へのリンクが4か所残る

Markdownのローカルリンクを確認し、調査開始時の `docs/` 76ファイル、`doc/` 43ファイルで次の欠落を確認した。コードブロック内、外部URL、リンクのanchorの妥当性はこの検査の対象外。

| 参照元 | 存在しない参照先 |
|---|---|
| `docs/dev/EldrVM_spec.md:247` | `doc/callable-display-origin-spec.md` |
| `doc/open-issues.md:351` | `doc/callable-display-origin-spec.md` |
| `doc/sr_revision_notes.md:21` | `doc/sr_call_resolution_syntax_plan.md` |
| `doc/test_command_release_build_memo.md:11` | `doc/lib_tests_reorganization_pr.md` |

対応・受入条件: 移管先の正本または現行テストへ導線を差し替える。`afdee5cf` で削除された呼出し計画をリンクのためだけに復活させない。リンク先が存在し、受理条件・未確定事項をたどれること。

### DA-15 SRメモの最新状況が予約名改修より前で止まる

- 根拠: `doc/sr_revision_notes.md:9,12` の状況表、`:94-102` のSR-06は `def on` によるshadowを現行確認として記録する。
- 現行: `bebb8fec` と `docs/site/callable-names.md` は標準callable名を予約する。`crates/sigil/src/resolver/declarations.rs:1263` の `validate_reserved_callable_declaration` が宣言を拒否する。
- 実測: `def on(x: Int, y: Int) -> Int { x + y }` はcheck exit 1、`Callable name on is reserved for its standard declaration Function::on`。
- 対応・受入条件: 当時の実施記録は履歴として区別し、状況表と後段への引継ぎを現行へ更新する。SR-07の本当に未確定な部分まで解決済み扱いにしない。旧shadow成功例を現行の受理例として案内しない。

### DA-16 OI-036は確定済みのFacet capture契約を未確定としている

- 根拠: `doc/open-issues.md:348-363` はFacet captureの表示名・canonical identityを未確定とする。
- 現行: `docs/dev/EldrVM_spec.md:249-253`、`lib/facet.srt:159-173` はFacet/view identityを明記する。`crates/forge/src/lib.rs:1525` の `facet_api_capture_preserves_resolved_callable_metadata` と `tests/fixtures/script/pass/functions/facet_view_capture_scope.{srt,expected}` が各APIとpath captureのmetadataを固定する。
- 対応・受入条件: 上記で固定済みの範囲を台帳から除く。optional/fallible・再captureなど、元の受入inventoryの残りを確認してから項目全体の削除を判断する。既存テストを読んだ静的照合であり、本調査ではその全inventoryを再実行していない。

### DA-17 RT-6にmap/filterの移行済み範囲が反映されていない

- 根拠: `doc/list_runtime_followups.md:64-66` はmap/filterをbuiltin化の対象外としてまとめる。
- 現行: `de1f700a`、`lib/types/list.srt:394-405` で両者は共有flat_map builtinへ合成される。
- 対応・受入条件: 専用builtinを追加していない点と、Builderを使う共有経路への移行済み範囲を分けて記述する。reverse/append/concat、多段pipelineの融合、物理コピーの計測は別の残件として保つ。map/filterが旧source再帰経路のままだと読める説明を除く。

### DA-18 プロセス設計案が存在しないOI-038を案内する

- 根拠: `doc/process_host_redesign_proposal.md:124` は `doc/open-issues.md` のOI-038を参照するが、現行台帳にはその項目がない。
- 対応・受入条件: 未採用案の未確定事項を台帳へ登録するか、案の中の該当節へ案内する。番号だけを予約して登録済みと読ませない。プロセスの現行仕様を、この案で上書きしない。

### DA-19 docs配下に過去の実装計画が残る

- 根拠: `docs/dev/README.md:6-8` は実装計画・superpowersの一時メモをdocへ整理する方針。現行には `docs/superpowers/plans/` / `specs/` / `records/` が残る。
- 例: `docs/superpowers/records/2026-08-21-type-identity-owner-registry-task6-checkpoint.md:35-46` に当時の未完了タスクと旧診断がある。複数のplanは削除済み `doc/要件定義v9.md` を入力にする。
- 対応・受入条件: 履歴と現行仕様を区別し、必要な契約が正本へ移管済みかを確認して配置を整理する。当時の失敗3件を現在のworkspace失敗として再起票しない。無差別な削除や、未採用案の正本化は行わない。

## 公開契約の追加整理

### DA-20 Shellのsignal終了時のexit_codeが説明されていない

- 根拠: `docs/dev/FS_Shell_spec.md:111-120` は起動後の非ゼロ終了をCommandResult.exit_codeで表す。`crates/eldr/src/builtin.rs:3407` はOS終了codeがなければ `-1` にする。
- 確認範囲: 静的読解のみ。signal終了のコマンドは実行していない。
- 対応・受入条件: `-1` をsentinelとして公開するのか、signalを別に表すのかを仕様化する。起動失敗と起動後の異常終了を区別し、標準 `@doc`・site・devを揃える。既存のOk/Err契約を独断で変更しない。説明だけなら局所文書対応、表現変更ならlevel4。

## 既存の残件・解決済みとの区別

| 項目 | 今回の扱い |
|---|---|
| 長いConsのdrop、Packedのsuffix保持、checkpointコピー、未分割builtin | `doc/list_runtime_followups.md` とrelease auditの既存残件。現行閾値・性能は未測定で、新規発見に数えない |
| LSPの各要求での全量再解析・context cache | `doc/v0.1_release_codebase_audit.md:90-91` の既存残件。`snapshot_for_uri` がanalyzeを呼ぶことは再確認 |
| ProjectRunner executor失敗のsource-only救済 | 同監査の完了項目。現行set_runner_selectionは診断を保持するため再起票しない |
| WarningのCLI・REPL未接続 | `docs/site/warnings.md` とOI-033に明示された既存課題 |
| release buildのtest --all | `doc/test_command_release_build_memo.md` の未設計範囲。開発用標準テスト契約から推定しない |
| Result/OptionT<Result>のREPLエラー保持 | HEADの回帰テストと文書を確認。解決済み追補は戻さない |
| compilerの再帰stack問題 | `doc/optimize/021_recursive_compiler_stack_usage.md` の修正・検証記録を確認。旧018の原因説明から現行stack overflowとは判断しない |
| 古いstdlib precompile・phase timingの監査 | 当時の計画・観測として扱う。現在のcacheやtimingの不在証拠には使わない |

## 検証結果と限界

- `cargo build -p rune --bin surtr`: exit 0。JSON、FS、比較helper、予約on、nested mapの主担当probeはこの後のbinaryで実行した。
- `/tmp` のJSON整数parse/stringify probe、nested map、比較helper: run exit 0、本文の出力を確認。
- JSON旧API、旧impl署名、File旧表記・戻り値: checkで狙った拒否診断を確認。FS巨大負depth: run exit 1でRuntimeErrorを確認。
- FileHandle保存・終了後利用: 一時ファイルを使ったrun exit 0、FileClosedを確認。
- LSP URI: 一時Rust probeから公開APIを直接呼んで確認。実際のeditor上のopen操作は未検証。
- 文書リンク検査: 調査開始時の119 Markdownファイルを対象に、DA-14の4か所を検出。
- workspace全体、標準Surtrテスト全件、GUI/TUI操作、性能測定は未実施。文書例の全件自動実行もしていない。

既存Scarテストは `rtk cargo nextest run -p scar -E 'binary(common_constructor_invocation) | test(typecheck_surface_bucket_0)'` を実行し、3 passed / 373 skipped、exit 0。対象のwitness共有とplain mapper推論のケースを含む。初回の内部関数名フィルタは登録名に一致せず0 passed / 376 skipped、exit 4だったため、suite/bucket名へ修正した。選択外のskippedを全件成功とは扱わない。

LSPは `cargo build -p surtr-lsp` がexit 0。この後の `target/debug/libsurtr_lsp.rlib` を使って次のprobeをcompile/runし、ともにexit 0、DA-03の結果を再確認した。

```rust
fn main() {
    for uri in [
        "file:///repo/main.srt",
        "file://localhost/repo/main.srt",
        "file://localhostevil/repo/main.srt",
        "file://remote/repo/main.srt",
        "file:///repo/main.srt?query",
        "file:///repo/main.srt#fragment",
    ] {
        println!("{} => {:?}", uri, surtr_lsp::file_uri_to_path(uri));
    }
}
```

上記を一時ディレクトリの `uri_probe.rs` へ保存し、rootで `rustc --edition=2021 <uri_probe.rsのパス> -L dependency=target/debug/deps --extern surtr_lsp=target/debug/libsurtr_lsp.rlib -o <probeのパス>`、生成したprobeの順に実行した。Fileの一時入力とURI probeは `/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr_docs_audit_final_s_u3g3c8/` に保存した。JSON・FS・mapper・比較・予約名の一時入力は `/tmp/surtr_docs_*.srt`。これらは再現補助であり、恒久的な回帰テストではない。

独立点検では、20件の集計、根拠、履歴と現行状態の区別、対応条件を確認した。witness共有の分類を文書不整合へ揃えた。報告書の差分チェックとMarkdownのID・code fence確認は成功。yomiyasuの候補は監査項目の並列構造と技術用語を保って点検した。

## 読み取り範囲

全docsの全文精査を完了したという報告ではない。119ファイルの導線検査に加え、次の文書・関連節を現行ソースと照合した。

- 言語領域: callable名・callable・capture・関数演算子・言語リファレンス・型注釈・trait/impl・struct/Record・Pattern/Extractor・Facetと、対応するdev契約・標準定義・Scar/Spire/Sigilの実装とテスト。
- runtime領域: Json、FS/Shell、Lazy、Doのdev/site全文、Processのsite、standard-modules。EldrVM、ProcessRuntime、diagnostics、error-handlingの関連節。
- tooling領域: Rune_cli、siteのtest/warnings/shell/file-io、internalのtail-call文書、再帰stack計画021/018、File/Shell標準定義。Xldr、LSP、observability、テスト方針の関連節。
- 文書管理: dev/site/internal入口、open-issues、SR改修メモ・標準環境計画、標準共有監査、release audit、List残件、release testメモ、プロセス設計案。`doc/optimize/*.md` 24ファイルは見出し・状態を横断確認。
- 未追跡の既存 `doc/callable_name_and_syntax_classification.md` は利用者の作業として保持し、現行実装の正本には使っていない。

## 次の作業

まずDA-01〜03の再現入力と公開契約を実装仕様へ整理する。文書修正はJSON/Fileの動かない例、callable予約・mapper推論・witness共有の説明を優先する。導線と残件管理は、移管先と未解決範囲を確認してからまとめて整合させる。各対応で置換した旧経路・旧受理例を現行の成功例として残さない。
