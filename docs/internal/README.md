# Internal Docs

`docs/internal/` は、内部向けドキュメントの案内ページです。

`docs/dev/` に出していない残りの資料は、基本的に内部検討用・設計補助用として扱います。  
`doc/` は draft、開発アイデア、タスク入力、作業途中メモの置き場です。

## 主な内部資料

- [../../doc/open-issues.md](../../doc/open-issues.md)
- [../../doc/example_project_mahjong.md](../../doc/example_project_mahjong.md)
- [../../doc/optimize/001_tail_call_optimization.md](../../doc/optimize/001_tail_call_optimization.md)
- [./tail-call-optimization.md](./tail-call-optimization.md)
- [../../doc/vscode/](../../doc/vscode/)
- [../../doc/vscode_extension_features_naming_surtr.md](../../doc/vscode_extension_features_naming_surtr.md)

## ルール

- 仕様変更は対象領域の `../dev/` spec、`../site/`、または `../../lib/*.srt` の `@doc` を先に更新する
- 公開説明は `../site/`
- 標準 API の説明は `../../lib/**/*.srt` の `@doc`
- 作業途中の実装計画、agent / superpowers のチェックリスト、調査メモは commit 対象の正本にしない
- 一時メモを残すなら `doc/` に移し、必要な仕様判断だけを `../dev/`、`../site/`、または標準定義 source の `@doc` に反映したうえで元メモは削除する
