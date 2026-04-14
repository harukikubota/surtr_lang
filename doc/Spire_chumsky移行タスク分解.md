# Spire `chumsky` 移行タスク分解

> 目的: `LSP/補完` 利用を見据えて、`Spire` parser を段階的に `chumsky` へ移行する。
> 前提: `doc/依存整理詳細設計.md` と `doc/Spire_chumsky移行詳細設計.md` を正本とする。

最終更新日: 2026-04-14（実装更新）

---

## 1. 現在地（2026-04-14 時点）

- `parse_with_context()` は `chumsky` ベースの top-level driver を経由する
- statement 本体は既存 parser を island parser として再利用している
- `Spire` の parse 制約は `ParseRules` へ集約済み（旧 `SourceRules` は廃止）
- runtime 制約（`CompileUnitKind` / `EntryPoint` / `RuntimeSourcePolicy`）は `sindr::policy` へ移設済み
- `lexer` / `token` は `spire` 内部実装へ閉じ、外部は `strip_test_annotations()` / `collect_entrypoint_annotations()` を利用する
- `parser` は `mod.rs + context.rs + validate.rs + syntax_token.rs + error_map.rs + completion.rs + diagnostic.rs + ty.rs + pattern.rs + interpolate.rs` に分割済み
- `Rich -> ParseError` 正規化を `error_map.rs` に集約済み（`Incomplete` 判定含む）
- `parse_incomplete_expr` / `parse_incomplete_stmt` と `CompletionContext` を公開済み
- `parse_with_context_diagnostic` と `LSP` 互換 DTO (`LspDiagnostic*`) を公開済み
- `cargo test -p spire --lib`, `cargo test -p rune --test repl`, `cargo test -p rune --test run_srt`, `cargo test --workspace` は通過済み
- 未着手: `expr/decl` 本体の `chumsky` 化、legacy recursive-descent 本体の完全撤去

---

## 2. 実行方針

- 外部契約 (`Ast`, `ParseError`, `ParserContext`, `ParseRules`, `parse*`) は維持
- 1 タスクごとに「parser unit + workspace test」を通して進める
- 置換順は `型 -> パターン -> 宣言 -> 式` を基本とする
- 旧実装は parity が取れた単位で削除し、二重保守期間を短くする

---

## 3. マイルストーン

## M1: 構造分割と基盤整備

### T1-1 `parser` モジュール分割

- 変更対象: `crates/spire/src/parser.rs` → `parser/mod.rs`, `parser/context.rs`, `parser/validate.rs`
- 内容:
  - `ParserContext`, `ParseRules`, parse 用内部 policy（`TopLevelDecl*` など）を `context.rs` へ分離
  - `validate_stmt_by_context` 系を `validate.rs` へ移設
  - `lib.rs` の公開面は現状互換維持（`lexer/token` は非公開のまま）
- DoD:
  - 既存 API シグネチャが変わらない
  - `cargo test -p spire --lib` 通過

### T1-2 内部 token adapter 導入

- 変更対象: `parser/syntax_token.rs`（新規）
- 内容:
  - private token 列（`lexer` 出力）→ internal `SyntaxToken` の変換
  - `::` 結合、`>>` 分解、span 補正をこの層に隔離
- DoD:
  - `expect_type_gt()` 依存を除去できる準備が整う
  - 変換ロジック単体テスト追加

### T1-3 error 正規化層を固定

- 変更対象: `parser/error_map.rs`（新規）または `chumsky_program.rs` 内分離
- 内容:
  - `Rich` → `ParseError` の写像を共通化
  - `Incomplete` 判定ルールを明文化・実装
- DoD:
  - REPL multiline 継続入力ケースが壊れない
  - `tests/integration/repl.rs` が通る

## M2: 文法本体の `chumsky` 化（段階置換）

### T2-1 型 parser (`ty.rs`) を `chumsky` 化

- 変更対象: type 関連 (`parse_type*`)
- 内容:
  - `Named/Generic/Tuple/Func/ImplTrait` を `chumsky` で実装
  - `Self/self/$Self/where` 制約を既存互換で維持
- DoD:
  - 型系テスト通過
  - 既存 `parse_type*` 呼び出しを新実装へ差し替え済み

