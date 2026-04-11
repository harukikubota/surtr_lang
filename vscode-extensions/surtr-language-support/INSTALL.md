# Surtr Language Support - Install

## VSIX を作る

```bash
cd vscode-extensions
npm install
cd surtr-language-support
npm run build
npm run package
```

## VSIX を入れる

```bash
code --install-extension surtr-language-support-0.1.0.vsix --force
```

## 確認

1. `.srt` ファイルを開く
2. 右下の language mode が `Surtr` になっていることを確認する
3. snippets と outline が利用できることを確認する
