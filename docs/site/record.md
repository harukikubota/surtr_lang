# Records

`defrecord` は、すべての field を公開する名前付きデータ型です。field 名で読むことも、宣言順の位置で扱うこともできます。Struct の `new` や `deconstruct` は必要ありません。

## 定義

```surtr
defrecord User(name: String, age: Int)
```

`defrecord` は `.srt` file に定義します。REPL で試す場合は `surtr repl --script file.srt` で preload してください。

field は1個以上必要です。`defrecord Empty()` と `defrecord Empty( )` はどちらもエラーです。Record field に `private` / `public` などの可視性指定は書けません。位置 path と競合する `_0`, `_1` なども field 名にできません。

## コンストラクト

`User(...)` にすべての field の値を渡します。位置指定は宣言順、名前指定は順不同です。

```surtr
ada = User("Ada", 37)
grace = User(age: 40, name: "Grace")
```

位置指定と名前指定の混在、field の省略・重複・未知の名前はエラーです。名前付き引数の `name: name` を `name` と略す書き方はありません。

## パターンマッチング

Record の `User(...)` は、コンパイラが提供する構造的な Pattern です。位置指定と名前指定のどちらでも、すべての field に子 Pattern を書きます。

```surtr
User(name, age) = ada
User(age: selected_age, name: selected_name) = ada

label = match ada {
  User(name: "Ada", age: _) => "Ada",
  User(_, _) => "other",
}
```

`User(name, age)` は宣言順の変数束縛です。名前指定は順不同ですが、子の照合と束縛は宣言順に行います。field の省略・重複・未知の名前、位置指定との混在はエラーです。

Record の外側の分解は必ず成功します。そのため、変数束縛や `_` だけの子 Pattern なら通常の `=` を使えます。literal や一般 Extractor など失敗し得る子を含む場合、Pattern 全体は partial となり、`=` には使えません。上の `match` のように照合結果を扱います。

Record head 内の `name: child` は名前付き field Pattern です。位置指定の子に型注釈を付ける場合は `User((name: String), age)` のように括ります。

## Record を引数に取る関数

Record は通常の引数型として使えます。field を読む関数は次のように書けます。

```surtr
def describe(user: User) -> String {
  user.name ++ " (" ++ to_string(user.age) ++ ")"
}

print(describe(ada))
# Ada (37)
```

## FacetPath: `.NAME` と `._N`

`User.name` は名前付き field path、`User._0` は宣言順の先頭 field を指す位置 path です。この例ではどちらも `name` field にアクセスします。値から直接読む `ada.name` と `ada._0` も同じ値です。

```surtr
name_by_field = Facet::view(User.name, ada)
name_by_position = Facet::view(User._0, ada)
age = Facet::view(User._1, ada)
direct_name = ada.name
direct_position = ada._0

older = Facet::put(User.age, ada, 38)
older_by_position = Facet::put(User._1, ada, 38)
```

位置 path は Record 型ごとに範囲を検査します。`User._2` はこの定義ではエラーです。`Tuple._0` は tuple 専用で Record に使えず、共通の `Record._0` root もありません。

実行時の field アクセスは同じですが、REPL の `:facet User._0` は入力時の `._0` を保持して位置アクセスとして表示します。`:facet User.name` は名前付き field path として表示します。

Facet の更新や path の合成は [Facet](./facet.md)、Pattern 全体の規則は [Pattern Matching](./pattern-matching.md)、Struct との違いは [Structs](./structs.md) を参照してください。
