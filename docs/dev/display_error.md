# エラー表示

CLI と REPL のエラー表示、実行時の詳細表示、スタックトレースの契約をまとめる。
本書は現行実装の開発者向け仕様であり、表示設定によって評価規則や Error の内容を変更しない。
診断の構造化データと renderer の規則は [diagnostics.md](diagnostics.md)、
Error の生成・位置・情報保持は [Error_spec.md](Error_spec.md)、VM の表現と実行境界は [EldrVM_spec.md](EldrVM_spec.md) を正本とする。

## 表示対象

| 対象 | 内容 | 表示の入口 |
|---|---|---|
| 静的診断 | parse / resolve / typecheck / codegen の失敗 | Rune のコンパイル処理、Xldr の入力評価 |
| VM の実行エラー | `eldr::RuntimeError`。message と実行時 context を持つ | VM 実行・background task・runtime 値の表示検証の失敗 |
| 言語レベルの Error | `deferror` などが生成する `RichError`。kind、message、Payload、location、cause、diagnostic、stack trace を持つ | `run` の最終 Error / `Err(...)`、REPL の評価結果、明示的な `eprint` |

`Result::Err` は値として扱える。途中で生成した `Err` をすべて自動表示する規則ではない。
`run` は最終結果の Error / `Err(...)` を診断として stderr に表示し、終了コードを `1` にする。
VM の実行エラーも stderr に表示して終了コード `1` を返す。
REPL の評価エラーはセッションの出力として扱い、セッションを終了しない。
CLI 起動時のエラーと終了処理は [Rune_cli_spec.md](Rune_cli_spec.md) に従う。

## ソース位置と cause

言語レベルの Error の主キャプションは、その Error を生成した位置を示す。

- 明示的な Error は外部コンストラクタの呼出式を指す。
- builtin / VM / Forge が定義を呼ぶ場合も発生元 source ID / span を渡し、`deferror` 宣言・内部 `Self` の位置で上書きしない。
- 構文 Pattern の不一致は実際に失敗した子 Pattern を指す。
- list の長さ不一致など、構造自体の失敗はその構造 Pattern 全体を指す。
- 関数、named Extractor、ExtractorClosure、`apply_pattern` で同じ位置規則を使う。

`Err` / `MatchResult::Err` への格納、SafeBind、MonadFail context の partial `<-` は、
元 Error の kind、message、Payload、location、cause を保持する。
MonadFail のない Option / List などの partial `<-` は、既存の Alternative の規則に従い
Error を破棄して empty を返す。この場合、伝播した Error の表示は生じない。
詳細は [Pattern_spec.md](Pattern_spec.md) と [Do_intrinsic_spec.md](Do_intrinsic_spec.md) を参照する。

新しい Error で wrap した場合は、新しい Error の構築位置を主キャプションにする。
元 Error とその位置は cause に保持する。通常の Error 診断では cause の kind と message を
`Caused by: ...` として補足するが、cause ごとのソースキャプションやスタックトレースを
すべて展開する表示ではない。

呼出し経路はスタックトレースで追跡する。先頭 frame の位置で Error の生成位置を上書きしない。
Error 名、message、carrier 名から位置を推測する経路は設けない。
具象 deferror constructor の capture は通常の callable / closure 経路を使い、
専用の生成位置や stack trace の補正を設けない。

REPL も script と同じ契約を使う。入力単位の元ソースを保持し、後続入力から関数や Extractor を
呼び出しても、Error を生成した入力内の行・列を表示する。複数行入力にも同じ規則を適用する。
span は Unicode scalar value 単位の character offset、行・列は `1` 始まりとし、
renderer に渡す直前に byte range へ変換する。
ソース本文を取得できない場合は、保持している Error の位置と message を text で表示する。
別の入力や呼出し位置を生成位置として代用しない。

### テストのアサーション失敗

`surtr test` の失敗イベントでは、標準 `Test` のアサーションが返した
標準 assertion Error 群のキャプションを、そのアサーションの呼出式に付ける。
保存済みの stack trace から関数の正規名と呼出位置を取得し、末尾呼出しも同じ規則で扱う。
`assert_ok_eq` などが内部で `assert_eq` を使う場合は、外側の公開アサーションの呼出式を指す。
キャプチャしたアサーションも、そのキャプチャを実行した呼出式を指す。
`assert`、`assert_satisfies`、比較4種、`assert_err_message_eq`、prefix/suffix 検査、`assert_some` も
標準 Test の正規名で認識する。同じ短名の利用者関数は対象にしない。
名前付き引数も同じ呼出式の位置規則に従う。上記のアサーションの引数に実値の label は付けない。
Error 自体の生成位置、cause、stack trace は変更しない。
アサーション以外の Error は、通常どおり Error の生成位置を表示する。

ファイル名に対応するソース本文を使い、別ファイルのヘルパー内で失敗した場合も
そのヘルパーのソースを表示する。本文が取得できない場合は、保持している位置を text で表示する。
`it` の名前や本文の文字列検索から位置を決めない。
`assert_eq` の直接呼出しでは、LHS/RHS label にその式の引数 span を使う。
キャプチャの呼出しでは引数の対応が変わり得るため、呼出式のキャプションだけを表示する。

