# Xldr 仕様書（V9）

> Xldr は Surtr の対話実行層である。
> 本書は REPL と将来のステッパー機能の外部契約を定義する。

---

## 1. 目的と責務

Xldr は以下を担う。

- `surtr repl` の実行本体
- REPL セッション状態の保持
- 入力ごとの増分コンパイルと増分実行
- `ReplEngine` が返す structured result の組み立て
- 対話モード向けの presenter と UI adapter
- 対話モード向けの表示と診断

Xldr は以下を担わない。

- CLI 引数解析
- `run`, `build`, `dump` のバッチ処理
- パーサ、名前解決器、型検査器、VM 自体の実装

---

## 2. Rune との境界

| 責務 | rune | xldr |
|---|---|---|
| CLI 引数解析 | ○ | × |
| `surtr run/build/dump` | ○ | × |
| `surtr repl` のディスパッチ | ○ | × |
| REPL ループ | × | ○ |
| REPL セッション状態 | × | ○ |
| REPL コマンド解決 | × | ○ |
| REPL result presenter | × | ○ |
| CLI/TUI adapter | × | ○ |
| 将来の TUI ステッパー | × | ○ |

---

## 3. セッションモデル

### 3.1 保持する状態

Xldr は対話セッション中に次を保持する。

- Sigil セッション
- Scar セッション
- Forge セッション
- Eldr VM
- 行番号付きの結果履歴
- 補完候補シンボル集合

### 3.2 初期化

- セッション開始時に標準 module source を `Bootstrap -> [SpecialTypes, Kernel, Add, Sub, Mul, Eq, Neq, Compare, Lt, Lte, Gt, Gte, Concat, Numeric, Show, Ordering, Ord, From, TryFrom, Int, String, Regex, Boolean, Error, List, Generator, HashMap, Result, Option, Lens, Float, Config, Project, Random, IO, StyledDoc] -> [Test]` の順で読み込む
- `Bootstrap` source は auto-import アンカーとして先頭に置き、標準 concrete error もここで登録する
- `SpecialTypes` source では `Unit`, `TypeRef<$T>`, `Hole`, `Closure`, `MatchArms<$Scrutinee, $Result>`, `CondClauses<$Result>` の canonical builtin type head を登録する
- `Kernel` source では `defmod Kernel` 配下の cross-cutting builtin を登録する
- 各 type file の top-level では対応する canonical builtin type head を登録する
- 現行実装の事前ロードファイルは `lib/bootstrap.srt` の後に、`lib/types/special_types.srt`, `lib/kernel.srt`, `lib/traits/operator/*.srt`, `lib/traits/*.srt`, type modules, `lib/lens.srt`, `lib/Config.srt`, `lib/Project.srt`, `lib/Random.srt`, `lib/IO.srt`, `lib/styled_doc.srt` を同一段として読み込み、`lib/test.srt` を次段として読み込む
- module stage の import 可視性は「前 stage + 同一 stage」とする。同一 stage 内の標準 module / 通常 module は file 読み込み順に依存せず明示 import / auto import でき、later stage 参照は compile error とする
- loader は追加標準 module も `./lib/**/*.srt` から収集し、`lib/tests/**` と built-in 標準 module と重複するものはデフォルト入力から除外する
- REPL user chunk は標準 module 読み込み後に `SourceKind::ReplChunk` として追加される
- REPL user chunk の top-level 宣言は `def` / `import` のみ許可し、型定義・`impl`・`defmod` は parse error とする
- REPL user chunk の top-level `def` は、セッション内の暗黙擬似モジュールに属する関数として扱う
- したがって REPL は「module 外に関数がある」例外ではなく、明示 `defmod` を省略した module-like namespace 実行として扱う
- 初期補完候補には `Ok`, `Err` と builtin 名を含める
- セッションは `.eldr` と live compile の両方から doc metadata を保持し、`:doc` 表示へ利用する
- `.eldr` から初期化した場合、標準 library の compile-time context は source から復元する
- `.eldr` に含まれる user-defined function は VM には常駐するが、新しい REPL 入力の名前解決対象としては復元されない
- したがって `.eldr` 復元は現時点では部分復元であり、完全な semantic restore は後続課題とする

`Bootstrap` / `Kernel` は REPL でも auto import 対象とし、明示 `import` は compile error とする。

### 3.3 失敗時の扱い

- 入力ごとに名前解決、型検査、コード生成の checkpoint を取る
- 途中で失敗した場合は、その入力ぶんの変更をロールバックする
- 失敗した入力は後続セッションへ持ち越さない

---

## 4. REPL 入出力契約

REPL 実装は次の 3 層に分ける。

- core: `ReplEngine` が入力処理、checkpoint/rollback、command 解決、doc/sig/save を担う
- presenter: `ReplResult` を CLI/TUI が消費しやすい表示単位へ変換する
- UI adapter: CLI/TUI が terminal I/O と color on/off、pane state を担当する

### 4.1 入力源

- TTY 対話入力
- pipe / redirect による標準入力

### 4.2 プロンプト

- 通常入力: `xldr(N)> `
- 継続入力: `...(N)> `

`N` は評価行番号であり、空行や継続待機だけでは進まない。

### 4.3 複数行入力

- `ParseError::Incomplete` を受けた場合、入力は pending バッファに保持する
- 次行以降を連結し、再度パースする

### 4.4 出力規約

- `print(...)` の副作用出力はプレフィクスなしで表示する
- 評価結果は `> ` プレフィクス付きで表示する
- バインド結果は `> name: Type = value` 形式で表示する
- 型定義評価は `> TypeName` 形式で表示する
- 表示対象のない `Unit` は表示しない
- `:doc` / `:sig` は evaluator result と同じ `> ` プレフィクスを付けず、presenter が専用レイアウトで表示する
- compile error / runtime diagnostic の人間向け表示は stderr に流し、structured result 側には UI テスト用の rendered lines を保持する

