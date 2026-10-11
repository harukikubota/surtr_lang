# docs 横断調査 — バグ・曖昧性・改修対応項目

## 項目一覧

| ID | 優先度 | 分類 | 対応対象 |
|---|---|---|---|
| DA-03 | 中 | 未確定 | LSP file URIのquery / fragmentの扱い |
| DA-16 | 低 | 検証範囲が未確認 | OI-036の不足テスト対応表 |
| DA-20 | 中 | 曖昧性 | Shellのsignal終了時のexit_code |

## 実装の対応項目

### DA-03 LSP file URIのquery / fragmentの扱い

- 現行: `file_uri_to_path` はquery / fragmentをファイル名に残す。
- 未確定点: query / fragmentを受理・除去・拒否のどれにするか、公開契約に定めがない。
- 受入条件: 採用する扱いを仕様化し、日本語・空白・percent encodingのround-tripを維持する。既存のauthority判定を変更しない。

## 導線と残件管理

### DA-16 OI-036の残る検証範囲

- 未確認点: optional / fallibleな複数segment、Facet由来のpartial application・変数経由の再capture、REPLの直接bindingとnested field表示について、契約を覆うテストの対応表が揃っていない。
- 受入条件: [OI-036](open-issues.md#oi-036-facet-capture-の残る検証範囲)の残る経路を既存テストと照合し、不足する境界だけを追加する。表示identityは[Eldr VM仕様](../docs/dev/EldrVM_spec.md)に従い、type acceptance、Facet dispatch、privacy checksを変えない。

## 公開契約の追加整理

### DA-20 Shellのsignal終了時のexit_codeが説明されていない

- 根拠: `docs/dev/FS_Shell_spec.md:111-120` は起動後の非ゼロ終了をCommandResult.exit_codeで表す。`crates/eldr/src/builtin.rs:3407` はOS終了codeがなければ `-1` にする。
- 確認範囲: 静的読解のみ。signal終了のコマンドは実行していない。
- 対応・受入条件: `-1` をsentinelとして公開するのか、signalを別に表すのかを仕様化する。起動失敗と起動後の異常終了を区別し、標準 `@doc`・site・devを揃える。既存のOk/Err契約を独断で変更しない。説明だけなら局所文書対応、表現変更ならlevel4。

## 既存の残件

| 項目 | 参照先 |
|---|---|
| List / runtimeの残る性能課題 | `doc/list_runtime_followups.md` と `doc/v0.1_release_codebase_audit.md` |
| LSPのsemantic cache・外部入力更新の責務 | `doc/v0.1_release_codebase_audit.md` |
| WarningのCLI・REPL未接続 | `docs/site/warnings.md` とOI-033 |
| release buildのtest --all | `doc/test_command_release_build_memo.md`の未設計範囲 |