### T2-2 パターン parser (`pattern.rs`) を `chumsky` 化

- 変更対象: bind/match pattern 関連
- 内容:
  - list/tuple/constructor/as/annotated pattern を置換
  - `SafeBind.LHS` と `match` 左辺の共通 parser 化
- DoD:
  - safebind/match 関連テスト通過
  - 旧 pattern parser 削除

### T2-3 宣言 parser (`decl.rs`) を `chumsky` 化

- 変更対象: `def*`, `import`, `impl`, `@@builtin*`
- 内容:
  - annotator (`@@doc`, `@@builtin`) 処理を宣言 parser に統合
  - builtin/result ctor の特例を helper 化
- DoD:
  - module/std-module/repl policy で回帰なし
  - 既存 decl parser 呼び出しを新実装へ置換

### T2-4 式 parser (`expr.rs`) を `chumsky` 化

- 変更対象: precedence/postfix/call/flow/match/cond/capture/closure
- 内容:
  - 優先順位を declarative 定義へ置換
  - trailing block sugar を専用モードで制御
  - interpolation 呼び出しを `interpolate.rs` に分離
- DoD:
  - `parser` テスト全件通過
  - run_srt / language_features 回帰なし

## M3: LSP/補完向け API 整備

### T3-1 部分入力 parser エンドポイント追加

- 変更対象: `spire` 公開 API（追加のみ）
- 内容:
  - `parse_incomplete_expr`, `parse_incomplete_stmt` のような内部 API を定義
  - 失敗時に `expected token set` と `cursor span` を取得可能にする
- DoD:
  - EOF 途中入力で `Incomplete` + 候補情報を返せる
  - REPL/LSP 用 fixture テスト追加

### T3-2 補完コンテキスト抽出

- 変更対象: `parser/completion.rs`（新規）
- 内容:
  - cursor 位置の期待文脈 (`ExprContext`, `TypeContext`, `DeclContext`) を返す
  - `import`/`path`/`call-arg-name` など最小セットを先に対応
- DoD:
  - 補完用途の golden test 追加
  - parser 本体と結合しすぎない構造を維持

### T3-3 診断情報の LSP 互換 DTO 変換

- 変更対象: `parser/diagnostic.rs`（必要に応じて `xldr` 側 adapter 追加）
- 内容:
  - `ParseError` + `Rich` 情報を LSP Diagnostic へ変換する adapter 追加
  - primary span と secondary hint の最小構成を返す
- DoD:
  - parse failure の位置が 1 文字ずれない
  - 代表ケースのスナップショットテスト通過

## M4: 旧実装の完全除去

### T4-1 legacy parser 削除

- 変更対象: 旧 recursive-descent 関連関数
- 内容:
- 置換済み関数を順次削除
- dead code / helper を掃除
- DoD:
  - `parser/mod.rs` の巨大単一構成が解消される
  - `cargo test --workspace` 通過

### T4-2 ドキュメント・運用更新

- 変更対象: `doc/Spire_chumsky移行詳細設計.md`, `doc/テスト方針.md`, 必要なら `docs/site/*`
- 内容:
  - 実装済み範囲と残課題を反映
  - 補完 API の利用方法を追加
- DoD:
  - 設計と実装の不一致がない

---

## 4. スプリント向け実行順（推奨）

1. `M1` 完了
2. `M2` を `T2-1 -> T2-2 -> T2-3 -> T2-4` の順で実施
3. `M3` で補完 API を整備
4. `M4` でレガシー撤去とドキュメント収束

---

## 5. 各タスク共通チェックリスト

- `cargo test -p spire --lib`
- `cargo test -p rune --test repl`
- `cargo test -p rune --test run_srt`
- `cargo test --workspace`
- 変更点に対応する parser 単体テスト追加

---

## 6. リスク管理メモ

- 最大リスクは「error 振る舞いの非互換」であり、`Incomplete` 判定の崩れは REPL/LSP へ直撃する
- 次点は「型膨張によるビルド遅延」で、巨大 combinator を避けて関数境界で分割する
- 二重実装期間は短く保ち、置換単位ごとに旧実装削除まで進める
