# AGENTS.md — Surtr Compiler

Surtr は静的型付き関数型言語のコンパイラ実装（Rust）。
パイプライン: **Spire → Sigil → Scar → Forge → Eldr**
CLI エントリーポイント: **Rune**

この言語はHobbyプログラミング言語です。
** コンパイラ、ランタイム、コードをシンプルに保ちつつ、構文の表現力で言語機能を強力にする。**

---

## Workspace

```
surtr/
├── Cargo.toml             # workspace 定義
├── crates/
│   ├── spire/             # Parser        : &str → Vec<Ast>
│   ├── sigil/             # Name resolver : Vec<Ast> → Vec<Resolved>
│   ├── scar/              # Type checker  : Vec<Resolved> → Vec<TypedNode>
│   ├── forge/             # Codegen       : Vec<TypedNode> → Bytecode
│   ├── eldr/              # VM            : Bytecode → execution
│   └── rune/              # CLI           : entrypoint
├── doc/                   # 仕様ドキュメント（読み取り専用）
└── tests/
    ├── spec/              # 仕様ベース成功系テスト (.srt + .expected)
    └── compile_errors/    # 仕様ベース失敗系テスト (.srt + .error)
```

---

## References（読み取り専用・明示的な指示があれば変更可能）

実装の正とするドキュメント。型定義・AST・Opcode を変更する前に必ず確認すること。

| ファイル | 内容 |
|---|---|
| `doc/要件定義.md`             | 言語仕様・コンパイラ設計の全体定義 |
| `doc/EldrVM_spec.md`         | VM仕様書 |
| `doc/テスト方針.md`            | テストの分離方法・レイヤー | 

---

## Documentation Workflow

- 要件定義書: 大元の仕様書（正本）
- `doc/open-issues.md`: 仕様策定の課題一覧（未解決・先送り）
- `doc/phase-N.md`: そのフェーズで実装する機能一覧（実装作業の起点）
- `doc/Idea-N.md`: 追加機能ドラフト（採択後に要件定義書または `open-issues` へ取り込む）

実装タスクの着手時は、`phase-N.md` を最優先で参照し、要件定義書との不整合があれば先にドキュメントを更新してからコードを変更すること。

---

## Pipeline Types

各フェーズの入出力型。クレート間でこの型境界を守ること。

```rust
fn parse(src: &str)                   -> Result<Vec<Ast>, ParseError>
fn resolve(ast: Vec<Ast>)             -> Result<Vec<Resolved>, ResolveError>
fn typecheck(resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError>
fn codegen(typed: Vec<TypedNode>)     -> Result<Bytecode, CodegenError>
fn execute(bytecode: Bytecode)        -> Result<(), RuntimeError>
```

---

## Coding Rules

### 全般

- フェーズ間のデータ型は各クレートの `pub` 型で表現する
- あるクレートの内部型を別クレートから直接参照しない
- エラー型はフェーズ固有のものを使う（下表）

| フェーズ | エラー型 |
|---|---|
| parse | `ParseError` |
| resolve | `ResolveError` |
| typecheck | `TypeError` |
| codegen | `CodegenError` |
| execute | `RuntimeError` |

### 組込み関数

- 組込み関数の追加・変更は **`eldr/src/builtin_registry.rs` の `BUILTINS` テーブルのみ** を起点にする
- Sigil・Scar・Eldr の3者がこのテーブルを参照する。他ファイルへの直接ハードコード禁止
- `unique_id` の割り当て順序は `BUILTINS` テーブルの定義順に従う

```rust
// BUILTINS テーブルの構造
pub const BUILTINS: &[BuiltinDef] = &[
    BuiltinDef { name: "print",     builtin_id: 0, ... },
    BuiltinDef { name: "to_string", builtin_id: 1, ... },
    BuiltinDef { name: "eprint",    builtin_id: 2, ... },
];
```

### Opcode 追加の判断基準

