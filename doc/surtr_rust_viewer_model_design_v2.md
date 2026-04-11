# Surtr / Eldr ViewerModel 設計メモ

## 目的

Rust を正本にしつつ、JS / TypeScript 側で扱いやすい ViewerModel を定義する。

前提は次の通り。

- バイナリ仕様の解釈は Rust に集約する
- UI は React / TypeScript 側で構築する
- JS 側では plain object と discriminated union として扱える形にする
- バイナリ内部表現をそのまま公開せず、表示用の ViewerModel に変換する
- 将来の opcode / chunk / debug 情報追加に耐えやすくする

---

## 基本方針

### 正本の位置

正本は Rust 側に置く。

```text
.eldr binary
  -> Rust decoder
  -> internal inspect model
  -> ViewerModel
  -> wasm / JSON
  -> React UI
```

このとき UI は binary format を知らず、ViewerModel だけを扱う。

### UI 向けに別モデルを持つ

内部型をそのまま JS に渡さない。

例えば内部が次のような命令でも、

```rust
Opcode::Call(fun_idx, arity, frame_size, flags)
```

UI に渡すときは次のようにする。

```json
{
  "kind": "Call",
  "fun_idx": 3,
  "arity": 2,
  "frame_size": 0,
  "flags": 0
}
```

この形なら TS 側で `kind` による分岐ができる。

---

## 推奨 crate 構成

```text
eldr_core
  - binary decoder
  - internal inspect model
  - viewer model
  - schema export

eldr_cli
  - inspect command
  - json dump

eldr_wasm
  - wasm bindgen entrypoints
  - bytes -> JS object

viewer_web
  - React / TypeScript UI
```

### 役割

#### eldr_core
- `.eldr` の decode
- internal inspect model の生成
- ViewerModel の生成
- JSON Schema の生成

#### eldr_cli
- ファイル読込
- inspect 実行
- JSON 出力

#### eldr_wasm
- バイト列受取
- `ViewerModel` を JS object に変換して返す

#### viewer_web
- React UI
- search/filter
- source highlight
- pane 構成
- テーブルやツリー表示

---

## ViewerModel 設計原則

### 1. UI が必要な情報だけを出す

内部で持っていても UI で使わない情報は出さない。

### 2. enum は `kind` で判別できるようにする

TS 側で discriminated union として扱える形にする。

### 3. 数値の意味はフィールド名に展開する

単なる配列ではなく、名前付きオブジェクトにする。

悪い例:

```json
{ "op": "Call", "args": [3, 2, 0, 0] }
```

良い例:

```json
{
  "kind": "Call",
  "fun_idx": 3,
  "arity": 2,
  "frame_size": 0,
  "flags": 0
}
```

### 4. JS が参照しやすい安定 ID を持つ

- function_id
- chunk_id
- opcode pc
- constant idx
- label id

### 5. source との対応を持つ

opcode と source を相互参照できるようにする。

---

## 推奨トップレベル構造

```json
{
  "schema_version": 1,
  "format": "eldr_viewer",
  "header": {},
  "chunks": [],
  "functions": [],
  "constants": [],
  "sources": [],
  "errors": []
}
```

### フィールド一覧

| field | 説明 |
|---|---|
| schema_version | ViewerModel の schema version |
| format | 識別用文字列 |
| header | ELDR header 情報 |
| chunks | chunk 一覧 |
| functions | function 一覧 |
| constants | constant 一覧 |
| sources | source file 一覧 |
| errors | error template 一覧 |

---

## Rust 側の推奨 struct

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ViewerFile {
    pub schema_version: u32,
    pub format: String,
    pub header: HeaderView,
    pub chunks: Vec<ChunkView>,
    pub functions: Vec<FunctionView>,
    pub constants: Vec<ConstantView>,
    pub sources: Vec<SourceFileView>,
    pub errors: Vec<ErrorTemplateView>,
}
```

---

## HeaderView

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HeaderView {
    pub magic: String,
    pub version: u32,
    pub debug_level: u32,
    pub num_chunks: u32,
}
```

### 目的
- 先頭ヘッダを UI に出す
- inspect の妥当性確認に使う

---

## ChunkView

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChunkView {
    pub chunk_id: String,
    pub tag: ChunkTagView,
    pub size: u32,
    pub payload_offset: u32,
    pub padded_size: u32,
    pub summary: ChunkSummaryView,
}
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind")]
pub enum ChunkTagView {
    Code,
    Data,
    Unknown { raw_tag: String },
}
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChunkSummaryView {
    pub function_ids: Vec<String>,
    pub constant_range: Option<IndexRangeView>,
    pub opcode_range: Option<IndexRangeView>,
}
```

### 目的
- chunk 一覧表示
- chunk ごとの関数や opcode へのリンク
- raw chunk ではなく要約を持つ

---

## FunctionView

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FunctionView {
    pub function_id: String,
    pub fun_idx: u32,
    pub name: Option<String>,
    pub entry_pc: u32,
    pub end_pc: Option<u32>,
    pub arity: u8,
    pub num_locals: u32,
    pub chunk_id: String,
    pub source_ref: Option<SourceRefView>,
    pub opcode_pcs: Vec<u32>,
}
```

