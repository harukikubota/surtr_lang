# キャプチャ演算子 `&`

Surtr の `&` は、既存の関数や method を「あとで呼べる関数値」に変える演算子です。
このページでは bare capture と placeholder capture の両方をまとめます。

## 先に覚えるルール

- `&f` は named capture です
- `&Type::method` も capture できます
- 引数位置を調整したいときは `&1`, `&2`, ... を使います
- `&f(10)` のような旧 partial capture は使えません
- placeholder は outermost な capture の中だけで有効です
- `&(&1)` や `&(&1 + &2)` のような anonymous capture は使えません

## named capture

もっとも基本の形は `&name` です。

```surtr
def add1(x: Int) -> Int { x + 1 }

f = &add1
print(to_string(f(41)))
```

qualified path も同じです。

```surtr
trim = &String::trim
show = &User::get_name
```

これは「その関数そのものを値として取り出す」と読むと分かりやすいです。

## placeholder capture

引数の一部を固定したいときは placeholder capture を使います。

```surtr
def add(x: Int, y: Int) -> Int { x + y }
def sub(x: Int, y: Int) -> Int { x - y }

inc = &add(&1, 1)
dec = &sub(&1, 1)
flip_sub = &sub(&2, &1)
```

`&1`, `&2`, ... は「この capture が受け取る引数の何番目か」を表します。

```surtr
inc(41)       # => add(41, 1)
flip_sub(2, 7) # => sub(7, 2)
```

placeholder の規則は次です。

- index は `1` から始まります
- 最大 index が、その capture の引数個数になります
- index は欠番なく連続していなければなりません

たとえば次は OK です。

```surtr
&ensure(&1, &pred, err)
&pair(&1, &2)
&sub(&2, &1)
```

次は不許可です。

```surtr
&add(&2, 10)   # `&1` がない
&add(&1, &3)   # `&2` がない
```

## outer capture だけで使える

placeholder は outermost な capture にだけ属します。

```surtr
&outer(&1, &pred)
```

このとき `&pred` のような named capture は使えます。
ただし nested capture argument block の中へ placeholder を持ち込むことはできません。

```surtr
&outer(&1, &inner(10))  # compile error
```

この制約は「`&1` がどの capture に属するか」を明確に保つためです。

## 旧 partial capture は廃止

以前のような prefix partial application は使いません。

```surtr
&add(10)  # compile error
```

代わりに、placeholder で位置を明示します。

```surtr
&add(&1, 10)
&add(10, &1)
```

この形にそろえることで、「何番目の引数が後から入るか」が source 上で見えるようになります。

## anonymous capture は使えない

`&(...)` の形で式全体を直接 capture することはできません。

```surtr
&(&1)
&(&1 + &2)
&(print("Hello"))
```

identity がほしいだけなら named function を使います。

```surtr
&id
```

式を関数値にしたいなら、named helper を切り出すか closure を使います。

```surtr
{|x| x + 1}
{|text| print(text)}
```

## closure とどう使い分けるか

capture が向く場面:

- 既存関数をそのまま渡したい
- 引数位置だけを placeholder で調整したい
- module / type method を短く書きたい

closure が向く場面:

- その場で新しい処理を書きたい
- 外側のローカル変数を組み合わせたい
- 複数文のロジックを書きたい

たとえば次の 3 つは近い用途ですが、書き味が少し違います。

```surtr
users |*> &User::get_name
users |*> &format_name(&1, suffix)
users |*> {|user| format_name(user, suffix)}
```

## よくある不許可

```surtr
&1
&add(10)
&(&1 + &2)
&outer(&1, &inner(10))
```

理由は次です。

- `&1` 単体は capture の本体を持たない
- `&add(10)` は旧 partial capture だから不許可
- `&(...)` は anonymous capture だから不許可
- nested capture argument block の中では outer placeholder は見えない

## 例

```surtr
def add(x: Int, y: Int) -> Int { x + y }
def wrap(value: String, left: String, right: String) -> String {
  left ++ value ++ right
}

inc = &add(&1, 1)
bracket = &wrap(&1, "[", "]")

print(to_string(inc(41)))
print(bracket("name"))
```

## 関連ページ

- 関数コールと関数値の総論: `./callables.md`
- パイプ apply / map / bind: `./pipe-operators.md`
- 関数演算子の一覧: `./function-operators.md`
