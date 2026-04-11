# Surtr Tools - Install

## VSIX を作る

```bash
cd vscode-extensions
npm install
cd surtr-tools
npm run build
npm run package
```

## VSIX を入れる

```bash
code --install-extension surtr-tools-0.1.0.vsix --force
```

## 設定

```json
{
  "surtr.compiler.path": "/absolute/path/to/surtr",
  "surtr.diagnostics.onSave": true
}
```
