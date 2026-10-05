# Developer Docs

`docs/dev/` は開発者向けドキュメントの入口です。

ここにある各ページが、開発者向け正本ドキュメントです。  
`doc/` は draft、開発アイデア、タスク入力、作業途中メモ、tmp ファイル置き場として扱います。
実装計画、作業チェックリスト、superpowers が生成した一時メモは正本ではないため、
残す必要がある場合でも `doc/` に再整理し、`docs/` 配下へは置かない。

## 正本の優先順位

Surtr は、全仕様を一枚の集約文書へ複製しません。現行挙動は Rust / Surtr のソースコードと
実行可能テストを優先し、文書は安定した外部契約と設計上の境界を説明します。文書と実装が
食い違う場合は、まず実装とテストで現行挙動を確認し、文書を追随させます。意図的に仕様を
変更する場合は、対象領域の正本文書を先に更新してから実装します。

| 対象 | 正本・入口 |
|---|---|
| 利用者向け言語 surface | [`../site/language-reference.md`](../site/language-reference.md) と各機能ページ |
| 標準 API | `../../lib/**/*.srt` の宣言と `@doc` |
| parser / resolver / typecheck / codegen / VM の責務 | 各 `../../crates/*/README.md` と本ディレクトリの対象 spec |
| builtin 名・signature・ID 順 | `../../crates/sindr/src/builtin.rs` の `BUILTIN_METAS` |
| 標準定義の stage・ロード順 | `../../crates/sindr/src/stdlib.rs` の `STDLIB_MODULE_SPECS` |
| source kind と compile policy | `../../crates/sindr/src/policy.rs` |
| 診断構造と span | [`diagnostics.md`](./diagnostics.md) |
| エラー表示・実行時オプション・REPL の表示レベル | [`display_error.md`](./display_error.md) |
| 未確定事項 | `../../doc/open-issues.md` |

API 一覧、builtin 一覧、標準モジュールの完全なロード順など、ソースから機械的に分かる情報を
手書きで複製しません。docs には責務、意味論、失敗境界、利用方法を残します。

## 仕様書

- [関数名の `?` suffix](./Callable_name_spec.md)
- [文字列リテラルの実装契約](./String_literal_spec.md)
- [EldrVM spec](./EldrVM_spec.md)
- [FS / Shell spec](./FS_Shell_spec.md)
- [Json / Encode / Decode spec](./Json_spec.md)
- [Process runtime spec](./ProcessRuntime_spec.md)
- [Pattern / Extractor implementation contract](./Pattern_spec.md)
- [Lazy special form implementation contract](./Lazy_spec.md)
- [Rune CLI spec](./Rune_cli_spec.md)
- [Rune observability](./Rune_observability.md)
- [Surtr LSP spec](./Surtr_LSP_spec.md)
- [Trait system implementation spec](./Trait_system_spec.md)
- [do intrinsic contract](./Do_intrinsic_spec.md)
- [Diagnostics contract](./diagnostics.md)
- [エラー表示](./display_error.md)
- [Xldr spec](./Xldr_spec.md)
- [テスト方針](./テスト方針.md)

`Process runtime spec` は process surface、BootPlan、Supervisor、handler dependency、
diagnostics の正本である。`EldrVM spec` は VM が受け取る正規化済み runtime 契約と
実行意味論のみを扱う。

process runtime の変更を追うときは、まず `Process runtime spec` を開く。
特に `@call` / `@cast` の戻り値契約、`@timeout(...)`、`TaskHandle` / `Task::await(...)`、
worker stop semantics はこのページを正本とし、`doc/` 配下の作業メモを別正本として扱わない。

## 併読するとよいもの

- [利用者向け docs](../site/README.md)
- [../../doc/open-issues.md](../../doc/open-issues.md)
- [../../AGENTS.md](../../AGENTS.md)
