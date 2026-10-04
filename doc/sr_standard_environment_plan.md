# SR-02〜04: 標準環境と import 解決の統一計画

記録日: 2026-10-04。入力は [SR 改修方針](sr_revision_notes.md) の SR-02〜04。現行コード、正本文書、テストの読み取りに基づく実施計画であり、この文書の作成時点では実装・テストを行っていない。

名前解決の入口、REPL の継続状態、Scar と共有する宣言 UID の契約に関わるため **level 4** とする。以下の内部 API 名は説明用であり、命名の確認を実装開始条件にはしない。

## 確定した方針と対象範囲

- 通常の実行・解析・REPL は標準環境を使う。呼出し側に autoimport の有効／無効を選ばせない。
- `@autoimport` 属性は維持する。その属性が付いた module / trait / impl Type の surface を、ファイルの開始時に必ず適用する。非 autoimport の標準 module は引き続き明示 import を必要とする。
- 単独 Sigil 入口と REPL も、staged 入口と同じ宣言・stage 可視性・import 環境を使う。import 宣言を未処理のまま捨てる入口は削除する。
- `print`、`to_string`、`inspect`、`eprint`、`set_exit_code` を初期 scope へ先置きする経路を削除する。標準ソースの実宣言と、その autoimport から名前・UID・型を得る。
- コンパイラが持つ型固有情報と標準宣言のマージは維持する。今回の改修を、builtin ID の変更や型の可視性の変更に広げない。
- 標準なしで通常ソースを実行するテスト構成は廃止し、共通 helper が標準環境を供給する。テストごとの切替引数は設けない。

`auto_import: bool` を一律に true にしてはいけない。現行の `DeclAttrs.auto_import` と `StagedModuleAst.auto_import` は各宣言の属性を表す。これを全件 true にすると、例えば非 autoimport の `Encode` / `Decode` まで裸名で導入され、名前の衝突や明示 import の重複判定が変わる。通常入口全体を無効化する設定フラグは今回の検索では確認していない。SR-02 は、属性を消す変更ではなく、標準環境を省略できる呼出し経路を整理する方針として実装する。

## 現行の根拠

行番号は調査時点の目安とし、実装着手時は関数名で再確認する。

| 対象 | 現行の処理と改修点 |
|---|---|
| `crates/sigil/src/resolver/mod.rs` の `resolve_with_warnings`（285行付近） | AST から owner registry を収集するが、`Resolver::new()` へ module / import 環境を渡さず `resolve_program` を呼ぶ |
| 同ファイルの `resolve_staged_program_from_state_with_warnings` | declaration index、UID、global scope、autoimport の出所と stage を構築する。共通化の基盤として使う |
| `resolver/imports.rs` の `build_module_scope_with_imports` | autoimport、明示 import、同名衝突、重複、stage 可視性を処理する。`ImportState` は導入済み module / member を保持する |
| `resolver/expr.rs` の `resolve_program`（2586行付近） | `Ast::Import` を無条件に読み飛ばす。staged 入口では処理済みだが、単独入口では未処理のまま消える |
| `resolver/session.rs` の `SigilSession::resolve_with_warnings` | scope と宣言情報を保持する一方、import 環境を構築せず同じ `resolve_program` を呼ぶ |
| `resolver/scope_init.rs` の `registered_builtin_scope` / `is_global_runtime_builtin` | 上記5関数を初期 scope に定義する。標準 Kernel 宣言とは別の導入経路になっている |
| `resolver/declarations.rs` の `assign_declaration_uids` | `initialize_scope` の予約済み UID 数を起点に宣言 UID を割り当てる。先置きの削除は UID 配置にも影響する |
| `crates/scar/src/checker/mod.rs` の初期環境構築 | `compiler_builtin_bindings()` が返す UID に builtin 型を bind する。Sigil の初期 scope と同時に整合させる |
| `crates/xldr/src/repl/logic/core.rs` の `apply_repl_imports` / `apply_preload_imports` | Xldr が先に import を scope に適用し、その後 SigilSession へ渡す。共通化後は二重適用を避ける |
| `crates/surtr-analysis/src/service.rs` の `analyze_single_document` | `sigil::resolve(ast)` を呼ぶため、単独入口の変更対象となる |

