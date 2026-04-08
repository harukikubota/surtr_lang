# eldr レビュー

2026-04-08 取り込みメモ:

- 整数オーバーフロー項目は `Int=BigInt` 移行で処理する
- tag と user `Int` の分離は今回の基盤変更対象とする
- `Float` の厳密契約は `doc/float.md` へ切り出し、本メモでは runtime テスト観点に集中する

観点：テスト網羅 / Rust依存の未検査ランタイムエラー / 今後起こりうるランタイムエラー

---

## 1. テスト網羅

### vm.rs ユニットテスト（25本）— 未カバーの正常系オペコード

以下のオペコードは integration test では触れているが、`vm.rs` の unit test では直接テストがない。

| カテゴリ | 未テストオペコード |
|---|---|
| 算術 | `AddInt`, `SubInt`, `MulInt`, `AddFloat`, `SubFloat`, `MulFloat` |
| 比較 | `EqInt`〜`GteInt`, `EqFloat`〜`GteFloat`, `EqStr`, `NeqStr`, `EqBool`, `NeqBool` |
| 単項 | `NegInt`, `NegFloat`, `NotBool` |
| 文字列 | `ConcatStr` |
| リスト | `ListNew`, `ListEmpty`, `ListNil`, `ListCons`, `ListIsEmpty`, `ListHead`, `ListTail`, `ListFromItems` |
| 構造体 | `StructNew`, `GetField`, `GetTag` |
| クロージャ | `CaptureClosure`, `CapturePartial`, `CallClosure` |
| その他 | `Pop`, `StoreLocal`, `LoadBuiltinRef`, `LoadFunctionRef`, `MakeError`, `MakeErrorLiteral`, `JumpIfFalse`, `JumpIfTrue` |

### vm.rs — 未テストのエラーパス

| パス | 場所 |
|---|---|
| `ListHead` on empty list | `vm.rs:861` |
| `ListTail` on empty list | `vm.rs:878` |
| `ListCons` with non-list tail | `vm.rs:836` |
| `ListIsEmpty` with non-list | `vm.rs:848` |
| `GetField` on non-tagged | `vm.rs:929` |
| `GetTag` on non-tagged | `vm.rs:939` |
| `JumpIfFalse`/`JumpIfTrue` with non-Bool | `vm.rs:1199, 1211` |
| `CaptureClosure`/`CapturePartial` with non-callable | `vm.rs:1102, 1123` |
| `CallClosure` with non-callable | `vm.rs:1141` |
| `CallClosure` クロージャ経由の arity mismatch | `vm.rs:1159` |
| `MakeError` with non-String | `vm.rs:995` |
| `verify_chunk` の duplicate `fun_idx` | `vm.rs:501` |

### builtin.rs — ユニットテスト不足

現状はアライメントチェック 1 本のみ。以下が未テスト：

- `safe_div` / `safe_mod` の型不一致エラーパス（`(Float, Int)` 等）
- `shl` / `shr` の負数・範囲超過（`checked_shl`/`checked_shr` 失敗パス）
- `set_exit_code` の `i32` 範囲超過（`i64::MAX` 等）
- `eprint` のバッファキャプチャモード vs 実 stderr 分岐

### error.rs — カバレッジの穴

- `report_runtime_error` 自体（source あり / source なし 両パス）のテストが 1 本もない
- `runtime_error_verbose_enabled` の環境変数パーサのテストがない（`"yes"`, `"true"` 等の各値）

---

## 2. Rust依存の未検査ランタイムエラー

> floatは今後対応のため除く

### 整数オーバーフロー（最重要）

`vm.rs` の以下の箇所は **debug ビルドでは panic、release ビルドでは黙って wrapping** する。

```rust
// vm.rs:742-744
Opcode::AddInt => self.int_binop(|a, b| Ok(Value::Int(a + b)))?,  // i64 overflow
Opcode::SubInt => self.int_binop(|a, b| Ok(Value::Int(a - b)))?,  // i64 underflow
Opcode::MulInt => self.int_binop(|a, b| Ok(Value::Int(a * b)))?,  // i64 overflow
```

```rust
// vm.rs:800
Opcode::NegInt => {
    let a = self.pop_int()?;
    self.stack.push(Value::Int(-a));  // -i64::MIN が panic
}
```

**修正方針**：`checked_add` / `checked_sub` / `checked_mul` / `checked_neg` を使い、
`None` を `RuntimeError::new("integer overflow")` に変換する。

```rust
// 修正例
Opcode::AddInt => self.int_binop(|a, b| {
    a.checked_add(b)
        .map(Value::Int)
        .ok_or_else(|| RuntimeError::new(format!("integer overflow: {} + {}", a, b)))
})?,
```

### float（今後対応）

`AddFloat`, `SubFloat`, `MulFloat`, `NegFloat` は bare `+`, `-`, `*` を使用。
panic はしないが `f64::INFINITY`, `f64::NAN` を黙って伝播する。

`builtin_safe_div` の float ゼロ除算チェック（`*b == 0.0`）は `b = f64::NAN` のとき
`false` になるため、`NaN` を分母に渡すと `NaN` が `Ok(...)` として返る。将来の地雷として記録。

---

## 3. 今後起こりうるランタイムエラー

### スタックオーバーフロー（未対応として認識済み）

メインループは iterative なので、Surtr コードの関数呼び出し自体は Rust スタックを消費しない
（`self.frames: Vec<CallFrame>` に積む）。ただし以下が残る：

- **`ListHandle` の Drop**：`Box<Node>` による linked list 実装の場合、非常に長いリストの
  Drop が再帰的になり Rust スタックオーバーフローを起こす可能性がある（実装要確認）
- **`push_atomic` の `self.clone()`**：REPL で大きな VM 状態を clone するたびにヒープ
  割り当てが発生する（スタック問題ではないが将来的な性能劣化）

### 無限ループ / 実行予算なし

実行オペコード数の上限がない。Surtr コードで無限ループを書くと VM スレッドが永続的にブロックする。
将来的にはオペコードカウンタや fuel-based execution の検討が必要。

### OOM（メモリ枯渇）

| 箇所 | 原因 |
|---|---|
| `vm.rs:814` `ListNew(n)` | `Vec::with_capacity(n as usize)` — `n` は `u32` なので最大 4 GiB 相当の確保が起こりうる |
| `vm.rs:889` `ListFromItems(n)` | 同上 |
| `vm.rs:900` `StructNew(num_fields)` | 同上 |
| `vm.rs:793` `ConcatStr` | 巨大文字列の無制限連結 |
| `self.frames` Vec | Surtr の無限再帰で無制限に成長 |

---

## 優先度まとめ

| 優先度 | 項目 |
|---|---|
| **高** | `AddInt`/`SubInt`/`MulInt`/`NegInt` を `checked_*` に変更（debug build の panic を防ぐ） |
| **高** | `ListHead`/`ListTail`/`GetField`/`GetTag` 等のエラーパス unit test 追加 |
| **中** | `builtin.rs` の型不一致・範囲超過エラーパスの unit test 追加 |
| **中** | `ListNew`/`ListFromItems`/`StructNew` の `n` 上限チェック検討 |
| **低** | `report_runtime_error` の unit test 追加 |
| **将来** | float オーバーフロー・NaN 伝播への対応 |
| **将来** | 実行予算（オペコード数上限）の追加 |
