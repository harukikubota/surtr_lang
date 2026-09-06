# Surtr — 開発ガイド

Surtr は Rust で実装する静的型付き関数型の Hobby 言語。
処理系と構文をシンプルに保ち、小さな式の組み合わせで表現力を得る。

## 設計原則

- 式はアトミックでコンポーザブルにする。各式の責務と境界を明確にし、既存の式・関数の合成を優先する。
- 構文の曖昧さ、暗黙の変換、場当たり的な特例を増やさない。現行仕様に沿って提案・実装する。
- 置き換えた旧経路は削除する。エラーにすべき入力や内部状態をフォールバックで隠さない。
- 旧仕様の例は必要な拒否テストとして残し、期待値・診断を現行仕様に合わせる。

## 正本と構成

着手時に `doc/要件定義v9.md` と変更対象の仕様・実装を確認する。
仕様変更を伴う場合は正本を先に整合させる。依頼範囲を越える言語設計の変更は確認する。

- `docs/dev/`: `EldrVM_spec.md`、`Xldr_spec.md`、`テスト方針.md` などの開発仕様。診断変更は `diagnostics.md`、観測機能は `Rune_observability.md` を参照。
- `doc/`: 要件定義のほか、設計案・実装計画。未確定事項は `doc/open-issues.md` に記録する。
- `lib/*.srt`: 標準定義。利用者向け説明は `@doc` を正本にする。
- `crates/`: Spire（parse）→ Sigil（resolve）→ Scar（typecheck）→ Forge（codegen）→ Eldr（VM）。Rune は CLI、Xldr は REPL、Sindr は共有表現。

## 実装の境界

- フェーズ間は公開型とフェーズ固有のエラーで接続し、他クレートの内部実装に依存しない。
- builtin の正本は `crates/sindr/src/builtin.rs` の `BUILTIN_METAS`。ID は定義順。Eldr の `BUILTIN_IMPLS` と対応させ、各フェーズに名前・ID を直書きしない。`@builtin` は宣言層。
- 通常の Surtr 関数で表せる機能は標準定義で合成する。runtime が必要な多相・副作用処理は builtin、専用 Opcode は単相・頻出・副作用なしの処理を候補にする。
- 名前解決は Sigil、型・フィールド解決は Scar、命令生成は Forge に置く。遅延評価が必要な `if` を通常呼び出しへ落とさない。
- `import` は module member を file scope に入れる。型名は flat namespace で解決し、型や `new` を import 対象にしない。同名衝突を曖昧に解決しない。
- `Int` は BigInt、`Float` は finite-only。固定幅の内部 ID（tag / builtin_id / fun_idx）と利用者の `Int` を分離する。

## テスト

変更した契約を検証できる最小範囲から実行する。対象が通ったら、影響が及ぶ理由のある範囲だけ広げる。

| 対象 | コマンド（リポジトリルートで実行） |
|---|---|
| 単一フェーズ（例: Scar） | `rtk cargo nextest run -p scar` |
| script fixture | `rtk cargo nextest run -p rune --test integration run_srt` |
| module fixture | `rtk cargo nextest run -p rune --test integration module_import_fixtures` |
| CLI / process 境界 | `rtk cargo nextest run --profile cold -p rune --test integration` |
| 通常の workspace 検証 | `rtk cargo nextest run --workspace` |
| merge 前・複数フェーズに及ぶ変更の全体検証 | `rtk cargo nextest run --profile ci --workspace` |

- `run_srt` などは `integration` 内のフィルタ名。独立した test target ではない。default profile は cold テストを除外する。
- 再発ケースと重要な成功・拒否境界を固定する。同じ契約を各層で重複検証せず、最も直接的な層に置く。既存テストの拡張で足りれば利用する。
- 内部契約は crate-local、PureSurtr の成功例は `lib/tests/`、script 境界は `tests/fixtures/script/{pass,fail}/`、複数ファイルは `tests/fixtures/modules/{pass,fail}/`、CLI 境界は `tests/integration/`。
- 成功 fixture は `.expected`、失敗 fixture は `.error` の `phase` / `contains` で検証する。module fixture は `entry.srt` を入口にする。
- 未確定仕様を ignored テストで蓄積しない。文書のみの変更にコンパイラ全体のテストは不要。実行コマンド・結果・未検証範囲を簡潔に報告する。

## 作業運用

- 既存の未コミット変更を保持し、変更は依頼範囲に絞る。並列作業は書き込み先と共有契約の競合を確認する。
- REPL を対話操作する場合は iTerm2 の `Codex` プロファイルを使い、参加コマンド（例: `tmux attach -t surtr-repl`）と detach（`Ctrl-b` → `d`）を提示する。
- 中断時は残作業と次の一手を残す。
