# Surtr File Icons - Install Guide

このディレクトリは、Surtr 用の VS Code ファイルアイコン拡張です。  
`.srt` と `.eldr` に同じアイコンを割り当てます。

## 1. すぐ試す (Development Host)

1. VS Code で `vscode-surtr-icons` フォルダを開く
2. `F5` で Extension Development Host を起動
3. 起動先で `Preferences: File Icon Theme` を開く
4. `Surtr Icons` を選択

## 2. 常用インストール (VSIX)

`vscode-surtr-icons` ディレクトリで実行:

```bash
npm i -D @vscode/vsce
npx vsce package
code --install-extension surtr-file-icons-0.0.1.vsix --force
```

その後、VS Code で `Preferences: File Icon Theme` から `Surtr Icons` を選択します。

## 3. vscode-icons と併用したい場合

VS Code の「File Icon Theme」は同時に 1 つしか有効化できません。  
そのため、`Surtr Icons` を選んだまま `vscode-icons` を重ねることはできません。

`vscode-icons` を使い続けたい場合は、`vscode-icons` 側のカスタムアイコン機能に Surtr のアイコンを登録します。

ポイント:

- `Surtr Icons` は選ばない
- `vscode-icons` を File Icon Theme にする
- `vsicons.customIconFolderPath` は `vsicons-custom-icons/` を含む親フォルダを指す
- アイコンファイル名は `file_type_surtr.png` か `file_type_surtr.svg` にする

`.vscode/settings.json` 例:

```json
{
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

必要ファイル:

- `/absolute/path/to/custom-icons/vsicons-custom-icons/file_type_surtr.png`

設定後に `Icons: Apply Icons Customization` を実行してください。

## 4. 反映されないときの確認

- 拡張が入っているか:
  - `code --list-extensions --show-versions | rg surtr-file-icons`
- テーマが `Surtr Icons` か、または `VSCode Icons` か（使う方式に応じて）
- インストール直後は `Developer: Reload Window` を実行

## 5. リポジトリ運用 (推奨)

`.gitignore` 例:

```gitignore
vscode-surtr-icons/node_modules/
vscode-surtr-icons/*.vsix
```

`vscode-surtr-icons/icons/` はそのまま追跡して問題ありません。
