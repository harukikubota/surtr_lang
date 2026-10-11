# Surtr紹介ページ設計

## 目的

Surtrを、Rust／関数型言語に慣れた開発者が10分程度で把握できる、1ページのMarkdown紹介ページとしてまとめる。

読者には、Surtrを次のように位置づける。

> 静的型付けされたElixirの使い心地に、Rustの型安全性と、ScalaのExtractor的なパターン分解を加えた関数型言語。

この説明は厳密な系譜の主張ではなく、読者が既知の言語との対応関係を短時間でつかむための導入上の比喩として使う。

## 成功条件

- 1ページを上から流し読みして、10分程度で読み終えられる。
- コード例をNotebookのCellのように区切り、入力と出力を追いやすくする。
- Result、SafeBind、Facetをページの中心に置く。
- Resultを、成功／失敗を値として運ぶモナドコンテナとして説明する。
- `match`、`=?`、`|*>`、`|>=`、`|*|`、`>*`、`>=>`の関係をコードで示す。
- `examples/task_state_*` の状態遷移を使い、Resultの直列合成を実践例にする。
- Facetを、ネストした値やResult fieldを安全に読む・更新する仕組みとして紹介する。
- REPLとScriptの両方で、すぐにコードを試せることを示す。
- 静的型、構造体／enum、trait、Extractor、Processなどの主要機能を短く一通り見せる。
- JSON、File I/O、Shellは、現状のAPIを強く紹介できる状態ではないため扱わない。
- 記法はリポジトリ内で確認できる実装済みのものに限定する。

## ページ形式

成果物は `docs/site/surtr-introduction.md` とする。

各セクションは次のようなNotebook風の単位で記述する。

````markdown
### Cell 03 — SafeBind

説明文

```surtr
...
```

```text
出力または評価結果
```
````

図は特別な描画環境に依存しないよう、基本はMarkdownのコードブロックによるASCII図を使う。Mermaidは必須にしない。

## 全体構成

### 1. 導入

短いキャッチコピーと、Surtrの立ち位置を示す。

```text
Elixir       Rust              Scala Extractor
  │           │                       │
  └── actor   └── 静的型・安全性      └── パターン分解
                  │
                  ▼
                Surtr
```

続けて、コンパイラの大きな流れを示す。

```text
REPL / Script
      │
      ▼
Surtr source
      │
      ▼
Spire → Sigil → Scar → Forge → Eldr
      │
      ▼
typed bytecode / VM
```

### 2. Cell 01 — REPLとScript

まずREPLで式を評価する。続けて、同じ言語をScriptとして実行する。

```text
xldr(1)> 1 + 2
> 3
```

```surtr
def add(x: Int, y: Int) -> Int {
  x + y
}

print(to_string(add(20, 22)))
```

```bash
surtr run main.srt
```

ここでは、対話的な試行とファイル実行が同じ言語モデルであることだけを伝える。

### 3. Cell 02 — 静的型と式

`Int`、`String`、`Boolean`、`Unit`、関数の引数と戻り値、式として値を返す関数を短く示す。

```surtr
name = "surtr"
score: Int = 42
enabled = True

def greet(name: String) -> String {
  "hello #{name}"
}
```

Rustの型注釈に近い明示性と、式中心の関数本体を対比する。

### 4. Cell 03 — Resultの基本

ResultをSurtrの中心的なコンテナとして導入する。

```text
Result<T>
├── Ok(value)   ── success path
└── Err(error)  ── failure path
```

```surtr
def parse_port(text: String) -> Result<Int> {
  port: Int =? try_from::<Int>(text)

  if(
    port > 0,
    Ok(port),
    Err(InvalidPort(port)),
  )
}
```

例外を投げるのではなく、失敗も型付きの値として返すことを説明する。

### 5. Cell 04 — `match`とExtractor的な分解

`match`を成功／失敗の分岐手段として示す。

```surtr
def render_bool(text: String) -> String {
  match parse_bool(text) {
    Ok(flag) => if(flag, "yes", "no"),
    Err(NoneError) => "missing or invalid",
    Err(err) => inspect(err),
  }
}
```

ScalaのExtractorに近い読み味として、constructor・enum variant・`Err(...)`の形で値を分解し、必要なデータだけを束縛できる点を紹介する。

網羅性が必要であること、guardやExtractorがあっても取りこぼしはcompile errorになることは1段落で触れる。

### 6. Cell 05 — SafeBind `=?`

SafeBindをResultの早期伝播として説明する。

```surtr
def load_pair(a: String, b: String) -> Result<Int> {
  left: Int =? try_from::<Int>(a)
  right: Int =? try_from::<Int>(b)

  Int::safe_div(left + right, 2)
}
```

概念図を添える。

```text
try_from(a)
  ├─ Ok(left)  → try_from(b)
  │                ├─ Ok(right) → next step
  │                └─ Err       → return Err
  └─ Err       → return Err
```

`match`は回復・変換・分岐を明示したいとき、`=?`は失敗をそのまま呼び出し元へ運びたいとき、という使い分けを示す。

### 7. Cell 06 — Resultをモナドコンテナとして合成する

