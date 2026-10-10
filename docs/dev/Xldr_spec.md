# Xldr 仕様書

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

Xldr は少なくとも次の session phase を区別する。

- `Bootstrap`: 標準定義ソースを compile して `InteractiveVm` 初期状態を組み立てる段階
- `Preload`: `--module` / `--script` / project runner 由来の compile 結果を live REPL 前に適用する段階
- `Live`: 通常の対話入力を `SourceKind::ReplChunk` として増分 compile / execute する段階

phase ごとの VM 実行ポリシーは Xldr が決め、Eldr へは `InteractiveChunkPolicy` として渡す。`Bootstrap` と `Preload` は `Preload` policy、`Live` は `ReplAppendOnly` policy を使う。

Xldr の compile-time prefix は `StagedCompilationSnapshot` / `CompilationPrefixSnapshot` として扱う。
これは標準定義、preload module/script/project stage、REPL live chunk の compile metadata を束ねる
aggregate であり、Eldr の runtime append policy とは別責務である。REPL command / completion は
この snapshot から得た `SymbolSemanticInfo` を query し、表示候補へ投影する。

### 3.2 初期化

- セッション開始時に標準 definition source を `Bootstrap` stage、その後の shared standard stage の順で読み込む。test extension は variant が有効な場合に shared standard stage へ追加する
- 標準モジュールの完全な inventory、順序、stage 分割は [`STDLIB_MODULE_SPECS`](../../crates/sindr/src/stdlib.rs) を正本とし、本書へ重複列挙しない
- `Bootstrap` source は auto-import アンカーとして先頭に置く。Error 定義は stage-0 で扱える固定・空 Payload の定義だけをここで登録する。String 補間・inspect・通常型を必要とする入力付き / Pattern / Facet Error は shared standard stage の `kernel.srt` 等に置く
- `SpecialTypes` source では `Unit`, `Hole`, `Closure`, `MatchArms<$Scrutinee, $Result>`, `CondClauses<$Result>`, `DoBlock<$Result>`, `BulkUpdateEntries<$State>`, `Lazy<$T>`, `ErrorKind`, `StandbyInit<$T>` の canonical builtin type head を登録する
- `Kernel` source では `defmod Kernel` 配下の cross-cutting builtin と、同ファイルのトップレベルにある入力付き / Pattern / Facet Error を登録する
- 各 type file の top-level では対応する canonical builtin type head を登録する
- 現行実装の事前ロードファイルは `lib/bootstrap.srt` の後に、`lib/types/special_types.srt`, `lib/function.srt`, `lib/kernel.srt`, `lib/traits/operator/*.srt`, `lib/traits/*.srt`, type modules, `lib/facet.srt`, `lib/Config.srt`, `lib/Project.srt`, `lib/Random.srt`, `lib/file.srt`, `lib/FileSystem.srt`, `lib/IO.srt`, `lib/Shell.srt`, `lib/styled_doc.srt`, `lib/test.srt` を同一段として読み込む
- module stage の import 可視性は「前 stage + 同一 stage」とする。同一 stage 内の標準定義ソース / 通常 module は file 読み込み順に依存せず明示 import / auto import でき、later stage 参照は compile error とする
- loader は追加標準定義ソースも `./lib/**/*.srt` から収集し、`lib/tests/**` と built-in 標準定義ソースと重複するものはデフォルト入力から除外する。symlinkは実体pathで探索範囲とtests除外を照合し、root外のソースを含めない。同じ実体は一度だけ収集し、祖先linkによる再訪は行わない
- 通常スクリプト用の標準ソース登録はvariantごとにプロセス内で一度収集する。各入口はその登録を複製し、include stageと入口を追加する。prefix内のSourceIdとstage順序を保ち、同じファイル名の本文・source kind・module pathが異なる登録は読み込みエラーとする。同じソースの再登録でもstage内のmodule重複を隠さない
- definition source の primary module path は parse 後 AST と namespace lowering 結果から導出し、loader / Xldr は token 走査で `defmod` head を推定しない
- qualified `defmod A::B` と `namespace A { defmod B { ... } }` は同じ canonical module path `A::B` として扱う
- 通常 module source 同士の同一 canonical module path は常に compile error とする。`impl` owner module は既存通常 module への拡張としてのみ同一 path を許可し、`normal A -> impl A -> normal A` のような通常 module 再定義は拒否する
- internal module path は `Global::Name` または `Namespace::Name` の canonical string を使うが、user-facing 表示では `Global::` を省略する
- 標準定義の semantic cache は、標準ソース、builtin metadata、semantic schema に加え、コンパイル時に確定した compiler source / build identity を鍵に含める。実装が変われば古い snapshot を拒否し、現在のコンパイラで再構築する
- Trait も所属 namespace の canonical identity を保持する。callable の表示では所属 path を保ち、暗黙の `Global::` を省略する。`Global::Function` は `Function`、`Global::Add` は `Add`、`Math::Add` は `Math::Add` と表示する
- `Result<T>` を正規表記とし、`Result<T, E>` は関数定義の直接の戻り値位置だけで許可する補助的な error contract 表記とする。値の推論型と callable 値の signature では `Result<T>` / `MatchResult<T>` と表示し、内部のエラー型を型引数として表示しない。束縛、型照会と入れ子の型にも適用する。`:sig` / `:doc` などの REPL コマンドによる宣言シグネチャの表示では、関数宣言の戻り値に明示した `Result<T, E>` と named `defextractor` の戻り値に明示した `MatchResult<T, Error>` を保持する。`ExtractorClosure` の型注釈では `MatchResult<T>` のみを受理する。定義で省略された Error 位置を補わない。値の型エラー診断と補完候補・シグネチャヘルプにも正規表記を使う。ExtractorClosure の値表示は束縛専用の特例を設けず、Eldr の共通 inspect 処理を使う。
- constructor capture は `Capture` origin、canonical constructor 名、capture site で具体化した signature を保持し、bare capture と partial capture、再 capture、nominal/container 内の表示で同じ metadata を使う
- `Ok` / `Err` / `True` / `False` は通常 Enum 宣言へ解決する。`:sig` / `:doc`、constructor capture の表示・定義参照は canonical 名を使う。chunk 追加・rollback・bytecode 復元後も既存 ID を保持し、新規 compiler symbol と callable は各 allocator の追加順で割り当てる。`Ok` / `Err` / `print` の固定 ID 枠は設けない。
- REPL user chunk は標準定義ソース読み込み後に `SourceKind::ReplChunk` として追加される
- `surtr repl --module <file>` は追加の definition source を 1 件だけ preload し、`Std + 単品 definition` として成立する場合に限って受理する
- `surtr repl --script <file>` は追加の script source を 1 件だけ preload し、`include` を解決したうえで declaration area を compile し、top-level expr があれば REPL 開始前に一度だけ実行する
- script 引数による REPL 開始は `Std + include module + script` を同一 compile unit として compile し、script runtime input を `InteractiveVm::push_chunk(..., Preload)` 経由で実行してから通常 REPL に入る
- project runner 引数による REPL 開始は、Rune が解決した compile 対象 module stage を Xldr に渡し、Xldr が `Std + project module stages` をまとめて compile 済みの `InteractiveVm` 初期状態として構築してから通常 REPL に入る
- `surtr repl --script <file>` が将来 `supervisor_init` を含む場合、preload compile unit の BootPlan として取り込み、REPL 開始後の user chunk では boot 構成を変更しない
- `--module` と `--script` を併用した場合は `module -> script` の順で同一 compile unit として読む
- preload mode は CLI 入口の `--module` / `--script` 引数で確定し、Xldr 側で source token を読んで mode 推定しない
- `include` や `Project::add_path(...)` で追加される file は definition source として扱い、script と definition の判定を再度行わない
- `surtr repl --module <file>` や `include` で入る definition source でも、module identity は file 名ではなく declared `defmod` の canonical path を使う
- preload 後の対話入力自体は引き続き `SourceKind::ReplChunk` として扱い、VM 実行 phase を `Live` へ切り替えたうえで append-only policy を適用する
- preload script が導入した binding / function / doc metadata は、そのまま後続の REPL 対話入力から参照できる
- preloadするファイルの `__FILE__`・`__DIR__`・`__LINE__` は、通常のファイルと同じくコンパイル時に確定する。loaderは原文とファイルパスを用い、module spanのrebase前に参照をリテラルへ変換する。補間内でも参照元の行番号を保持する。ファイル情報のない入力で `__FILE__`・`__DIR__` を参照した場合はエラーとし、cwdで代用しない。
- preload script に `defagent` / `defgenserver` / `defsupervisor` などの process 宣言が含まれる場合、REPL は declaration area から process module stage を抽出し、後続の対話入力でも concrete process surface と runtime metadata を継続参照できる
- REPL user chunk の top-level 宣言は `def` / `import` のみ許可し、`const`、型定義、`impl`、`defmod` は parse error とする
- REPL user chunk の top-level `def` は、セッション内の暗黙擬似モジュールに属する関数として扱う
- REPL user chunk の top-level value binding は同名再束縛を許可するが、既存 binding slot を再利用せず append-only に新 slot を割り当てる
- REPL user chunk の top-level `def` body は、同一セッションの top-level value binding を参照してはならない。参照可能なのは通常関数と同じく引数、関数内 local、visible function/import、標準定義だけとする
- この top-level `def` capture 禁止は REPL source semantics であり、VM の append-only policy とは別責務として Xldr/Sigil 側で検証する
- したがって REPL は「module 外に関数がある」例外ではなく、明示 `defmod` を省略した module-like namespace 実行として扱う
- Eldr の `last_result` は REPL 表示・履歴・将来の command 用 property であり、通常の名前解決対象にはしない
- 初期補完候補には `Ok`, `Err` と builtin 名を含める
- セッションは `.eldr` と live compile の両方から doc metadata を保持し、`:doc` 表示へ利用する
- `.eldr` から初期化した場合、標準 library の compile-time context は source から復元する
- `.eldr` に含まれる user-defined function は VM には常駐するが、新しい REPL 入力の名前解決対象としては復元されない
- したがって `.eldr` 復元は現時点では部分復元であり、compile semantic aggregate の復元欠落を通知する。完全な semantic restore は後続課題とする

