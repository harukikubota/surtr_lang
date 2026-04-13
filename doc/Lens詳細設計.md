# Surtr Lens 詳細設計（Stage1 + Stage2実装進捗）

最終更新日: 2026-04-13

---

## 1. 目的

Surtr の Lens は compiler-managed な `Lens<S, A>` として実装する。
現時点では Stage1 の基盤に加えて、Stage2 の一部（`set/over` と `Tuple._N` 文脈付き root）まで反映済み。

- `Lens::view`
- `Lens::compose`
- `Lens::set`
- `Lens::over`
- value-root access sugar（`value.path`）
- 文脈依存な `Lens::view` 返り型

first-class Lens と standalone `_0` は導入しない方針を維持する。

---

## 2. Public Contract

### 2.1 Surface

```surtr
@@builtin type Lens<$S, $A>

@@autoimport
defmod Lens {
  @@builtin def view(lens: Lens<$S, $A>, source: $S) -> Result<$A>
  @@builtin def set(lens: Lens<$S, $A>, source: $S, value: $A) -> Result<$S>
  @@builtin def over(lens: Lens<$S, $A>, source: $S, update_fun: ($A -> Result<$A>)) -> Result<$S>
  @@builtin def compose(outer: Lens<$S, $A>, inner: Lens<$A, $B>) -> Lens<$S, $B>
}
```

- Stage1 で使う合成 API 名は `compose` だけに固定する（`join` は使わない）。
- enum selector は PascalCase 固定（例: `Expr.Add`）。
- tuple access は `._0`, `._1`, ... 固定（`.0`, `.1` は不許可）。
- mismatch error 名は `VariantMismatch` で統一する。

### 2.2 文脈依存 `view` 返り型

`Lens::view` と `value.path` は同じ規則で型付けする。

1. 初期文脈 `C`:
   - source が `Result<T>` なら `C = Result`
   - それ以外は `C = Plain`
2. segment 遷移:
   - field / tuple segment では `C` を維持
   - variant segment を含んだ時点で `C = Result`（mismatch 可能）
3. 最終返り値:
   - `C = Plain` なら `A`
   - `C = Result` なら `Result<A>`

要点:

- `Result` が絡まない access は成功確定なので plain 値を返す。
- 失敗可能文脈（source が `Result` / variant selector を含む）だけ `Result` を返す。

---

## 3. Stage1 制約

- `Lens` は compile-time capability として扱い、runtime 値としては運搬しない
- スコープ内での束縛は許可する
  - `lens = User.name`
  - `Lens::view(lens, user)`
- 関数境界での Lens 運搬は禁止する
  - 関数引数・戻り値の `Lens<...>` 注釈は禁止
  - 閉包 capture で Lens を持ち出すことは禁止
- 関数には Lens ではなく `view` 済み値（`A` / `Result<A>`）を渡す
- `_0` 単体 root 定数は未対応
  - `pair._0` は可
  - `Tuple._0` は **型文脈あり** のときのみ可
  - `_0` 単体は不可

### 3.1 private capability 境界（Stage1）

- `Type.private_field` の capability 参照は禁止（`Field 'T.private_field' is private`）
- `value.private_field` は値アクセスとしては許可する
- ただし closure 内で private field を参照する形（例: `{|| user.password}`）は、scope 外 escape を防ぐため禁止する

---

## 4. Lowering 方針

### 4.1 Scar

- `Ty::Lens` を導入する。
- `Resolved::FieldAccess` を Stage1 Lens 規則で型付けする。
- 専用 `Resolved` ノードは導入しない（既存 `Resolved::FieldAccess` を維持）。
- `Lens::compose` / `Lens::view` は intrinsic として検査し、path metadata を構築する。

### 4.2 Forge

- Lens を runtime value 化しない。
- compile-time path metadata から直接 lowering する。
- `Plain` 文脈:
  - `GetField` / `GetTupleField` を直適用し plain 値を返す。
- `Result` 文脈:
  - source `Result` は `Err` を伝播する。
  - success 側で path を進め、最終的に `Ok(focus)` を構築する。
  - variant mismatch 時は `Err(VariantMismatch(...))` を構築する。

### 4.3 Eldr

- `view` / `compose` / `set` / `over` は `BUILTIN_METAS` へ追加する（末尾 ID 追加）。
- これらが runtime builtin として直接呼ばれた場合は防御的 `RuntimeError` を返す。
  - 意味: Forge lowering 漏れ検出。

---

## 5. 標準ライブラリ / ロード順

- `lib/lens.srt` を追加し `@@autoimport defmod Lens` を提供する。
- `Lens` を標準ロード順へ追加する。
- `VariantMismatch` の `deferror` を標準で提供する（bootstrap）。

---

## 6. テスト観点（Stage1）

- Spire:
  - `pair._0` 成功
  - `pair.0` parse error
- Scar:
  - `user.name` は plain（`String`）
  - `result_user.name` は `Result<String>`
  - `expr.Add` は `Result<...>`、`expr.add` は失敗
  - `Lens::compose` の整合 / 不整合
  - `Lens::set` / `Lens::over` の型契約
  - `Tuple._N`（型文脈あり）成功 / 無文脈失敗
  - 関数境界運搬制約違反（arg/return/capture）
  - runtime container（tuple/list/constructor）への Lens 混入拒否
  - private capability 境界（`Type.private_field` NG、`value.private_field` OK、closure 内 private NG）
- Forge/Eldr:
  - plain/result 文脈の lowering 分岐
  - variant mismatch で `VariantMismatch` を `Err(...)` で返す
- Fixture:
  - `.0/.1` を `._N` へ移行
  - `join` 名は `compose` に置換
