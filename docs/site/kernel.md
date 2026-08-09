# Kernel

`Kernel` は auto import される最小の標準 API です。  
cross-cutting builtin と special form の説明は `../../lib/kernel.srt` が正本です。

## よく使うもの

- `print`
- `inspect`
- `if`
- `if_then`
- `assert`
- `ensure`
- `and`
- `or`
- `set_exit_code`

## `print`

```text
xldr(1)> print("hello")
hello
xldr(2)>
```

## `if`

```text
xldr(1)> print(match False { True => "T", _ => "F", })
F
xldr(2)>
```

`if` / `if_then` は surface では通常の call-style に見えますが、意味論としては lazily selected branch を持つ special form です。

## `ensure`

`ensure(value, pred, err)` は value を 1 回だけ評価し、predicate に通して成功なら `Ok(value)` を返します。

## `uncons`

`Kernel::uncons(term)` は builtin extractor です。

- `List` では `(head, tail)`
- `String` では `(head, tail)`

pattern position の `[head, ..tail]` はこの extractor alias です。

## `inspect`

debug-oriented な文字列表現が欲しいときは `inspect(...)` を使います。

```text
xldr(1)> pair = ("alice", 42)
> pair: (String, Int) = ("alice", 42)
xldr(2)> print(inspect(pair))
("alice", 42)
xldr(3)>
```

## `Function::always`

`Function::always(value)` は ignored-input callable を返します。

```text
xldr(1)> always = always(1)
> always: (_ -> Int)
xldr(2)> print(to_string(keep_one("ignored")))
1
xldr(3)>
```

ここで見えている `_` は、internal な `Hole` marker の surface 表記です。  
詳しくは `./special-types.md` を参照してください。

## 関連ページ

- Lazy 引数と括弧の評価順: `./lazy-evaluation.md`
- pattern 利用は `./pattern-matching.md`
- extractor 利用は `./extractors.md`
- 標準定義ソース全体は `./standard-modules.md`
- `TypeRef` / `Hole` / `Unit` は `./special-types.md`

## 確認したソース

- ソース
  - `../../lib/kernel.srt`

## 躓きやすいポイント

- `if`, `if_then`, `and`, `or` は call-style に見えても、評価規則は compiler が special-form 的に扱います。
- Lazy 引数位置の `(expr)` は eager boundary です。選択前に `expr` を一度評価するため、短絡を保ちたい branch / RHS に不要な括弧を付けないでください。
- `uncons` は通常関数呼び出しとしてではなく、主に `match` / `=?` 側の分解契約として読むと理解しやすいです。
