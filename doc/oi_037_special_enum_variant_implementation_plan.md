# OI-037 Result / Boolean constructor の通常 Enum 経路統合

状態: 実装完了。2026-10-04 の実装依頼により専用ワークツリーで作業した。
この文書は実装入力と検証記録であり、利用者向け・開発者向けの現行契約は正本文書を参照する。
全体検証と最終差分レビューの結果は末尾に記録する。

## 入力仕様と目的

`doc/open-issues.md` の OI-037 と、2026-10-04 の追加指定を実装対象とする。
`Result` / `Boolean` は通常 Enum と同じ宣言、owner / variant 解決、型引数指定、
constructor capture、Pattern を使い、variant 固有の型制約と runtime lowering だけを
コンパイラの canonical metadata で区別する。変更は level 4 とする。

- `Result::Ok`、`Result::Err`、`Boolean::True`、`Boolean::False` は、パーサ側で
  `TypeIdentity::SpecialEnumVariant` として通常 variant と区別する。
  この分類を後段まで別経路として維持する必要はなく、早い段階で通常 Enum の表現へ
  正規化する。owner 自体の identity と variant の分類を混同しない。
- bare `Ok` / `Err` / `True` / `False` は対応する canonical variant の alias とする。
  alias 用の宣言、constructor family、UID、runtime tag を作らない。
- bare alias 名は共有の予約名登録で保護する。他の owner、定義、import alias、
  local binding、引数から bare 名を再定義・shadow できない。
  `Other::Ok` のような修飾 variant 名は bare alias と別の名前として扱う。
  `MatchResult::Ok` / `MatchResult::Err` もこの規則で保持し、bare alias を追加しない。
- AST / Resolved は通常 Enum と同じ形を使う。`ResultCtorDecl` と
  `DeclarationKind::ResultCtor`、placeholder UID、名前だけによる variant fallback は削除する。
- `Ok` / `Err` / `print` の固定 fid 割り当てを削除し、コンパイラが登録・追加した順に
  通常の allocator で割り当てる。canonical 名からの参照も alias も登録結果を使う。
- 標準 Enum の canonical shape、Result の Error 制約、nested Result の失敗伝播、
  Boolean の runtime 表現は保持する。schema / VM version は上げない。

## 統合後の契約

### canonical metadata

Sindr に4つの special variant の owner、qualified name、bare alias、payload arity、
lowering kind を一元登録する。パーサでの分類は、宣言の登録・名前解決までに通常 Enum の
owner / variant 表現へ正規化する。Sigil は通常 Enum 宣言の登録時にその metadata を
照合し、通常 variant と同様に UID を割り当てる。alias は同じ UID を参照し、ResolvedId の
canonical 名と symbol identity は qualified spelling と一致させる。
正規の標準宣言がない状態では未定義エラーとし、テスト用の synthetic constructor を
製品の初期 scope に残さない。単独フェーズのテストで必要なら正規の Enum 宣言を入力する。

### パーサでの区別と早期正規化

パーサは special variant の構文上の分類と bare alias を識別してよい。
AST の本体は通常 Enum と同形とし、正規化後の direct call、capture、Pattern は
同じ canonical owner / variant への参照を使う。`SpecialEnumVariant` というパーサ側の
分類を Scar / Forge 用の別 AST や別 constructor family として持ち越さない。
型制約・lowering に必要な情報は、正規化済み variant の canonical metadata から取得する。

`module_path` の有無で通常経路と special 経路を切り替えない。
モジュールは名前解決・可視性・宣言の所在を決める情報として扱い、同じ canonical 宣言の
呼び出し方式や型推論経路を選ぶ条件にはしない。標準の canonical 宣言であることと
宣言の shape を検証する契約は保持する。
旧 `should_parse_result_ctor_decl()` の `module_path.is_some()` による分岐と、
Result constructor 宣言だけを別 module へ移す staging / fallback 処理は削除する。

