# Records

## Recordとは

`defrecord` で定義する Record は、名前付きの field を持つデータ型です。Struct と違って `new` や `deconstruct` によるカプセル化はせず、Tuple のように全 field をそのまま公開します。field には名前でも宣言順の位置でもアクセスできる点が特徴で、**構造体（Struct）と Tuple の中間に位置するデータ型**だとイメージすると分かりやすいです。

## 定義

```surtr
defrecord User(name: String, age: Int)
```

- field は1個以上必要です（`defrecord Empty()` のような0個の定義はできません）。
- field に `private` / `public` などの可視性は指定できません。
- 位置 path と同じ名前になる `_0`, `_1` などは field 名として使えません。

## コンストラクト

`User(...)` の形ですべての field に値を渡します。位置指定なら宣言順、名前指定なら順不同で書けます。

```surtr
ada = User("Ada", 37)
grace = User(age: 40, name: "Grace")
age = 37
another_ada = User(name: "Ada", age)  # age: age の省略記法
```

`name: expr` のような名前指定が一つもなければ、すべて位置指定として宣言順に対応します。`User(name, age)` も位置指定です。名前指定が一つ以上あれば、裸の変数 `age` を `age: age` として使えます。値はその位置で見える同名の変数から取ります。入れ子の構築にある名前指定は、外側の分類には影響しません。

名前指定の中では、数値や演算などの式には field 名が必要です。たとえば `User(name: "Ada", 37)` や `User(name: "Ada", age + 1)` は使えず、`age: 37` や `age: age + 1` と書きます。field の省略・重複・存在しない名前、未束縛の変数、型の不一致もエラーになります。値の型から field 名を推測することはありません。記述順にかかわらず、値は field の宣言順に評価・配置されます。

constructor を関数値として使う場合は capture します。

```surtr
make_user: (String, Int -> User) = &User
make_twenty: (String -> User) = &User(&1, 20)
```

capture の引数は位置指定だけで、固定値を含む引数ブロックには placeholder が必要です。
named argument や `&User("Ada", 20)` は拒否されます。詳細は
[`capture-operator.md`](./capture-operator.md) を参照してください。

## パターンマッチング

`User(...)` はコンパイラが提供する構造的な Pattern としても使えます。位置指定・名前指定のどちらでも、すべての field に対応する子 Pattern を書きます。

```surtr
User(name, age) = ada
User(age: selected_age, name: selected_name) = ada
User(name: selected_name, age) = ada  # age: age の省略記法

label = match ada {
  User(name: "Ada", age: _) => "Ada",
  User(_, _) => "other",
}
```

名前指定が一つもなければ位置指定です。`User(name, age)` は宣言順に変数を束縛します。名前指定が一つ以上あれば、裸の束縛名 `age` を `age: age` として使えます。この `age` は既存変数の参照ではなく、新しい束縛です。名前指定の記述順は自由ですが、照合・束縛は宣言順で行われます。入れ子の Pattern にある名前指定は、外側の分類には影響しません。

名前指定の中では、literal、`_`、型注釈付き Pattern、入れ子の Pattern、Extractor などには field 名が必要です。`User(name: selected_name, _)` は使えず、`User(name: selected_name, age: _)` と書きます。field の省略・重複・存在しない名前、子 Pattern の型不一致、束縛名の重複はいずれもエラーです。

Record の分解は必ず成功するため、子 Pattern が変数束縛や `_` だけであれば通常の `=` を使えます。一方、literal や一般的な Extractor など失敗しうる子 Pattern を含む場合、Pattern 全体が partial になるため `=` は使えず、上の `match` のように照合結果を扱う必要があります。

`name: child` は名前付き field Pattern です。位置指定の子に型注釈を付けたい場合は `User((name: String), age)` のように括弧で囲みます。

## Record を引数に取る関数

Record は通常の型と同じように関数の引数として使えます。

```surtr
def describe(user: User) -> String {
  user.name ++ " (" ++ to_string(user.age) ++ ")"
}

print(describe(ada))
# Ada (37)
```

## FacetPath で値を読み書きする

Facet は、コンパイラが管理する Lens 操作で、値の一部を読み書きするための仕組みです。Record の field には FacetPath でアクセスでき、名前で指定する方法（`.NAME`）と、宣言順の位置で指定する方法（`._N`）の2通りがあります。

```surtr
name_by_field = Facet::view(User.name, ada)
name_by_position = Facet::view(User._0, ada)
age = Facet::view(User._1, ada)

direct_name = ada.name
direct_position = ada._0

older = Facet::put(User.age, ada, 38)
older_by_position = Facet::put(User._1, ada, 38)
```

`User.name`（名前 path）と `User._0`（位置 path）はどちらも同じ `name` field を指します。値から直接読む `ada.name` と `ada._0` も同様です。

位置 path の範囲は Record 型ごとにチェックされるため、`User._2` のように存在しない field を指すとエラーになります。また `Tuple._0` は Tuple 専用の FacetPath であり Record には使えません（Record と Tuple に共通する `Record._0` のような root もありません）。

実行時の field アクセスの挙動はどちらの書き方でも同じですが、REPL の `:facet` コマンドでは入力した形式がそのまま表示されます。`:facet User._0` と入力すれば位置 path として、`:facet User.name` と入力すれば名前 path として表示されます。

## 関連ドキュメント

- Facet の更新や path の合成について: [Facet](./facet.md)
- Pattern 全体の規則: [Pattern Matching](./pattern-matching.md)
- Struct との違い: [Structs](./structs.md)
