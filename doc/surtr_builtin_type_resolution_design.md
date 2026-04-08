# Surtr: 標準型の宣言収集と組み込み意味付けの分離

2026-04-08 注記:

- 現行 surface syntax は `@builtin(...)` ではなく `@@builtin` を使う
- 今後の parser 追加対象は `@@builtin` 単独行の次に `type` 宣言が続く形式
- 本メモの `type` 宣言は将来設計であり、現時点では pending テストで先置きする

## 目的

- 標準型 (`Int`, `String`, `List<$A>` など) を、可能な限りユーザ定義型と同じコンパイルフローで扱う。
- コンパイラ内部で型名を直接ハードコードして登録する構成を避ける。
- ただし、リテラル・演算子・特殊構文・VM 命令選択に必要な組み込み意味論は、明示的にコンパイラへ結び付ける。

---

## 結論

`type` 宣言を先に収集し、その後に関数シグネチャを解決する **2段階以上の解決フロー** を取れば、
標準型の **名前登録** はソース由来にできる。

ただし、以下は依然としてコンパイラが知っている必要がある。

- 整数リテラルがどの型へ結び付くか
- 文字列リテラルがどの型へ結び付くか
- `[]` や list pattern がどの型へ結び付くか
- 条件式に使える型
- 演算子の特殊型付けや VM 命令選択

したがって、設計上は次の分離が必要になる。

### 分離方針

- **名前登録**: ソースから収集する
- **意味登録**: コンパイラが `TypeIdentity` に対して後付けする

---

## 期待できる効果

### 1. 標準型の名前をコンパイラが直接生やさなくてよい

たとえば `Int`, `String`, `List<$A>` をソース上の `type` 宣言から収集できる。

### 2. 標準モジュールも通常の名前解決フローに乗せられる

- `Kernel`
- `String`
- `List`
- その他の標準モジュール

これらが標準型を参照していても、型収集後にシグネチャ解決すれば処理できる。

### 3. ユーザ定義と標準定義のパイプラインを寄せられる

- ユーザ型も `type/struct/record/...` から収集
- 標準型も `type` から収集
- ユーザ関数も標準関数も同じシグネチャ解決器で扱う

### 4. 型名ではなく Identity ベースで組み込み意味を持たせられる

コンパイラは文字列 `"Int"` や `"String"` を直接特別扱いするのではなく、
解決済みの `TypeIdentity` に対して意味を紐付けられる。

---

## 基本原則

### 原則 1: すべての型名は宣言から生まれる

標準型であっても、コード上では宣言を持つ。

```surtr
@@builtin
type Int

@@builtin
type String

@@builtin
type List<$A>
```

### 原則 2: コンパイラが知るのは「名前」ではなく「意味カテゴリ」

コンパイラ内部で必要なのは、たとえば次のような意味カテゴリである。

- `BuiltinKind::Int`
- `BuiltinKind::String`
- `BuiltinKind::Bool`
- `BuiltinKind::List`
- `BuiltinKind::Float`

### 原則 3: 意味付け対象は `TypeIdentity`

型収集後、コンパイラは次のような対応を保持する。

- `TypeIdentity(Prelude::Int)` -> `BuiltinKind::Int`
- `TypeIdentity(Prelude::String)` -> `BuiltinKind::String`
- `TypeIdentity(Prelude::List)` -> `BuiltinKind::List`

この対応があることで、
名前文字列への依存を減らしつつ、特殊構文や VM 命令選択を実装できる。

---

## 推奨コンパイルフロー

## Phase 0: 標準ソース読み込み

- Prelude / Kernel / 標準モジュール群を通常ソースとして読み込む
- ユーザコードと同じ AST 形式に落とす

### この段階でやること

- ファイル読込
- パース
- モジュール境界の確定

---

## Phase 1: 宣言収集

型とモジュールの存在だけを先に集める。

### 収集対象

- `type`
- `struct`
- `record`
- `enum`（導入する場合）
- `defmod` ヘッダ
- `impl` ヘッダ
- `def` ヘッダ（名前だけでも可）

### 生成物

- `TypeIdentity`
- `ModuleIdentity`
- シンボル表
- 型引数個数（arity）

### 目的

後段のシグネチャ解決で、型名が未解決にならないようにする。

---

## Phase 2: 組み込み意味の紐付け

`@@builtin` のようなメタ情報を読み、`TypeIdentity` に対して組み込み意味を登録する。

### 例

```surtr
@@builtin
type Int

@@builtin
type String

@@builtin
type Bool

@@builtin
type List<$A>
```

### 生成物

- `TypeIdentity -> BuiltinKind` の対応表

### この段階の利点

- 型名の存在は宣言由来
- 意味付けだけをコンパイラが行う
- ユーザ定義と標準定義のフロー差分を局所化できる

---

## Phase 3: 関数シグネチャ解決

Phase 1 で収集した型表を用いて、関数やモジュール API の型注釈を解決する。

### 解決対象

- `def`
- `defmod`
- `impl`
- 引数型
- 戻り値型
- 必要なら制約や where 句

### 例

```surtr
defmod String {
  def len(value: String) -> Int
}


defmod List {
  def map(xs: List<$A>, f: ($A -> $B)) -> List<$B>
}
```

ここでは `String`, `Int`, `List<$A>` がすでに解決可能になっている。

---

## Phase 4: 本体解決

式・パターン・呼び出し・演算子を解決する。

### 解決対象