### fid の追加順への統一

利用者が指定した fid の固定割り当てには、現行の compile-space UID の bootstrap も含める。
Sigil の constructor seed、Scar `initialize_env()` の `Ok=0` / `Err=1`、
`builtin_uid()` の `BUILTIN_UID_BASE + builtin_id` による `print` 等の固定 UID を削除する。
builtin の compiler symbol も通常の登録処理で追加し、その時点の allocator の次の ID を使う。
runtime builtin metadata の一覧を理由に compiler symbol 用の ID 領域を先取りしない。

compiler symbol UID と bytecode の `fun_idx` は、それぞれの登録・追加順で割り当てる。
後段で生成する constructor capture、wrapper、closure、specialization も同じ関数追加処理を使い、
`Ok` / `Err` / `print` 専用の固定関数枠を作らない。direct call に関数本体が必要なければ、
fid 統一のためだけに余分な wrapper を生成しない。
runtime の `BuiltinId` は `BUILTIN_METAS` の登録を参照し、compiler UID / `fun_idx` との
対応は宣言・callable metadata に持つ。compiler fid を builtin ID へ算術変換しない。
Result の runtime tag 0/1 は variant layout の契約として扱い、fid の割り当てには使わない。

割り当て順はコンパイラが宣言・関数を追加する順とし、既存の stage 内の宣言順を尊重する。
HashMap の走査順や関数の表示名で順序を変えない。前方参照は通常の predeclaration の
登録結果へ解決する。REPL の継続 chunk は保持済み allocator の次から追加し、
checkpoint / rollback / bytecode 復元でも割り当て済み ID と次の追加位置を整合させる。
登録済み target がない場合はエラーとし、旧固定値や名前で補完しない。

### constructor / capture / Pattern

`Ok(value)` と `Result::Ok(value)` は同じ経路を使う。
`Result<Int>::Ok(1)`、`Result<_>::Ok(1)` は通常 Enum の owner 型引数規則を使う。
`Boolean::True` / `True` は nullary variant 値で、nullary capture は `(-> Boolean)` を作る。
placeholder capture は通常 constructor と同様に位置引数だけを受け、引数ブロックには
1個以上の placeholder を必要とする。固定式は callable 呼び出し時に評価する。

`Result::Err` の payload は concrete `deferror` 制約を守る。Error の一般保持・公開を
constructor capture で解除しない。Error を通常 callable の入出力へ露出する capture は
既存の Error 規則に沿って拒否する。通常 Enum 共通の機能と special variant の制約を
分離し、別の旧 capture 経路へ戻さない。
`Result<T>` の failure 部分に収まる Error は、この公開禁止には該当しない。
`deferror` の引数から concrete Error を構築する式を capture 内に固定することは許可する。

Result の期待型は payload 推論と nested failure に伝える。明示 owner 型引数と期待型が
矛盾する入力は拒否する。通常 Enum の未確定 slot は既存どおり拒否する。
`Result::Err` の成功 slot の多相性は special variant の規則として保持し、
`err = Err(NoneError)` のような既存の失敗値を禁止しない。bare / qualified / 明示 `_`
の spelling でこの規則を変えない。capture が通常の callable binding を作る場合は
既存の具体的な callable signature を必要とする規則に従う。

Pattern は AST の既存形式を受けられるが、Sigil 以降では canonical variant を参照する。
別 Enum の同名 variant を Result / Boolean と扱わない。網羅性検査、tag、Boolean literal
lowering も解決後の identity / metadata を使う。

### 表示と診断

診断 subject、LSP 定義参照・hover、REPL `:sig` / `:doc` は canonical variant を使う。
bare alias の補完 label は短い名前を使ってよいが、detail は qualified name とする。
旧 Result constructor 宣言構文は受理しない。既存テストは現行の宣言・拒否理由へ更新する。

## 実装順序と検証

主要な変更箇所は次のとおり。

