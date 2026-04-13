# Surtr Lens 詳細設計

> 目的: Surtr に compiler-managed な `Lens<$S, $A>` を導入し、構造アクセスと更新を
> `Result` ベースの直列合成モデルとして定義する。
> 本書は今回の大規模改修に向けた詳細設計であり、`doc/要件定義v9.md` には概要のみを載せる。

最終更新日: 2026-04-13

---

## 1. スコープ

今回の改修で入れるもの:

- `@@builtin type Lens<$S, $A>`
- `User.name` / `Config.port` のような型ルート lens path
- `Expr.add` のような enum variant selector lens
- tuple lens path
- `user.name` のような value-root access sugar
- `Lens::view / set / over / join`
- lens view の `Result` 統一
- `VariantMissMatch` を使う enum / ADT path failure
- `Result` bind へ接続しやすい直列合成

今回の改修で入れないもの:

- 並列分岐や branch 探索
- bulk update DSL
- lens capability の厳密な持ち出し制御
- `Result` をまたぐ自動 path lift
- variant payload の named field 化

---

## 2. 設計原則

### 2.1 Lens は compiler-managed

Surtr の lens は user-defined optic ではなく、compiler が型定義から導出する path capability である。

- `defstruct` / `defrecord` の field
- tuple 要素
- `defenum` の variant selector
- 上記の直列 `join`

### 2.2 Lens access は常に `Result`

view 系アクセスは pure / failure-aware を分けず、常に `Result` を返す。

- field path 成功時は `Ok(value)`
- enum variant mismatch 時は `Err(VariantMissMatch(...))`
- tuple index 不整合や path 不成立も `Err(...)`

これにより、呼び出し側は `=?`, `match`, `Result::map_err`, `|>=`, `|=>` で一貫して扱える。

### 2.3 エラー定義はコード上に置く

Lens 導入で必要になる失敗値は、runtime 専用の匿名エラーにせず code-level に置く。

今回の追加 error は次を想定する。

```rust
deferror VariantMissMatch(detail: String) { detail }
```

runtime は ADT path 操作の場面で `detail` を組み立ててよいが、error kind 自体は code-level に固定する。

### 2.4 ADT 操作と branch は分離する

今回の lens 機構は「構造 path を `Result` で返す」ことに集中する。

- branch の拡張は別 issue
- `VariantMissMatch` を握りつぶして次候補へ進む制御は、今回の core には入れない
- 呼び出し側が必要なら `map_err` や `match` で意味付けを変える

### 2.5 更新は `S -> Result<S>`

更新系 API の核は次に揃える。

- `Lens::set(lens, source, value) -> Result<$S>`
- `Lens::over(lens, source, f) -> Result<$S>`

bulk update DSL はこの核の上に将来追加する。

---

## 3. Surface Contract

### 3.1 公開型と公開 API

```rust
@@builtin type Lens<$S, $A>

defmod Lens {
  def view<$S, $A>(lens: Lens<$S, $A>, source: $S) -> Result<$A>
  def set<$S, $A>(lens: Lens<$S, $A>, source: $S, value: $A) -> Result<$S>
  def over<$S, $A>(
    lens: Lens<$S, $A>,
    source: $S,
    update_fun: ($A -> Result<$A>)
  ) -> Result<$S>
  def join<$S, $A, $B>(outer: Lens<$S, $A>, inner: Lens<$A, $B>) -> Lens<$S, $B>
}
```

`Lens` は opaque でよい。ユーザは field list や variant metadata を直接観測できない。

### 3.2 Path Surface

型ルート path:

```rust
User.name
Address.city
Expr.add
Expr.add._0
_0
_1
```

値ルート sugar:

```rust
user.name
expr.add
pair.0
pair.1
```

既存 constructor は維持する。

```rust
Expr::Add(left, right)
Light::Green
```

### 3.3 Segment の種類

- field segment
  - 例: `User.name`
- tuple segment
  - canonical path は `_0`, `_1`, ...
- variant segment
  - 例: `Expr.add`
  - constructor `Expr::Add` とは別物

### 3.4 Variant selector 名

constructor `Enum::Variant` から lens selector `Enum.variant` を導出する。

- surface constructor は既存どおり `::`
- lens selector は `.` の path segment
- selector 名は variant short name の lower-camel 変換を canonical とする

例:

- `Expr::Add` -> `Expr.add`
- `Light::Green` -> `Light.green`

### 3.5 value-root sugar の意味

`user.name` は direct field load ではなく、対応する lens path を `user` に適用した sugar である。

