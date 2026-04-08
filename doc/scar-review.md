# scar レビュー

レビュー観点: 検査漏れ・多相型受け入れ・テスト網羅

---

## 1. 検査漏れ

### [Bug] `Binding` パターンが exhaustive と認識されない

**場所:** `crates/scar/src/checker.rs:2328–2333`

```rust
fn check_match_exhaustive(...) -> Result<(), TypeError> {
    if arms.iter().any(|(pat, _)| matches!(pat, TypedMatchPattern::Wildcard)) {
        return Ok(());
    }
    // TypedMatchPattern::Binding はここでショートサーキットされない
```

`match result { x => x }` のような変数バインディングアームは `_` と同様に catch-all だが、
exhaustiveness チェックは `Wildcard` のみを考慮する。
これにより `TypedMatchPattern::Binding` を持つ match が不当な non-exhaustive エラーになる。

**修正案:**

```rust
if arms.iter().any(|(pat, _)| matches!(
    pat,
    TypedMatchPattern::Wildcard | TypedMatchPattern::Binding(_)
)) {
    return Ok(());
}
```

---

### [Bug] `check_struct_lit` — 余剰フィールドを無視

**場所:** `crates/scar/src/checker.rs:2969–2993`

```rust
for (def_name, def_ty) in &def.fields {
    let (_, resolved_val) = field_vals.iter().find(|(n, _)| n == def_name)...
    // field_vals に余剰フィールドがあっても検出されない
```

`User { name: "a", age: 30, unknown: "!" }` のように定義にないフィールドを渡しても型エラーにならない。
`def.fields` の走査しかしておらず `field_vals` 側のチェックが欠落している。

**修正案:** ループ後に `field_vals` 内の各フィールド名が `def.fields` に存在するかを確認する。

---

### [Bug] `check_constructor_call` — named args の重複を検出しない

**場所:** `crates/scar/src/checker.rs:3191–3218`

```rust
} else if all_named {
    for arg in args {
        if let ResolvedRecordLitArg::Named(name, expr) = arg {
            let idx = def.fields.iter().position(|(n, _)| n == name)...;
            ...
            typed_fields[idx] = Some(typed_val);  // 重複時に上書き、エラーなし
```

`Pair(x: 1, x: 2)` のように同一フィールドを重複指定しても `typed_fields[idx]` が上書きされてエラーにならない。
`check_app` の `UserFunc` 分岐には重複チェックがある（L1728–1733）のに、この分岐には漏れている。

**修正案:** `typed_fields[idx].is_some()` であれば重複エラーを返す。

---

### [Bug] `check_binop` Eq/Neq — エラー生成前に substitution を汚染する

**場所:** `crates/scar/src/checker.rs:2052–2065`

```rust
BinOp::Eq | BinOp::Neq => match (&lt, &rt) {
    (Ty::Int, Ty::Int) | ... => Ok(Ty::Bool),
    _ if !self.types_compatible(&lt, &rt) => Err(...), // ← エラーパスでも &mut self を変更
    _ => Err(... "not supported" ...)
}
```

`types_compatible` は `&mut self` を受け取り、型変数バインドを副作用として実行する。
エラーパスのガード節で呼ぶと、エラーを返しつつも substitution テーブルが汚染される。

---

### [軽微] `check_deferror_def` — show_expr のスパン情報が失われる

**場所:** `crates/scar/src/checker.rs:3331–3337`

```rust
show_checker.check_node(show_expr).map_err(|err| TypeError {
    message: err.message,
    span: span.clone(),  // show_expr の正確な位置ではなく deferror 全体のスパン
    hint: err.hint,
})?;
```

show 式内のエラーが常に `deferror` キーワードのスパンに置き換えられるため、エラー箇所の特定が困難になる。
`err.span` をそのまま使うべき。

---

## 2. 多相型受け入れ

### クロージャ本体の substitutions が外側に伝播されない

**場所:** `crates/scar/src/checker.rs:1892–1925`（`check_def` も同様 L2803–2815）