LSP completion では stage-0 anchor `Bootstrap` 自体を候補に出さないが、REPL query
metadata は維持する。`:doc Bootstrap` は module documentation を返し、Bootstrap 配下の
特殊形式・builtin surface は既存の bare / qualified 名で `:doc` と `:sig` から参照できる。
`Bootstrap` は callable owner ではないため、`:sig Bootstrap` の module signature / member
一覧 surface は追加しない。

`Bootstrap` / `Kernel` と、`@autoimport` が付いた標準 trait / 標準 `impl Type` owner helper surface は REPL でも auto import 対象とし、`Bootstrap` / `Kernel` への明示 `import` は compile error とする。

REPL と preload の明示 import は Sigil の共通環境で一度だけ検査・適用する。Xldr は適用結果から表示・completion を更新する。導入済み module / member の状態は scope と同じ checkpoint に含め、同じ compile unit の重複 import を拒否する。

### 3.3 失敗時の扱い

- 入力ごとに名前解決、型検査、コード生成の checkpoint を取る
- 途中で失敗した場合は、その入力ぶんの変更をロールバックする
- 失敗した入力は後続セッションへ持ち越さない
- 実行失敗時の VM checkpoint / rollback には process runtime 状態、singleton slot、waiting table、deadline queue、標準 I/O handler buffer cursor、test runner の状態を含める