### 目的
- 関数一覧
- entry_pc からジャンプ
- function -> opcodes の逆引き

### 補足
`name` は不明でもよい。  
UI 上で `fn#3` のようなフォールバック名を生成できる。

---

## ConstantView

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind")]
pub enum ConstantView {
    Int {
        idx: u32,
        value: i64,
    },
    Float {
        idx: u32,
        value: f64,
    },
    Str {
        idx: u32,
        value: String,
    },
    Bool {
        idx: u32,
        value: bool,
    },
    Unit {
        idx: u32,
    },
}
```

### 目的
- constant table 表示
- opcode から constant 逆参照

### 方針
- `kind` を明示する
- `idx` を各 variant 内に持つ
- UI でのラベル生成を単純にする

---

## OpcodeView

これが最重要。

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OpcodeRowView {
    pub pc: u32,
    pub function_id: Option<String>,
    pub op: OpcodeView,
    pub source_ref: Option<SourceRefView>,
    pub label: Option<String>,
}
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind")]
pub enum OpcodeView {
    LoadConst {
        const_idx: u32,
    },
    LoadBuiltinRef {
        builtin: String,
    },
    LoadFunctionRef {
        fun_idx: u32,
    },
    LoadLocal {
        local_idx: u32,
    },
    StoreLocal {
        local_idx: u32,
    },
    AddInt,
    SubInt,
    MulInt,
    DivInt,
    ModInt,
    AddFloat,
    SubFloat,
    MulFloat,
    DivFloat,
    EqInt,
    NeqInt,
    LtInt,
    LteInt,
    GtInt,
    GteInt,
    EqFloat,
    NeqFloat,
    LtFloat,
    LteFloat,
    GtFloat,
    GteFloat,
    EqStr,
    NeqStr,
    EqBool,
    NeqBool,
    ConcatStr,
    NegInt,
    NegFloat,
    NotBool,
    ListNew {
        len: u32,
    },
    ListEmpty,
    StructNew {
        type_id: u32,
    },
    GetField {
        field_idx: u32,
    },
    GetTag,
    CallBuiltin {
        builtin_id: u16,
        arity: u8,
    },
    Call {
        fun_idx: u32,
        arity: u8,
        frame_size: u32,
        flags: u32,
    },
    MakeClosure {
        capture_count: u8,
    },
    CallClosure {
        arity: u8,
        frame_size: u32,
        flags: u32,
    },
    MakeError {
        template_id: u32,
    },
    Jump {
        target_pc: u32,
    },
    JumpIfFalse {
        target_pc: u32,
    },
    JumpIfTrue {
        target_pc: u32,
    },
    Pop,
    Return,
    Halt,
    Unknown {
        opcode_name: String,
        raw_args: Vec<u32>,
    },
}
```

### 設計意図

#### `kind` を discriminator にする
TS 側で

```ts
switch (row.op.kind) {
  case "LoadConst":
  case "Call":
  case "Return":
}
```

ができるようにする。

#### 数値配列ではなく名前付き引数にする
UI 側でラベルや詳細パネルを作りやすい。

#### `Unknown` を持つ
将来の互換性が上がる。  
古い UI でも最低限読める。

---

## SourceFileView / SourceRefView

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceFileView {
    pub source_id: String,
    pub name: Option<String>,
    pub text: Option<String>,
}
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceRefView {
    pub source_id: String,
    pub span_start: u32,
    pub span_end: u32,
    pub line: u32,
    pub column: u32,
}
```

### 目的
- source pane を表示する
- opcode クリック時のハイライト
- source から opcode 逆引き

### 方針
- `text` は埋め込んでもよいし省略してもよい
- source 本文を持たない場合でも span 情報は残す

---

## ErrorTemplateView

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ErrorTemplateView {
    pub template_id: u32,
    pub kind: String,
    pub format: String,
    pub num_params: u8,
    pub source_ref: Option<SourceRefView>,
}
```

### 目的
- エラーテンプレート一覧表示
- opcode の `MakeError` と紐付け
- ソース位置の表示

---

