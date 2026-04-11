# Surtr VSCode Extensions Workspace

`vscode-extensions/` には Surtr 向けの VSCode 拡張をまとめて置く。

- `surtr-language-support`: 言語登録、TextMate grammar、snippets、簡易 document symbols
- `surtr-tools`: `surtr` CLI と連携する diagnostics / run / build / test / dump コマンド
- `eldr-viewer`: `surtr dump --format viewer-json` を使う React webview viewer
- `surtr-file-icons`: `.srt` / `.eldr` 向け file icons

## 開発

```bash
cd vscode-extensions
npm install
npm run build
```

個別拡張の開発方法は各フォルダの `README.md` と `INSTALL.md` を参照。