---

## 4. REPL 入出力契約

REPL 実装は次の 3 層に分ける。

- core: `ReplEngine` が入力処理、checkpoint/rollback、command 解決、doc/sig/save を担う
- presenter: `ReplResult` を CLI/TUI が消費しやすい表示単位へ変換する
- UI adapter: CLI/TUI が terminal I/O と color on/off、pane state を担当する
- core は process stderr/stdout へ直接出力しない。compile / runtime diagnostic は `ReplResult` / `ReplOutput` の rendered lines と structured diagnostic として返し、CLI/TUI adapter が必要に応じて stderr へ投影する

### 4.1 入力源

- TTY 対話入力
- pipe / redirect による標準入力
- REPLの式では `__FILE__`・`__DIR__`・`__LINE__` の呼出しを拒否する。ファイル中では括弧なしで呼び出し、括弧付きの呼出しは拒否する。`__ENV__` は予約名として使用・定義を拒否し、標準宣言は配置しない。

### 4.2 プロンプト

- 通常入力: `xldr(N)> `
- 継続入力: `...(N)> `

`N` は評価行番号であり、空行や継続待機だけでは進まない。

### 4.3 複数行入力

- `ParseError::Incomplete` を受けた場合、入力は pending バッファに保持する
- 次行以降を連結し、再度パースする