次の対応表で演算子を整理する。

| 抽象 | Surtr | 役割 |
|---|---|---|
| pure | `Ok(value)` / `return(value)` | 値をResultへ入れる |
| fmap | `result \|*> f` | 成功値だけを変換する |
| bind | `result \|>= f` | Resultを返す処理へ接続する |
| applicative | `result \|*| other` | Result内の関数と値を組み合わせる |
| lifted compose | `&f >* &g` | 後段の純粋関数をResultの内側へ接続する |
| Kleisli compose | `&f >=> &g` | Result返却関数を直列合成する |

代表例は `lib/tests/result.srt` の実例から選ぶ。

```surtr
def parse_int(text: String) -> Result<Int> {
  try_from::<Int>(text)
}

def require_small(value: Int) -> Result<Int> {
  if(value < 100, Ok(value), Err(NoneError))
}

pipeline = &parse_int >=> &require_small

print(inspect(pipeline("42")))
print(inspect(pipeline("142")))
```

```text
Ok(42)
Err(...)
```

ここで「Resultは単なるエラー戻り値ではなく、Functor／Applicative／Monadの接続規則を持つコンテナ」であることを明示する。

### 8. Cell 07 — Resultで状態遷移を表現する

`examples/task_state_types.srt` と `examples/task_state_machine.srt`を使い、状態遷移をResultで直列化する。

```surtr
draft = Task::start("write task-state sample")
open = Task::open(draft)
doing = open |>= Task::assign("haruca")
done = doing |>= Task::complete()
archived = done |>= Task::archive()
```

```text
Draft → Open → Doing → Done → Archived
```

不正な遷移は`Err(InvalidTaskTransition(...))`になり、後続の処理を実行しないことを示す。

### 9. Cell 08 — Facetで構造を安全に読む・更新する

Facetを、同一スコープ内のpath capabilityとして短く説明し、Resultとの接続に重点を置く。

```surtr
defstruct User {
  name: String,
  score: Int,
  nickname: Result<String, NoneError>,
}

name =? Facet::view(User.name, user)

updated =? Facet::over(
  User.score,
  user,
  {|score| Ok(score + 1)},
)
```

Result field全体の書き換えは`over_result`で行う。

```surtr
updated =? Facet::over_result(
  User.nickname,
  user,
  {|old| Ok(Ok("new-name"))},
)
```

```text
User
 ├─ name      ── view       ── String
 ├─ score     ── over       ── Result<User>
 └─ nickname  ── over_result ── Result<User>
                                │
                                └─ =? で次の処理へ接続
```

`/`によるnested path、`~source.path` shorthand、List／HashMap path、`bulk_update`は、一覧として軽く触れる。

### 10. Cell 09 — その他の言語機能

次の項目をそれぞれ1〜2文と短いコードで横断する。

- `defstruct` / `defenum` / `impl`
- closure、capture `&`、関数値
- `List`、`Tuple`、`Range`、`String`、`Float`
- traitと演算子dispatch
- `import`と`include`
- Extractor、sequence decomposition、exhaustiveness
- Process、`Task::async`、`Task::await`

この節では機能の存在と記法を示すだけにし、Result／SafeBind／Facetの説明を薄めない。

### 11. まとめと次の入口

最後に、読者の目的別に入口を置く。

```text
試す        → surtr repl
実行する    → surtr run example.srt
型で守る    → Result / SafeBind / match
合成する    → |*> / |>= / >* / >=>
構造化する  → defstruct / defenum / impl
更新する    → Facet
並行処理    → Process / Task
```

## ソースの選定方針

利用者向け説明は `docs/site/` を基礎にし、コード例は次の優先順位で抽出する。

1. `examples/guess.srt`：Script、Result、SafeBind、入力処理、エラー分岐
2. `examples/task_state_types.srt` / `examples/task_state_machine.srt`：enumとResultによる状態遷移
3. `lib/tests/result.srt`：Result演算子、Monad／Applicativeの接続、モナド則
4. `docs/site/facet.md` / `lib/tests/facet.srt`：Facetのread／set／over／over_result
5. `docs/site/pattern-matching.md`：match、exhaustiveness、Extractor的分解
6. `docs/site/language-guide.md` / `docs/site/language-features.md`：基本構文、module、import、include
7. `lib/traits/` と `lib/types/`：trait／Resultの宣言と`@doc`
8. `examples/process/` と `lib/tests/process.srt`：Processの短い紹介

JSON、File I/O、Shellはページ本文の対象外とする。

## 表現上の注意

- 「Elixir + Rust + Scala」は導入のための比較であり、互換性や同一設計を主張しない。
- Resultのエラー型は、一般的な`Error`値を保存するのではなく、具象errorを`Err(...)`へ入れて運ぶSurtrの規則に合わせる。
- `=?`はResult向けの糖衣構文ではなく、language-levelのSafeBindとして説明する。
- `Facet`はfirst-classな汎用lensと断定せず、path capabilityとResultを返す安全な構造操作として説明する。
- 未実装・弱いAPIを、機能一覧のためだけに水増ししない。
