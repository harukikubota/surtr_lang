# Surtr Lens 仕様草案（Stage1コード整合 + Stage2確定）

最終更新日: 2026-04-13

---

## 0.5 実装状況（2026-04-13）

- 実装済み:
  - `view` / `compose` / `set` / `over`（Scar + Forge + Eldr）
  - Lens 非運搬モデル（arg / return / capture 禁止）
  - `Tuple._N` の type-root path（**型文脈あり**の場合）
  - variant mismatch の `VariantMismatch` 統一
- 未実装:
  - `return user.password` に対する warning（任意仕様）

---

## 0. 規範優先順位

本書は Lens 仕様の設計正本だが、フェーズごとに優先順位を固定する。

- Stage1（実装済み範囲）: 実装コードを正とする  
  判定順: `crates/**` / `lib/lens.srt` / `tests/**` > 本書
- Stage2（未実装範囲）: 本書を正とする  
  判定順: 本書 > 未実装コード断片・過去メモ

この優先順位により、Stage1 は「現行挙動の明文化」、Stage2 は「実装前提の確定仕様」として扱う。

---

## 1. 目的

Surtr の Lens は一般 optics ライブラリではなく、**compiler-managed な構造 path capability** として扱う。

本書の目的は次の 2 点。

- Stage1: 既存実装と一致した外部契約を固定する
- Stage2: `set/over` と **Lens非運搬（スコープ内消費）モデル** を確定する

---

## 2. Stage1（コード整合済み）仕様

### 2.1 Surface

```surtr
@@builtin type Lens<$S, $A>

@@autoimport
defmod Lens {
  @@builtin def view(lens: Lens<$S, $A>, source: $S) -> Result<$A>
  @@builtin def compose(outer: Lens<$S, $A>, inner: Lens<$A, $B>) -> Lens<$S, $B>
}
```

- 合成 API 名は `compose` のみを正とする
- enum selector は PascalCase 固定（例: `Expr.Add`）
- tuple access は `._0`, `._1`, ... 固定（`.0`, `.1` は不許可）
- variant mismatch エラー名は `VariantMismatch` で統一する

### 2.2 path の形

- `Type.segment` は compile-time Lens path を表す
- `value.segment` は path を value に即時適用する sugar として扱う
- 対象 segment は次の 3 種
  - struct/record field
  - tuple index (`_N`)
  - enum variant selector

### 2.3 `view` の返り型規則（文脈依存）

`Lens::view` と `value.path` は同じ規則で型付けする。

1. 初期文脈 `C`
   - source が `Result<T>` なら `C = Result`
   - それ以外は `C = Plain`
2. segment 遷移
   - field / tuple segment では `C` を維持する
   - variant segment を含んだ時点で `C = Result` へ昇格する
3. 最終返り型
   - `C = Plain` なら `A`
   - `C = Result` なら `Result<A>`

備考:

- `lib/lens.srt` の `view` 宣言は `Result<$A>` だが、Scar は上記規則で返り型を refine する

### 2.4 Stage1 制約

Stage1 はコード優先だが、運用方針として次を採用する。

- Lens は compile-time capability であり runtime 値として運搬しない
- Lens は **同一スコープ内で消費** する
- 関数境界を越える受け渡しは Lens ではなく、`view` 済みの `A` / `Result<A>` で行う
- standalone `_0` は未対応（`pair._0` のみ可）

### 2.5 Lowering / runtime 契約

- Lens path は runtime 値化しない（compile-time metadata として扱う）
- Forge は path metadata から `view` を直接 lowering する
- variant mismatch は `Err(VariantMismatch(...))` を構築する
- `view` / `compose` が runtime builtin として直接到達した場合は防御的 `RuntimeError` を返す

---

## 3. Stage2（仕様確定・実装進行中）契約

この章は仕様確定で、実装は段階的に反映済み。

### 3.1 Surface 拡張

```surtr
@@builtin type Lens<$S, $A>

@@autoimport
defmod Lens {
  @@builtin def view(lens: Lens<$S, $A>, source: $S) -> Result<$A>
  @@builtin def set(lens: Lens<$S, $A>, source: $S, value: $A) -> Result<$S>
  @@builtin def over(
    lens: Lens<$S, $A>,
    source: $S,
    update_fun: ($A -> Result<$A>)
  ) -> Result<$S>
  @@builtin def compose(outer: Lens<$S, $A>, inner: Lens<$A, $B>) -> Lens<$S, $B>
}
```

- 公開合成名は Stage2 でも `compose` のみ
- 互換別名は導入しない

### 3.2 `set/over` の意味

