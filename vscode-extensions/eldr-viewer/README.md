# Eldr Viewer

`surtr dump --format viewer-json` の出力を React webview で表示する VSCode 拡張です。

- active `.srt` または `.eldr` から viewer JSON を取得
- function / opcode / constant / source の 4 ペイン表示
- Rust 正本の ViewerModel を受け取って描画

## 開発

```bash
cd vscode-extensions
npm install
cd eldr-viewer
npm run build
npm test
```
