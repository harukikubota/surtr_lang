# Surtr VSCode Extensions 実装計画

## Summary

- VSCode 拡張のルートは `./vscode-extensions/` を採用する。
- 拡張は `surtr-language-support`、`surtr-tools`、`eldr-viewer` の 3 本で段階化し、既存の file icon 拡張は `surtr-file-icons` として同居させる。
- 各拡張フォルダには `README.md` と `INSTALL.md` を配置する。
- Surtr source の正本拡張子は `.srt` とし、`.eldr` は viewer / file icon の対象として扱う。
- `eldr-viewer` の webview client は TypeScript + React で実装する。

## Public APIs / Contracts

- `surtr check <file.srt> --format json` を追加し、機械向け diagnostics JSON を stdout に出力する。
- `surtr dump <file.eldr|entry.srt> --format viewer-json` を追加し、VSCode viewer が使う ViewerModel JSON を stdout に出力する。
- diagnostics JSON は `errors: [{ kind, phase, line, column, span, message, expected?, got?, hint? }]` を基本形にする。
- viewer JSON は raw `dump --format json` とは分離し、`schema_version` と `format` を持つ Rust 正本の ViewerModel とする。

## Implementation Order

1. `doc/vscode/implementation_plan.md` を作成する。
2. Rust 側で machine-readable diagnostics と viewer-json を追加する。
3. `vscode-extensions/` npm workspace と共通設定を作る。
4. `surtr-language-support` を先行実装する。
5. `surtr-tools` を実装し、CLI diagnostics と run/build/test/dump の入口をつなぐ。
6. `eldr-viewer` を実装し、React webview で ViewerModel を表示する。
7. 既存 `vscode-surtr-icons` を `vscode-extensions/surtr-file-icons` へ移す。

## Acceptance

- `cargo test` / `cargo nextest run --workspace` 相当の Rust 側テストで、`check --format json` と `dump --format viewer-json` が検証される。
- 各 VSCode 拡張に `README.md` と `INSTALL.md` があり、開発起動と VSIX 導入手順が再現できる。
- `surtr-language-support`、`surtr-tools`、`eldr-viewer`、`surtr-file-icons` が同一 `vscode-extensions/` 配下でビルド・テスト可能である。
