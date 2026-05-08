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
- Eldr `InteractiveVm`
- Eldr VM 内の process runtime 状態
- 行番号付きの結果履歴
- 補完候補シンボル集合

REPL セッションの BootPlan はセッション開始時に固定する。対話入力の chunk は
既存 VM / process runtime 状態へ増分適用されるが、session 中に boot 構成を変更しない。

### 3.2 初期化

- セッション開始時に標準 definition source を `Bootstrap -> [SpecialTypes, Kernel, Add, Sub, Mul, Eq, Neq, Compare, Lt, Lte, Gt, Gte, Concat, Numeric, Show, Ordering, Ord, From, TryFrom, Int, String, Regex, Boolean, Error, List, Generator, HashMap, Result, Option, Facet, Float, Config, Project, Random, IO, StyledDoc, Test]` の順で読み込む
- `Bootstrap` source は auto-import アンカーとして先頭に置き、標準 concrete error もここで登録する
- `SpecialTypes` source では `Unit`, `TypeRef<$T>`, `Hole`, `Closure`, `MatchArms<$Scrutinee, $Result>`, `CondClauses<$Result>` の canonical builtin type head を登録する
- `Kernel` source では `defmod Kernel` 配下の cross-cutting builtin を登録する
- 各 type file の top-level では対応する canonical builtin type head を登録する
- 現行実装の事前ロードファイルは `lib/bootstrap.srt` の後に、`lib/types/special_types.srt`, `lib/kernel.srt`, `lib/traits/operator/*.srt`, `lib/traits/*.srt`, type modules, `lib/facet.srt`, `lib/Config.srt`, `lib/Project.srt`, `lib/Random.srt`, `lib/IO.srt`, `lib/styled_doc.srt`, `lib/test.srt` を同一段として読み込む
- module stage の import 可視性は「前 stage + 同一 stage」とする。同一 stage 内の標準定義ソース / 通常 module は file 読み込み順に依存せず明示 import / auto import でき、later stage 参照は compile error とする
- loader は追加標準定義ソースも `./lib/**/*.srt` から収集し、`lib/tests/**` と built-in 標準定義ソースと重複するものはデフォルト入力から除外する
- REPL user chunk は標準定義ソース読み込み後に `SourceKind::ReplChunk` として追加される
- `surtr repl --module <file>` は追加の definition source を 1 件だけ preload し、`Std + 単品 definition` として成立する場合に限って受理する
- `surtr repl --script <file>` は追加の script source を 1 件だけ preload し、`include` を解決したうえで declaration area を compile し、top-level expr があれば REPL 開始前に一度だけ実行する
- script 引数による REPL 開始は `Std + include module + script` を同一 compile unit として compile し、script runtime input を `InteractiveVm::push_atomic()` 経由で実行してから通常 REPL に入る
- project runner 引数による REPL 開始は、Rune が解決した compile 対象 module stage を Xldr に渡し、Xldr が `Std + project module stages` をまとめて compile 済みの `InteractiveVm` 初期状態として構築してから通常 REPL に入る
- `surtr repl --script <file>` が将来 `supervisor_init` を含む場合、preload compile unit の BootPlan として取り込み、REPL 開始後の user chunk では boot 構成を変更しない
- `--module` と `--script` を併用した場合は `module -> script` の順で同一 compile unit として読む
- preload mode は CLI 入口の `--module` / `--script` 引数で確定し、Xldr 側で source token を読んで mode 推定しない
- `include` や `Project::add_path(...)` で追加される file は definition source として扱い、script と definition の判定を再度行わない
- preload 後の対話入力自体は引き続き `SourceKind::ReplChunk` として扱い、REPL 増分ポリシーは広げない
- preload script が導入した binding / function / doc metadata は、そのまま後続の REPL 対話入力から参照できる
- preload script に `defagent` / `defgenserver` / `defsupervisor` などの process 宣言が含まれる場合、REPL は declaration area から process module stage を抽出し、後続の対話入力でも concrete process surface と runtime metadata を継続参照できる
- REPL user chunk の top-level 宣言は `def` / `import` のみ許可し、`const`、型定義、`impl`、`defmod` は parse error とする
- REPL user chunk の top-level `def` は、セッション内の暗黙擬似モジュールに属する関数として扱う
- REPL user chunk の top-level value binding は同名再束縛を許可するが、既存 binding slot を再利用せず append-only に新 slot を割り当てる
- REPL user chunk の top-level `def` body は、同一セッションの top-level value binding を参照してはならない。参照可能なのは通常関数と同じく引数、関数内 local、visible function/import、標準定義だけとする
- したがって REPL は「module 外に関数がある」例外ではなく、明示 `defmod` を省略した module-like namespace 実行として扱う
- Eldr の `last_result` は REPL 表示・履歴・将来の command 用 property であり、通常の名前解決対象にはしない
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
- 実行失敗時の VM checkpoint / rollback には process runtime 状態、singleton slot、waiting table、deadline queue、標準 I/O handler buffer cursor を含める

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
- 標準 I/O は VM 内部 buffer 直書きではなく、標準 I/O handler 経由の出力として presenter が扱う
- REPL はアプリケーション側の DSL-visible 標準 I/O handler 設定を上書きしない。TTY 入力行の保護が必要な場合、`StdOut` / `StdErr` の host terminal backend だけを REPL UI adapter 管理の一時 buffer に流し、出力行を描画してから prompt と入力 buffer を復元する
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
| `:v <N>` | 行 `N` の結果を再表示する。binding value の再表示は別 surface として扱い、query command には混ぜない。 |
| `:doc <target>` | public declaration の `@doc` を引く。定義 doc、型 doc、constructor / extractor doc、impl doc、binding 起点 doc、process surface doc を表示する。binding lookup を明示する時は `$name` を使う。typed query は `gt(Int, Int)`, `gt($left, $right)`, `ret |>= up`, `Result<Int> |>= &parse_int`, `xs |> map(&to_string)` のような command query 専用 surface に限定する。`literal` / 任意式 / generic type variable は query 引数に受けない。callable binding が closure のときは `Closure` type doc を返し、続けて binding 付属の `@doc` 本文と最小限の補足情報（signature / captures / provenance）を表示する。process surface では hidden stdlib surface (`GenServer::spawn` など) と concrete public surface (`MyServer::spawn` など) の両方を引け、concrete query は hidden stdlib doc 本文を流用しつつ表示 symbol / signature だけ concrete 名に差し替える。private declaration は undocumented 扱いにせず、private surface であることを明示して拒否する。 |
| `:sig <target>` | public declaration の signature を表示する。関数、operator、constructor、extractor、enum 定義 surface、callable binding、impl specialization、process surface を表示対象に含む。bare `:sig Ty` は constructor signature、`Ty(args...)` は constructor 照合、`Ty!()` は extractor signature、`StringEncoding` のような enum は variant constructor surface 一覧を返す。enum variant 単体は query target にしない。typed query は concrete type、visible binding、`$binding`、`CaptureQuery` のみを引数に受ける。process surface では hidden stdlib 名と concrete public 名の両方を受け、表示名は query 側に揃える。private declaration は generic な not-found に落とさず、private surface であることを明示して拒否する。 |
| `:info <target>` | 定義、binding、dispatch、operator application query、singleton process owner、PID binding の解決情報を表示する。`$name` による binding 強制、typed call / typed operator の正規化結果、選択 impl、関連 command を出せることを契約に含める。process runtime lookup は singleton を owner 名、worker を PID binding で引く。PID binding の `:info` は raw inspect 表示や数値 PID を出さず、型と process metadata を返す。 |
| `:type <binding>` | REPL binding の型と `TypeIdentity` を表示する。`$name` による binding 強制を許可する。通常の値は binding のみを対象とし、定義名、typed query、任意式は受けない。process runtime lookup では singleton process owner 名を追加で受け、worker process は PID binding 経由のみを受ける。 |
| `:facet <facet-target>` | FacetPath 定義または `$facet_binding` の canonical path、segment 一覧、停止点を表示する。値 access 式や一般の callable / plain value は受けず、Facet query surface は command query 専用の制限された対象に限る。 |
| `:error [full|summary]` | エラー表示モードを切り替える（省略時は現在値表示） |
| `:save <path>` | 現在の REPL session を `.eldr` に保存する |

