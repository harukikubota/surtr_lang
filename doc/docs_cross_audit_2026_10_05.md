# docs 横断調査 — バグ・曖昧性・改修対応項目

## 項目一覧

| ID | 優先度 | 分類 | 対応対象 |
|---|---|---|---|
| DA-03 | 中 | 未確定 | LSP file URIのquery / fragmentの扱い |
| DA-16 | 低 | 検証済み | Facet captureのテスト対応表と不足境界の補完 |
| DA-20 | 中 | 曖昧性 | Shellのsignal終了時のexit_code |

## 実装の対応項目

### DA-03 LSP file URIのquery / fragmentの扱い

- 現行: `file_uri_to_path` はquery / fragmentをファイル名に残す。
- 未確定点: query / fragmentを受理・除去・拒否のどれにするか、公開契約に定めがない。
- 受入条件: 採用する扱いを仕様化し、日本語・空白・percent encodingのround-tripを維持する。既存のauthority判定を変更しない。

## 導線と残件管理

### DA-16 OI-036の残る検証範囲

- 対応済み（2026-10-11）: [Eldr VM仕様](../docs/dev/EldrVM_spec.md)のCallable表示節に、既存の8 API生成テストと追加した実行・REPLテストの対応表を記載した。optional fieldは現行の`Option<T>`とrequired selectorで検証し、廃止済みselectorの拒否テストは今回追加しない。
- 追加した境界: `CaptureTree.rows.Some.[0].[1]`を4 capture形式で読み、metadata、成功、variant mismatch、外側・内側Listの範囲外を確認する。部分適用と変数経由の再captureは残りのsignatureと`view` / `put`の由来を保持し、closure literalと区別する。REPLは直接binding、単独評価、List・tuple・HashMap・Result・record fieldの表示とfieldから取得した関数の実行を確認する。
- 処理系と標準定義の変更はなく、type acceptance、Facet dispatch、privacy checksは維持する。OI-036は不足境界と対応表を補完したため削除した。
- 検証: `rtk cargo nextest run -p xldr --test repl_core repl_core_bucket_0`は1件成功、`rtk cargo nextest run -p rune --test integration run_srt`は9件成功。最終差分に対する`rtk cargo nextest run --profile ci --workspace`はcoldを含む2,495件がすべて成功し、`rtk proxy cargo run -- test --quiet --all`も終了コード0で完了した。

## 公開契約の追加整理

### DA-20 Shellのsignal終了時のexit_codeが説明されていない

- 根拠: `docs/dev/FS_Shell_spec.md:111-120` は起動後の非ゼロ終了をCommandResult.exit_codeで表す。`crates/eldr/src/builtin.rs:3407` はOS終了codeがなければ `-1` にする。
- 確認範囲: 静的読解のみ。signal終了のコマンドは実行していない。
- 対応・受入条件: `-1` をsentinelとして公開するのか、signalを別に表すのかを仕様化する。起動失敗と起動後の異常終了を区別し、標準 `@doc`・site・devを揃える。既存のOk/Err契約を独断で変更しない。説明だけなら局所文書対応、表現変更ならlevel4。

## 既存の残件

| 項目 | 参照先 |
|---|---|
| List / runtimeの残る性能課題 | `doc/optimize/runtime/list_runtime_followups.md` と `doc/v0.1_release_codebase_audit.md` |
| LSPのsemantic cache・外部入力更新の責務 | `doc/v0.1_release_codebase_audit.md` |
| WarningのCLI・REPL未接続 | `docs/site/warnings.md` とOI-033 |
| release buildのtest --all | `doc/test_command_release_build_memo.md`の未設計範囲 |
