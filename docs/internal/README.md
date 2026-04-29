# Internal Docs

`docs/internal/` は、内部向けドキュメントの案内ページです。

実ファイルの正本は [`../../doc/`](../../doc/) にあります。  
`docs/dev/` に出していない残りの資料は、基本的に内部検討用・設計補助用として扱います。

## 主な内部資料

- [../../doc/open-issues.md](../../doc/open-issues.md)
- [../../doc/float.md](../../doc/float.md)
- [../../doc/example_project_mahjong.md](../../doc/example_project_mahjong.md)
- [../../doc/optimize/001_tail_call_optimization.md](../../doc/optimize/001_tail_call_optimization.md)
- [./tail-call-optimization.md](./tail-call-optimization.md)
- [../../doc/stdmod/](../../doc/stdmod/)
- [../../doc/surtr_ansi_doc_spec.md](../../doc/surtr_ansi_doc_spec.md)
- [../../doc/surtr_rust_viewer_model_design_v2.md](../../doc/surtr_rust_viewer_model_design_v2.md)
- [../../doc/vscode/](../../doc/vscode/)
- [../../doc/vscode_extension_features_naming_surtr.md](../../doc/vscode_extension_features_naming_surtr.md)

## ルール

- 仕様変更は先に `../../doc/要件定義v9.md` と各 spec を更新する
- 公開説明は `../site/`
- 標準 API の説明は `../../lib/*.srt` の `@@doc`