- Sindr: `names.rs` の identity / capability / 予約名と、共有する special variant metadata。
  `builtin.rs` の compiler UID 固定式と、runtime builtin ID を使う metadata の境界。
- Spire: `ast.rs`、`parser/decl.rs`、`parser/expr.rs`、`parser/pattern.rs`。
  旧 constructor 宣言を削除し、bare Boolean を通常 constructor の AST にする。
  パーサでの special variant 分類を早期正規化し、module 名の有無で経路を分けない。
- Sigil: `resolved.rs`、`resolver/declarations.rs`、`scope_init.rs`、`imports.rs`、
  `expr.rs`、`patterns.rs`、`session.rs`、`semantic_metadata.rs`。
- Scar: `checker/predeclare.rs`、`definitions.rs`、`expr.rs`、`patterns.rs` と
  `env.rs` / `typed.rs`。通常の解決済み variant と canonical metadata から型制約を適用する。
  `checker/mod.rs` の固定 UID の型 binding と、関数 ID の allocator / 復元処理。
- Forge: `lib.rs` の旧宣言の staging と、`codegen.rs` の名前による特別扱い。
  関数追加時の ID 割り当て、constructor capture、chunk / bytecode の再配置。
- Xldr / analysis: constructor 分類、標準定義の staging、query / 補完 / 定義参照。

| 受入入力・観測 | 期待する結果 |
|---|---|
| `Ok` / `Result::Ok`、`Err` / `Result::Err` | パーサ側では special variant と識別し、正規化後は同じ constructor UID、owner / variant、診断 subject |
| `True` / `Boolean::True`、`False` / `Boolean::False` | 同じ variant identity、Boolean 値、定義参照 |
| `Result<Int>::Ok(1)` / `Result<_>::Ok(1)` | Result の成功型は Int |
| `Result<Int>::Ok("text")` | 明示型引数との不一致で拒否 |
| `value: Result<Int> = Result<_>::Err(NoneError)` | 期待型で成功 slot を確定し、outer failure を構築 |
| `err = Err(NoneError)` / `err = Result::Err(NoneError)` / `err = Result<_>::Err(NoneError)` | Result の失敗値の多相性を保持し、spelling で推論規則を変えない |
| `&Ok` / `&Result::Ok`、`&Ok(&1)` / `&Result::Ok(&1)` | 同じ unary callable と型推論 |
| `&True` / `&Boolean::True`、False 側 | 同じ nullary callable。呼び出すと対応する Boolean 値 |
| `&Result<Int>::Ok` / `&Result<_>::Ok(&1)` | 通常 Enum の owner 型引数・placeholder 規則を共有 |
| `wrap: (Int -> Result<Int>) = &Result<_>::Ok` | 期待 callable 型から owner 型引数を確定 |
| 外側の宣言で導入済みの `$T` を使う `&Result<$T>::Ok` | 宣言済みの型変数を保持。新しい未確定 slot と混同しない |
| `&Err` / `&Result<Int>::Err` / `&Result<Int>::Err(&1)` | 通常 callable の Error 入力を公開するため拒否 |
| `&Result<Int>::Err(SomeError(&1))`（`SomeError` は Int 引数の deferror） | Int 入力から concrete Error を構築する callable として受理 |
| `&Ok(value: &1)`、`&Ok(1)` | named capture / placeholder のない引数ブロックを拒否 |
| `Err(1)` / `Result::Err(1)`、抽象 Error、nested Err | 同じ concrete Error 制約で拒否 |
| `Ok(Err(NoneError))`、nested Result を期待する outer Err | 内側の失敗と外側の失敗の区別・既存 lifting を保持 |
| bare / qualified Result Pattern と Boolean Pattern | payload、網羅性、実行結果が一致 |
| `Other::Ok` / `Other::Err`、`MatchResult::Ok` / `MatchResult::Err` | Result の special variant と同一視せず、それぞれの canonical 契約で処理する |
| 別 owner の variant を `Ok` 等へ import / alias 登録 | bare alias の予約違反で拒否 |
| std のない isolated resolver 入力 | special variant を synthetic UID で補完しない |
| `Ok` / `Err` / `print` と通常宣言を順序を変えて追加 | 実際の登録順に ID を割り当て、対象の固定値を前提にしない |
| 同じ canonical variant の bare / qualified / capture | 同じ登録済み target を参照し、alias 用の fid を追加しない |
| 通常関数の前方参照、builtin direct call / capture | 登録 metadata から target を解決し、compiler fid の固定式を使わない |
| 同一入力を繰り返しコンパイル | compiler の追加順と ID 割り当てが再現する |
| module 名が付く段階と正規化済みの段階 | 同じ canonical 宣言の constructor / capture / Pattern に別経路を作らない |
| REPL の chunk 追加・rollback・bytecode 復元 | 既存 target の ID を保ち、新規 target は allocator の次から追加する |
| `:sig` / `:doc`、hover / completion / go-to-definition | 表示・参照は同じ canonical 宣言。bare label だけ短縮可能 |