REPL command query は Surtr 式 parser ではなく、command query parser と semantic resolver の組で扱う。

- `:v <N>` は visible binding table を引き直さず、評価時に commit された値を履歴から再表示する。後続の同名再束縛は過去行の再表示結果を変えない

- 共通引数 surface は `ConcreteTypeKey | BindingKey | ForcedBindingKey | CaptureQuery` に限定する
- `ConcreteTypeKey` は `Int`, `Result<Int>`, `(Int -> String)` のような具象型のみを受け、`$T`, `List<$T>`, `impl Numeric` は受けない
- `ForcedBindingKey` は `$name` で表し、binding lookup を明示する
- `CaptureQuery` は `&to_string`, `&add(Int, &1)`, `&replace($from, &1, $to)` のような command query 専用 pattern とし、literal、任意式、placeholder 付き capture の再帰を禁止する
- operator query は `lhs OP rhs` (`|>`, `|*>`, `|>=`, `>>`, `>*`, `>=>`) を取り、RHS は実コードの引数注入規則に沿う限定 surface のみ許可する
- `_1` は pipe RHS の注入位置を示す query token であり closure 生成記法ではない
- `to_string()`, `to_string(_1)`, `1 + 2`, `pair._1` のような任意式 surface は command query としては受けない
- 多相関数の `:sig` は定義 signature を保持したまま、specialized 節で concrete type / binding 解決後の置換結果を表示する
- `:doc` / `:sig` は public declaration のみを query surface とし、private hit を認識できた場合は private-surface guidance を返す
- 具象 process の REPL 公開面は annotation 由来で決まり、annotation 付き関数だけが public surface になる。annotation なし関数は `defp` 相当として `:doc` / `:sig` / 補完対象に含めない
- process public surface の名前解決は通常関数と同じであり、visible な concrete 関数名は `import` により unqualified 参照できる
- compiler-managed hidden process surface (`pid`, `spawn`, common owner helper, hidden lower 名) は query / completion / import 対象に含めない
- `@call` / `@cast` / `@get` / `@set` などの annotation 名そのものは query target にしない。annotation により公開された concrete 関数名だけを query surface とする

### 5.2 予約済み

| コマンド | 説明 |
|---|---|
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
- `Bootstrap` / `Kernel` の明示 import や、user chunk での `@builtin` 利用禁止も通常の compile error と同じ診断経路で表示する
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