正本は `docs/site/standard-modules.md` の「auto import されるもの」、`docs/site/language-reference.md` のモジュール・import、`docs/dev/Xldr_spec.md` の REPL autoimport、`docs/dev/テスト方針.md` の標準 prelude と import 境界である。`lib/bootstrap.srt` と `lib/kernel.srt` の実宣言も合わせて確認する。`print` が import なしで使える理由は Kernel の autoimport と明記されており、初期 scope の先置きを維持する根拠にはしない。

## 推奨する内部 API と状態

### 標準環境の所有者

標準ソースの収集・parse とファイル I/O は、現在の loader を持つ Rune / Xldr と解析側の責務とする。Sigil に Xldr / Rune / Scar への依存を追加しない。Sigil は parse 済みの stage 群と宣言情報を受け取り、名前解決用の環境を構築する。

概念上は次の形に揃える。既存の `PrecollectedDeclarations`、`ResolveResumeState`、`SigilSession` を再利用し、同じ表を別の正本として重複保持しない。

```rust
// API の形を示す擬似コード。実装済みの宣言ではない。
ResolveEnvironment::from_stages(module_stages, precollected) -> Result<ResolveEnvironment, ResolveError>
environment.resolve_unit(ast, unit_context, resume_state) -> Result<ResolvedUnit, ResolveError>
SigilSession::from_environment(environment, unit_context, resume_state) -> Result<SigilSession, ResolveError>
session.resolve_chunk(ast) -> Result<ResolvedUnit, ResolveError>
```

環境を受け取らず通常ソースを解決する旧 `resolve(ast)` / `resolve_with_warnings(ast)`、空の `SigilSession::new()` を通常入口として残さない。関数名を維持して必須引数を追加してもよい。互換 overload、空環境への自動降格、失敗時だけ標準を後付けする再試行は設けない。

immutable な環境には次を持たせる。

- declaration index と owner registry、および同じ順序から得た declaration UID・kind・hidden 情報
- stage ごとの module 群と所属 owner。現在 stage より後の宣言を import しないための情報
- module / trait / impl Type に由来する autoimport の出所と定義 stage
- trait constructor slot など、既存 staged 解決が利用する共有情報
- 標準宣言の解決済み部分と対応する再開位置。既存 semantic snapshot を使う場合も、同一の declaration UID 割当との対応を保持する

ファイルまたは REPL compile unit の可変状態には次を持たせる。

- 現在の module path、stage index、compile unit の種別と scope
- 次の local UID、確定した宣言追加分、owner registry の追加分
- 導入済み module / member の import 状態、autoimport の出所、shadowing 情報
- warning と表示に必要な import 結果。Xldr の `:imported` や completion が同じ解決結果を利用できる情報

環境構築で全 stage の宣言を収集しても、各 stage の可視性を緩めない。標準の Bootstrap から後続 stage を構築する過程は内部の構築段階であり、ユーザーソースを標準なしで実行する別モードにはしない。通常ソース用の入口は、loader が作った標準環境を必須にする。

現在、初期 scope には process / Task の lowering が使う compiler runtime builtin もある。それらと利用者向け5関数の導入を区別する。今回削除するのは標準実宣言で導入できる5関数の先置きである。コンパイラ生成命令に必要な runtime identity は正本 metadata から得る既存経路を維持し、標準関数の欠落を補う公開 alias として使わない。

### import を一度だけ適用する

既存の `build_module_scope_with_imports` を、環境と file-local な import 状態を受け取る共通処理へ整理する。通常のファイルは開始時に autoimport を適用し、その後ファイル内の明示 import を処理する。ネストした module / impl body は既存の scope 境界を維持する。

REPL は一つの compile unit の状態を継続する。各 chunk で標準 autoimport を重ねて登録しない。chunk の明示 import はその状態に対して一度だけ適用し、成功時だけ状態を確定する。解析・型検査・実行のいずれかで失敗した場合、import 状態と scope / UID / declaration 情報を同じ checkpoint へ戻す。従来の REPL が提供する rollback の範囲を維持する。

Xldr の `apply_repl_imports` / `apply_preload_imports` と、そこで使う独自の import 解決 helper は共通処理への移管後に削除する。表示用ラベル、導入一覧、completion の更新だけを Xldr に残す。preload と通常 chunk で import の可否を別々に判定しない。

