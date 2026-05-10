# Developer Docs

`docs/dev/` は開発者向けドキュメントの入口です。

ここにある各ページが、開発者向け正本ドキュメントです。  
`doc/` は draft、開発アイデア、タスク入力、作業途中メモ、tmp ファイル置き場として扱います。

## 仕様書

- [EldrVM spec](./EldrVM_spec.md)
- [Json / Encode / Decode spec](./Json_spec.md)
- [Process runtime spec](./ProcessRuntime_spec.md)
- [Rune observability](./Rune_observability.md)
- [Xldr spec](./Xldr_spec.md)
- [テスト方針](./テスト方針.md)

`Process runtime spec` は process surface、BootPlan、Supervisor、handler dependency、
diagnostics の正本である。`EldrVM spec` は VM が受け取る正規化済み runtime 契約と
実行意味論のみを扱う。

process runtime の変更を追うときは、まず `Process runtime spec` を開く。
特に `@call` / `@cast` の戻り値契約、`@timeout(...)`、`TaskHandle` / `Task::await(...)`、
worker stop semantics はこのページを正本とし、`doc/` 配下の作業メモを別正本として扱わない。

## 併読するとよいもの

- [../../doc/要件定義v9.md](../../doc/要件定義v9.md)
- [../../doc/open-issues.md](../../doc/open-issues.md)
- [../../AGENTS.md](../../AGENTS.md)