## CLI の実行時オプション

### 通常表示と `--error-context verbose`

`surtr run <file.srt|file.eldr>` は通常、詳細な診断を表示する。
REPL の `:error summary` に相当する `run` オプションはない。
実行時 context と text stack trace の追加表示は、次の指定で有効にする。

```sh
surtr run sample.srt --error-context verbose
surtr run sample.eldr --error-context verbose
```

| 設定 | 表示 |
|---|---|
| 指定なし | 通常の診断。追加の `Runtime context:` と `Stack trace:` は表示しない |
| `--error-context verbose` | 通常の診断の後に実行時 context を追加し、保持している stack trace があれば表示する |

追加表示の対象は、VM 実行・background task の実行エラーと、`run` の最終 Error / `Err(...)`。
静的診断や成功終了に runtime context を追加する設定ではない。

`Runtime context:` には `pc`、`opcode`、`function`、`stack_depth`、`frame_depth` を表示する。
VM の実行エラーでは、保持している `details` も表示する。
最終結果の Error / `Err(...)` では、終了時の VM context と、その Error に保存された stack trace を使う。
終了時の `pc` や `function` と、Error の生成位置は別の情報である。

`--error-context` が受け付ける値は `verbose` のみ。
値の省略、未対応値、重複指定は CLI エラーとして拒否する。
`normal` や `off` を指定する形式はなく、追加表示を無効にするときはオプションを省略する。

### `SURTR_VERBOSE_RUNTIME_ERROR`

```sh
SURTR_VERBOSE_RUNTIME_ERROR=1 surtr run sample.srt
SURTR_VERBOSE_RUNTIME_ERROR=1 surtr repl
```

環境変数の値が `1`、`true`、`TRUE`、`yes`、`YES` のとき、
Xldr の共通 renderer は `RuntimeError` の診断に、保持している `pc`、`opcode`、`function`、
`call_site`、`details` を help として追加する。その他の値や未設定では無効になる。

この環境変数は `RichError` / `Err(...)` の表示には適用しない。
stack trace の表示も有効にしない。
CLI の `--error-context verbose` とは独立しており、両方を有効にすると
診断内の help と診断後の runtime context の両方が表示される。
REPL の `summary` では先頭の非空行だけを残すため、追加された help も省略される。

### VM dump と実行イベントの trace

| オプション | 用途 |
|---|---|
| `--vm-dump <path>` | 実行終了時の VM 情報を JSON ファイルへ保存する。既定ではエラー終了時に保存する |
| `--vm-dump-on error\|always` | dump の保存条件。`error` は実行エラー・最終 `Err`・非 0 終了、`always` は成功終了も含む |
| `--trace-call` | 呼び出し・return の実行イベントを記録し、stderr に出力する |
| `--trace-opcode` | opcode の実行イベントを記録し、stderr に出力する |
| `--trace-limit <n>` / `--trace-filter <csv>` | 実行イベントの trace 件数・対象を制限する |

Error に保存する stack trace は、失敗までの呼出し経路を表す。
`--trace-call` / `--trace-opcode` は実行中のイベントを観測する設定であり、
`--trace-limit` / `--trace-filter` は Error の stack trace 表示に適用しない。

VM dump は `--error-context verbose` を省略しても Error の stack trace を保存する。
VM の実行エラーは `runtime_error.stack_trace`、最終結果の言語レベルの Error は
`result.error.stack_trace` に入る。後者の `result.error.location` は Error の生成位置である。
JSON の stack trace に、text 表示の 32 frame 制限は適用しない。
コンパイルに失敗して VM 実行に到達しなかった場合、VM dump は生成しない。
dump 全体の形式と観測設定は [Rune_observability.md](Rune_observability.md) を参照する。

静的診断を JSON で取得するときは `surtr check sample.srt --format json` を使う。
`check` は VM を実行せず、実行時 Error の取得には使わない。
JSON の構造化診断契約は [diagnostics.md](diagnostics.md) に従う。

## スタックトレースの text 表示

CLI の `--error-context verbose` と REPL の `:stacktrace verbose` は、
保持している stack trace をエラーメッセージの後に次の形式で表示する。

```text
Stack trace:
  0: <function> at <file>:<line>:<column>
  1: <function> at <file>:<line>:<column> tail-call
```

上記は表示形式を示す。実際の frame 数、関数名、位置は Error に保存された情報による。

- Error に保存された順序で表示し、frame 番号は `0` 始まりにする。通常は内側の呼出しから外側へ並ぶ。
- 関数名のない frame は `<top-level>` と表示する。
- location のある frame には `at file:line:column` を付ける。位置のない frame に位置を補わない。
- tail-call の frame には `tail-call` を付ける。TCO の経路も VM の breadcrumb として追跡する。
- CLI / REPL の text 表示は最大 `32` frame とし、超過分は `... N frame(s) omitted` と表示する。
- trace が空なら `Stack trace:` の見出しも表示しない。

