# rune クレート レビュー & リファクタリング提案

## 概要

`crates/rune/src/main.rs` は現在 **1287 行**あり、CLIエントリポイントとしての責務を大きく超えた実装が集中しています。本ドキュメントではコードレビューの結果と分割案を示します。

---

## レビュー

### 良い点

- パイプラインの各フェーズにコメントが付いており可読性が高い（`// Phase 1: Spire — parse` など）
- `E-1 contract` のような設計判断のコメントがある
- `ScriptPlanError`、`TestCase` など意味のある型を定義している
- `dump.rs` を既に分離している

---

### 問題点

#### 1. `main.rs` が 1287 行で責務が多すぎる

現在 `main.rs` に以下がすべて混在しています。

| 責務 | 主な関数 |
|---|---|
| CLIエントリ・コマンド振り分け | `main`, `print_usage` |
| オプション解析 | `parse_run_options`, `parse_repl_options`, `parse_test_options` |
| run / build コマンド実行 | `run_command`, `run_source_file`, `run_eldr_file`, `build_command` |
| コンパイルパイプライン | `compile_source`, `parse_program_with_module_sources` |
| エントリポイント注釈処理 | `prepare_script_compile_plan`, `collect_entrypoint_annotations`, `erase_span`, `rewrite_script_ast_for_entry` |
| テストフレームワーク全体 | `test_command` + 関連 10 関数 |
| モジュールソース収集 | `collect_lib_root_sources`, `collect_additional_std_module_inputs`, `derive_primary_module_path` |
| VM実行・エラー報告 | `execute_bytecode`, `report_final_result_error_if_any`, `report_error_value` |
| 文字列ユーティリティ | `char_to_byte_index`, `slice_by_char_range`, `line_column_for_char_offset` |

---

#### 2. `dump.rs` が `super::` で `main.rs` の内部関数を直接呼んでいる

```rust
// crates/rune/src/dump.rs
super::prepare_script_compile_plan(file_path, &source, cli_entry)
super::collect_default_script_compile_sources(file_path, &compile_plan.source_for_parse)
super::compile_source(&compile_sources, &compile_plan)
```

`dump.rs` が `main.rs` の実装詳細に依存しており、`main.rs` を分割できない原因になっています。
コンパイル関連のロジックを独立したモジュール（後述の `compile.rs`）に切り出すことで解消できます。

---

#### 3. `ExecutionEnv` が事実上使われていない

```rust
fn run_command(options: RunOptions, _env: ExecutionEnv) -> Result<(), i32> { ... }
fn build_command(input_srt: &str, output_eldr: Option<&str>, _env: ExecutionEnv) -> Result<(), i32> { ... }

fn test_command(options: TestOptions) -> Result<(), i32> {
    let _env = ExecutionEnv::Test;  // ローカルで生成するが未使用
    ...
}
```

`Dev` / `Test` の分岐が将来用に予約されているものの、現在はすべて未使用です。
意図を残すならコメントで記録し、型は削除するのが適切です。

---

#### 4. エラー型が `i32` のまま

`Result<(), i32>` が全域で使われており、エラーが exit code としか扱えません。
コンパイルエラーと実行時エラーの区別ができず、`?` 演算子も使いにくい構造です。

```rust
// 現状: エラー詳細が i32 に潰れる
fn compile_source(...) -> Result<forge::bytecode::Bytecode, i32>
fn execute_bytecode(...) -> Result<(), i32>
```

---

#### 5. テスト実行時のコンパイルエラーが詳細を失う

```rust
// commands/test 相当の処理内
let bytecode = compile_source(&compile_sources, &compile_plan)
    .map_err(|_| "compile error while evaluating test expression".to_string())?;
//                ^^^ エラー詳細を捨てている
```

テスト式のコンパイル失敗時に、どの式が・どの理由で失敗したかが出力されません。

---

#### 6. `derive_primary_module_path` で手動トークン走査している

```rust
fn derive_primary_module_path(source: &str) -> Option<String> {
    let tokens = spire::lexer::tokenize(source).ok()?;
    // Defmod トークンの後を手動でスキャン...
}
```

`spire` が AST を返せるなら AST から `defmod` 宣言を取るべきです。
トークン仕様の変更（空白・コメント扱いなど）の影響を直接受ける脆弱な実装です。

---

## 分割案

### ファイル構成

```
crates/rune/src/
├── main.rs                  ← main() + print_usage() のみ (~40行)
├── commands/
│   ├── mod.rs               ← pub use
│   ├── run.rs               ← run_command, run_source_file, run_eldr_file,
│   │                           execute_bytecode,
│   │                           report_final_result_error_if_any,
│   │                           report_error_value
│   ├── build.rs             ← build_command, default_output_path
│   ├── test.rs              ← test_command, TestCase, TestSelector, TestOperator,
│   │                           TestLocation, parse_test_selector,
│   │                           test_case_matches_selector,
│   │                           collect_test_cases_from_source,
│   │                           evaluate_expression, report_test_failure,
│   │                           split_test_expression,
│   │                           find_def_name_for_test_chain,
│   │                           build_expression_script_source
│   └── dump.rs              ← 現 dump.rs から super:: 依存を除去
├── compile.rs               ← ScriptCompilePlan, ScriptPlanError, EntryAnnotation,
│                               compile_source, parse_program_with_module_sources,
│                               collect_default_script_compile_sources,
│                               prepare_script_compile_plan,
│                               collect_entrypoint_annotations,
│                               erase_span, rewrite_script_ast_for_entry
├── loader.rs                ← collect_lib_root_sources,
│                               collect_additional_std_module_inputs,
│                               module_path_from_file_name,
│                               derive_primary_module_path
└── util.rs                  ← char_to_byte_index, slice_by_char_range,
                                line_column_for_char_offset, display_path
```

---

### 分割後の `main.rs` イメージ

```rust
mod commands;
mod compile;
mod loader;
mod util;

const RUNE_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = env::args().collect();

    let result = match args.get(1).map(String::as_str) {
        Some("--version") => { println!("surtr {}", RUNE_VERSION); Ok(()) }
        Some("run")   => commands::run::dispatch(&args[2..]),
        Some("repl")  => commands::repl::dispatch(&args[2..]),
        Some("build") => commands::build::dispatch(&args[2..]),
        Some("test")  => commands::test::dispatch(&args[2..]),
        Some("dump")  => commands::dump::dispatch(&args[2..]),
        Some("tui")   => commands::tui::dispatch(&args[2..]),
        _             => { print_usage(); Err(1) }
    };

    if let Err(code) = result {
        process::exit(code);
    }
}
```

---

### `dump.rs` の `super::` 依存解消

```rust
// 変更前 (dump.rs)
super::prepare_script_compile_plan(file_path, &source, cli_entry)

// 変更後 (compile.rs に切り出し後)
crate::compile::prepare_script_compile_plan(file_path, &source, cli_entry)
```

---

## 優先度まとめ

| 優先度 | 対応内容 |
|---|---|
| 高 | `compile.rs` に切り出して `dump.rs` の `super::` 依存を解消 |
| 高 | テスト関連を `commands/test.rs` に分離 |
| 高 | コンパイルパイプラインを `compile.rs` に分離 |
| 中 | モジュールローダーを `loader.rs` に分離 |
| 中 | ユーティリティを `util.rs` に分離 |
| 低 | `ExecutionEnv` の整理（使うか削除するか決める） |
| 低 | エラー型を `i32` から専用型へ移行 |
