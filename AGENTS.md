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
├── lib/                   # 標準モジュール source (`@@doc` を含む)
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
| `doc/EldrVM_spec.md`         | VM仕様書 |
| `doc/テスト方針.md`            | テストの分離方法・レイヤー |
| `doc/float.md`               | `Float` の暫定仕様メモ |
| `doc/Rune_observability.md`  | `Rune` / `Eldr` の観測系オプション設計 |

---

## Documentation Workflow

- `doc/`: 正本仕様
  - `要件定義v9.md`, `EldrVM_spec.md`, `Xldr_spec.md`, `テスト方針.md`, `open-issues.md`, `float.md`, `Rune_observability.md`, `Enum.md`
- `docs/`: 補助資料・公開向けガイド
- `lib/*.srt`: 標準モジュールの利用者向けドキュメント。`@@doc` を正本とする
- `crates/**`: 実装者向け内部契約。公開境界は rustdoc で残す

実装タスクの着手時は `doc/要件定義v9.md` と該当 spec を最優先で参照し、不整合があれば先に正本を更新してからコードを変更すること。

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
- `Bootstrap` / `Kernel` / `Result` と、`@@autoimport` 付き標準 trait は auto import 対象として扱う
- 明示 `import` は同じ file 内の auto-import 名を shadow してよい
- 明示 `import` 同士、および auto-import 同士の同名衝突は compile error とする
- `new` と構造体名そのものは import 対象外。`import User` は無効、`User` は型/構造体 head としてそのまま解決する
- 型名は `Mod::Type` ではなく bare identifier / generic で解決する
- 型名は flat type namespace で扱う。「どの file からも同じ見え方で使う」前提でよいが、同一可視圏で同名型が複数見える場合は compile error とする

---

## Current Focus

- `Int` は `BigInt` を採用し、通常算術でオーバーフローしない前提で扱う
- runtime 内部 ID（tag / builtin_id / fun_idx）は固定幅の内部識別子として扱い、user-visible `Int` と混同しない
- `Float` は実装を維持するが、厳密契約は `doc/float.md` で継続整理する
- `type` は予約語として扱う
- `@@builtin` の surface 宣言は標準 module 内の宣言層であり、`@@builtin def` / `@@builtin type` を受理するが、追加・変更の正本ではない
- 標準モジュールの利用者向け説明は `lib/*.srt` の `@@doc` に載せる

---

## Testing

### ユニットテスト

各クレートに `#[test]` を書く。デフォルトのテストランナーは `cargo nextest run` とし、workspace 全体が通ること。

未実装・未確定の将来仕様は、原則として skipped / ignored テストではなく `doc/open-issues.md` に退避すること。

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
cargo nextest run --workspace
cargo nextest run -p rune --test run_srt
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
- テスト: 最低 `cargo nextest run --workspace`、必要に応じ `cargo nextest run -p rune --test run_srt`
- 共有: 変更理由・影響範囲・未解決事項を PR/ログに明記

---

## REPL 操作セッション運用（必須）

- REPL 操作セッションは iTerm2 プロファイル `Codex` を使用する
- セッション作成時は、人間が同じ画面を確認できる参加コマンドを必ず提示する
  - 例: `tmux attach -t surtr-repl`
- セッション継続運用時は detach 手順も併記する
  - 例: `Ctrl-b` → `d`
- REPL 関連改修（`crates/xldr/**`, `crates/rune/**` の REPL 経路, `doc/Xldr_spec.md`, REPL 統合テスト）ではこのフローを必ず適用する

---

## 作業フロー集約（旧 `作業フロー.md`）

`作業フロー.md` は tmp 扱いとし、運用上の正本はこの `AGENTS.md` に集約する。

### 現在地（2026-04-18）

- `Int = BigInt`
- runtime 内部 ID の分離
- `type` の予約語化
- `AstTy::Generic`
- match arm LHS の `SafeBind` 系統一
- closure 引数型注釈の任意化
- `@@builtin type` 受理
- `@@doc """..."""` の導入
- `.srt -> .eldr` の doc metadata 持ち運び
- 標準モジュールの type 単位分割

### 今回完了した実装単位

#### 1. Frontend / AST

- `@@doc """..."""` を `defmod` / `def` / `@@builtin type` / `@@builtin def` の直前で受理
- triple-quoted doc string を raw text として保持
- `BuiltinTypeHead` を generic parameter 付きで保持
- `type` を予約語として固定

#### 2. Builtin type 契約

- compiler が canonical head を内部固定で保持
- 標準モジュール source は次と完全一致でなければ compile error
  - `Int`
  - `Float`
  - `String`
  - `Boolean`
  - `Unit`
  - `Error`
  - `List<$A>`
  - `Result<$T>`
- `Result<T, E>` は戻り値位置専用の補助表記として維持

#### 3. 標準モジュール再編

- load order を `Bootstrap -> [Kernel, Int, String, Boolean, Error, List, Result, Float] -> user` に固定
- cross-cutting builtin 関数は `lib/kernel.srt` の `defmod Kernel` 配下に置く
- canonical builtin type 宣言は各対応 file のトップレベルに置く
  - `Unit` は `lib/kernel.srt`
  - `Int` は `lib/int.srt`
  - `String` は `lib/string.srt`
  - `Boolean` は `lib/boolean.srt`
  - `Error` は `lib/error.srt`
  - `List<$A>` は `lib/list.srt`
  - `Result<$T>` は `lib/result.srt`
  - `Float` は `lib/float.srt`
- type ごとの標準モジュールを追加
  - `lib/int.srt`
  - `lib/string.srt`
  - `lib/boolean.srt`
  - `lib/error.srt`
  - `lib/list.srt`
  - `lib/result.srt`
  - `lib/float.srt`
- 新設標準モジュールには module-level `@@doc` と `dummy()` を配置

#### 4. `.eldr` doc metadata

- `Bytecode.docs` を追加
- `.eldr` に optional な `Docs` chunk を追加
- 旧 `.eldr` は `Docs` chunk なしでも decode 可能
- `surtr dump --format json` で `doc_count` を確認可能

#### 5. REPL / Xldr

- session が std module / live chunk / `.eldr` の doc metadata を保持
- `:doc <symbol>` を実装
- `push_atomic` は checkpoint + append/rollback 方式へ移行済み

#### 6. 正本ドキュメントの再配置

- `doc/` は正本だけを残す
- `docs/` は補助資料・公開ガイドに限定
- `lib/*.srt` は利用者向け標準モジュールドキュメント
- `crates/**` は rustdoc による実装境界の記録先

### 参照先

- 言語仕様: `doc/要件定義v9.md`
- VM 仕様: `doc/EldrVM_spec.md`
- REPL 仕様: `doc/Xldr_spec.md`
- テスト方針: `doc/テスト方針.md`
- 将来課題: `doc/open-issues.md`
- `Float` 暫定メモ: `doc/float.md`

### 将来課題

- `Float` の厳密契約
- project runner
- REPL command 拡張
- macro
- closure の `expected=None` 推論強化
- OOM / host failure の詳細契約
- Enum 設計

### 次に着手するときの入口

1. 仕様を変えるなら `doc/要件定義v9.md` と `doc/open-issues.md` を先に更新する
2. 標準ライブラリの利用者向け説明は `lib/*.srt` の `@@doc` を更新する
3. バイトコードや REPL まわりを変えるときは `doc/EldrVM_spec.md` と `doc/Xldr_spec.md` を一緒に見る
4. 将来仕様の先置きは `doc/open-issues.md` に退避し、仕様確定後に通常テストとして追加する

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