- 式
- パターン
- 変数参照
- 関数呼び出し
- モジュール参照
- 演算子解決

ここでは、組み込み意味が必要な箇所で `BuiltinKind` を参照する。

---

## Phase 5: 組み込み意味論の適用

解決済みの `TypeIdentity` と `BuiltinKind` を使って、特殊構文や専用命令を処理する。

### 例

- `1` を `BuiltinKind::Int` に結び付くリテラルとして扱う
- `"abc"` を `BuiltinKind::String` に結び付くリテラルとして扱う
- `[]`, `[a, b]`, `[head, ..tail]` を `BuiltinKind::List` に結び付ける
- `if` 条件に `BuiltinKind::Bool` を要求する
- `+` を `Int` / `Float` の専用演算へ落とす
- bytecode / VM 命令を選択する

---

## ハードコードを減らせる範囲

## 減らせるもの

- 標準型の名前登録
- 標準モジュールの型参照解決
- 標準関数のシグネチャ解決
- ユーザ定義と別経路の型収集処理

## 残るもの

- リテラルの意味付け
- 特殊構文の意味付け
- 演算子の専用型規則
- VM 命令との対応
- 一部の最適化規則

したがって、**完全にノーハードコードにはならないが、ハードコードの責務を「意味付け」に限定できる**。

---

## `@@builtin` 方式を推奨する理由

標準型の意味付け方法としては、属性または予約メタの導入が最も扱いやすい。

### 推奨形

```surtr
@@builtin
type Int

@@builtin
type Float

@@builtin
type String

@@builtin
type Bool

@@builtin
type List<$A>
```

### 利点

- ソース上に定義が存在する
- 名前文字列より `TypeIdentity` に寄せられる
- 標準ライブラリ側の構成変更にある程度強い
- ユーザ定義とほぼ同じ解決フローで通せる

### 注意点

- `@@builtin` 自体はコンパイラが理解する必要がある
- どの builtin kind が存在するかは仕様で固定される

ただし、この特別扱いは非常に小さい。

---

## 予約パス方式との比較

属性ではなく、特定のモジュールパスや定義位置で builtin を判断する方式もある。

### 例

- `Prelude::Int`
- `Prelude::String`
- `Prelude::List`

### 問題点

- パス名の変更に弱い
- 名前解決と意味付けがやや結び付きすぎる
- 標準ライブラリの再編に対してコンパイラ側の追従が必要になる

そのため、原則としては **属性方式のほうが安定** する。

---

## `List` への適用例

```surtr
@@builtin
type List<$A>


defmod List {
  def len(xs: List<$A>) -> Int
  def map(xs: List<$A>, f: ($A -> $B)) -> List<$B>
  def first(xs: List<$A>) -> $A?
}
```

### この設計で得られるもの

- `List` の型名はソースから収集できる
- `defmod List` は通常のシグネチャ解決で通せる
- `[]` や list pattern だけを builtin semantics で処理できる
- enum 表現や内部レイアウトをコード上へ露出しなくてよい

---

## ユーザ定義との整合

ユーザ定義型となるべく同じフローに寄せるという目的に対して、
この設計はかなり相性がよい。

### 共通化できる処理

- 宣言収集
- 型名解決
- モジュール参照解決
- 関数シグネチャ解決
- 名前空間管理

### 分離が必要な処理

- literal typing
- 特殊構文
- 特殊演算子
- VM 専用命令への lowering

つまり、**宣言と解決は共通化し、意味論だけを builtin 層へ分離する** のが最適である。

---

## 避けたい構成

以下の構成は避けたほうがよい。

### 非推奨

- コンパイラが `Int`, `String`, `List` という名前を直接生成する
- 標準型が AST / シンボル表にソース由来で存在しない
- 標準関数だけ別のシグネチャ解決器を通す
- 標準型とユーザ型で別の型解決ルートを持つ

これらは後から整合性が崩れやすい。

---

## 推奨設計の要約

### 推奨構成

1. 標準型もソース上で `type` 宣言する
2. 先に型収集して `TypeIdentity` を確定する
3. `@@builtin` により builtin 扱い対象を収集する
4. 標準モジュールも通常のシグネチャ解決を通す
5. 最後に builtin semantics を適用する

### 一文でまとめると

**標準型も宣言から生やし、コンパイラはその `TypeIdentity` に組み込み意味を後付けする。**

この構成にすると、ユーザ定義とほぼ同じフローに寄せつつ、必要な特殊扱いだけを局所化できる。

---

## 実装メモ

### 最低限必要な内部テーブル

- `TypeTable`
- `ModuleTable`
- `BuiltinTypeMap: TypeIdentity -> BuiltinKind`
- `FunctionSignatureTable`

### `BuiltinKind` の例

```text
Int
Float
String
Bool
List
```

### 将来的な拡張候補

- `Char`
- `Unit`
- `Result`
- `Error`
- `Pid`
- `Message` 系

ただし、`Result` や `Error` は単なる builtin type ではなく、
型規則や構築規則そのものが特殊になる可能性があるため、`List` や `Int` と同列かは別途検討が必要。

---

## 最終判断

- **型名登録のハードコードは避けられる**
- **意味付けのハードコードは最小限残る**
- **その境界を `TypeIdentity` ベースに置くのが重要**
- **ユーザ定義と同じフローへかなり寄せられる**

以上より、Surtr では
**「宣言収集 -> builtin 紐付け -> シグネチャ解決 -> 本体解決」**
の流れを基本方針とするのが妥当である。
