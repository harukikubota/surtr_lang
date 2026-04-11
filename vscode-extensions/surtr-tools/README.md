# Surtr Tools

`surtr` CLI を使って VS Code から Surtr のチェックと実行コマンドを呼び出します。

- save 時の diagnostics
- current file の run / build
- workspace test 実行
- raw bytecode JSON dump

## 前提

- `surtr` バイナリが PATH 上にあること
- もしくは `surtr.compiler.path` で絶対パスを設定すること

## 開発

```bash
cd vscode-extensions
npm install
cd surtr-tools
npm run build
```