概念上は次と同値に扱う。

```rust
Lens::view(User.name, user)
```

ただし `user` の concrete type から root type を復元して sugar 展開するため、実装上の desugar は Scar で行う。

### 3.6 tuple access の互換

tuple 用の first-class path は `_0`, `_1`, ... とする。

一方で既存コード移行を容易にするため、今回のスコープでは value-root の `.0`, `.1`, ... を残してよい。

```rust
pair.0
```

は概念上

```rust
Lens::view(_0, pair)
```

と同値に扱う。

---

## 4. 型意味論

### 4.1 `Lens<$S, $A>`

`Lens<$S, $A>` は source type `$S` から focus type `$A` への path capability を表す。

- source type は path 全体の入力型
- focus type は成功時に観測・更新する対象型
- view は常に `Result<$A>`
- set / over は常に `Result<$S>`

### 4.2 enum variant selector の focus type

variant selector `Enum.variant` の focus は variant payload とする。

- payload arity = 0
  - focus type は `Unit`
- payload arity = 1
  - focus type はその単一 payload type
- payload arity >= 2
  - focus type は tuple `(T0, T1, ...)`

例:

```rust
defenum Expr {
  Int(Int),
  Add(Expr, Expr),
  Halt,
}
```

- `Expr.int : Lens<Expr, Int>`
- `Expr.add : Lens<Expr, (Expr, Expr)>`
- `Expr.halt : Lens<Expr, Unit>`

これにより、named payload 化を待たずに tuple lens と合流できる。

### 4.3 join の意味

`Lens::join(outer, inner)` は直列合成のみを扱う。

- `outer` が `Err(e)` を返したらそこで停止
- `outer` が `Ok(a)` を返したときだけ `inner` を適用
- 失敗はそのまま `Err(e)` として返す

### 4.4 `Result` 自動 lift は入れない

今回のスコープでは path 自身が `Result` をまたいで進む自動 lift は導入しない。

例:

```rust
user_result |>= User.name
```

のように、呼び出し側が `Result` bind してから次の path へ進む。

---

## 5. Error Contract

### 5.1 `VariantMissMatch`

variant selector が現在値に一致しない場合は `Err(VariantMissMatch(...))` を返す。

代表例:

- `Expr.add` を `Expr::Int(1)` に対して view する
- `Expr.add._0` を `Expr::Halt` に対して view する
- `Expr.halt` を `Expr::Add(...)` に対して set / over する

### 5.2 runtime message の責務

runtime は ADT 操作部分だけ詳細 message を補ってよい。

例えば detail には次のような文字列を入れてよい。

- `expected Expr.add, got Expr::Int`
- `expected Light.green, got Light::Red`

ただし lens 自体が独自の診断 API を持つ必要はない。

### 5.3 エラーの再解釈

利用者が `VariantMissMatch` を別の domain error として扱いたい場合は `Result::map_err` を使う。

```rust
Result::map_err(user_expr.add, SomeDomainError("..."))
```

---

## 6. 更新意味論

### 6.1 `Lens::set`

`Lens::set(lens, source, value)` は path の末端だけを置き換えた新しい `$S` を返す。

- success 時は `Ok(updated_source)`
- path 不成立時は `Err(...)`

### 6.2 `Lens::over`

`Lens::over(lens, source, update_fun)` は view 後に `update_fun` を適用して再構築する。

手順:

1. `lens` を `source` に view する
2. `Ok(focus)` なら `update_fun(focus)` を呼ぶ
3. `Ok(new_focus)` なら source を再構築する
4. 途中の失敗はそのまま返す

### 6.3 enum 更新

enum variant selector を含む path の更新は、match した variant の payload を取り出してから再構築する。

例:

```rust
Lens::set(Expr.add._0, expr, new_left)
```

は概念上次の処理になる。

```rust
match expr {
  Expr::Add(left, right) => Ok(Expr::Add(new_left, right)),
  _ => Err(VariantMissMatch(...)),
}
```

### 6.4 zero-arity variant の更新

`Lens::set(Light.green, value, ())` のような zero-arity variant update は、その variant への再構築として扱う。

この形式は一見不自然だが、variant selector の focus を `Unit` に揃えることで API を単純に保てる。

---

## 7. AST / Resolver / Typechecker

### 7.1 Spire

parser の基本方針:

- dotted syntax は既存どおり連鎖として parse する
- `_0`, `_1`, ... は identifier として読んでよい
- `Enum::Variant(...)` constructor syntax は維持する

parser 段では `Type.path` と `value.path` を完全には分けず、Sigil / Scar で意味づけする。