| 段階 | 対象と責務 | 受入条件 |
|---|---|---|
| 1 | Sindr の special variant metadata、TypeIdentity、予約名、compiler ID と runtime ID の境界 | 4 variant と alias が一意に対応し、固定 fid / UID 式を使わない登録契約を定める |
| 2 | Spire の通常 Enum AST と早期正規化、Sigil の宣言登録・scope・alias・Pattern | 旧 ResultCtor AST / kind / seed と module 有無による経路分岐を削除し、両 spelling が同じ追加順 UID / canonical identity を返す |
| 3 | Scar の constructor 推論・capture・Error 制約・確定診断 | 明示型引数、期待型、placeholder、nullary capture の成功と型不一致・Error 公開の拒否を固定する |
| 4 | Forge / Eldr の tag・Boolean lowering、関数追加順、Pattern / match、chunk / bytecode 復元 | 関数 ID は追加順で一元割り当て。Result は tag 0/1 の既存 layout、Boolean は既存 primitive layout として動く |
| 5 | Xldr / analysis の表示・補完・宣言参照 | alias / qualified が同じ定義を参照し、旧 family を表示しない |
| 6 | 正本文書の更新と OI-037 の整理 | 実装・テストで確認した最終契約を配備し、完了後に Open Issue を削除する |
| 7 | 全体検証と独立レビュー | 全件 Green、旧経路なし、最終差分のレビュー指摘を解決する |

変更する契約の成功・拒否テストを先に追加・更新し、意図した理由の Red を確認してから
実装する。各段階は関連 crate の対象テストから確認し、最後に以下を実行する。

```sh
cargo fmt --all --check
git diff --check
SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace
cargo run -- test --quiet --all
```

最終レビューでは alias の shadowing、canonical identity の欠落、通常 Enum と分裂した
constructor / Pattern / capture、名前文字列 fallback、runtime layout、テスト不足を確認する。
`Ok` / `Err` / `print` の固定 fid / UID、builtin ID から compiler UID を作る式、
module 名の有無による special constructor 分岐が残っていないことも確認する。
修正後は必要な検証を再実行する。依頼されていないコミット・マージは行わない。

## 正本文書と進捗

対象は `docs/site/capture-operator.md`、Enum / Result の利用者向け説明、
`docs/dev/diagnostics.md`、`docs/dev/Surtr_LSP_spec.md`、`docs/dev/Xldr_spec.md`、
必要な Pattern / VM 契約、および `lib/types/result.srt` / `lib/types/boolean.srt` の source docs。

- [x] 現行経路の調査と入力仕様の整理
- [x] プランの独立レビューと指摘の反映
- [x] 契約テストの Red
- [x] 実装と関連テストの Green
- [x] 正本文書の整合
- [x] 全体検証と最終差分の独立レビュー

