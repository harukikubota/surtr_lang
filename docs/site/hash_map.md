# HashMap

`HashMap<V>` は String のキーに V 型の値を対応付ける immutable な map です。値の型は一つの map 内でそろえます。挿入や削除は新しい map を返します。

## 作る・読む・更新する

```surtr
scores = hash!["alice" => 10, "bob" => 20]
HashMap::map_get(scores, "alice") # Ok(10)
HashMap::map_get(scores, "carol") # Err(HashMapKeyMissing: hash map key not found: carol)
HashMap::map_contains_key(scores, "bob") # True
HashMap::map_len(scores) # 2
updated = HashMap::map_insert(scores, "alice", 15)
removed = HashMap::map_remove(updated, "bob")
HashMap::entries(removed) # [("alice", 15)]
```

literal のキーには String 型の式も使えます。同じキーを複数回指定すると、後の値を保存します。

```surtr
key = "alice"
map = hash![key => 10, key => 15]
HashMap::map_get(map, key) # Ok(15)
empty: HashMap<Int> = hash![]
HashMap::map_from_entries([("alice", 10), ("bob", 20)])
```

`map_remove` は欠落キーを指定しても元と同じ内容の map を返します。`map_get` の欠落 Error は、検索したキーを保存し、message に `hash map key not found: <key>` を含めます。

`HashMap::map_keys`、`HashMap::map_values_list`、`HashMap::entries` はキーの昇順で返します。`inspect` と `to_string` も同じ順で literal の形を表示します。`HashMap::map_values(map, callback)` はキーを保ち、キーの昇順に各値を一度変換します。callback が返す Result の Err も通常の値として保存します。

## 指定キーを Pattern で取り出す

式位置の `hash![key => value]` は map を作り、Pattern 位置の `hash![key => child]` は指定キーとその値を照合します。未指定のキーがあっても成功します。

```surtr
scores = hash!["alice" => 10, "bob" => 20]
match scores {
  hash!["alice" => points] when points > 0 => points,
  _ => 0,
} # 10
if_let(scores, hash!["bob" => points], points + 1, 0) # 21
is_match(scores, hash!["alice" => _]) # True
apply_pattern(scores, hash!["bob" => _1]) # Ok(20)
```

`"alice" => _` でもキーの存在は必要です。`apply_pattern` でキーが欠落すると `Err(HashMapKeyMissing(...))` を返します。入れ子では内側の欠落キーを報告し、値の Pattern が失敗したときはその子自身の Error を返します。

```surtr
map = hash!["user" => hash!["name" => "Ada"]]
apply_pattern(map, hash!["user" => hash!["name" => _1]]) # Ok("Ada")
```

キーには String 型の通常式、または外側の束縛を参照する pin を使えます。キー式は Pattern 開始前の scope を参照します。その Pattern が新たに束縛する値はキーに使えません。

```surtr
wanted = "alice"
is_match(scores, hash![^wanted => 10]) # True
is_match(scores, hash![wanted => 10])  # True
```

対象は一度評価し、キーを記述順に評価・検索して、取得した値を子 Pattern で照合します。最初の失敗で停止し、後続のキー式・子 Pattern・本文は実行しません。同じキーを二度書くと両方の子 Pattern を検査します。literal の構築時の上書きとは異なります。

## 空 Pattern と失敗の扱い

Pattern の `hash![]` は任意の HashMap に成功します。空 map だけを照合する Pattern ではありません。

```surtr
match scores {
  hash!["alice" => 0] => "zero",
  hash![] => "other",
}
hash![] = scores # 必ず成功する分解
HashMap::map_len(scores) == 0 # 空 map かを調べる
```

非空 Pattern はキーが欠落し得るため、通常の `=` には使えません。`match`、Pattern consumer、SafeBind `=?`、do の `<-` で成功と失敗を扱います。

```surtr
def alice_score(source: Result<HashMap<Int>>) -> Result<Int> {
  hash!["alice" => points] =? source
  Ok(points)
}
alice_score(Ok(scores)) # Ok(10)
```

SafeBind は RHS の canonical Result の外側一段だけを分解します。RHS が Err ならキー式を評価せず、その Error を失敗先へ渡します。Result で包まれていない HashMap にも非空の partial Pattern で SafeBind を使えます。空 Pattern の SafeBind は `Result<HashMap<V>>` に使えますが、HashMap を直接渡すと拒否されます。

map 内の Result 値は自動で分解しません。変数に束縛すれば Result 全体を受け取り、`Ok(child)` を書けば成功値を照合します。指定していないキーの Err は照合に影響しません。

```surtr
values: HashMap<Result<Int>> = hash!["a" => Ok(2), "extra" => Err(NoneError())]
apply_pattern(values, hash!["a" => _1])     # Ok(Ok(2))
apply_pattern(values, hash!["a" => Ok(_1)]) # Ok(2)
```

OR・alias・projection の使用範囲は [Pattern Matching](./pattern-matching.md) に従います。SafeBind と do の失敗先は [Error Handling](./error-handling.md) と [do](./do.md) を参照してください。map の値を更新する Facet は [Facet](./facet.md) で説明しています。

REPL の `:doc HashMap` と `:doc HashMap::map_get` でも標準 API を確認できます。API の正本は [`lib/types/hash_map.srt`](../../lib/types/hash_map.srt) の `@doc` です。
