# SD-09 runtime 値の表示検証計画

入力仕様は [標準定義・共有表現の調査](spec_bypass_audit_standard_shared.md#sd-09-runtime-値の表示が未知-tag欠損-payload-を許す)。表示 API の失敗を VM・REPL・観測へ伝えるフェーズ間契約を変更するため level 4 とする。

未知 tag、reserved Result の payload 数不一致、既知 struct / record のフィールド数不一致、enum の識別子と payload 不一致を表示時に拒否する。List・tuple・map・既知型のフィールド内でも失敗を伝える。正常な表示形式は維持する。

1. Sindr の表示回帰テストを追加し、従来の成功フォールバックで失敗することを確認する。
2. `Value::to_display_string` を `Result` に変更し、共有表現の表示エラーを Eldr の `RuntimeError` に変換する。Eldr の inspect と呼出し側で伝播する。
3. `docs/dev/EldrVM_spec.md` を整合させ、Sindr / Eldr の局所テストを実行する。
4. 親作業で REPL・CLI 呼出し側を統合し、別エージェントレビューと CI profile の workspace 全体、標準テストを確認する。

局所検証: `rtk cargo nextest run -p sindr display`、`rtk cargo nextest run -p eldr`。正しい Result と利用者定義型の既存表示テストを維持する。スキーマ・VM バージョンは変更しない。

## 検証で判明した表現契約

`TypeEntry.field_names` は enum の payload のみを記録する。runtime の `fields` には先頭 discriminant があるため、enum の検証数は metadata の数 + 1 とする。正常 fixture の表示失敗でこの差を確認した。

比較 builtin の `Ordering` 生成は fields を空にしており、通常の enum constructor と異なっていた。生成と predicate の両方を、Less / Equal / Greater の discriminant 0 / 1 / 2 を持つ通常表現へ統一する。旧 fields 空の受入経路は削除する。

## 完了記録（2026-10-04）

最終差分の独立レビューで指摘なし。`rtk cargo nextest run --profile ci --workspace` は2282件成功、`rtk proxy cargo run -- test --quiet --all` は exit 0。整形・差分チェックも成功した。詳細は入力仕様の SD-09 実施記録と統合検証記録を参照する。
