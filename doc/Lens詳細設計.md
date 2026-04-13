# Surtr Lens 詳細設計（Stage1）

最終更新日: 2026-04-13

---

## 1. 目的

Stage1 では、compiler-managed な `Lens<S, A>` を導入し、次を確定する。

- `Lens::view`
- `Lens::compose`
- value-root access sugar（`value.path`）
- 文脈依存な `Lens::view` 返り型

`set/over`、first-class Lens、standalone `_0` は Stage2 以降へ送る。

---

## 2. Public Contract

### 2.1 Surface

```surtr
@@builtin type Lens<$S, $A>

@@autoimport
defmod Lens {
  @@builtin def view(lens: Lens<$S, $A>, source: $S) -> Result<$A>
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

- `Lens` は first-class 非対応
  - 束縛不可（`lens = User.name` 不可）
  - 引数受け渡し不可
  - 戻り値化不可
  - capture 不可
- 許可するのは即時式での利用のみ
  - `Lens::view(User.name, user)`
  - `Lens::view(Lens::compose(User.profile, Profile.name), user)`
- `_0` 単体 root 定数は未対応
  - `pair._0` は可
  - `_0` 単体は不可

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

- `view` / `compose` は `BUILTIN_METAS` へ追加する（末尾 ID 追加）。
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
  - first-class 制約違反（bind/arg/return/capture）
- Forge/Eldr:
  - plain/result 文脈の lowering 分岐
  - variant mismatch で `VariantMismatch` を `Err(...)` で返す
- Fixture:
  - `.0/.1` を `._N` へ移行
  - `join` 名は `compose` に置換

