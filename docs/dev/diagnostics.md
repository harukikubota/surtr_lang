# Diagnostics 開発指針

`crates/diagnostics` の user-facing diagnostics に適用する正本ルール。診断文を追加・変更するときは、表示文より先に `DiagnosticSpec` の役割分担と構造化契約を確認する。

## 適用範囲

対象は parser、resolver、typechecker、runtime、REPL が生成する `DiagnosticSpec` と、その Ariadne / JSON 出力。

対象外:

- `DebugLabel` を使う inspect / debug 表示
- compiler 内部のログ・トレース
- concrete `deferror` の runtime 値
- JSON クライアント固有のレイアウト

## `DiagnosticSpec` の役割

| フィールド | 役割 | 入れてよい内容 |
|---|---|---|
| `message` | headline | 診断の主原因。短い一文 |
| `labels` | source caption | span に結び付く対象、関連定義、失敗箇所、期待値と実際値 |
| `notes` | 補足 | ルール、推論・変換過程、runtime context、入力の分類 |
| `help` | 修正案 | 利用者が取るべき操作、代替構文、書き換え例 |

source span を必要としない説明や修正案を `labels` に置かない。迷う場合は「その文がコード上のどこを指すか」を基準にし、指さない説明は `notes`、利用者への命令は `help` に置く。

## 実装規則

- `labels` の各 `span` は、表示する本文と対応するソース範囲を指す。関連ファイルの定義は `source_id` を設定する。
- headline だけで十分な診断に無理な source label を追加しない。
- `kind`、`phase`、`primary_span`、`expected`、`got`、`hint` の意味を表示文の改善目的で変更しない。
- テンプレートを変更したら、source label・note・help の分類が変わる renderer 判定 helper も確認する。

現在の代表例:

- Unit 型のパターン案内は `parse.rs` の `help` に置く。`Help:` を label 本文に埋め込まない。
- operator の `OP rule` は演算子 span を指す source label、`BIND_RULE_TEXT` は typecheck の `notes`、`=?` などの書き換え案は `help` に置く。
- extractor の `input source` は `notes`、extractor 定義は関連 source label に置く。
- runtime の失敗値・pattern・`call target` は label、`expected rule`・`runtime rule`・`opcode`・入力分類は `notes` に置く。
- `assert_eq` の LHS/RHS term は比較対象の span を指すため label、失敗の説明は `help` に置く。

## 出力契約

### Human-readable

renderer は `message`、`labels`、`notes`、`help` をそれぞれ headline、source caption、note、help として出力する。Ariadne の色、罫線、空白、label の順序は安定契約にしない。

### JSON

`serializable_diagnostic_by_id` が出力する次の値を安定させる。

```json
{
  "kind": "TypeError",
  "phase": "typecheck",
  "line": 2,
  "column": 14,
  "span": [13, 23],
  "message": "expected Int, got String",
  "expected": "Int",
  "got": "String",
  "hint": "..."
}
```

自然言語の `message`、label 本文、note、help は意味を保つ範囲で変更できる。クライアントが新しい情報へ依存する場合は文字列解析を増やさず、typed field を追加する。

## テスト規則

- unit test は `kind` と主な構造化値を確認し、`labels`・`notes`・`help` を個別に検証する。
- ルールや help が label に戻っていないことを、代表的な診断の負の assertion で固定する。
- renderer test は headline、source、note、help の存在と意味を確認し、ANSI・罫線・空白・全文一致に依存しない。
- compile-error fixture は既存 parser の形式を使う。

```text
phase: typecheck
contains: expected Int
contains: got String
```

- `stdout`、exit code、runtime value、無関係な CLI 文言の厳密な検証は弱めない。

## 検証コマンド

```bash
cargo nextest run -p diagnostics --lib
cargo nextest run -p rune --test integration run_srt
cargo nextest run -p rune --test integration module_import_fixtures
cargo nextest run --workspace
```

変更範囲に応じて focused test を先に実行し、最後に workspace 全体を実行する。失敗が既存か変更起因かを分けて記録する。
