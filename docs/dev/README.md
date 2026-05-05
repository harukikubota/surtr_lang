# Developer Docs

`docs/dev/` は開発者向けドキュメントの入口です。

ここにある各ページが、開発者向け正本ドキュメントです。  
`doc/` は draft、開発アイデア、タスク入力、作業途中メモ、tmp ファイル置き場として扱います。

## 仕様書

- [EldrVM spec](./EldrVM_spec.md)
- [Process runtime spec](./ProcessRuntime_spec.md)
- [Rune observability](./Rune_observability.md)
- [Xldr spec](./Xldr_spec.md)
- [テスト方針](./テスト方針.md)

`Process runtime spec` は process surface、BootPlan、Supervisor、handler dependency、
diagnostics の正本である。`EldrVM spec` は VM が受け取る正規化済み runtime 契約と
実行意味論のみを扱う。

## 併読するとよいもの

- [../../doc/要件定義v9.md](../../doc/要件定義v9.md)
- [../../doc/open-issues.md](../../doc/open-issues.md)
- [../../AGENTS.md](../../AGENTS.md)

`docs/process_runtime_handoff_2026-05-02.md` は過去実装の引き継ぎメモであり、
正本仕様ではない。
