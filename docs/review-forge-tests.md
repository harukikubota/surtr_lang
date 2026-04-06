# Forge コードレビュー: 構文・バイトコード整合性とテスト網羅

対象: `crates/forge/src/codegen.rs` および `crates/forge/src/lib.rs`

---

## 1. バグ・潜在的な問題

### [重要] `emit_match` — 最終アームが失敗した際のスタック破壊

**場所:** `codegen.rs` `emit_match` (l.1863–1903)

```rust
let next_arm = if i + 1 < arms.len() {
    arm_labels[i + 1]
} else {
    end_label   // ← 最終アームのパターン失敗時もここへジャンプ
};
// ...
self.patch_label(end_label);  // ← body 未実行のままここへ到達
```

最終アームのパターンが一致しない場合、`end_label` にジャンプするが、
そこはボディを実行せずに通過した後のためスタックに値が積まれていない。
`scar` が網羅性を静的保証しているなら回避されるが、codegen 単体では無防備。
他のアームは `arm_labels[i+1]` へ飛んで次のパターンを試みるのに対し、
最終アームだけエラーにならずに素通りする。

**対応案:** 最終アームの `next_arm` を `end_label` ではなく専用の
`no_match_label` にし、そこで `emit_pattern_mismatch_failure` を呼ぶ。

---

### [重要] `finalize()` — ラベル未解決を黙殺して PC=0 に解決

**場所:** `codegen.rs` l.2075

```rust
let pos = self.label_positions.get(label).copied().unwrap_or(0) as u32;
```

`patch_label` されていないラベルは PC=0 (プログラム先頭) へのジャンプになる。
誤ったバイトコードがサイレントに生成される。

**対応案:**

```rust
let pos = self.label_positions
    .get(label)
    .copied()
    .expect("BUG: label used but never patched") as u32;
```

あるいは `Result` を返すよう `finalize` のシグネチャを変更する。

---

### [軽微] `emit_safebind` — 死んだ空 `if` ブロック

**場所:** `codegen.rs` l.1103–1105

```rust
if matches!(pat, TypedPattern::Wildcard(_)) {
    // no-op
}
let unit_idx = self.add_constant(Constant::Unit);
```

何も行わない空ブロック。削除漏れと思われる。

---

### [軽微] `binop_to_opcode` — エラー時スパンが常に `(0, 0)`

**場所:** `codegen.rs` l.2033, 2058–2062

```rust
let dummy_span = Span { start: 0, end: 0 };
// ...
_ => Err(CodegenError {
    message: format!("Unsupported binop {:?} for type", op),
    span: dummy_span,  // 常に (0, 0)
})
```

呼び出し元 `emit_node` はノードのスパンを持っているため、
そちらを引数として渡すか、`emit_node` 側でスパンを上書きすべき。

---

### [軽微] `emit_match` — `arm_labels[0]` を確保するが未使用

**場所:** `codegen.rs` l.1875–1878

```rust
for _ in arms {
    arm_labels.push(self.fresh_label());
}
```

`arm_labels[i+1]` しか使用しない設計のため `arm_labels[0]` は
`patch_label` も jump target にもならない。ラベル ID を 1 個無駄に消費する。

**対応案:** `arms.len().saturating_sub(1)` 個だけ確保するか、
インデックスを `i` ではなく `i+1` ベースで管理する。

---

## 2. 構文 ↔ バイトコード 整合性確認

主要パスを実装と照合した結果、以下はすべて期待通りのバイトコードを生成している。

| 構文 | 期待バイトコード列 | 状態 |
|------|------------------|------|
| `x = rhs` | `emit(rhs)` → `StoreLocal` → `LoadConst(Unit)` | ✅ |
| `x + y : Int` | `emit(x)` → `emit(y)` → `AddInt` | ✅ |
| `if(c, t, e)` | `emit(c)` → `JumpIfFalse(else)` → `emit(t)` → `Jump(end)` → `emit(e)` | ✅ |
| `if(c, t)` (else なし) | `emit(c)` → `JumpIfFalse(end)` → `emit(t)` → `Pop` → `LoadConst(Unit)` | ✅ |
| `[1, 2, 3]` | 各要素 `LoadConst` → `ListFromItems(3)` | ✅ |
| `User { name: "a", age: 1 }` | `LoadConst(tag)` → 各フィールド → `StructNew(2)` | ✅ |
| `user.age` (index=1) | `emit(user)` → `GetField(1)` | ✅ |
| `"${x} hello"` | `emit(x)` → `CallBuiltin(to_string,1)` → `LoadConst(" hello")` → `ConcatStr` | ✅ |
| `deferror Oops { "msg" }` 本体 | `emit(body)` → `MakeError(template_id)` → `Return` | ✅ |
| `match x { ... }` 成功パス | scrutinee `StoreLocal` → パターンテスト → ボディ → `Jump(end)` | ✅ |
| closure `fn(x) -> x + 1` | `LoadFunctionRef` → captures `LoadLocal` × n → `CaptureClosure(n)` | ✅ |
| `safe_div(a, b)` | `emit(a)` → `emit(b)` → `CallBuiltin(3, 2, ...)` | ✅ |

`normalize_function_table` の remap は `LoadFunctionRef` と `Call` のみを対象としているが、
`CallClosure` は fun_idx フィールドを持たないため問題なし。

---

## 3. テスト網羅の不足

現在の unit テストは 4 本のみで、構造的不変条件
(deprecated opcode なし / 関数テーブルインデックス / 型タグ) だけを検証している。
以下は未カバー。

| 未テスト項目 | 追加すべき理由 |
|-------------|--------------|
| `match` バイトコード列 | `emit_match` の jump 構造とスクルティニー保存スロット |
| `if/else` ジャンプ解決済みアドレス | `JumpIfFalse`・`Jump` の絶対 PC が正しいか |
| closure / `CaptureClosure` | キャプチャ変数のスロット順とクロージャ本体の入口 PC |
| `SafeBind` (Result 系) | `GetTag` → `GetField(0)` の順序と error 伝搬 |
| `SafeBind` (List 系) | exact-list パターンの `ListIsEmpty` / `ListHead` / `ListTail` 列 |
| `MakeError` の発行 | `deferror` constructor が `MakeError` を含むか |
| `ListFromItems` | リストリテラルの opcode |
| 文字列補間バイトコード | `CallBuiltin(to_string)` + `ConcatStr` の順序 |
| REPL `localize_chunk_indices` | `const_base` / `error_template_base` のオフセット補正が正しいか |
| `normalize_function_table` remap | 非連続 fun_idx が連番に詰め直されるか |
| `GetField(0)` | 第 1 フィールドアクセス (現テストは index 1 のみ) |
| `top_level_returns_result` モード | `emit_pattern_failure` の `Halt` 分岐 |

---

## 4. 推奨対応まとめ

### 優先度 高

1. **`finalize()` の `unwrap_or(0)` を hard error に変更**
   — ラベル未解決バグを実行時ではなく codegen 時に検出する。

2. **`emit_match` に「全アーム不一致」エラーパスを追加**
   — 型チェッカーの網羅性保証に codegen が依存しないようにする。

### 優先度 中

3. `emit_safebind` の空 `if` ブロック削除
4. `binop_to_opcode` のエラースパンを呼び出し元から渡す
5. `match` / `if/else` / closure / `SafeBind` / `ListFromItems` の unit テスト追加
6. `GetField(0)` のテスト追加 (既存テストは index 1 のみ)

### 優先度 低

7. `arm_labels[0]` の無駄な確保を解消
