# Eldr Viewer - Install

## VSIX を作る

```bash
cd vscode-extensions
npm install
cd eldr-viewer
npm run build
npm run package
```

## VSIX を入れる

```bash
code --install-extension eldr-viewer-0.1.0.vsix --force
```

## 使い方

1. `.srt` または `.eldr` を開く
2. Command Palette から `Surtr: Open Eldr Viewer` を実行する
3. `surtr.compiler.path` が必要なら設定する