---

## 5. REPL コマンド

### 5.1 実装済み

| コマンド | 説明 |
|---|---|
| `:help`, `:h [command]` | REPL コマンド一覧、または指定コマンドのヘルプを表示する |
| `:quit`, `:exit` | REPL を終了する |
| `:v <N>` | 行 `N` の結果を再表示する |
| `:doc <symbol|query>` | 現在セッションで見える symbol の doc を表示する。名前だけなら定義 doc を返し、typed query (`gt(Int, Int)`, `ret |>= up`, `num |> (Int -> String)`) を与えたときは impl / 特化先 doc を返す。operator alias (`+`, `|>` など) は trait 定義 surface へ正規化する。`=` / `=?` は trait ではなく binding form として synthetic な Bind / SafeBind doc を返す。helper 名は current scope に従って解決し、auto-import helper の bare 名は trait 定義 surface を返してよいが、auto-import されない helper は qualified path か明示 import 後の short name でしか見えない。callable binding を与えた場合、capture は capture 元の doc を返し、匿名 closure は `Closure` type doc を返したうえで binding ごとの signature / captures / provenance を補足表示する。` :doc Ty` は type-bound doc を返す規約を維持し、`Closure` もこの経路で表示できる。`Tuple` は doc 専用 surface とし、tuple shape、`Tuple._N` LensPath、`t._1` の値アクセス説明をまとめて返す。symbol が現在セッションで見えていても doc entry が無い場合は not found ではなく undocumented guidance を返す。bare symbol は current scope に従って解決し、auto-import されない helper は qualified path か明示 import 後のみ有効とする。 |
| `:sig <symbol|query|expr>` | 現在セッションで見える関数の signature を表示する。名前だけなら定義 signature を返し、typed query を与えたときは specialized 表示を返す。trait / operator は「型なしなら定義、型ありなら実装」とし、`gt(Int, Int)`, `ret |>= up`, `:sig |>`, `:sig id(1)` のような query を受け付ける。operator alias は trait surface へ正規化し、`=` / `=?` は binding form surface (`pattern = expr`, `pattern =? expr`) を返す。helper 名は current scope に従って解決する。auto-import されない helper は qualified path か明示 import 後のみ bare 名で有効とする。`Tuple` は doc 専用 surface なので `:sig Tuple` は無効とする。`pair._1` のような field access sugar は `Lens::view` へ展開した expression query として扱い、他の `Lens::view` / `Lens::set` / `Lens::over` / `Lens::compose` は通常の関数 query と同じ defined / specialized 表示を返す。 |
| `:error [full|summary]` | エラー表示モードを切り替える（省略時は現在値表示） |

REPL query grammar は `symbol | typed-call | typed-operator | expr` の共有 parser で扱う。

- `typed-call`: `callee(arg1, arg2, ...)`
- `typed-operator`: `lhs OP rhs` (`|>`, `|*>`, `|>=`, `>>`, `>*`, `>=>`)
- query operand は既存 binding、literal、`_ : Type`、型式 (`Int`, `Result<Int>`, `(Int -> String)`) を受け付ける
- 多相関数の `:sig` は定義 signature を保持したまま、specialized 節で型変数を置換した結果を表示する
- `:sig pair._1` は `Lens::view(Tuple._1, pair)` 相当の sugar 展開として表示してよい

### 5.2 予約済み

| コマンド | 説明 |
|---|---|
| `:type <binding>` | 現在見えている binding の型と TypeIdentity を表示する |
| `:env` | 現在の束縛一覧を表示する |
| `:reset` | セッションを初期化する |
| `:step [expr]` | ステッパーを起動する |

### 5.3 不明コマンド

- `:` で始まる未定義コマンドはエラーとして表示する
- 未定義コマンドの表示には `:help` の入力案内を含める
- セッションは継続する

---

## 6. 補完と履歴

- TTY モードでは行編集、履歴、補完を提供する
- 補完対象は REPL コマンドと現在スコープで見えるシンボルである
- 補完候補は入力済み接頭辞に基づいて抽出する

候補一覧表示の改善や型文脈つき補完は `doc/open-issues.md` の将来課題として扱う。

---

## 7. 診断表示

- 対話モードでは ariadne ベースの人間向け診断を標準とする
- 型エラーでは、可能な範囲で関数宣言や `if` / `match` の分岐位置に補助ラベルを付ける
- REPL 診断は入力継続よりも「その入力単位で失敗してロールバックする」ことを優先する
- `Bootstrap` / `Kernel` の明示 import や、user chunk での `@@builtin` 利用禁止も通常の compile error と同じ診断経路で表示する
- `:error summary` では診断の 1 行目のみ表示し、`:error full` では source snippet を含む詳細を表示する
- REPL 診断の span / line / column は compiler 正本と同じく character offset 契約に従い、表示直前にだけ byte range へ変換する

---

## 8. ステッパー（将来）

### 8.1 目的

- Bytecode 実行過程を 1 命令ずつ確認できるようにする
- REPL セッションとは独立した観察用実行系として動作させる

### 8.2 想定 UI

- Source
- Bytecode
- Stack
- Locals

### 8.3 必要基盤

| 基盤 | 役割 |
|---|---|
| `SourceMap` | opcode とソース位置の対応 |
| `VM::step()` | 1 命令ずつの実行 |
| `VMSnapshot` | VM 状態の読み出し |
| `StepperHistory` | 表示の巻き戻し |

ステッパーは REPL の本体 VM へ副作用を反映しない。

---

*Surtr — 既存の妥協を、型で焼き払う。*