- `set`: focus を `value` に置換し、更新後 source を返す
- `over`: focus を取り出して `update_fun` を適用し、成功時に更新後 source を返す
- 返り型は常に `Result<S>` とする（文脈に関係なく統一）
- 失敗は次を含む
  - source が `Err(...)`
  - path 中の variant mismatch
  - `update_fun` が `Err(...)` を返す（`over` のみ）

### 3.3 Lens 非運搬（スコープ内消費）モデル

Stage2 では Lens を first-class 化しない。採用するのは次のモデル。

- Lens は compile-time capability としてのみ存在する
- Lens 自体をスコープ内外へ運搬しない（arg / return / export / container 格納を不許可）
- Lens は生成されたスコープで `view/set/over` により消費する
- 関数に渡すのは Lens ではなく `A` または `Result<A>` のみ

補足:

- Forge は Lens capability を静的 path へ展開して消去する
- runtime に Lens 実体は現れない
- lowering 漏れで runtime builtin 到達が起きた場合は防御的 `RuntimeError` とする

### 3.4 tuple index `_N` の扱い

Stage2 でも `_N` は path segment 専用とし、standalone root は導入しない。

- 許可: `pair._0` / `Tuple._0` のような `._N` 形式（`Tuple._N` は型文脈必須）
- 不許可: `_0` 単体
- 無文脈後方推論は採用しない

### 3.5 failure / diagnostic 契約

- variant mismatch は `VariantMismatch` で統一する
- `set/over` の失敗は `Result` 経由で伝播する
- 診断メッセージは「どの segment で失敗したか」を含める
- private capability の境界規則は厳格に維持する

---

## 4. 可視性と private capability

- field 可視性は対応 Lens capability の可視性として扱う
- private capability の参照許可範囲は `impl T` と `impl Trait for T`
- private capability の scope 外 escape は不許可
- 持ち出し検査は「Lens を消費するスコープ」で完結させる

### 4.1 判定例

- `user.name`: OK（Lens を消費して値 `A` を得る）
- `User.name`: OK（public field の capability 参照）
- `{|user| user.name}`: OK（public field）
- `return {|| user.name}`: OK（public field）
- `return {|| user.password}`: NG（private capability の持ち出し）
- `return user.password`: 仕様上は許可（値持ち出し）。必要に応じて warning を出してよい
- `return User.password`: NG（private capability の明示的持ち出し）

---

## 5. 互換性ポリシー

- Stage1 の既存挙動は変更しない
- Stage2 の追加は前方拡張として導入する
- 返り型契約は次で固定する
  - `view`: 文脈依存（plain / Result）
  - `set/over`: 常に `Result<S>`

---

## 6. Test Plan（文書整合 + Stage2受け入れ）

### 6.1 文書整合チェック

- 旧合成 API 記述（`compose` 以外）が残っていないこと
- `view` の文脈依存返り型規則と矛盾する記述が本文に残っていないこと
- すべてのサンプルが `compose` と `._N` ルールに一致すること
- Stage1 と Stage2 の節が混在していないこと

### 6.2 Stage2 実装時の必須受け入れシナリオ

- `set/over` 成功系
  - plain path 更新成功
  - variant path 更新成功
- `set/over` 失敗系
  - variant mismatch による `Err(VariantMismatch(...))`
  - `over` の `update_fun` 失敗伝播
  - source が `Err(...)` のときの短絡伝播
- Lens 非運搬モデル
  - Lens arg / return / export / container 格納が禁止されること
  - `view` 済み値（`A` / `Result<A>`）は関数受け渡し可能なこと
  - lowering 漏れ時の runtime 防御エラー
- tuple index `._N`
  - `pair._0` / `Tuple._0` 成功
  - `_0` 単体は失敗
- private capability
  - `return {|| user.password}` が拒否されること
  - `return User.password` が拒否されること
  - `return user.password` は許可（warning は任意）

---

## 7. 実装メモ（Stage2）

- Scar
  - Lens 非運搬モデルを前提に scope 境界で escape を検査する
  - `_N` は segment 専用（`._N` のみ）を維持する
- Forge
  - Lens capability を runtime 表現に変換せず compile-time 展開で消去する
  - `set/over` を path segment 単位で更新 lowering する
- Eldr
  - `view/set/over/compose` の runtime 直接呼び出しは防御的 `RuntimeError` を維持する

---

## 8. まとめ

Surtr Lens は compile-time path capability として運用し、Lens 自体はスコープ内で消費する。
関数境界で運ぶのは Lens ではなく `view` 後の値（`A` / `Result<A>`）とする。
`_N` は standalone では導入せず、`._N` 形式のみを維持する。
