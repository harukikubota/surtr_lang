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
├── doc/                   # 正本仕様ドキュメント
├── docs/                  # 補助資料・公開向けガイド
├── lib/                   # 標準定義ソース (`@doc` を含む)
└── tests/
    ├── spec/              # 仕様ベース成功系テスト (.srt + .expected)
    └── compile_errors/    # 仕様ベース失敗系テスト (.srt + .error)
```

---

## References（読み取り専用・明示的な指示があれば変更可能）

実装の正とするドキュメント。型定義・AST・Opcode を変更する前に必ず確認すること。

| ファイル | 内容 |
|---|---|
| `doc/要件定義v9.md`           | 言語仕様・コンパイラ設計の全体定義 |
| `docs/dev/EldrVM_spec.md`   | VM仕様書 |
| `docs/dev/テスト方針.md`    | テストの分離方法・レイヤー |
| `docs/dev/Rune_observability.md` | `Rune` / `Eldr` の観測系オプション設計 |
| `docs/dev/diagnostics.md` | user-facing diagnostics の message / label / note / help と JSON 契約 |

---

## Documentation Workflow

- `docs/dev/`: 開発者向け正本仕様
  - `EldrVM_spec.md`, `Xldr_spec.md`, `テスト方針.md`, `Rune_observability.md`, `diagnostics.md`
- `doc/`: draft、開発アイデア、タスク入力、tmp ファイル置き場
  - `要件定義v9.md`, `open-issues.md` など
- `docs/`: 補助資料・公開向けガイド
- `lib/*.srt`: 標準定義ソースの利用者向けドキュメント。`@doc` を正本とする
- `crates/**`: 実装者向け内部契約。公開境界は rustdoc で残す

実装タスクの着手時は `doc/要件定義v9.md` と `docs/dev/` 配下の該当 spec を最優先で参照し、不整合があれば先に正本を更新してからコードを変更すること。user-facing のエラーメッセージ、source label、note、help、JSON 診断を追加・変更するときは、必ず `docs/dev/diagnostics.md` を参照すること。

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

- 組込み関数の追加・変更は **`crates/sindr/src/builtin.rs` の `BUILTIN_METAS` テーブルのみ** を起点にする
- Sigil・Scar・Forge・Eldr の4者がこのテーブルを参照する。他ファイルへの直接ハードコード禁止
- `builtin_id` の割り当て順序は `BUILTIN_METAS` テーブルの定義順に従う

```rust
// BUILTIN_METAS テーブルの構造
pub const BUILTIN_METAS: &[BuiltinMeta] = &[
    BuiltinMeta { name: "print",     builtin_id: 0, ... },
    BuiltinMeta { name: "to_string", builtin_id: 1, ... },
    BuiltinMeta { name: "eprint",    builtin_id: 2, ... },
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
- runtime tag は user-visible `Int` と分離する

### `import` と型名解決の注意

- `import` は Elixir 風に、module の public な import 可能 member を現在 file scope へ unqualified 名で入れる仕組みとして扱う
- 主対象は関数名・trait helper・module member であり、型名そのものを import する仕組みではない
- `import Mod`, `import Mod::name`, `import Mod::{name1, name2}` を受理する
- `Bootstrap` / `Kernel` / `Result` と、`@autoimport` 付き標準 trait は auto import 対象として扱う
- 明示 `import` は同じ file 内の auto-import 名を shadow してよい
- 明示 `import` 同士、および auto-import 同士の同名衝突は compile error とする
- `new` と構造体名そのものは import 対象外。`import User` は無効、`User` は型/構造体 head としてそのまま解決する
- 型名は `Mod::Type` ではなく bare identifier / generic で解決する
- 型名は flat type namespace で扱う。「どの file からも同じ見え方で使う」前提でよいが、同一可視圏で同名型が複数見える場合は compile error とする

---

## Current Focus

- `Int` は `BigInt` を採用し、通常算術でオーバーフローしない前提で扱う
- runtime 内部 ID（tag / builtin_id / fun_idx）は固定幅の内部識別子として扱い、user-visible `Int` と混同しない
- `Float` は finite-only の `f64` ラッパーとして扱う
- `type` は予約語として扱う
- `@builtin` の surface 宣言は標準定義ソース内の宣言層であり、`@builtin def` / `@builtin type` を受理するが、追加・変更の正本ではない
- 標準定義ソースの利用者向け説明は `lib/*.srt` の `@doc` に載せる

---

## Testing

### ユニットテスト

各クレートに `#[test]` を書く。デフォルトのテストランナーは `cargo nextest run` とし、workspace 全体が通ること。

未実装・未確定の将来仕様は、原則として skipped / ignored テストではなく `doc/open-issues.md` に退避すること。

### 仕様ベーステスト

```
tests/fixtures/script/pass/
  functions/*.srt + *.expected
  stdmod/*.srt + *.expected
  ...

tests/fixtures/script/fail/
  typecheck/*.srt + *.error
  exhaustiveness/*.srt + *.error
  ...

tests/fixtures/modules/pass/<case>/
  entry.srt + entry.expected
  *.srt

tests/fixtures/modules/fail/<case>/
  entry.srt + entry.error
  *.srt
```

実行方法:

```bash
cargo nextest run --workspace
cargo nextest run -p rune --test integration run_srt
cargo nextest run -p rune --test integration module_import_fixtures
```

`script/pass` は `stdout` を `.expected` と比較して一致すれば合格。
`script/fail` は `.error` の `phase` / `contains` を満たせば合格。

### エラーケース

コンパイルエラーになるべき script fixture は `tests/fixtures/script/fail/**.srt` に配置し、
対応する `.error` で `phase` / `contains` を検証すること。
multi-source module のコンパイルエラーは `tests/fixtures/modules/fail/<case>/entry.srt` と
`entry.error` で固定すること。

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

## Codex App 運用ガイド（2026-04 反映）

`2026-04-16` の Codex 大型アップデート（computer use / 複数エージェント並列 / plugin 連携 / automations / memory）を前提に、Surtr では次の運用を標準とする。

### 1. タスク分解

- 1タスクは「1時間程度 or 数百行程度」で完了する粒度に分ける
- 仕様確認タスク（Ask）と実装タスク（Code）を分離する
- ブロッカー直結タスクは手元で先に処理し、独立サブタスクを並列化する

### 2. 並列エージェントの担当境界

- 書き込み対象をクレート単位で明示して競合を避ける
- 例:
  - Agent A: `crates/spire/**`
  - Agent B: `crates/sigil/**`
  - Agent C: `tests/**` と `doc/**` の最小更新
- 同一ファイルを複数エージェントに同時に触らせない

### 3. Computer Use の適用範囲

- API 非対応ツール操作、UI/フロントの手動確認、E2E 的な画面確認で使用する
- コンパイラ本体の編集・検証は従来どおりリポジトリ中心（CLI / テスト）で行う
- destructive 操作（大量削除・履歴破壊）は必ず人間確認を挟む

### 4. Plugins / 連携

- 開発管理情報は plugin 経由で収集し、実装前に要件を固定する
- PR コメント対応はレビュー指摘を収集してから実装修正に入る
- 外部情報を取り込んだ変更は、最終的に Surtr の正本仕様（`doc/`）に整合させる

### 5. 長期タスク運用（automations / memory）

- 継続タスクは自動再開可能な単位でチェックポイントを残す
- 中断時は「次の1手」を明文化してスレッドに残す
- 個人設定依存の判断は `AGENTS.md` と issue 記述に寄せ、暗黙メモリ依存を避ける

### 6. 完了条件（Definition of Done）

- 実装: 仕様差分が説明可能
- テスト: 最低 `cargo nextest run --workspace`、必要に応じ `cargo nextest run -p rune --test integration run_srt`
- 共有: 変更理由・影響範囲・未解決事項を PR/ログに明記

---

## REPL 操作セッション運用（必須）

- REPL 操作セッションは iTerm2 プロファイル `Codex` を使用する
- セッション作成時は、人間が同じ画面を確認できる参加コマンドを必ず提示する
  - 例: `tmux attach -t surtr-repl`
- セッション継続運用時は detach 手順も併記する
  - 例: `Ctrl-b` → `d`
- REPL 関連改修（`crates/xldr/**`, `crates/rune/**` の REPL 経路, `docs/dev/Xldr_spec.md`, REPL 統合テスト）ではこのフローを必ず適用する

---

*Surtr — 既存の妥協を、型で焼き払う。*