`resolve_program` が import を IR に含めないこと自体は維持する。ただし共通の前処理で検査・適用済みの AST だけが到達できる内部境界にする。未処理の import を発見しても無条件 skip する公開経路はなくす。

## 実装順序

1. **正本とテスト入口を整える。** 標準環境を通常入口の前提として開発文書に明記する。現在の標準 loader / semantic snapshot を再利用するテスト helper を用意する。5関数について Kernel 宣言の canonical UID への解決を成功条件とし、未処理 import を単独入口が受理するケースを Red にする。
2. **Sigil の環境と import 処理を共通化する。** staged 処理から環境構築を抽出し、単独入口も必ず同じ処理を通す。宣言順、stage 可視性、import の出所を保持する。既存の staged 入口を旧実装の並存先にはしない。
3. **SigilSession と REPL を移行する。** import 状態を checkpoint に含め、Xldr の独自適用を同時に撤去する。preload、通常 chunk、rollback、completion / `:imported` を確認する。
4. **残る入口とテスト helper を移行する。** Rune と解析側、Sigil / Scar / Forge の helper が同じ環境を供給する。標準なしの通常実行経路と、それを期待するテストを残さない。内部の owner / scope / import 検証そのものは、通常実行と混同せず直接の内部テストとして保持できる。
5. **5関数の先置きを削除する。** `is_global_runtime_builtin` を削除し、Sigil の UID 配置と Scar の初期型登録を同時に整合させる。標準宣言による builtin 関数型の登録を利用する。snapshot / cache が旧 UID と新 UID を混在させないことを確認し、既存のソース由来 cache key の対象に変更ファイルが入るよう整える。スキーマ・VM バージョンは上げない。
6. **旧経路を除去して全体検証・レビューを行う。** エラーを標準追加や別 resolver の再試行で救済する分岐がないことを確認する。最終差分を別エージェントがレビューし、コード修正後は必要な全体検証を再実行する。

各段階は依存順に進める。入口の移行前に先置きだけを削除する部分修正を完了扱いにしない。コミット単位で更新する場合も、対応する入力ファイルの実施記録に未移行範囲を明記する。

## 呼出し側とテスト helper の移行表

| 箇所 | 移行内容 | 維持する契約 |
|---|---|---|
| `sigil::resolve` / `resolve_with_warnings` | 必須の解決環境を受け取り、staged と同じ import 処理へ接続 | warning、関連 span、解決先の canonical identity |
| Sigil staged API | 共通環境を構築・使用する入口へ整理 | module 並列解決、UID 決定性、stage 制限、process specs / boot plan |
| `SigilSession` | 環境付き初期化、import 状態の継続と checkpoint 化 | REPL の定義継続、失敗時 rollback、既存宣言の UID |
| `crates/rune/src/compile.rs` | 標準 snapshot と user stage から環境を供給 | Script / Module の違い、source identity、診断 |
| `crates/xldr/src/lib.rs` | 標準 semantic snapshot の構築を共通環境に接続 | Bootstrap→標準 stage の順序、キャッシュの再利用 |
| `crates/xldr/src/repl/logic/core.rs` | import 解決を Sigil へ移管。独自適用 helper を撤去 | preload / chunk、`:imported`、completion、rollback |
| `crates/surtr-analysis/src/service.rs` | `analyze_single_document` へ解析 context から標準環境を供給 | strict parse の診断、editor-only tolerant 情報の境界 |
| `crates/sigil/src/resolver/tests.rs` | `parse_and_resolve` と warning / session helper を標準環境付きにする | autoimport と明示 import の境界。テストごとの有効化フラグを不要にする |
| `crates/scar/tests/support/mod.rs` | `CachedStdPrelude` の stage / declaration / resume state を再利用 | 既存の共有 prelude と bucket 構成、型検査の開始状態 |
| Scar の直接 `sigil::resolve(ast)` を使う統合テストと crate-local テスト | 共通 helper または環境付き API へ移行。標準と同名の仮定義は、検証目的を保つ固有名へ直す | 本来検証する型規則。blankslate 成功を目的にしない |
| `crates/forge/src/lib.rs` の標準 prelude helper | 解決環境を供給し、旧初期 UID への依存を除く | codegen の検証対象と bytecode 結果 |
| `tests/integration/` の共通 compile helper | 通常 loader 経由の標準環境を使用することを確認 | script / module / CLI / REPL の実行境界 |