### 7.2 Sigil

Sigil では次を追加する。

- type-root dotted chain を `Resolved::LensRef` として解決する
- variant selector metadata を enum variant table と結びつける
- value-root dotted chain は既存 `FieldAccess` 系の sugar として保持する

必要な metadata:

- root source type
- segment list
- segment kind
- enum variant selector の constructor 対応

### 7.3 Scar

Scar では次を追加する。

- `Ty::Lens(Box<Ty>, Box<Ty>)`
- `TypedInner::LensConst`
- `TypedInner::LensView`
- `TypedInner::LensSet`
- `TypedInner::LensOver`
- `TypedInner::LensJoin`

型検査の役割:

- `User.name` に `Lens<User, String>` を与える
- `Expr.add` に `Lens<Expr, (Expr, Expr)>` を与える
- `user.name` を value-root sugar として `Result<String>` にする
- `Lens::join` の source / focus 整合性を検査する
- `Lens::set` / `Lens::over` の更新関数型を検査する

### 7.4 value-root sugar の型検査

`user.name` は先に `user` を型検査し、その concrete type から対応する root lens を引く。

例:

- `user: User` なら `user.name` -> `Lens::view(User.name, user)`
- `expr: Expr` なら `expr.add` -> `Lens::view(Expr.add, expr)`
- `pair: (Int, String)` なら `pair.0` -> `Lens::view(_0, pair)`

---

## 8. Lowering / Runtime

### 8.1 表現方針

今回の core では lens を runtime 上の opaque value として持つ。

理由:

- function 内ローカル束縛での再利用を許可しやすい
- `Lens::join` を通常の API として実装しやすい
- 将来の escape rule 強化と独立に core surface を固定しやすい

### 8.2 lens runtime value

runtime lens value は次の情報を持てばよい。

- source type marker
- focus type marker
- segment list

segment kind:

- `Field(name, index)`
- `TupleIndex(index)`
- `Variant(enum_name, variant_name, tag, payload_arity)`

### 8.3 `Lens::view`

runtime は segment を左から順に評価する。

- field / tuple は値を取り出す
- variant は現在値の tag を見て一致判定する
- variant mismatch なら `VariantMissMatch` を組み立てて失敗する

### 8.4 `Lens::set` / `Lens::over`

更新系は path の途中値をスタックに積み、末端更新後に逆順で再構築する。

- struct / record は field order で再構築
- tuple は tuple 全体を再構築
- enum は constructor payload を再構築して variant を再生成

### 8.5 compiler-special と builtin テーブル

surface 上は `@@builtin type Lens<$S, $A>` と `defmod Lens { ... }` を置く。

内部では次を compiler-special / builtin 扱いにする。

- `Lens`
- `Lens::view`
- `Lens::set`
- `Lens::over`
- `Lens::join`

---

## 9. 互換性と移行影響

### 9.1 破壊的変更

今回の導入は既存 field access 契約を壊す。

現在:

```rust
user.name : String
pair.0    : Int
```

導入後:

```rust
user.name : Result<String>
pair.0    : Result<Int>
```

既存の spec / compile_errors / integration / lib tests は広く更新が必要になる。

### 9.2 enum `.idx` は復活させない

enum に対する accessor は `Enum.variant` 側へ寄せる。

- discriminant accessor の再導入は行わない
- ADT の観測は constructor / match / lens selector に限定する

---

## 10. テスト方針

### 10.1 spec

追加する主な spec:

- struct / record lens view
- tuple lens view
- enum variant selector view
- `Lens::join`
- `Lens::set`
- `Lens::over`
- `=?` や `|>=` と組み合わせた直列利用

### 10.2 compile_errors

追加する主な失敗系:

- 無効な path segment
- tuple index out of bounds
- variant selector の型不整合
- `Lens::join` の source / focus mismatch
- `Lens::over` の更新関数型 mismatch

### 10.3 integration / unit

- Spire: dotted chain / tuple sugar parse
- Sigil: lens ref 解決
- Scar: `Ty::Lens`, value-root sugar, enum selector typing
- Forge / Eldr: view / set / over の runtime 再構築

---

## 11. 今回見送る課題

以下は今回の core から外し、個別 issue で追跡する。

- lens capability の持ち出し制御
  - ISSUE: `LENS-1`
- branch / 並列探索と `VariantMissMatch` 消費規則
  - ISSUE: `LENS-2`
- bulk update DSL
  - ISSUE: `LENS-3`

今回の core は「構造 path を `Result` で返す直列合成 API」に限定する。