作業前から存在した `doc/test_command_release_build_memo.md` の変更と
`doc/lib_tests_reorganization_pr.md` は本作業の対象に含めない。

計画作成時には、Boolean の bare 名が `Ast::Lit(..., Lit::Bool(...))` になることを
ソースと一時的な最小テストの失敗で確認した。確認用のテスト追加は元へ戻した。
実装後の検証結果は、以下の実装記録に示す。


## 2026-10-04 の実装記録

作業ワークツリー: `codex/oi037-special-enum`。開始点は `9a2374f8`。
元の作業ツリーにあった別件の変更はコピーせず、この入力計画だけを引き継いだ。
実装完了時点ではコミット・マージは未実施。以後のコミット・統合履歴は Git を参照する。

- Sindr の4 variant metadata と bare alias の共有予約を登録した。
- Spire / Sigil で通常 Enum AST、宣言 UID、alias、Pattern へ統合し、旧 ResultCtor と固定 builtin UID を削除した。
- Scar は解決済み variant metadata で Result / Boolean 制約を適用する。明示 owner 型引数、Error 公開、nested failure、Err 多相性を検証した。
- compiler symbol UID と関数 ID は登録順を使い、復元時の alias 同 UID の二重再配置も解消した。
- Forge は Typed variant の lowering metadata を受け取る。Xldr の query・補完・capture 継続と `.eldr` 復元、LSP の同一定義参照を固定した。
- canonical Enum 宣言を必要とする単独フェーズの既存テストは、正規の標準宣言を入力する形へ更新した。

Red では isolated `Ok` の synthetic 解決、bare alias AST、`&True` の構文拒否、
明示 Result owner 型引数の無視、表示名順の UID、復元時 alias 二重再配置、
Enum variant 定義参照の欠落、予約 alias の他 owner への tail 補完を確認した。

初回 workspace gate は作業中の差分に対して実行したため、Scar prepared prefix の
source key 不一致が発生した。加えて process timeout の生成 helper 登録漏れと
REPL signature 表示・テスト名の不一致を検出し、対応した。
初回標準テストの File 5件は、新規ワークツリーの `tmp/sandbox` 未作成で失敗した。
ディレクトリを準備した再実行では標準テスト全件が成功した。

最終検証はすべて成功した。

| 検証 | 結果 |
|---|---|
| `cargo fmt --all --check` | 成功 |
| `git diff --check` | 成功 |
| `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace` | 2,253 passed、除外なし |
| `cargo run -- test --quiet --all` | 終了コード0、標準テスト全件成功 |
| 最終差分の独立レビュー | blocking 指摘なし。import 境界テスト不足と shape 補完の指摘を反映済み |

標準 Enum と別 owner の同名 variant の各 capture、および nested Result は、
レビュー担当の独立 CLI probe でも期待出力・終了コード0を確認した。

内部 timeout helper を正規登録したことで、既存の bare helper 拒否 fixture 2件は
未定義から compiler-internal の可視性拒否へ変わる。期待文言を現在の拒否条件へ更新し、
script fixture 全9 bucket と最終 workspace 全件を確認した。

全件再実行で、変更していない VM の `due_timer_requeues_sleeping_process_without_host_sleep` が
一度失敗した。実時間で進む現在 tick に対して、テストが固定 tick 10 で期限処理するための
タイミング依存を確認した。単独再実行と最終全件再実行では成功し、除外や timeout 変更は行っていない。

完了した OI-037 は `doc/open-issues.md` から削除した。schema / VM version の変更はない。


## 2026-10-04 の main 統合

変更は処理系の統合、回帰テスト、正本文書の3コミットに分割した。
main 側で実施済みの標準テスト再配置に合わせ、Boolean の追加ケースは
`lib/tests/basic_types/boolean.srt`、Result の追加ケースは
`lib/tests/monads/result.srt` へ引き継いだ。旧配置のファイルは復活させない。
この統合ターンでは、依頼に従いテストを再実行していない。