| 条件 | 判定 |
|---|---|
| 単相 + 頻出 + 副作用なし | 専用 Opcode（例: `AddInt`, `ConcatStr`） |
| 多相（`forall A`） | `CallBuiltin` |
| 副作用あり（I/O） | `CallBuiltin` |

### Sigil — `if` の専用ノード変換

`Ast::App(Var("if"), args)` を検出したら `Resolved::If(cond, then, else_opt)` に変換する。
通常の関数呼び出しとして処理しないこと（Forge の遅延評価生成に必要）。

### Scar — フィールド名解決

`FieldAccess` のフィールド名はScar でインデックスに解決し `TypedInner::FieldAccess(expr, idx: u32)` として出力する。
Forge は `GetField(idx)` を emit するだけでよい。

### TypeRegistry

- Forge が構築し Eldr がランタイムで参照する
- tag 番号の予約: `Ok = 0`, `Err = 1`
- ユーザ定義型の tag は出現順で連番割り当て

---

## Current Scope: Phase 1 Only

### 実装対象

- プリミティブ型: `Int` / `Float` / `String` / `Boolean` / `Unit`
- コレクション: `List` リテラル・空リスト・型推論
- データ定義: `defstruct` / `defrecord` / `deferror`
- 制御構造: `if` 式 / `match`（Boolean・Result のみ、網羅必須）
- 演算子: 算術 / 比較 / `==` `!=`（Int・String・Bool のみ）/ `++`（文字列結合）
- 組込み関数: `print` / `to_string` / `eprint`

### 実装しないもの（フェーズ2以降）

- `def`（関数定義）・クロージャ・ラムダ
- `|>` パイプライン
- `@@builtin` 構文・`builtin.srt`
- `.eldr` ファイル出力
- `MakeFrame` / `PopFrame` / `Return` Opcode
- 名前付き引数（`defrecord` を除く）
- 式埋め込み文字列（`"#{expr}"`）
- トレイト・型エイリアス・NewType
- マクロシステム

上記をフェーズ1のコードに混入させないこと。

---

## Testing

### ユニットテスト

各クレートに `#[test]` を書く。`cargo test` ですべて通ること。

### 仕様ベーステスト

```
tests/spec/
  control/*.srt + *.expected
  types/*.srt + *.expected
  ...

tests/compile_errors/
  type_mismatch/*.srt + *.error
  exhaustiveness/*.srt + *.error
  ...
```

実行方法:

```bash
cargo test -p rune --test spec_fixture_tests
```

`spec` は `stdout` を `.expected` と比較して一致すれば合格。
`compile_errors` は `.error` の `phase` / `contains` を満たせば合格。

### エラーケース

コンパイルエラーになるべきファイルは `tests/compile_errors/**.srt` に配置し、
対応する `.error` で `phase` / `contains` を検証すること。

---

## Error Output Format

人間向け（ariadne）と機械向け（JSON）の両方を出力する。

```
Error: TypeMismatch
  --> main.srt:2:14
  |
2 | bad: Int = "bad type"
  |            ^^^^^^^^^^ expected Int, got String
```

```json
{
  "errors": [{
    "kind": "TypeMismatch",
    "phase": "typecheck",
    "line": 2,
    "column": 14,
    "span": [13, 23],
    "expected": "Int",
    "got": "String",
    "hint": "The type annotation requires Int but the value is String"
  }]
}
```

---

## Crate Naming Rationale

| クレート | 名前 | 由来 |
|---|---|---|
| Parser | Spire | 最初に触れる構造。構文の尖端 |
| Name resolver | Sigil | 名前を実体に結びつける印 |
| Type checker | Scar | 試練を通過した証 |
| Codegen | Forge | 火と金属が形を生む場所 |
| VM | Eldr | 古ノルド語で「火」。炎が着地する場所 |
| CLI | Rune | 人とコンパイラの間の文字 |

---

*Surtr — 既存の妥協を、型で焼き払う。*