## 補助型

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IndexRangeView {
    pub start: u32,
    pub end: u32,
}
```

---

## JS / TS で扱いやすくするためのルール

### 1. tuple を使わない
TS 側で意味が読みにくい。

### 2. `kind` を必ず持つ
enum の全 variant で discriminator を揃える。

### 3. `Option<Option<T>>` を避ける
nullability が複雑になる。

### 4. viewer model は平坦寄りにする
内部構造の都合を持ち込まない。

### 5. index と name を両方持てるなら両方持つ
UI では `idx` と表示名の両方が必要なことが多い。

---

## TS 側の期待形

Rust から JSON Schema を生成し、TS 型を自動生成する。

TS 側では概ね次のように扱える。

```ts
export type OpcodeView =
  | { kind: "LoadConst"; const_idx: number }
  | { kind: "Call"; fun_idx: number; arity: number; frame_size: number; flags: number }
  | { kind: "Return" }
  | { kind: "Unknown"; opcode_name: string; raw_args: number[] }
```

これにより React 側で安全に分岐できる。

```ts
function opcodeLabel(op: OpcodeView): string {
  switch (op.kind) {
    case "LoadConst":
      return `LoadConst ${op.const_idx}`
    case "Call":
      return `Call fn=${op.fun_idx}/${op.arity}`
    case "Return":
      return "Return"
    case "Unknown":
      return `Unknown ${op.opcode_name}`
  }
}
```

---

## wasm での返し方

### 推奨
Rust で `ViewerFile` を作り、`serde_wasm_bindgen` で JS object として返す。

```rust
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn inspect_view(bytes: &[u8]) -> Result<JsValue, JsValue> {
    let model = build_viewer_model(bytes).map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_wasm_bindgen::to_value(&model).map_err(|e| JsValue::from_str(&e.to_string()))
}
```

### 理由
- JS 側で plain object として扱える
- React state にそのまま入れやすい
- JSON 文字列の再 parse が不要

---

## schema 生成

### 推奨フロー

```text
Rust ViewerModel
  -> schemars
  -> JSON Schema
  -> json-schema-to-typescript
  -> generated .d.ts
```

### 効果
- Rust 側の変更が UI 型へ反映される
- UI での古い手書き型とのズレを防ぎやすい
- opcode variant 追加時の未対応を検知しやすい

---

## バージョン管理

### schema_version を必ず持つ
ViewerModel 自体の schema version を持つ。

```rust
pub const VIEWER_SCHEMA_VERSION: u32 = 1;
```

### 互換性方針
- フィールド追加: 原則後方互換
- 必須フィールド変更: schema version を上げる
- variant 名変更: schema version を上げる
- variant 追加: 可能なら後方互換、ただし UI は default を持つ

---

## 変換レイヤの分離

ViewerModel の生成は 2 段階に分けると保守しやすい。

```text
Binary -> InspectModel -> ViewerModel
```

### InspectModel
- バイナリに近い
- デバッグや検証向け
- raw offset や chunk metadata を持つ

### ViewerModel
- UI 向け
- 表示都合で冗長でもよい
- function や source とリンク済み

この分離により、UI の要求変更が binary decoder に波及しにくくなる。

---

## 初期実装優先順位

### 1. 最低限
- ViewerFile
- HeaderView
- FunctionView
- ConstantView
- OpcodeRowView
- SourceRefView

### 2. 次
- ChunkView
- ErrorTemplateView
- SourceFileView

### 3. 後で追加
- label reverse index
- function call graph
- control flow graph 用ノード
- opcode category
- search index

---

## 実装上の注意

### 1. `Unknown` を残す
将来の opcode 追加に強くなる。

### 2. 文字列化しすぎない
数値 index は index のまま保持する。  
表示名は別フィールドにする。

### 3. UI 専用情報は ViewerModel に足してよい
例えば:
- `display_name`
- `short_label`
- `opcode_count`

### 4. ただし内部ロジックを UI に漏らしすぎない
ViewerModel は表示用の整形済みデータであり、実行ロジックの API ではない。

---

## 最終推奨

Surtr / Eldr では次を推奨する。

- Rust を正本にする
- internal inspect model と viewer model を分ける
- viewer model は `kind` を使う tagged enum にする
- wasm では plain object を返す
- TS 型は schema から自動生成する
- UI は generated type のみ参照する

これにより、

- バイナリ仕様変更
- opcode 追加
- chunk 拡張
- debug 情報追加

に対して、JS 側の追従コストを下げやすい。

---

## まとめ

### 採用方針
- Rust ViewerModel 方式を採用
- JS 側で扱いやすい plain object 形式にする
- enum は `kind` 判別に統一する
- schema 生成と TS 型生成を前提にする

### 期待できる効果
- UI 側の補完と型検査が効く
- 変更検知がしやすい
- binary decoder と UI の責務分離ができる
- wasm / CLI の両方で再利用できる

### 非推奨
- 内部 enum をそのまま export
- 数値配列だけの opcode 引数
- UI 側の手書き型を正本にする
- binary 仕様を JS 側で直接解釈する
