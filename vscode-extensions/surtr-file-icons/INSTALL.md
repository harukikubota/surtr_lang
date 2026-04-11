# Surtr File Icons - Install Guide

このディレクトリは、Surtr 用の VS Code file icon 拡張です。  
`.srt` と `.eldr` に同じアイコンを割り当てます。

## 1. Development Host

1. VS Code で `vscode-extensions/surtr-file-icons` を開く
2. `F5` を押して `Surtr Icons: Development Host` を起動する
3. 起動先で `Preferences: File Icon Theme` を開く
4. `Surtr Icons` を選択する

## 2. VSIX を作る

```bash
cd vscode-extensions
npm install
cd surtr-file-icons
npm run package
```

## 3. VSIX を入れる

```bash
code --install-extension surtr-file-icons-0.1.0.vsix --force
```

## 4. `vscode-icons` と併用する

`vscode-icons` を使い続けたい場合は、`vsicons-custom-icons/file_type_surtr.png` を
custom icon asset として使います。

```json
{
  "workbench.iconTheme": "vscode-icons",
  "vsicons.customIconFolderPath": "/absolute/path/to/custom-icons",
  "vsicons.associations.files": [
    {
      "icon": "surtr",
      "extensions": ["srt", "eldr"],
      "format": "png"
    }
  ]
}
```