```rust
let mut body_checker = Checker::with_env_and_params(fun_env, ...);
let typed_body = body_checker.check_node(body)?;
// body_checker.substitutions は使われない
self.env.next_tyvar = self.env.next_tyvar.max(body_checker.env.next_tyvar);
self.env.next_tag   = self.env.next_tag.max(body_checker.env.next_tag);
```

`next_tyvar` / `next_tag` は同期されるが `substitutions` は伝播されない。
アノテーションなしクロージャで型変数が body 内部で具体化されても、外側のチェッカーには反映されず
`$n` のまま残る可能性がある。

**修正案:**

```rust
for (k, v) in body_checker.substitutions {
    self.substitutions.entry(k).or_insert(v);
}
```

---

### `check_match` — Binding パターン使用時に型変数の伝播が不完全

**場所:** `crates/scar/src/checker.rs:2426–2429`

```rust
ResolvedMatchPattern::Binding(id) => {
    self.env.bind_var(id.unique_id, scrut_ty.clone());
```

`scrut_ty` がまだ型変数 `$n` の段階で Binding パターンに入った場合、変数は `Ty::Var(n)` として登録される。
後続の body でその変数を使い具体型が推論されても、変数 `id.unique_id` の登録型 `$n` は更新されない。
この状態で Forge に渡すと型変数が残る。

---

### `check_closure` — expected=None 時のパラメータ型推論が限定的

**場所:** `crates/scar/src/checker.rs:1875`

```rust
None => params.iter().map(|_| self.env.fresh_tyvar()).collect(),
```

アノテーションなしクロージャは全パラメータに fresh tyvar を割り当てるが、呼び出しサイト側の期待型との
統合は行われない。クロージャを引数として渡す場合、期待型が来なければ型推論の精度が落ちる。

---

## 3. テスト網羅

現行テストは SafeBind と forward reference に重点が置かれており、以下が不足している。

| カテゴリ | 未テスト項目 |
|---|---|
| **match exhaustiveness** | Result の Ok/Err 片方のみ → エラー |
| | List の nil/cons 片方のみ → エラー |
| | **Binding パターン → 通過すべき（現在バグで失敗する）** |
| **BinOp** | Int/Float 算術の正常系・異常系 |
| | 型混在（Int + Float → エラー） |
| | Concat 正常系・異常系 |
| **if-else** | 両分岐の型不一致 → エラー |
| | 条件が非 Boolean → エラー |
| **Struct リテラル** | フィールド型不一致 → エラー |
| | フィールド欠落 → エラー |
| | **余剰フィールド → エラー（現在バグで通過する）** |
| **Constructor** | named args 順序不同の正常系 |
| | **named args 重複 → エラー（現在バグで通過する）** |
| | positional/named 混在 → エラー |
| **Closure** | アノテーションなしパラメータの型推論（具体型に解決されること） |
| **Capture** | 部分適用の戻り型（残パラメータを持つ `Func` が返ること） |
| **Def** | 戻り値型不一致 → エラー |
| | entrypoint にパラメータあり → エラー |
| | entrypoint の戻り型が `Result<()>` 以外 → エラー |
| **SafeBind 型伝播** | `=?` error 型が関数戻り値と不一致 → エラー |
| **文字列補間** | Result 型の式を補間 → エラー |
| **Err(...)** | 非 deferror 値を渡す → エラー |
| **Named args (UserFunc)** | 引数順序の入れ替えが正しく解決される |

---

## 優先度まとめ

| 優先度 | 項目 |
|---|---|
| **高** | `Binding` が exhaustive 扱いされない（確定バグ） |
| **高** | Struct リテラルの余剰フィールド未検出 |
| **高** | Constructor の named arg 重複未検出 |
| **中** | クロージャ / def の substitution 非伝播 |
| **中** | `check_binop` Eq/Neq ガード節の副作用 |
| **低** | `check_deferror_def` の show_expr スパン消失 |
