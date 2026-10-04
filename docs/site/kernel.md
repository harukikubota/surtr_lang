# Kernel

`Kernel` は auto import される最小の標準 API です。  
cross-cutting builtin と special form の説明は `../../lib/kernel.srt` が正本です。

## よく使うもの

- `print`
- `inspect`
- `if`
- `if_then`
- `require`
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
List と String は適用時に別々の concrete 静的契約として検査されます。runtime primitive は共有しますが、裸の generic target、Union、runtime assertion を surface 型として公開しません。

## `inspect`

値の内容を確認する文字列表現が欲しいときは `inspect(...)` を使います。Stringは引用符で囲み、
制御文字と文字としての補間開始をエスケープして表示します。入れ子のStringとHashMapキーにも同じ規則を使います。

```text
xldr(1)> pair = ("alice", 42)
> pair: (String, Int) = ("alice", 42)
xldr(2)> print(inspect(pair))
("alice", 42)
xldr(3)>
```

```surtr
print(inspect("\u{1b}" ++ "a"))
print(inspect("\#{name}"))
```

```text
"\u{1b}a"
"\#{name}"
```

LFとタブは `\n` と `\t`、その他のC0・DEL・C1制御文字は `\u{...}` にします。
Unicodeエスケープの表示は小文字・不要な先頭ゼロなしです。
String単体の引用表示は、通常の文字列式として再入力すると元の値になります。
ただし、構造体の構築経路や Error の表示は文字列リテラルと異なるため、`inspect` 全体を再入力できることは保証しません。

構造体は private フィールドも含め、全フィールドの名前と値を定義順に表示します。List、Tuple、Result などに入った構造体も同じ規則に従います。呼び出し側のスコープや `Show` の有無によって表示を変えず、`Show` を自動で呼び出しません。`private` は情報の秘匿を保証せず、`inspect` の戻り値はフィールドへのアクセスや更新権限を与えません。

`to_string` は対象型の `Show` 実装に従い、`Show` がない型は型検査で拒否します。`inspect` による暗黙の代用はありません。手書きの `Show` で明示的に `inspect` を呼ぶことはできます。
`to_string(String)` の生文字列と `print` による文字列の直接出力には、この引用処理を加えません。
`eprint(String)` は同じ引用表示を標準エラー出力へ書き出します。
詳しい規則は[文字列の入力と表示](./strings.md)を参照してください。

## `set_exit_code`

`set_exit_code(code)` は現在の実行に process exit code を設定します。script source では直接使えます。
project compile では設定された entrypoint 内だけで使え、definition check と REPL chunk では拒否されます。

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
- `Hole` / `Unit` は `./special-types.md`

## 確認したソース

- ソース
  - `../../lib/kernel.srt`

## 躓きやすいポイント

- `if`, `if_then`, `and`, `or` は call-style に見えても、評価規則は compiler が special-form 的に扱います。
- Lazy 引数位置の `(expr)` は eager boundary です。選択前に `expr` を一度評価するため、短絡を保ちたい branch / RHS に不要な括弧を付けないでください。
- `uncons` は通常関数呼び出しとしてではなく、主に `match` / `=?` 側の分解契約として読むと理解しやすいです。