表示を無効にしても、Error が持つ stack trace は消去しない。
表示のために、既に終了した別の呼出しの trace を混ぜたり、Error の生成位置を変更したりしない。

## REPL の表示レベル

REPL は診断の表示量と stack trace の表示を別々に設定する。
新しいセッションの既定値は `:error full` と `:stacktrace off`。
各設定は、その後の診断・評価エラーの表示に使う。

| コマンド | 挙動 |
|---|---|
| `:error` | 現在値を `error display mode: ...` と表示する |
| `:error full` | source snippet、ラベル、note、help を含む詳細な診断を表示する |
| `:error summary` | レンダリングした診断の先頭の非空行だけを表示する |
| `:stacktrace` | 現在値を `stacktrace display mode: ...` と表示する |
| `:stacktrace off` | stack trace を追加表示しない |
| `:stacktrace verbose` | 実行時 Error / `Err(...)` の診断の後に text stack trace を追加する |
| `:stacktrace full` | 将来の HTMLViewer 表示用の予約値。現時点では未対応メッセージを返し、現在値を変更しない |

`full` という同じ値でも、`:error full` は実装済み、`:stacktrace full` は未対応である。
上記以外の値はコマンド引数の診断で拒否し、現在値を変更しない。
`summary` は診断データの削除や reason の変更ではなく、表示する行の制限である。

```text
:error summary
:stacktrace verbose
def fail() -> Result<Int> { Err(NoneError) }
fail()
```

この組合せでは、エラーの見出しを 1 行表示した後に stack trace を表示する。
`:error full` に変えても stack trace の設定は変わらず、
`:stacktrace off` に変えても診断の表示量は変わらない。
静的診断には runtime stack trace を追加しない。
REPL コマンド全体の契約は [Xldr_spec.md](Xldr_spec.md) を参照する。

### 明示的な `eprint`

`eprint` はプログラムが明示的に stderr へ出力する処理であり、
REPL の `:error` / `:stacktrace` による自動診断の表示設定は適用しない。
Error 値には `Error: <kind>: <message>` と、cause があれば `Caused by: ...` を出力する。
stderr を直接出力する場合は、保持している行・列が有効なら生成位置も出力する。
REPL の stderr capture 経由では位置行を別途出力しない。
REPL 専用の行加算や列の置換は行わず、stack trace も追加しない。

## 実装と検証の入口

| 責務 | 実装 |
|---|---|
| 表示量、runtime 診断、cause、REPL の stack trace 追加 | [`crates/xldr/src/error_display.rs`](../../crates/xldr/src/error_display.rs) |
| REPL の既定値と設定コマンド | [`crates/xldr/src/repl/logic/core.rs`](../../crates/xldr/src/repl/logic/core.rs) |
| CLI オプション、runtime context、最終結果の診断、VM dump | [`crates/rune/src/commands/run.rs`](../../crates/rune/src/commands/run.rs) |
| stack frame の text 書式 | [`crates/eldr/src/error.rs`](../../crates/eldr/src/error.rs) |
| テストの失敗位置とアサーションの表示 | [`crates/eldr/src/vm.rs`](../../crates/eldr/src/vm.rs)、[`crates/rune/src/commands/test.rs`](../../crates/rune/src/commands/test.rs) |
| Error の生成位置・cause・trace の保持 | [`crates/sindr/src/runtime.rs`](../../crates/sindr/src/runtime.rs)、[`crates/eldr/src/vm.rs`](../../crates/eldr/src/vm.rs) |
| 明示的な stderr 出力 | [`crates/eldr/src/builtin.rs`](../../crates/eldr/src/builtin.rs) の `builtin_eprint` |

表示契約を変更するときは、以下の既存テストで対応する境界を確認する。
Ariadne の色、罫線、空白を契約として固定しない。

- [`tests/integration/error_source_locations.rs`](../../tests/integration/error_source_locations.rs):
  生成位置、Pattern の失敗箇所、伝播、wrap、bytecode roundtrip、human / VM dump の位置一致。
- [`tests/integration/test_command.rs`](../../tests/integration/test_command.rs):
  各アサーションの呼出位置、`do` / 文末 `?`、同名の `it`、キャプチャ、名前付き引数、別ファイルのヘルパー。
- [`tests/integration/run_eldr.rs`](../../tests/integration/run_eldr.rs):
  `run_error_context_verbose_*`、通常表示での trace 非表示、tail-call、builtin、closure、VM dump。
- [`crates/xldr/tests/repl_core.rs`](../../crates/xldr/tests/repl_core.rs):
  表示設定の既定値・切替、`:error` と `:stacktrace` の独立性、予約値 `full`、
  過去入力内の Error 生成位置。
- [`crates/xldr/src/error_display.rs`](../../crates/xldr/src/error_display.rs) の単体テスト:
  元ソースの選択、cause、`SURTR_VERBOSE_RUNTIME_ERROR` の追加 help。