文字列の未完入力はSpireの `IncompleteInput` に従う。parserへ渡したソースが末尾の `\`、`\#`、
`\u`、`\u{1b` で終わる場合など、追加入力によって成立できる接頭辞を未完として扱う。
未知エスケープ、無効な桁、桁数超過など、不正が確定した入力を待機にしない。
REPLが物理行を連結するときの改行もソース文字である。Unicodeエスケープ内では空白・改行を拒否するため、
escape途中の入力を次の行で完成できることは保証しない。境界と元ソースspanは
[文字列リテラルの実装契約](./String_literal_spec.md#診断と未完入力)に定める。

### 4.4 出力規約

- `print(...)` の副作用出力はプレフィクスなしで表示する
- 標準 I/O は VM 内部 buffer 直書きではなく、標準 I/O handler 経由の出力として presenter が扱う
- REPL はアプリケーション側の DSL-visible 標準 I/O handler 設定を上書きしない。TTY 入力行の保護が必要な場合、`StdOut` / `StdErr` の host terminal backend だけを REPL UI adapter 管理の一時 buffer に流し、出力行を描画してから prompt と入力 buffer を復元する
- 評価結果は `> ` プレフィクス付きで表示する
- バインド結果は `> name: Type = value` 形式で表示する
- Stringの束縛・単独評価・`:v` による履歴値表示と入れ子の表示は、Eldrの `inspect` とSindrの共通引用処理を使う。ESCと `a` を持つ値は `"\u{1b}a"` とし、値由来の生のC0・DEL・C1制御文字を引用表示に残さない。色なしでも同じ可視表記を使い、CLIの色付け用ANSI列は維持する。端末別の引用処理は設けない。入力との往復保証は[文字列リテラルの実装契約](./String_literal_spec.md#共通の引用表示)に従う
- 評価結果・束縛値・`:v` の表示で未知 tag や payload 数不一致を検出した場合は、RuntimeError としてセッションを終了する。正常な `Err` 値の診断表示ではセッションを継続する。最終 `Err` の payload も表示前に検証し、欠損や余剰を通常の言語エラーとして扱わない。
- 型定義評価は `> TypeName` 形式で表示する
- 表示対象のない `Unit` は表示しない
- `:doc` / `:sig` は evaluator result と同じ `> ` プレフィクスを付けず、presenter が専用レイアウトで表示する
- compile error / runtime diagnostic の人間向け表示は UI adapter が stderr に流し、structured result 側には UI テスト用の rendered lines を保持する

---

## 5. REPL コマンド

### 5.1 実装済み

| コマンド | 説明 |
|---|---|
| `:help`, `:h [command]` | REPL コマンド一覧、または指定コマンドのヘルプを表示する |
| `:quit`, `:exit`, `:q` | REPL を終了する |
| `:v <N>` | 行 `N` の結果を再表示する。binding value の再表示は別 surface として扱い、query command には混ぜない。 |
| `:doc <target>` | public declaration 自身の `@doc` を引く。Trait 本体と定義内メソッドの doc は独立し、実装側の doc は収集しない。bare helper と演算子は canonical Trait メソッドを参照する。通常関数・capture・preload は現在選択された宣言だけを参照し、別宣言の本文で補わない。closure / extractor closure binding は対応する型の説明、non-callable binding はその型の説明を表示する。Struct の `Ty`・`Ty::new`・`Ty!` は型・constructor・Extractor をそれぞれ参照する。Record・具象Error・Enum の `:doc Ty!` は `:doc Ty` へ案内する。ユーザーが doc を付与できない process 生成メンバーは hidden 標準関数の本文を使い、symbol / signature は具体的な process 名に揃える。通常のユーザー定義メンバーへこの対応を広げない。private 宣言は明示して拒否する。宣言 signature は source の表記を保持し、通常 impl の `Self` は具体的な owner type に正規化する。Trait 定義の `Self` は保持する。 |
| `:sig <target>` | public declaration の定義 signature を表示する。通常の REPL scope で解決し、local binding は関数名や Trait family を shadow する。qualified 名で宣言を直接参照できる。関数・Trait family・固定演算子・callable binding・process surface を扱う。`:sig Ty` は Struct・Record・具象Error の constructor、Enum では variant constructor 一覧を表示する。`:sig Ty!` は Struct の Extractor、Record・具象Error・Enum のパターン形を表示する。具象Error は constructor のブロック引数とパターンの payload を区別する。引数指定による specialization は行わない。`FacetPathKind` と kind alias は `:info` へ案内する。 |
| `:info <target>` | 定義、binding、dispatch、operator family / target、singleton process owner、PID binding の解決情報を表示する。command input は通常の REPL scope で名前解決し、local binding は callable family を shadow する。qualified 名は shadowing を避ける escape hatch として使う。一般式 evaluation や旧 command-query 専用 surface には広げず、symbol / family / target / process / binding inspection に留める。process runtime lookup は singleton を owner 名、worker を PID binding で引く。PID binding の `:info` は raw inspect 表示や数値 PID を出さず、型と process metadata を返す。 |
| `:type <binding>` | REPL binding の型と `RuntimeTypeDisplay` を表示する。これは runtime 表示カテゴリであり compile-space `TypeIdentity` ではない。command input は通常の REPL scope で名前解決し、local binding は callable 名を shadow する。通常の値は visible binding lookup のみを対象とし、定義名、trait target query、任意式は受けない。process runtime lookup では singleton process owner 名を追加で受け、worker process は PID binding 経由のみを受ける。struct / record owner への field-oriented lookup はこの変更では追加しない。 |
| `:facet <facet-target>` | ルート型のメンバー一覧、FacetPath 定義、facet binding、または Facet API 消費の canonical path、5-slot type、derived kind、template/pending stage、API eligibility、slot 確定、result type、segment 一覧、停止点を表示する。Record の `Type._N` は対応する名前付き field と同じアクセスへ正規化するが、表示上の origin は `._N` のまま保持する。表示 metadata は Forge が Scar の typed Facet node から生成する共通情報を用い、Xldr は再構築しない。 |
| `:error [full|summary]` | エラー表示モードを切り替える（省略時は現在値表示） |
| `:stacktrace [off|verbose|full]` | stack trace 表示モードを切り替える（省略時は現在値表示）。既定値は `off`。`verbose` はエラーメッセージの後に text stack trace を表示する。`full` は HTMLViewer 経由の将来モードとして予約し、現時点では未対応メッセージを返して状態を変えない。`:error` とは分離し、診断密度と stack trace 表示は独立に扱う。 |
| `:save <path>` | 現在の REPL session を `.eldr` に保存する |
| `:vars` | visible な top-level value binding の索引を表示する。値自体は出さず、`line` / `name` / `type` に相当する簡易一覧を返す。preload script 由来の binding も同じ行番号体系に含める。 |
| `:imported` | 現在の REPL compile unit に効いている import 面を表示する。`src` / `item` / `via` に相当する簡易一覧を返し、`@autoimport defmod` / `import Ty` は module 名だけ、`@autoimport impl Type` / `@autoimport deftrait` / `import Ty::fun` / `import Ty::{a, b}` は導入された member 名まで表示する。 |
| `:defs` | visible な top-level `def` の索引を表示する。REPL 対話入力、`--script` preload、script preload の `include` 由来で REPL compile unit から見える定義を、`line` / `name` / `arity` に相当する簡易一覧で返す。 |
| `:history [selector]` | REPL 履歴を一覧表示する。`selector` は省略、単一行 `N`、列挙 `N1, N2, .., Nn`、範囲 `N..M` を受ける。`N > M` の reverse range と範囲外 index は command 全体を error にする。 |
| `:reload [all|defs]` | preload 条件と top-level `def` から session を再構築する。`all` は起動時 preload に加えて REPL 中の top-level `def` も再投入し、`defs` は起動時 preload だけで再構築する。どちらも value binding は破棄する。 |
| `:clear` | セッション状態を変えずに画面表示だけをクリアする。TTY または host 制約で clear が使えない場合は短い非対応メッセージを返す。 |

### 5.2 Facet root と definition inspection

- `:doc Ty` は canonical type doc を表示する。`:doc Facet.Ty` は PathConstructable `Ty` の
  Facet root doc を表示する。`Ty.prop` は standalone declaration ではないため doc target とせず、
  `:info Ty` または `:facet Ty.prop` へ誘導する
- `:sig Ty` は既存どおり constructor / owner signature を表示する。`Facet.Ty` と `Ty.prop` は
  constructor signature を持たないため、それぞれ `:doc Facet.Ty` と `:info Ty` / `:facet Ty.prop`
  へ誘導する
- `:facet Ty` は FacetPath のルート定義を表示する。struct / record の field、enum の variant を名前と型の列で揃え、struct にだけ field policy (`public` / `private` と `readonly`) を付ける。variant の型は payload が 0 個なら `Unit`、1 個ならその型、複数なら tuple とする。定義の型引数名を保持する
- `:info Ty` は同じ定義一覧を用いる。private field も inspection には含めるが、通常 source の visibility は変えない
- `Error` は共通 `kind` / `message`、具象 `deferror` は共通情報と保存 Payload の field を表示する。外部 constructor の入力署名は Payload 一覧に含めない。Error を通る path は readonly で、Payload 消費時の局所具象 identity 検査を維持する
- `Tuple` / `List` / `HashMap` は、それぞれ `Tuple._N`、`List.[index]` / `List.[start..end]`、`HashMap.[key]` の selector 形式を表示する。裸のルートから未確定の要素型や要素数を推測しない
- ルート判定は可視の宣言 identity を使い、同名 binding に隠された型へ戻らない。非対応型はルート対象外として拒否し、存在しない field 等の path エラーをルート表示で隠さない。引数補完は可視のルート型と Facet binding を候補にする
- `:facet Path` は Path inspection である。private segment を含む structurally valid Path を
  inspection でき、current REPL scope で binding または Facet API consumption が可能かを
  availability として補足する。通常 source の private capability 規則は変更しない

REPL command query は Surtr 式 parser ではなく、command query parser と semantic resolver の組で扱う。

- `:v <N>` は visible binding table を引き直さず、評価時に commit された値を履歴から再表示する。後続の同名再束縛は過去行の再表示結果を変えない
- `:history` は履歴一覧 command であり、結果再表示は `:v <N>` の責務として分離する
- `:vars` / `:defs` / `:imported` / `:history` は罫線付き table ではなく、既存 command と同じ温度感の簡易一覧または `label: value` 行で表示する
- `:defs` は REPL 擬似モジュールだけでなく script preload の擬似モジュール経路も含め、REPL session に登録された visible top-level definition metadata を集約して列挙する
- `:reload` の既定値は `all` とし、起動時 preload に加えて REPL 中の top-level `def` も再投入して session を再構築する
- `:reload defs` は REPL 中の top-level `def` を再投入せず、起動時引数で確定した preload 条件だけで再構築する
- `:reload` は両モードとも value binding を破棄する

- command query は通常の REPL scope で名前解決し、local binding が関数名や trait family を shadow する
- 共通 query は名前・修飾名・公開された固定 symbol と `Facet.Ty`・field path を字句として受け付ける。`?` / `!` suffix、TupleCtor の `(,)` とその修飾名を含む。`True`・`False`は Boolean variant の宣言名として照会できる。
- `:doc` / `:sig` / `:info` は呼び出し、空引数、型引数、引数型、operator target、literal、capture、一般式を拒否する。`Ty!` は REPL 照会表記であり、Record・具象Error・Enum の式やパターンへ `!` を追加しない。
- `:doc` は value binding で型 doc fallback を行う。`ret = Ok(1)` のあと `:doc ret` は `Result` 側 doc を返す
- `:sig` は callable / family / owner / process surface を対象にし、non-callable value binding を拒否する
- 演算子照会は bare token と修飾された固定 symbol を受け付ける。`:sig |*> Option` などの実装指定は拒否する
- `__FILE__`・`__DIR__`・`__LINE__` は `:doc` と `:sig` でBootstrapの組込み関数宣言の説明・シグネチャを照会できる。`:info` と一般の補完には含めない。`__ENV__` は照会対象にしない。
- 固定関数演算子 `|>` / `>>` / `>*` / `>=>` は `Bootstrap` の builtin 宣言を bare token の `:doc` / `:sig` から引く。`->` は `Facet::compose` の doc / signature に接続する。`/` / `%` は `Div::safe_div` / `Mod::safe_mod` の定義メソッドの doc / signature に接続する。削除済みの関数型 trait や `Compose` の lookup へ戻さない
- facet path / facet API lookup は `:sig` に含めず、completion と `:facet` に委譲する
- 多相関数の `:sig` は定義 signature を表示し、入力引数による置換や実装選択を行わない
- `:doc` / `:sig` は public declaration を主 query surface とし、private hit を認識できた場合は private-surface guidance を返す
- 通常関数captureのdocは、元宣言のcanonical名とrebase済みsource spanに対応させてREPL内で保持する。bindingでshadowした後に同名関数を宣言しても、既存captureへ新宣言の本文を渡さない。generic specializationとrecaptureは元宣言の対応を使う。Traitメソッドcaptureは実行先implではなく、metadataが指す定義メソッド自身のdocを参照する。docがないメソッドをTrait本体やimpl本文で補わない。
- `.eldr` のdoc表示は保存済みDocs chunkを正本にする。既存の部分的なsemantic復元では、互換性を検証した標準環境と保存済みDocEntryが一致するcapture対応だけを復元し、標準bootstrapの本文を無条件に補わない。
- 具象 process の REPL 公開面は annotation 由来で決まり、annotation 付き関数だけが public surface になる。handler annotation のない `def` は拒否し、`defp` への変更を案内する。`defp` は外部 API を持たず、`:doc` / `:sig` / 補完対象に含めない
- process public surface の名前解決は通常関数と同じであり、visible な concrete 関数名は `import` により unqualified 参照できる
- compiler-managed hidden process surface は completion / import 対象には含めないが、`:doc` / `:sig` の process query では `Agent::pid` や `GenServer::spawn` のような hidden lower symbol を明示名で引ける
- concrete singleton process query (`Counter::pid`, `MyServer::pid`) は hidden lower doc 本文を流用してよいが、表示 symbol / signature は query した concrete 名に揃える
- `:sig ProcName` は process owner summary query として扱い、heading (`GenServer MyServer`, `Agent MyWorker` など) に加えて `@init` と public messaging surface を複数行で返す。singleton の場合だけ `@pid` を含める
- `:sig pid` は binding が `PID<T>` のとき process-handle messaging summary query として扱い、`PID<T> messaging` heading と public messaging surface を返す。`@init` と `@pid` は含めない
- `:info` は singleton owner 名と PID binding の両方を process-handle lookup として受ける
- `:type` は singleton owner 名と PID binding を受けるが、任意式や typed query までは広げない
- `@call` / `@cast` / `@get` / `@set` などの annotation 名そのものは query target にしない。annotation により公開された concrete 関数名だけを query surface とする
- 例:
  - `:sig Compare`
  - `:sig compare`
  - `:sig |*>`
  - `:doc Compare`
  - `:doc compare`
  - `:doc User`
  - `:doc User::new`
  - `:doc User!`
  - `:doc ret`

### 5.2 予約済み

| コマンド | 説明 |
|---|---|
| `:env` | 設定や mode のような session environment を表示・変更する（`vars` とは分離する） |
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
- REPL core は matching completion candidate を全件保持する。CLI UI は表示件数だけを制限し、将来のページャ導入時にも同じ候補集合を再利用できる構成を保つ
- CLI REPL は実行ディレクトリの `.xldr.yaml` を読み、`repl.cli.completion_candidates` でユーザに表示する補完候補件数を上書きできる。既定値は `5`
- preload script の入力行も REPL 履歴の一部として扱い、`vars` と `history` は同じ行番号体系を共有する
- 演算子 RHS 位置では演算子そのものを補完候補に出さず、シグネチャ表示行に現在位置で期待される型を表示する。例: `1 + ` は `Int + [Int]`、`x |> ` は `Int |> [(Int -> _)]` を表示する
- `|>` / `|*>` / `|>=` / `>>` / `>*` / `>=>` のような関数演算子は、左から右へ段階的に推論できた型を次段に渡す。未確定の型は `_` として表示し、後続段の部分推論を妨げない
- 演算子 RHS 位置の候補は、期待型が分かる場合に一致候補を優先表示する。関数演算子では RHS として使える callable surface を候補に含める
- call-site 補完はカーソルが属する最内 call を候補・期待型の基準にする。ネストした call 内では signature help を最大 2 段表示し、外側から内側へ 0 / 2 spaces でインデントする。3 段以上のネストでは最外側から省略し、最内側 2 段だけを表示する。例: `if(String::contains(w` では `if(...)` と `String::contains(...)` を表示し、候補は `w` prefix の `String::contains` 第1引数候補にする。`if(String::contains(word, needle), ` のように inner call を閉じた後は `if(...)` だけを表示する

---

標準 `Extractor` module は `lib/extractor.srt` をロードする。`:doc Extractor` はmodule、`:doc Extractor::from_result` / `:sig Extractor::from_result` は同じcanonical関数を解決する。from_resultで生成した値のsignatureは通常Closureではなく専用ExtractorClosure型として表示する。

`apply_pattern` は通常の式と同様に chunk 間で評価できる。projection と照合内 binding を REPL の変数一覧へ公開せず、返却 Result を通常 binding へ代入した場合だけその変数を公開する。`:sig apply_pattern` / `:doc apply_pattern` は canonical Kernel consumer の公開 signature を示す。

## 7. 診断表示

表示レベル、stack trace、Error の生成位置の契約は [display_error.md](display_error.md) を参照する。

- 対話モードでは ariadne ベースの人間向け診断を標準とする
- 型エラーでは、可能な範囲で関数宣言や `if` / `match` の分岐位置に補助ラベルを付ける
- REPL 診断は入力継続よりも「その入力単位で失敗してロールバックする」ことを優先する
- `Bootstrap` / `Kernel` の明示 import や、user chunk での `@builtin` 利用禁止も通常の compile error と同じ診断経路で表示する
- `:error summary` では診断の 1 行目のみ表示し、`:error full` では source snippet を含む詳細を表示する
- REPL 診断の span / line / column は compiler 正本と同じく character offset 契約に従い、表示直前にだけ byte range へ変換する
- 入力単位のソースを不変の source ID と表示名で保持し、AST 全体の span に source ID を付けて VM へ渡す。現在入力への上書きで、以前の関数・Extractor の生成位置を変更しない
- Error の主キャプションは script と同じく生成位置を使い、伝播と cause の位置を保持する。`eprint` と `dbg!` も元入力内の行・列を使う
- 保存した `.eldr` は入力ソースを保持する。再開後の入力は、保存済み source ID と表示名に衝突させない
- 入力長が source span の区画幅以上、または source ID を付けた span が runtime の `u32` 範囲を超える場合は、実行前に診断で拒否する

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