単独 Sigil のテストに上位 crate への dev-dependency 循環を作らない。標準ソースから stage を作る補助が必要なら、既存 parse / lowering API と標準 source manifest を使う共通の test support に置く。既に存在する Scar / Forge の helper と読み込み順を照合し、テストごとの手書き標準関数定義を増やさない。

## 成功・拒否の受入条件

| 入力・条件 | 期待結果 |
|---|---|
| 通常環境で `print("x")` / `inspect(1)` | 明示 import なしで成功。解決先は Kernel の実宣言と同じ UID |
| 通常環境で `map_err` など属性付き owner の裸関数を使用 | 現行の autoimport 対象と同じ解決先を得る |
| 非 autoimport module に `helper` を定義し、別ファイルで `helper()` | import がなければ名前解決エラー。`import M::helper` があれば成功 |
| `import MissingModule` だけを置く | 単独 Sigil 入口でも import エラー。未使用だからと捨てない |
| `import Kernel` / `import Kernel::print` | file-start autoimport との重複として拒否。入口ごとの別判定をしない |
| 同じ module / member の再 import、別の import による同じ裸名の導入 | 既存の衝突・重複契約どおり拒否 |
| 同一 stage の module を逆の列挙順で読み込む | 同じ解決結果と UID を得る |
| 後続 stage の module を前の stage から import | 可視性エラー。全宣言の precollect を可視性解除に使わない |
| REPL で import 後、次の chunk からその関数を呼ぶ | 成功。import 一覧と completion に同じ結果を反映 |
| REPL chunk 内の import 後に型検査または実行が失敗 | scope と import 状態を共に rollback。失敗した import の一部だけを残さない |
| 正常 source を cold / cached snapshot の両経路で処理 | 同じ canonical identity と結果。異なる UID 配置を合成しない |

通常環境の成功例を各層に重複して大量追加しない。autoimport の既定適用と import の拒否は Sigil、継続・rollback は Xldr、snapshot 合成は既存の loader / integration テストで直接固定する。

## 検証と完了条件

各実装段階は契約を固定するテストを先に追加・修正し、対象の失敗を確認してから Green にする。

- 名前解決: `rtk cargo nextest run -p sigil`
- UID と型環境: `rtk cargo nextest run -p scar`
- codegen と snapshot: 変更した helper / snapshot に対応する Forge / Xldr の局所テスト
- 実行境界: `rtk cargo nextest run -p rune --test integration run_srt`、`rtk cargo nextest run -p rune --test integration module_import_fixtures`
- 最終検証: `rtk cargo nextest run --profile ci --workspace`、`cargo run -- test --quiet --all`、format と `git diff --check`

別エージェントレビューでは、初期 scope の標準5関数の残存、import の無条件 skip、REPL の二重適用、UID と cache の不整合、標準なしテスト経路、stage 可視性の緩和を重点確認する。全件 Green と最終差分レビューを満たしてから完了とする。

## SR-01 などとの依存

SR-02〜04 の環境統一は、SR-01 の関数ごとの keyword / shadowing 規則を新たに決めなくても着手できる。既存の canonical identity と現在の shadowing 判定を共通環境へ運び、呼出しの解決結果を変えない。

例えば `map_err` と同名のユーザー定義がある場合に special form と通常関数のどちらを選ぶか、通常呼出しとパイプで差を許すかは SR-01 の対象である。環境移行中の都合で新しい予約語化や裸名による救済を追加しない。その境界を変更する必要が見つかった場合は具体的な入力と現行の解決先を記録し、SR-01 の仕様整理へ分離する。

SR-07 の Expr / Pattern フロー変更、SR-09 の `dbg!`、SR-10 の構文変更、SR-12 の Const 可視性もこの計画へ混ぜない。import の共通化では現在の import 対象判定を維持し、SR-12 の変更は後で共通処理に一度だけ適用できる形にする。

この範囲では、実装開始を止める未確定の公開挙動は確認していない。環境の型名、所有方法、既存 API の引数配置は内部実装の選択として決める。
