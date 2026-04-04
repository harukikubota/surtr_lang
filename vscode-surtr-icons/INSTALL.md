# Surtr File Icons - Install Guide

このディレクトリは、Surtr 用の VS Code ファイルアイコン拡張です。  
`.srt` と `.eldr` に同じアイコンを割り当てます。

使い方は次の 2 パターンです。

- `Surtr Icons` を単体の File Icon Theme として使う
- `vscode-icons` を使ったまま、Surtr 用の拡張子だけカスタム追加する

## 1. すぐ試す (Development Host)

このリポジトリには `.vscode/launch.json` を追加してあり、`F5` ですぐ単体確認できます。

1. VS Code で `vscode-surtr-icons` フォルダを開く
2. `F5` を押して `Surtr Icons: Development Host` を起動する
3. 起動先で `Preferences: File Icon Theme` を開く
4. `Surtr Icons` を選択する

この Development Host は `--disable-extensions` 付きなので、他のアイコン拡張に影響されずに確認できます。

## 2. 常用インストール (VSIX)

`vscode-surtr-icons` ディレクトリで実行:

```bash
npm i
npx vsce package
code --install-extension surtr-file-icons-0.0.1.vsix --force
```

その後、VS Code で `Preferences: File Icon Theme` から `Surtr Icons` を選択します。

## 3. `vscode-icons` と併用する

VS Code の `File Icon Theme` は同時に 1 つしか有効化できません。  
そのため、`Surtr Icons` と `vscode-icons` を同時にテーマとして有効化することはできません。

`vscode-icons` を使い続けたい場合は、`Surtr Icons` 拡張を選ぶのではなく、`vscode-icons` のカスタムアイコン機能に Surtr 用アイコンを追加します。

### 前提

- File Icon Theme は `VSCode Icons` を選ぶ
- `Surtr Icons` はテーマとして選ばない
- `vsicons.customIconFolderPath` には `vsicons-custom-icons/` を含む親フォルダを指定する
- カスタムアイコンのファイル名は `file_type_<icon名>.png` または `file_type_<icon名>.svg` にする

### 手順

1. 任意のカスタムアイコン用ディレクトリを用意する
2. その中に `vsicons-custom-icons/` を作る
3. `vsicons-custom-icons/surtr.png` をコピーして `file_type_surtr.png` という名前で配置する

例:

```text
/absolute/path/to/custom-icons/
└── vsicons-custom-icons/
    └── file_type_surtr.png
```

`.vscode/settings.json` 例:

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

設定後に次を実行してください。

1. `Icons: Apply Icons Customization`
2. `Developer: Reload Window`

### 追加できないときの典型原因

- `vsicons.customIconFolderPath` が `vsicons-custom-icons/` 自体を指している
- アイコン名が `surtr.png` のままで、`file_type_surtr.png` になっていない
- `Surtr Icons` を選んだままで、`VSCode Icons` に戻していない
- `Icons: Apply Icons Customization` を実行していない

## 4. 反映されないときの確認

- 拡張が入っているか:
  - `code --list-extensions --show-versions | rg 'surtr-file-icons|vscode-icons'`
- 単体利用なら `Preferences: File Icon Theme` が `Surtr Icons` になっているか
- 併用利用なら `Preferences: File Icon Theme` が `VSCode Icons` になっているか
- インストール直後は `Developer: Reload Window` を実行する

## 5. リポジトリ運用メモ

`.gitignore` 例:

```gitignore
vscode-surtr-icons/node_modules/
vscode-surtr-icons/*.vsix
```

画像アセットは `vsicons-custom-icons/` に置いておけば問題ありません。
