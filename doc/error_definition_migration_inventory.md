# Error 定義・実行時生成の移行棚卸しと修正方針

本書は未実装の移行方針である。現行実装を示す一覧と、変更後の署名・保存 Payload・message の責務を分けて記載する。今回は文書だけを作成し、製品コードとテストは変更・実行しない。

## 現行と移行方針

`lib/**/*.srt` の `deferror` を検索し、`lib/tests/**` を除く **51定義**を確認した。現行ヘッダはすべてコンストラクタ入力であり、下表の現行引数を保存 Payload と読んではならない。現行 `RichError` が保持するのは kind、message、location、cause、diagnostic、stack_trace であり、元の入力を保存する欄はない（[runtime.rs:937](../crates/sindr/src/runtime.rs)）。定義本体は String を返し、入力から message を作る（[definitions.rs:3983–4044](../crates/scar/src/checker/definitions.rs)）。

最終形では、発生条件を選ぶ責務は生成側、message テンプレートと保存 Payload を決める責務は `deferror` 側に置く。生成側は入力値を渡し、完成済みの診断文章を組み立てない。入力と Payload が同じ場合も、それぞれ別の宣言として記述する。

message／detail という String 引数を一律に削除する方針ではない。OS・正規表現エンジン・JSON parser の原文 detail、利用者が指定する説明文、polymorphic な値の `inspect` 結果は入力データとして残せる。定義側が接頭辞、文型、項目の配置を決める。内部で既に分かっている index・length・値・原因を完成文章へ潰して渡す使い方はやめる。

同じ構造の情報を受けることを理由に異なる発生条件を一つへ寄せない。分割後も、一般的な `detail: String` を受ける旧定義を受渡し用の入口として残さない。外部原文 detail を持つ定義も、read、write、spawn 等の失敗対象を名前と入力で限定する。

本書全体の対象は標準51定義と、標準に定義のないruntime生成の言語レベル Error である。この標準節では51定義とその生成・呼出し側を扱い、runtime独自生成は末尾の別節に記載する。コンパイラ診断の ParseError／ResolveError／TypeError と、内部不整合を表す Rust RuntimeError は対象外であり、recoverable Error へ変換しない。fixture／test内の独自kindは本番の一覧から除外し、実装段階2／6で構文と期待値を追従する範囲とする。

## 一覧の読み方

新入力は外部コンストラクタの引数、Payload は保存フィールドを表す。`[]` は空 Payload、`入力と同じ` はその行または分割表の入力と同じ名前・型・順序で保存することを意味する。表の署名は今回定める移行先であり、現行で受理されるコードではない。詳細な受渡し表現と実行命令は実装時に検証する。

根拠欄には定義位置と確認した生成・呼出し位置を示す。`B` は `crates/eldr/src/builtin.rs`、`V` は `crates/eldr/src/vm.rs`、`F` は `crates/forge/src/codegen.rs`、`C` は `crates/eldr/src/vm/process_continuation.rs`、`R` は `crates/sindr/src/runtime.rs` を表す。リンク先と行番号はこの棚卸し時点のもの。

## 標準51定義の移行表

| # | 現行定義・引数 | 最終定義・保持／分割 | 新入力 | 保存 Payload | 定義側の message 方針 | 根拠 |
|---|---|---|---|---|---|---|
| 1 | `NoneError()` | Option の欠落用として保持、他領域は下記「欠落の分離」 | `()`、分離先は同節 | `[]`、分離先は同節 | 固定 `None Value.`、分離先は条件固有文型 | [bootstrap.srt:357](../lib/bootstrap.srt)、[option.srt:221](../lib/types/option.srt)、[generator.srt:70](../lib/types/generator.srt)、[list.srt:84,416](../lib/types/list.srt) |
| 2 | `ZeroDivisionError()` | 除算用を保持、剰余ゼロは `ZeroModuloError` へ分離 | いずれも `()` | いずれも `[]` | `division by zero` と `modulo by zero` を各定義に固定 | [bootstrap.srt:366](../lib/bootstrap.srt)、[B:1576,1584,1600](../crates/eldr/src/builtin.rs) |
| 3 | `EmptyList()` | 空Listの構造分解失敗は `EmptyHeadTailListPattern` へ改名。旧定義は削除 | `()` | `[]` | Forge最終表 F01 の固定文型。API固有の空入力はそれぞれの定義へ分離 | [bootstrap.srt:373](../lib/bootstrap.srt)、[kernel.srt:366](../lib/kernel.srt) |
| 4 | `NotImplemented()` | `todo` の未実装結果として保持 | `()` | `[]` | 固定 `not implemented` | [bootstrap.srt:381](../lib/bootstrap.srt)、[kernel.srt:426](../lib/kernel.srt) |
| 5 | `IndexOutOfBounds(detail: String)` | List通常index、Facet index、Facet逆順rangeへ分割 | `ListIndexOutOfBounds(index: Int, length: Int)`、`FacetListIndexOutOfBounds(index: Int, length: Int)`、`FacetListRangeReversed(start: Int, end: Int, length: Int)` | 各入力と同じ | index／length、range start／endから定義側で生成 | [bootstrap.srt:388](../lib/bootstrap.srt)、[list.srt:118–119](../lib/types/list.srt)、[B:2618–2674](../crates/eldr/src/builtin.rs) |
| 6 | `KeyNotFound(key: String)` | `FacetKeyNotFound` へ用途を限定して改名 | `key: String` | `key: String` | `facet key not found: #{key}` | [bootstrap.srt:391–395](../lib/bootstrap.srt)、[B:2679](../crates/eldr/src/builtin.rs) |
| 7 | `VariantMismatch(detail: String)` | Facet read／updateのvariant selector不一致へ分割 | `FacetReadVariantMismatch`、`FacetUpdateVariantMismatch`。各入力は `segment_index: Int, segment: String, enum_name: String, expected_variant: String, actual_variant: String` | 各入力と同じ | segment、期待variant、実variantを各操作固有の文型へ配置 | [bootstrap.srt:402](../lib/bootstrap.srt)、[F:9549–9556,9808–9815](../crates/forge/src/codegen.rs)。actual取得は下記移行方針 |
| 8 | `NegativeShiftCount(bits: Int)` | 負のshift量として保持。正の固定幅非表現は `ShiftCountTooLarge` へ分離 | 両定義とも `bits: Int` | 両定義とも `bits: Int` | 負数は現行テンプレート、過大値は `shift count is too large: #{bits}` | [int.srt:19,505](../lib/types/int.srt)、[B:1998–2002,2013–2017](../crates/eldr/src/builtin.rs) |
| 9 | `NegativeBitIndex(index: Int)` | 負のbit indexとして保持 | `index: Int` | `index: Int` | 現行テンプレートを保持 | [int.srt:27,511](../lib/types/int.srt) |
| 10 | `InvalidBitWidth(width: Int)` | 非正のcustom widthとして保持 | `width: Int` | `width: Int` | 現行テンプレートを保持 | [int.srt:60,542](../lib/types/int.srt) |
| 11 | `ParseIntError(detail: String)` | 基数固定parseの空／符号のみ／無効桁へ分割 | 下記「整数parse」の基数固定定義 | 各入力と同じ | 原因別の固定文型。基数は `IntBase` 入力 | [int.srt:67,234–242,392–455](../lib/types/int.srt) |
| 12 | `ParseIntLiteralError(detail: String)` | literal parseの空／符号のみ／無効桁／prefixへ分割 | 下記「整数parse」のliteral定義 | 各入力と同じ | literal固有の文型。prefixを完成messageにしない | [int.srt:74,246–261,347–371,468–474](../lib/types/int.srt) |
| 13 | `BitIndexOutOfRange(index: Int, width: Int)` | 非負でwidth以上のbit indexとして保持 | `index: Int, width: Int` | 入力と同じ | 現行テンプレートを保持 | [int.srt:83,512](../lib/types/int.srt) |
| 14 | `NegativeRepeatCount(count: Int)` | String反復の負数として保持。StyledDocの負widthは分離 | `count: Int`、`NegativeStyledDocWidth(width: Int)` | 各入力と同じ | repeat count と document width を別文型にする | [string.srt:19,283](../lib/types/string.srt)、[styled_doc.srt:410–418](../lib/styled_doc.srt) |
| 15 | `InvalidCharList(detail: String)` | 1文字ではないList要素として保持・構造化 | `index: Int, value: String` | 入力と同じ | `expected single-character string at index #{index}, got #{inspect(value)}` | [string.srt:48,112–118](../lib/types/string.srt) |
| 16 | `InvalidStringEncoding(detail: String)` | ASCII encode、整数値範囲、UTF-8 decodeへ分割 | 下記「String encoding」 | 各入力と同じ | encodingと原因ごとに定義側が生成 | [string.srt:57](../lib/types/string.srt)、[B:2191–2263](../crates/eldr/src/builtin.rs) |
| 17 | `InvalidIntegerLiteral(input: String)` | StringからIntへの変換失敗として保持 | `input: String` | `input: String` | 現行テンプレートを保持。Int parse群へ集約しない | [string.srt:64,493–501](../lib/types/string.srt) |
| 18 | `InvalidBooleanLiteral(input: String)` | StringからBooleanへの変換失敗として保持 | `input: String` | `input: String` | 現行テンプレートを保持 | [string.srt:71,519](../lib/types/string.srt) |
| 19 | `InvalidRangeStep(message: String)` | 整数range／文字rangeのstepゼロへ分割 | `ZeroIntegerRangeStep(step: Int)`、`ZeroCharacterRangeStep(step: Int)` | 各 `step: Int` | 各rangeを明記したstepゼロの固定文型 | [generator.srt:22,161,213](../lib/types/generator.srt) |
| 20 | `InvalidCharRange(message: String)` | start／stop、文字数／ASCIIを分ける | 下記「Generator・Duration」 | 各 `value: String` | 対象端点と原因を各定義に固定 | [generator.srt:27,168–177,197–198](../lib/types/generator.srt) |
| 21 | `InvalidGeneratorCount(requested: Int)` | 有限用を保持、無限用へ分離 | `requested: Int`、`InvalidInfiniteGeneratorCount(requested: Int)` | 各 `requested: Int` | finite／infinite take_exactの非正countを各定義で表示 | [generator.srt:33,99](../lib/types/generator.srt)、[infinite_generator.srt:58](../lib/types/infinite_generator.srt) |
| 22 | `GeneratorShortage(requested: Int, actual: Int)` | 有限の不足として保持。無限側は内部保証として別扱い | `requested: Int, actual: Int` | 入力と同じ | 現行テンプレートを保持 | [generator.srt:39,105](../lib/types/generator.srt)、[infinite_generator.srt:49–63](../lib/types/infinite_generator.srt) |
| 23 | `InvalidDuration(message: String)` | 負millisの構築／減算結果負値へ分割 | `NegativeDurationMillis(value: Int)`、`NegativeDurationDifference(lhs_millis: Int, rhs_millis: Int)` | 各入力と同じ | `duration must be non-negative: #{value}`／`duration subtraction would be negative: #{lhs_millis}ms - #{rhs_millis}ms` | [duration.srt:17,35,54](../lib/types/duration.srt) |
| 24 | `JsonParseError(line: Int, column: Int, detail: String)` | JSON構文parse失敗として保持。内部の深さ制限は追加定義表へ分離 | `line: Int, column: Int, detail: String` | 入力と同じ | 位置＋外部parser原文detailを定義側で組立てる | [json.srt:21](../lib/types/json.srt)、[B:4263–4287,4363–4387](../crates/eldr/src/builtin.rs) |
| 25 | `JsonDecodeError(path: String, expected: String, got: String)` | missing field／indexと各shape不一致へ分割 | 下記「JSON」 | 各入力と同じ | field／index／期待shapeを定義側に固定 | [json.srt:28,78–91,116–166](../lib/types/json.srt) |
| 26 | `JsonEncodeError(detail: String)` | BigIntのJSON number表現範囲外を `JsonIntegerOutOfRange` へ分離 | `value: Int` | `value: Int` | `JSON integer cannot be represented as a JSON number: #{value}` | [json.srt:35](../lib/types/json.srt)、[B:4391–4398,4503–4506](../crates/eldr/src/builtin.rs) |
| 27 | `RegexCompileError(detail: String)` | Regex compileの外部構文失敗として保持 | `pattern: String, detail: String` | 入力と同じ | `regex compile failed for #{inspect(pattern)}: #{detail}`、原文detailは保持 | [regex.srt:23,35](../lib/types/regex.srt)、[B:2850](../crates/eldr/src/builtin.rs) |
| 28 | `InvalidRandomRange(start: Int, end: Int)` | 空／逆順の半開区間として保持 | `start: Int, end: Int` | 入力と同じ | 現行の区間表示テンプレートを保持 | [Random.srt:12,36–64](../lib/Random.srt)、[B:3798–3818](../crates/eldr/src/builtin.rs) |
| 29 | `InputError(detail: String)` | EOF、prompt、line／char read、terminal、UTF-8へ分割 | 下記「IO」 | 各入力と同じ。固定失敗は `[]` | EOFや未対応buildは固定文、外部read失敗は原文detailを固有文型へ配置 | [IO.srt:5,27,40](../lib/IO.srt)、[B:3064–3107,3469–3540](../crates/eldr/src/builtin.rs) |
| 30 | `FileNotFound(path: String)` | Fileの対象欠落として保持 | `path: String` | `path: String` | 現行テンプレートを保持 | [file.srt:15](../lib/file.srt)、[B:4841–4858](../crates/eldr/src/builtin.rs) |
| 31 | `FilePermissionDenied(path: String)` | Fileの権限拒否として保持 | `path: String` | `path: String` | 現行テンプレートを保持 | [file.srt:19](../lib/file.srt)、[B:4844,4852](../crates/eldr/src/builtin.rs) |
| 32 | `FileAlreadyExists(path: String)` | Fileの既存対象との衝突として保持 | `path: String` | `path: String` | 現行テンプレートを保持 | [file.srt:23](../lib/file.srt)、[B:4845,4853](../crates/eldr/src/builtin.rs) |
| 33 | `FileInvalidPath(path: String)` | Fileのpath不正として保持 | `path: String` | `path: String` | 現行テンプレートを保持 | [file.srt:27](../lib/file.srt)、[B:4846,4854](../crates/eldr/src/builtin.rs) |
| 34 | `FileClosed()` | 閉じた／登録のないhandle利用として保持 | `()` | `[]` | 固定 `file is already closed`。取得できないpathを補わない | [file.srt:31](../lib/file.srt)、[B:4902](../crates/eldr/src/builtin.rs)、[V:4088–4091](../crates/eldr/src/vm.rs) |
| 35 | `FileEncodingError(detail: String)` | decode由来の一括read／chunk read失敗を分離。非readのInvalidDataは操作別OS失敗へ | `FileReadEncodingError(path: String, detail: String)`、`FileChunkEncodingError(path: String, detail: String)` | 各入力と同じ | read対象と原文decode detailを定義側で組立てる | [file.srt:35](../lib/file.srt)、[B:4855,4910](../crates/eldr/src/builtin.rs)、[V:6031–6054](../crates/eldr/src/vm.rs) |
| 36 | `FileIoError(detail: String)` | 操作固有のOS失敗とhandle mode不一致へ分割 | 下記「File・FileSystem・Shell」 | 各入力と同じ | 操作固有文型、raw OS detailは保持、mode不一致は型付きmodeを表示 | [file.srt:39,132–252](../lib/file.srt)、[B:4900–4911](../crates/eldr/src/builtin.rs)、[V:4092–4114](../crates/eldr/src/vm.rs) |
| 37 | `FileSystemNotFound(path: String)` | 単項操作は保持、move／copyは下記二項操作表へ分離。FileSystemの対象欠落として保持、pathを型付きにする | `path: FilePath` | `path: FilePath` | `filesystem path not found: #{to_string(path)}` | [FileSystem.srt:105,154–163](../lib/FileSystem.srt)、[B:4863,4870](../crates/eldr/src/builtin.rs) |
| 38 | `FileSystemAlreadyExists(path: String)` | 単項操作は保持、move／copyは下記二項操作表へ分離。FileSystemの既存対象との衝突として保持 | `path: FilePath` | `path: FilePath` | 対象path＋既存衝突の固有文型 | [FileSystem.srt:109](../lib/FileSystem.srt)、[B:4865,4872](../crates/eldr/src/builtin.rs) |
| 39 | `FileSystemPermissionDenied(path: String)` | 単項操作は保持、move／copyは下記二項操作表へ分離。FileSystemの権限拒否として保持 | `path: FilePath` | `path: FilePath` | 対象path＋権限拒否の固有文型 | [FileSystem.srt:113](../lib/FileSystem.srt)、[B:4864,4871](../crates/eldr/src/builtin.rs) |
| 40 | `FileSystemNotDirectory(path: String)` | metadata成功時の非directoryに限定。不在・取得失敗は原因別へ | `path: FilePath` | `path: FilePath` | 対象path＋directory要求の固有文型 | [FileSystem.srt:117](../lib/FileSystem.srt)、[B:4691](../crates/eldr/src/builtin.rs) |
| 41 | `FileSystemIsDirectory(path: String)` | directory不許可条件として保持。利用は実装前に確認 | `path: FilePath` | `path: FilePath` | 対象path＋directory不許可の固定文型 | [FileSystem.srt:121](../lib/FileSystem.srt)、[B:4885](../crates/eldr/src/builtin.rs)はmessage分岐を確認、生成利用全件は未確認 |
| 42 | `FileSystemInvalidPath(path: String)` | raw path不正とparent／name欠落を分離 | 単項操作は `FileSystemInvalidPath(raw: String)`、二項操作は下記表。`FileSystemParentMissing(path: FilePath)`、`FileSystemNameMissing(path: FilePath)` | 各入力と同じ | invalid raw pathと構成要素欠落を別文型にする | [FileSystem.srt:125,148–151](../lib/FileSystem.srt)、[B:3225,3236,4866](../crates/eldr/src/builtin.rs) |
| 43 | `FileSystemInvalidDepth(depth: Int)` | tree_depthの負depthとして保持 | `depth: Int` | `depth: Int` | 現行テンプレートを保持 | [FileSystem.srt:129](../lib/FileSystem.srt)、[B:3277–3282](../crates/eldr/src/builtin.rs) |
| 44 | `FileSystemUnsupported(detail: String)` | directory copy不許可を `FileSystemDirectoryCopyUnsupported` へ限定 | `from: FilePath, to: FilePath` | 入力と同じ | `directory copy is not supported: #{to_string(from)} -> #{to_string(to)}` | [FileSystem.srt:133](../lib/FileSystem.srt)、[B:3328–3337](../crates/eldr/src/builtin.rs) |
| 45 | `FileSystemIoError(detail: String)` | stat／ls／tree／mutation固有のOS失敗へ分割 | 下記「File・FileSystem・Shell」 | 各入力と同じ | 操作・対象を固有文型にし、raw OS detailを保持 | [FileSystem.srt:137,154–163](../lib/FileSystem.srt)、[B:4718–4750,4861–4876](../crates/eldr/src/builtin.rs) |
| 46 | `ShellCommandNotFound(command: String)` | host NotFoundの起動失敗を `ShellSpawnResourceNotFound` へ改名。旧定義は削除 | `command: String, args: List<String>, cwd: FilePath, detail: String` | 入力と同じ | `shell could not start #{command} in #{to_string(cwd)}: #{detail}`。欠落資源をcommandだと断定しない | [Shell.srt:1,62](../lib/Shell.srt)、[B:3377–3381](../crates/eldr/src/builtin.rs) |
| 47 | `ShellSpawnFailed(detail: String)` | exec起動失敗として保持・構造化 | `command: String, args: List<String>, cwd: FilePath, detail: String` | 入力と同じ | `shell spawn failed for #{command} in #{to_string(cwd)}: #{detail}`、argsも保存 | [Shell.srt:5](../lib/Shell.srt)、[B:3368–3388](../crates/eldr/src/builtin.rs) |
| 48 | `ShellWorkingDirectoryNotFound(path: String)` | cd対象不在、非directory、metadata取得失敗を分離 | `ShellWorkingDirectoryNotFound(path: FilePath)`、`ShellWorkingDirectoryNotDirectory(path: FilePath)`、`ShellWorkingDirectoryInspectFailed(path: FilePath, detail: String)` | 各入力と同じ | 各発生条件の固有文型 | [Shell.srt:9,61](../lib/Shell.srt)、[B:3350–3358](../crates/eldr/src/builtin.rs) |
| 49 | `ShellUnsupported(detail: String)` | 移行対象として保持。利用条件は実装前に確認 | `feature: String` | `feature: String` | `shell feature is unsupported: #{feature}`。完成messageをfeatureへ流さない | [Shell.srt:13](../lib/Shell.srt)、今回の標準・Rust検索では生成呼出し未検出。削除は確定しない |
| 50 | `ShellIoError(detail: String)` | stdout／stderr UTF-8、cwd canonicalizeへ分割 | `ShellStdoutEncodingError(command: String, detail: String)`、`ShellStderrEncodingError(command: String, detail: String)`、`ShellWorkingDirectoryResolveFailed(path: FilePath, detail: String)` | 各入力と同じ | 対象stream／cwdを定義側に固定、raw外部detailを保持 | [Shell.srt:17](../lib/Shell.srt)、[B:3392–3408,4756–4765](../crates/eldr/src/builtin.rs) |
| 51 | `TestAssertionFailed(detail: String)` | 明示fail／Boolean／比較／shape／text／近似／kind・cause検査へ分割 | 下記「Test」 | 各入力と同じ。固定失敗は `[]` | 各assert固有の文型を定義側へ移す。利用者noteは保持 | [test.srt:4,56–57,170–615](../lib/test.srt)、[B:2328–2382](../crates/eldr/src/builtin.rs)、[V:118](../crates/eldr/src/vm.rs) |

## 分割する発生条件と署名

以下の分割表は、入力欄と同じフィールドを同じ順序で保存する。空入力の Payload は `[]` とする。新しい enum や generic Error 定義を前提にせず、既存の通常型だけを使う。

### 欠落の分離

`NoneError` は `Option::None` のResultへの変換と、利用者が明示的に表す抽象的な欠落には維持する。既存標準の具体的な欠落は、次の定義へ分ける。単なる別名ラッパーではなく、生成条件・戻り値の補助表記・利用例を該当定義へ移す。

| 発生条件 | 最終定義・新入力 | 定義側の文型 | 現行根拠 |
|---|---|---|---|
| 有限Generatorの終端 | `GeneratorExhausted()` | `generator has no next item` | [generator.srt:67–70](../lib/types/generator.srt) |
| List firstの空入力 | `ListFirstEmpty()` | `first requires a nonempty list` | [list.srt:84](../lib/types/list.srt) |
| List lastの空入力 | `ListLastEmpty()` | `last requires a nonempty list` | [list.srt:96](../lib/types/list.srt) |
| List findの該当なし | `ListFindNoMatch()` | `no list element satisfies the predicate` | [list.srt:414–417](../lib/types/list.srt) |
| List find_mapのSomeなし | `ListFindMapNoValue()` | `no list element produces Some` | [list.srt:428–433](../lib/types/list.srt) |
| List最大値／最小値／両端値の空入力 | `ListMaximumEmpty()`、`ListMinimumEmpty()`、`ListMinMaxEmpty()` | 各演算がnonemptyを要求する固定文型 | [list.srt:497–562](../lib/types/list.srt)。maxとmax_by、minとmin_byは同じ数学的失敗条件として各組内で共有 |
| String prefix不一致 | `StringPrefixMissing(value: String, prefix: String)` | `string does not start with #{inspect(prefix)}: #{inspect(value)}` | [string.srt:313–322](../lib/types/string.srt) |
| String suffix不一致 | `StringSuffixMissing(value: String, suffix: String)` | suffix固有の文型 | [string.srt:334–339](../lib/types/string.srt) |
| String separator未検出 | `StringSeparatorMissing(value: String, separator: String)` | separator固有の文型 | [string.srt:350–351](../lib/types/string.srt) |
| HashMapの通常lookup欠落 | `HashMapKeyMissing(key: String)` | `hash map key not found: #{key}` | [hash_map.srt:71](../lib/types/hash_map.srt)。Facetの `FacetKeyNotFound` と分ける |

文字列処理の再帰内部で入力が短くなっても、外部操作の元の入力をエラーコンストラクタへ渡す。prefix照合をseparator検索の内部判定に使う場合は、その失敗を公開prefix Errorとして漏らさず、検索結果に対応する定義を選ぶ。

Regexのfind／captures／groupもNoneErrorを使う（[regex.srt:48,59,145,156](../lib/types/regex.srt)）。公開条件は次の署名とする。`RegexFindNoMatch(pattern: String, input: String)`、`RegexCapturesNoMatch(pattern: String, input: String)`、`RegexCaptureIndexMissing(index: Int)`、`RegexCaptureNameMissing(name: String)`。groupの「未知の指定」と「存在するgroupが非参加」は別条件なので、後者は `RegexCaptureIndexUnmatched(index: Int)`、`RegexCaptureNameUnmatched(name: String)` に分ける。find／capturesではpatternとinputをその場で取得できる（B:2863–2869,2941–2947）。groupのindex版は `groups.get(index)` の指定範囲外と格納されたNone、name版は `name_to_index` の欠落とgroupのNoneを別分岐にすることで区別できる（B:2909–2936）。groupの定義はpatternを要求しないので、capture handleへpatternを追加する変更は前提にしない。

Int parseの `_digit_value` のNoneError（[int.srt:228–230](../lib/types/int.srt)）は公開parse失敗ではなく内部の桁候補判定に使われ、その呼出し側が原因別parse Errorを選ぶ。内部判定の型変更はこの標準定義移行の前提にしない。

### 整数parse

| 発生条件 | 最終定義・新入力 | 定義側の文型 |
|---|---|---|
| 基数固定parseの空入力 | `IntParseEmpty(base: IntBase)` | `empty #{IntBase::label(base)} integer input` |
| 基数固定parseの符号のみ | `IntParseSignWithoutDigits(base: IntBase, input: String)` | 基数と元の符号入力を表示 |
| 基数固定parseの無効桁 | `IntParseInvalidDigit(base: IntBase, digit: String, index: Int)` | `invalid digit for #{IntBase::label(base)} integer: #{inspect(digit)} at index #{index}` |
| literalの空入力 | `IntLiteralEmpty()` | `empty integer literal input` |
| literalの符号のみ | `IntLiteralSignWithoutDigits(input: String)` | `sign without digits in integer literal: #{inspect(input)}` |
| literal prefix後の桁なし | `IntLiteralMissingDigits(base: IntBase)` | `missing digits after integer base prefix: #{IntBase::prefix(base)}` |
| 未知prefix | `IntLiteralUnknownPrefix(prefix: String)` | `unknown integer base prefix: #{prefix}`、入力は `0` を含む実際のprefix |
| literalの無効桁 | `IntLiteralInvalidDigit(base: IntBase, digit: String, index: Int)` | integer literal固有の無効桁文型 |

indexは現在の各parserが数えている位置の意味を維持し、この移行でbyte／文字indexを変更しない。各parse関数が基数と元入力を保持して渡す。基数名やprefixを呼出し側で完成文へ展開しない。

### String encoding

| 発生条件 | 最終定義・新入力 | 定義側の文型 |
|---|---|---|
| ASCIIへ変換できない文字 | `StringAsciiCharacterUnsupported(index: Int, value: String)` | `ASCII encoding does not support character at index #{index}: #{value}` |
| UTF-8用byte整数が範囲外 | `StringUtf8ByteOutOfRange(index: Int, value: Int)` | `UTF-8 byte out of range at index #{index}: #{value}` |
| ASCII用整数が範囲外 | `StringAsciiCodeOutOfRange(index: Int, value: Int)` | `ASCII code out of range at index #{index}: #{value}` |
| 完結した不正UTF-8 sequence | `StringUtf8InvalidSequence(index: Int, length: Int)` | `invalid UTF-8 byte sequence at index #{index} (len #{length})` |
| 不完結UTF-8 sequence | `StringUtf8IncompleteSequence(index: Int)` | `incomplete UTF-8 byte sequence at index #{index}` |

`to_u32()` の失敗をすべて「negative code」と表示する現行文言は、正の巨大Intにも成立しない（B:2223–2227）。最終定義は選んだencodingの整数範囲外を意味し、負数と上限超過を同じ範囲条件で検査・表示する。ユーザーIntを固定幅整数へ無理に縮めてから Payload を作らない。

### Generator・Duration

文字rangeの端点検査を、`CharacterRangeStartLengthInvalid(value: String)`、`CharacterRangeStopLengthInvalid(value: String)`、`CharacterRangeStartNonAscii(value: String)`、`CharacterRangeStopNonAscii(value: String)` へ分ける。入力文字列を保存し、start／stopと「single character」／「ASCII」の文型を定義側へ固定する。端点を表す自由な `label: String` と完成messageを受ける汎用定義にはしない。

`GeneratorShortage` は有限列の不足を表す。無限Generatorの通常takeは不足を返さないと文書化されているため、無限側の長さ不整合をこの通常失敗へ寄せない。takeの正常返却が要求数を満たす既存契約を検証し、標準側の不足Err生成を削除する。内部で長さ契約を検査する必要がある場合はRust RuntimeErrorで扱う。Rust RuntimeErrorを標準Errorへ変換する方針にはしない（[infinite_generator.srt:49–63](../lib/types/infinite_generator.srt)）。

Durationは値をmillisのIntとして受ける二定義に分ける。減算の両オペランドを保持し、負の結果だけを保存して発生条件を失わない。

### JSON

`JsonParseError` のdetailは外部parserの説明を残す。生成側の位置取得・原文抽出と、定義側の表示組立てを分離する。現在Rust側にある `json parse error at ...` をdetailとして再入力しない。

| 発生条件 | 最終定義・新入力 | 定義側の文型 |
|---|---|---|
| object fieldなし | `JsonFieldMissing(key: String)` | `json field missing at $.#{key}` |
| array indexなし | `JsonIndexMissing(index: Int)` | `json array index missing at $[#{index}]` |
| object要求へのshape不一致 | `JsonObjectExpected(path: String, got: String)` | `json decode error at #{path}: expected object, got #{got}` |
| array要求へのshape不一致 | `JsonArrayExpected(path: String, got: String)` | array固有文型 |
| string要求へのshape不一致 | `JsonStringExpected(path: String, got: String)` | string固有文型 |
| int要求へのshape不一致 | `JsonIntExpected(path: String, got: String)` | int固有文型 |
| float要求へのshape不一致 | `JsonFloatExpected(path: String, got: String)` | float固有文型 |
| boolean要求へのshape不一致 | `JsonBooleanExpected(path: String, got: String)` | boolean固有文型 |

`got` は現在の `Json::kind` が返す型名tokenであり、完成messageではない。field／index取得と型変換で同じshapeを要求する場合は、要求shapeとpathが同じ意味を持つ定義だけを共有する。

JsonEncodeの確認できた通常入力の失敗は、大きなIntをserde_jsonのnumberへ変換できない条件である（B:4391–4398）。NaN／infinityの枝やJsonValue以外の枝を、今回の公開Error署名を埋めるための通常失敗として断定しない。Floatはfinite-only、引数はJsonValueという既存契約と照合し、内部契約違反を別に扱う（B:4427–4438,4490–4508）。

### IO

| 発生条件 | 最終定義・新入力 | 定義側の文型 |
|---|---|---|
| 一文字入力のEOF | `InputCharacterEnd()` | `end of character input` |
| 一行入力のEOF | `InputLineEnd()` | `end of line input` |
| prompt出力失敗 | `InputPromptWriteFailed(prompt: String, detail: String)` | `input prompt write failed: #{detail}` |
| 非terminalの一文字read失敗 | `InputCharacterReadFailed(detail: String)` | `character input read failed: #{detail}` |
| 一行read失敗 | `InputLineReadFailed(detail: String)` | `line input read failed: #{detail}` |
| 一文字入力の不正UTF-8 | `InputCharacterEncodingError(detail: String)` | `invalid UTF-8 character input: #{detail}`、decode原文を保持 |
| EOFによる不完結UTF-8 | `InputCharacterIncomplete()` | `incomplete UTF-8 input before EOF` |
| terminal raw mode開始失敗 | `TerminalInputModeStartFailed(detail: String)` | 開始失敗固有の文型 |
| terminal event read失敗 | `TerminalInputReadFailed(detail: String)` | terminal read固有の文型 |
| terminal raw mode復元失敗 | `TerminalInputModeRestoreFailed(detail: String)` | 復元失敗固有の文型 |
| keyの非対応 | `TerminalInputKeyUnsupported(key: String)` | `unsupported key input: #{key}`、key種別の外部表現を入力 |
| terminal inputを含まないbuild | `TerminalInputUnavailable()` | `terminal key input is unavailable in this Eldr build` |

現在、terminalやUTF-8のhelperは発生原因をStringへまとめている（B:3485–3540）。新しい生成側は文章をparseしてkindを選ぶのではなく、失敗した分岐で対応するコンストラクタを選ぶ。EOFとOS read失敗、terminal機能の非対応を同じdetail入口へ戻さない。

### File・FileSystem・Shell

FileのOS失敗は、`FileReadFailed`、`FileWriteFailed`、`FileAppendOpenFailed`、`FileAppendWriteFailed`、`FileAppendCloseFailed`、`FileDeleteFailed`、`FileOpenFailed`、`FileReadChunkFailed`、`FileWriteChunkFailed`、`FileFlushFailed`、`FileCloseFailed` に分ける。各定義の入力・Payloadは `path: String, detail: String` とし、`FileOpenFailed` はさらに `mode: FileMode` を最後に持つ。定義ごとに `file read failed for ...` 等の操作固有文型を持たせる。pathが現在helperへ渡されていないhandle経路は、`open_files` の `VmOpenFile.path` とmodeを取得して渡す。このpathは現在保存するhost pathである（V:254–256,4072–4077）。closeは登録を削除してからflushするため、削除前または取り出したresourceからpathを保持して失敗データへ渡す（V:4132–4139）。存在しないhandleの失敗は `FileClosed()` へ分離し、pathを捏造しない。

handle modeの不一致は `FileHandleNotReadable(path: String, mode: FileMode)`、`FileHandleNotWritable(path: String, mode: FileMode)` に分け、入力と同じPayloadを保存する（V:4092–4114）。内部で把握しているmodeをrawdetailへ潰さない。UTF-8のchunk失敗も内部の固定文言と外部decode detailを分け、`FileChunkEncodingError` に渡すdetailを完成messageにしない。

FileSystemのその他OS失敗は、`FileSystemStatFailed`、`FileSystemListFailed`、`FileSystemTreeReadFailed`、`FileSystemMkdirFailed`、`FileSystemMkdirAllFailed`、`FileSystemRemoveFailed` に分け、各入力・Payloadを `path: FilePath, detail: String` とする。move／copyは `FileSystemMoveFailed(from: FilePath, to: FilePath, detail: String)`、`FileSystemCopyFailed(from: FilePath, to: FilePath, detail: String)` とし、宛先を失わない。名称はAPI固有で、単にoperation名を自由なStringで渡す共通Errorを作らない。tree traversalの失敗pathは最初のrootではなく、その時点の対象pathを渡す（B:4718–4750）。

既存のpath欠落／権限／既存衝突は、同じ領域・同じ発生条件を持つ定義として保持する。FileのString pathとFileSystem／ShellのFilePathを共有定義へ集約しない。FilePathは現行newがrawを包む通常値であるため、型付きpathをPayloadへ渡せる（[FileSystem.srt:5–12](../lib/FileSystem.srt)）。不正raw pathそのものを検査する定義は `raw: String` を受ける。

Fileの現行 `file_path_error_result` は `InvalidData` を操作にかかわらずFileEncodingErrorへ分類し、readingと表示する（B:4841–4855）。一括readはOS読込みとUTF-8 decodeの失敗を内部で区別し、decode由来と確認できた失敗だけFileReadEncodingErrorへ渡す。write／append／delete／open等で受けたInvalidDataは各操作のOS失敗定義へ渡す。非readでの実到達を今回再現したと主張しない。

二つのpathを受けるmove／copyは、既存の共有path原因へfromだけを渡すと宛先を失う（B:3324,3341）。この二項操作だけは、次の8定義に分ける。各入力・Payloadは **`from: FilePath, to: FilePath, detail: String`** の順とし、raw OS detailを保存する。どちらのendpointが原因かを推測して断定しない。

| 操作・host原因 | 最終定義 | 定義側の文型 |
|---|---|---|
| move・NotFound | `FileSystemMoveNotFound` | `filesystem move path not found: #{to_string(from)} -> #{to_string(to)}: #{detail}` |
| move・PermissionDenied | `FileSystemMovePermissionDenied` | moveの権限拒否を両pathとdetailで表示 |
| move・AlreadyExists | `FileSystemMoveAlreadyExists` | moveの既存衝突を両pathとdetailで表示 |
| move・InvalidInput | `FileSystemMoveInvalidPath` | moveのpath不正を両pathとdetailで表示 |
| copy・NotFound | `FileSystemCopyNotFound` | copyの対象不在を両pathとdetailで表示 |
| copy・PermissionDenied | `FileSystemCopyPermissionDenied` | copyの権限拒否を両pathとdetailで表示 |
| copy・AlreadyExists | `FileSystemCopyAlreadyExists` | copyの既存衝突を両pathとdetailで表示 |
| copy・InvalidInput | `FileSystemCopyInvalidPath` | copyのpath不正を両pathとdetailで表示 |

単項pathの同じ原因だけは領域内の定義を共有できる。FileSystemのls／treeのroot前提判定とShellのcdは、現在 `is_dir() == false` が不在、非directory、metadata取得失敗を畳んでいる（B:3352–3358,4689–4691）。この前提判定を、symlinkを辿る `fs::metadata` へ置き換え、成功して非directoryの場合だけNotDirectoryを生成する。NotFound／PermissionDenied等はその原因に対応させ、FileSystemのその他取得失敗はListFailed／TreeReadFailed、Shellの取得失敗はShellWorkingDirectoryInspectFailedへ渡す。root判定を `symlink_metadata` に変えてリンクを辿る既存の意味を変えない。treeの子要素の再帰判定とentryのmetadata取得は既存の意味を維持し、dangling symlinkを正常収集できる入力まで失敗へ変えない（B:4661,4747）。

Shell::execのhost NotFoundもcommand欠落を確定しない。cwdや実行資源の不在でも同じ分類になるため、ShellSpawnResourceNotFoundはcommand、args、cwd、OS原文を受ける。commandの不在を推測する事前検索や文言parseを追加しない。終了codeが非ゼロでも `CommandResult` の正常値である契約は維持する（[Shell.srt:21–25](../lib/Shell.srt)）。

`ShellUnsupported` と `FileSystemIsDirectory` は移行対象として維持する。検索で生成利用が確認できなかったことだけを理由に削除しない。実装前に利用条件と入力取得位置を確認し、確認していない発生条件を既存挙動として説明しない。

Facetのvariant不一致はreadとupdateの定義を分ける。現行Forgeは静的なsegmentと期待variantを文章化している。移行後は失敗した値のtag／discriminantとregistry metadataからactual variantを取得する実装変更を行い、構造化入力へ追加する。actual情報を既に保存しているという現行説明にはしない。registryと値の内部不整合はRust RuntimeErrorとし、actualを空Stringや `unknown` へ置き換えるfallbackを作らない。

### Test

任意の型 `$A` をそのままgeneric Payloadへ入れる新機能は追加しない。既存assertが必要とする `inspect` 済み値を `String` として渡し、比較文型は定義側で持つ。利用者指定のnoteは保持する。次の各定義は入力と同じPayloadを保存する。

| 発生条件 | 最終定義・新入力 | 定義側の文型 |
|---|---|---|
| 明示的 `fail` | `TestExplicitFailure(note: String)` | 定義側でnoteをそのままmessageにする。空Stringも保持 |
| `assert` のFalse | `TestConditionFalse(note: String)` | 定義側でnoteをそのままmessageにする。空Stringも保持 |
| predicate拒否 | `TestPredicateRejected(note: String, actual: String)` | `#{note}\nactual: #{actual}` |
| True要求／False要求 | `TestExpectedTrue()`、`TestExpectedFalse()` | 各Boolean期待の固定文 |
| equality／inequality | `TestEqualityMismatch(expected: String, actual: String)`、`TestExpectedUnequal(actual: String)` | equality／inequality各文型 |
| lt／lte／gt／gte | `TestLessThanFailed(lhs: String, rhs: String)`、`TestLessEqualFailed(lhs: String, rhs: String)`、`TestGreaterThanFailed(lhs: String, rhs: String)`、`TestGreaterEqualFailed(lhs: String, rhs: String)` | 各演算子を定義側へ固定 |
| prefix／suffix／substring | `TestPrefixMissing(prefix: String, actual: String)`、`TestSuffixMissing(suffix: String, actual: String)`、`TestFragmentMissing(fragment: String, actual: String)` | 各String条件の固有文型 |
| Some要求でNone | `TestExpectedSome()` | `expected Some, got None` |
| Some値要求でNone | `TestExpectedSomeValue(expected: String)` | `expected Some(#{expected}), got None` |
| None要求でSome | `TestExpectedNone(actual: String)` | `expected None, got Some(#{actual})` |
| Ok要求でErr | `TestExpectedOk(error: Error)` | 既存Errorを入力・Payloadに保持して定義側でinspect |
| Ok値要求でErr | `TestExpectedOkValue(expected: String, error: Error)` | 期待値と実Errorの固有文型 |
| Err要求でOk | `TestExpectedErr(actual: String)` | `expected Err, got Ok(#{actual})` |
| Err message要求でOk | `TestExpectedErrMessage(expected: String, actual: String)` | message期待固有文型 |
| Err text要求でOk | `TestExpectedErrFragment(fragment: String, actual: String)` | fragment期待固有文型 |
| Err textにfragmentなし | `TestErrorFragmentMissing(fragment: String, actual: String)` | Error表示文字列へのfragment期待固有文型 |
| 負の近似tolerance | `TestNegativeTolerance(tolerance: Float)` | `invalid negative tolerance: #{tolerance}` |
| 近似値不一致 | `TestApproxMismatch(expected: Float, actual: Float, tolerance: Float)` | 近似比較固有文型 |
| kind不一致 | `TestErrorKindMismatch(expected: String, actual: String)` | kind期待の固有文型。入力Stringは静的markerを解決したkindと実kind |
| kind要求でOk | `TestExpectedErrorKind(expected: String, actual: String)` | Errのkind期待固有文型 |
| cause chain不一致 | `TestCauseChainMismatch(expected: List<String>, actual: List<String>)` | 定義側で最初の不一致index／長さ差を求めて表示 |
| cause chain要求でOk | `TestExpectedCauseChain(expected: List<String>)` | chain期待とOk結果の固有文型 |

doc plain／ANSI、stdout／stderrなど、既存で `assert_eq` へ委譲するAPIは、等値不一致という同じ検査条件を保持する。各失敗分岐が完成messageを作って渡す `_fail(detail)` は残さず、対応定義を呼ぶ。Test::assert／failの利用者noteは説明そのものを指定する既存APIであり、生成側が内部データから完成messageを作る経路とは区別する。専用定義がnoteをそのまま表示することを選び、空Stringも保持する（test.srt:162,474）。意味のない固定接頭辞を追加して既存の説明を変えない。

Testの失敗分類には現行で `TestAssertionFailed` のkind完全一致が使われる（V:118）。分割後は宣言identityのknown setまたはgroup metadataで新しいassertion定義群を分類する。名前のsuffixやmessage文言を推測するfallbackを追加しない。ユーザー定義Errorを名前の似かたでassertion扱いしない。

## 実装前に残る確認と受入条件

各最終署名はここに記載した発生条件を基準とする。検索未検出の利用は実装前の確認項目である。Regexの入力取得とgroupの原因区別、File handleのpath／mode取得は現行データから可能であり、上記の受渡し経路へ変更する。条件を確認できないまま自由なdetail入口を恒久化したり、存在しないデータを空値で埋めたりしない。

1. 標準51定義について、現行入力と新入力・保存Payloadの対応を追える。保持／分割の行を抜かさず、テスト内の独自deferrorを標準定義数へ混ぜない。保存Payload名に禁止名 `kind`・`message` を使わない。
2. 保持する固定Errorは空Payload、構造化するErrorは表のフィールド名・型・順序を保存する。外部コンストラクタ入力をそのまま保存する場合も明示的に構築する。
3. 細分化する旧Error名を新Errorへの汎用入口として残さない。呼出し元、戻り値の補助表記、Pattern、assertion、`@doc`と関連文書を追従する。
4. 生成側の完成message組立てを除去し、OS／parser原文detailと構造化データだけを定義へ渡す。definitionとruntimeでmessageテンプレートを二重に持たない。
5. 今回確認した言語レベルの失敗とRust内部不整合を区別し、後者を新しい標準Errorへ変換しない。未確認の到達可能性は確認事項として残す。
6. 仕様作成段階ではビルド・テストを実行しない。後続実装では対象条件の成功・拒否・保存値・表示・発生位置を検証し、Error Payload変更の全体検証に含める。



## Rust・VMからの実行時生成一覧

生成 helper の kind 指定と分岐から43の直接 runtime kind、MakeError のコンパイラ出力専用3 kind（`EmptyList`、`VariantMismatch`、`InvalidMatchResult`）、計46 kindを確認した。下表は72の用途行へ分けている。同一kindの複数用途は別行にし、内部不整合として移す現行生成も省略していない。標準51定義と34 kindが重複し、現行の対象は計63の定義名・生成kindである。標準にない12 kindは追加定義または内部不整合への分離が必要である。

現行の直接生成入口は意味入力を完成 String へ変換して `builtin_rich_error`（B:4919）、`process_error`（V:2835）、または `MakeError`（V:5430）へ渡す。これらに渡るのは入力署名を検査した `.srt` 呼出しではなく、kindとmessage／diagnosticの部品である。**全行で現在の保存Payloadはない**。意味入力欄は生成直前のコードで利用可能な値を示し、現在保存している値とは区別する。

「標準表 #」の最終署名は前半の同じ番号の行と分割節を使う。末尾のコンパイラ生成行は後続のForge用途表を使う。runtime専用の定義は直後の表で確定する。

| 現行kind | 発生用途 | 根拠 | 現行srt定義・入力 | 生成直前の意味入力 → 現行入口 | 最終対応 |
|---|---|---|---|---|---|
| ZeroDivisionError | Int / Float safe_div、Int safe_mod / SafeModInt | `B:1572` / `B:1596` / `V:4827` | `lib/bootstrap.srt:366` `0引数` | 生成前:a,b/数値種別。入口:固定messageのみ | 標準表 #2 |
| NegativeShiftCount | shl / shr の負数、正のusize非表現値（builtin と Opcode の両方） | `B:1994` / `B:2009` / `V:4838` / `V:4850` | `lib/types/int.srt:19` `(bits: Int)` | bits:Int。to_usize失敗を全てnegative扱い。入口:完成message | 標準表 #8 |
| NegativeBitIndex | test_bit / set_bit / clear_bit / toggle_bit、builtin bit_index_to_usizeとOpcode | `B:4107` / `V:4862` / `V:4882` / `V:4901` / `V:4920` | `lib/types/int.srt:27` `(index: Int)` | index:Int。入口:完成message | 標準表 #9 |
| InvalidStringEncoding | ASCII encodeで非ASCII文字 | `B:2195` | `lib/types/string.srt:57` `(detail: String)` | encoding, idx, ch。入口:完成message | 標準表 #16 |
| InvalidStringEncoding | from_codepointsの負数又はu32非表現値 | `B:2224` | `lib/types/string.srt:57` `(detail: String)` | encoding,idx,code:Int。入口:完成message | 標準表 #16 |
| InvalidStringEncoding | from_codepointsの255/127超過 | `B:2239` | `lib/types/string.srt:57` `(detail: String)` | idx,raw,encoding。入口:完成message | 標準表 #16 |
| InvalidStringEncoding | from_codepointsのinvalid / incomplete UTF-8 | `B:2248` | `lib/types/string.srt:57` `(detail: String)` | valid_up_to, error_len:Option<usize>。入口:完成detail | 標準表 #16 |
| TestAssertionFailed | assert_err_kindで別kind | `B:2333` | `lib/test.srt:4` `(detail: String)` | expectedkind,actualkind。入口:完成message | 標準表 #51 |
| TestAssertionFailed | assert_err_kindでOk | `B:2338` | `lib/test.srt:4` `(detail: String)` | expectedkind,value。入口:expectedとinspect(value)の完成message | 標準表 #51 |
| TestAssertionFailed | assert_cause_chainでOk・順序違い・長さ違い | `B:2349` | `lib/test.srt:4` `(detail: String)` | expected/actual List<String>, mismatch index。入口:完成message | 標準表 #51 |
| IndexOutOfBounds | Facet List view/setのindex不正 | `B:2618` / `B:2683` / `B:2700` | `lib/bootstrap.srt:388` `(detail: String)` | index:Int,len:usize。入口:完成message | 標準表 #5 |
| IndexOutOfBounds | Facet List rangeのendpoint不正 / start>end | `B:2655` / `B:2729` / `B:2758` | `lib/bootstrap.srt:388` `(detail: String)` | start,end,len。入口:endpointはindex用message、逆順range用message | 標準表 #5 |
| KeyNotFound | Facet HashMap get/set | `B:2679` / `B:2767` / `B:2775` | `lib/bootstrap.srt:395` `(key: String)` | key:String。入口:完成message | 標準表 #6 |
| RegexCompileError | Regex::compile | `B:2842` | `lib/types/regex.srt:23` `(detail: String)` | pattern:String,regex::Error。入口:err.to_stringのみ | 標準表 #27 |
| NoneError | HashMap::map_getの欠損 | `B:2585` | `lib/bootstrap.srt:357` `0引数` | map,key。入口:固定message | 標準表 #1 |
| NoneError | Regex::captures / findで不一致 | `B:2863` / `B:2941` | `lib/bootstrap.srt:357` `0引数` | pattern,input。入口:固定message | 標準表 #1 |
| NoneError | RegexCaptures::getのidx非表現/範囲外/未参加capture | `B:2909` | `lib/bootstrap.srt:357` `0引数` | index,groups.len,matched Option。入口:固定message | 標準表 #1 |
| NoneError | RegexCaptures::get_nameの名前未定義/未参加 | `B:2925` | `lib/bootstrap.srt:357` `0引数` | name,index,groups。入口:固定message | 標準表 #1 |
| InvalidRandomRange | Random int_until/int_range/next_int_until/next_int_range | `B:3437` / `B:3453` / `B:3792` / `B:3814` | `lib/Random.srt:12` `(start: Int, end: Int)` | start,end:Intをhelperまで保持。入口:完成message | 標準表 #28 |
| PatternMismatch | unconsの空List/空String、MatchResult::Errとして生成 | `B:4550` / `B:4574` | なし | source値とList/String種別。入口:固定message | Uncons専用定義表 |
| InputError | IO::get/get_line EOF | `B:3064` / `B:3089` | `lib/IO.srt:5` `(detail: String)` | read operation。入口:end of inputだけ | 標準表 #29 |
| InputError | IO prompt書込み失敗 | `B:3066` / `B:3091` / `B:3469` | `lib/IO.srt:5` `(detail: String)` | prompt,host io error。入口:prompt write failed: ...という完成message | 標準表 #29 |
| InputError | char/line stdin読込み失敗 | `B:3085` / `B:3107` / `B:3521` | `lib/IO.srt:5` `(detail: String)` | read operation,host error。入口:err文字列 | 標準表 #29 |
| InputError | stdin char UTF-8不正/途中EOF | `B:3533` / `B:3540` | `lib/IO.srt:5` `(detail: String)` | UTF-8 valid_up_to/error_len/途中byte列。入口:Stringのみ | 標準表 #29 |
| InputError | terminal raw mode有効/無効/イベント読取り失敗、terminal-io未搭載、非対応key | `B:3485` / `B:3502` / `B:3507` | `lib/IO.srt:5` `(detail: String)` | phase,host error,key code。入口:Stringへまとめる | 標準表 #29 |
| JsonParseError | 最外構文parseとnested変換parse | `B:4263` / `B:4280` / `B:4363` | `lib/types/json.srt:21` `(line: Int, column: Int, detail: String)` | line,column,detailは途中であるがJsonParseConversionError::Invalid(String)に潰す | 標準表 #24 |
| JsonParseError | nested array/object深さ>=127 | `B:4328` | `lib/types/json.srt:21` `(line: Int, column: Int, detail: String)` | source offset,depth,127。入口:完成message | JsonParseDepthLimitExceeded（追加定義表） |
| JsonEncodeError | JSON整数がi64/u64のいずれにも収まらない | `B:4391` / `B:4503` | `lib/types/json.srt:35` `(detail: String)` | BigInt value。入口:Recoverable(String)完成detail | 標準表 #26 |
| JsonEncodeError | 非finiteFloat | `B:4427` / `B:4503` | `lib/types/json.srt:35` `(detail: String)` | NaN/Inf。入口:固定detail | 標準表 #26 |
| JsonEncodeError | JsonValueではないruntime値 | `B:4490` / `B:4503` | `lib/types/json.srt:35` `(detail: String)` | other:Value。入口:debug文字列 | 標準表 #26 |
| FileNotFound | File path操作 read/write/delete、append/openのhost failure | `B:4843` / `B:3111` / `B:3120` / `B:3130` / `B:3151` / `B:3160` | `lib/file.srt:15` `(path: String)` | path,io::Error。入口:kind+完成messageのみ | 標準表 #30 |
| FilePermissionDenied | File path操作 read/write/delete、append/openのhost failure | `B:4844` / `B:3111` / `B:3120` / `B:3130` / `B:3151` / `B:3160` | `lib/file.srt:19` `(path: String)` | path,io::Error。入口:kind+完成messageのみ | 標準表 #31 |
| FileAlreadyExists | File path操作 read/write/delete、append/openのhost failure | `B:4845` / `B:3111` / `B:3120` / `B:3130` / `B:3151` / `B:3160` | `lib/file.srt:23` `(path: String)` | path,io::Error。入口:kind+完成messageのみ | 標準表 #32 |
| FileInvalidPath | File path操作 read/write/delete、append/openのhost failure | `B:4846` / `B:3111` / `B:3120` / `B:3130` / `B:3151` / `B:3160` | `lib/file.srt:27` `(path: String)` | path,io::Error。入口:kind+完成messageのみ | 標準表 #33 |
| FileEncodingError | File path操作 read/write/delete、append/openのhost failure | `B:4847` / `B:3111` / `B:3120` / `B:3130` / `B:3151` / `B:3160` | `lib/file.srt:35` `(detail: String)` | path,io::Error。入口:kind+完成messageのみ | 標準表 #35 |
| FileIoError | File path操作 read/write/delete、append/openのhost failure | `B:4848` / `B:3111` / `B:3120` / `B:3130` / `B:3151` / `B:3160` | `lib/file.srt:39` `(detail: String)` | path,io::Error。入口:kind+完成messageのみ | 標準表 #36 |
| FileClosed | read_chunk/write_chunk/flush、およびwith_open callback後close | `B:4902` / `V:4083` / `V:4101` / `V:4123` / `V:4132` | `lib/file.srt:31` `0引数` | handle_id,operation。入口:固定message | 標準表 #34 |
| FileEncodingError | read_chunkのinvalid leading byte/UTF-8 sequence、path read invaliddata | `B:4910` / `V:6031` | `lib/file.srt:35` `(detail: String)` | read内部first byte,utf8error、path readはio::Error。入口:Encoding(String) | 標準表 #35 |
| FileIoError | chunk read/write/flush/closeのhost failure、wrong mode | `B:4907` / `B:4911` / `V:4083` / `V:4101` / `V:4123` / `V:4132` | `lib/file.srt:39` `(detail: String)` | path Option,handle_id,mode/operationは上流にある。入口:Io(ioErr)又はMessage(String) | 標準表 #36 |
| FileSystemNotFound | FS mkdir/mkdir_all/rm/mv/cp/stat、ls/treeのmetadata/read_dir/entry読込み | `B:4863` / `B:3287` / `B:3296` / `B:3305` / `B:3319` / `B:3328` / `B:4659` / `B:4718` | `lib/FileSystem.srt:105` `(path: String)` | raw path,io::Error。mv/cpのtoは生成helperで喪失。入口:完成message | 標準表 #37 |
| FileSystemPermissionDenied | FS mkdir/mkdir_all/rm/mv/cp/stat、ls/treeのmetadata/read_dir/entry読込み | `B:4864` / `B:3287` / `B:3296` / `B:3305` / `B:3319` / `B:3328` / `B:4659` / `B:4718` | `lib/FileSystem.srt:113` `(path: String)` | raw path,io::Error。mv/cpのtoは生成helperで喪失。入口:完成message | 標準表 #39 |
| FileSystemAlreadyExists | FS mkdir/mkdir_all/rm/mv/cp/stat、ls/treeのmetadata/read_dir/entry読込み | `B:4865` / `B:3287` / `B:3296` / `B:3305` / `B:3319` / `B:3328` / `B:4659` / `B:4718` | `lib/FileSystem.srt:109` `(path: String)` | raw path,io::Error。mv/cpのtoは生成helperで喪失。入口:完成message | 標準表 #38 |
| FileSystemInvalidPath | FS mkdir/mkdir_all/rm/mv/cp/stat、ls/treeのmetadata/read_dir/entry読込み | `B:4866` / `B:3287` / `B:3296` / `B:3305` / `B:3319` / `B:3328` / `B:4659` / `B:4718` | `lib/FileSystem.srt:125` `(path: String)` | raw path,io::Error。mv/cpのtoは生成helperで喪失。入口:完成message | 標準表 #42 |
| FileSystemIoError | FS mkdir/mkdir_all/rm/mv/cp/stat、ls/treeのmetadata/read_dir/entry読込み | `B:4867` / `B:3287` / `B:3296` / `B:3305` / `B:3319` / `B:3328` / `B:4659` / `B:4718` | `lib/FileSystem.srt:137` `(detail: String)` | raw path,io::Error。mv/cpのtoは生成helperで喪失。入口:完成message | 標準表 #45 |
| FileSystemInvalidPath | parentのparent無し / nameのUTF-8名無し | `B:3222` / `B:3233` | `lib/FileSystem.srt:125` `(path: String)` | path、file_name OptionとUTF-8失敗。入口:path→完成message | 標準表 #42 |
| FileSystemInvalidDepth | tree_depthの負depth | `B:3269` | `lib/FileSystem.srt:129` `(depth: Int)` | depth:Int,path。入口:完成message | 標準表 #43 |
| FileSystemNotDirectory | ls/treeのroot.is_dir()==false | `B:4688` | `lib/FileSystem.srt:117` `(path: String)` | root_raw,is_dir false。入口:完成message | 標準表 #40 |
| FileSystemUnsupported | cp対象sourceがdirectory | `B:3328` | `lib/FileSystem.srt:133` `(detail: String)` | from,to。入口:固定message | 標準表 #44 |
| ShellWorkingDirectoryNotFound | Shell::cdで!is_dir | `B:3350` | `lib/Shell.srt:9` `(path: String)` | path。入口:完成message | 標準表 #48 |
| ShellCommandNotFound | exec Command::output NotFound | `B:3377` | `lib/Shell.srt:1` `(command: String)` | command,argv,cwd,io error。入口:完成message | 標準表 #46 |
| ShellSpawnFailed | exec その他host spawn failure | `B:3384` | `lib/Shell.srt:5` `(detail: String)` | command,argv,cwd,err。入口:完成message | 標準表 #47 |
| ShellIoError | exec stdout/stderr UTF-8 decode failure | `B:3392` / `B:3402` | `lib/Shell.srt:17` `(detail: String)` | command,utf8 error。入口:完成message | 標準表 #50 |
| ShellIoError | cd canonicalize failure | `B:4756` | `lib/Shell.srt:17` `(detail: String)` | path,host err。入口:完成message | 標準表 #50 |
| HandlerInitFailed | FileOutHandlerにpath namedarg無し | `V:2068` | なし | target/path presence。入口:固定message | runtime専用定義表 |
| HandlerInitFailed | FileOutHandler open失敗 | `V:2077` | なし | path,io err。入口:完成message | runtime専用定義表 |
| HandlerWriteFailed | FileOutHandler write失敗 | `V:2084` | なし | path,text,io err。入口:完成message | runtime専用定義表 |
| UnknownHandlerTarget | OutHandler target dispatch不明 | `V:2091` | なし | other:String。入口:完成message | runtime専用定義表 |
| SupervisorAdoptForbidden | supervisorのallow_adopt=false | `V:2105` | なし | supervisor_name。入口:完成message | runtime専用定義表 |
| InvalidPid | adopt/state/storeでregistryにpid無し | `V:2111` / `V:2332` / `V:2379` | なし | pid.id,pid.process_name、operation。入口:完成message | runtime専用定義表 |
| InvalidPid | state/storeでprocess spec型名不一致 | `V:2345` / `V:2392` | なし | pid,actual_name,expected process_name。入口:完成message | runtime専用定義表 |
| InvalidPid | workers初期生成/refill continuationの結果がOk(PID/WorkerLease)でない | `V:2639` / `C:339` / `C:384` | なし | value:Value,continuation用途。入口:固定message | runtime専用定義表 |
| SupervisorAdoptInvalidPid | Runnable/WaitingのWorkerでない | `V:2117` | なし | supervisor,pid,process_name,entry.status/spec.instance。入口:完成message | runtime専用定義表 |
| ProcessStateUnavailable | process_stateでmaterialized state無し | `V:2362` | なし | pid.id、entry state。入口:完成message | runtime専用定義表 |
| InvalidWorkerStrategy | runtime schema/tag/field/type/representable i64不整合 | `V:2256` / `V:2280` / `V:2299` / `C:1157` | なし | valueからtypedfieldへdecode途中、Err:String。入口:messageのみ | runtime専用定義表 |
| InvalidWorkerStrategy | bounds条件init==target,0<=min<=target<=max違反 | `C:1165` | なし | init,min,max,target:i64。入口:固定message | runtime専用定義表 |
| Timeout | future deadline終了（task/genserver等のfuture用途） | `V:2871` / `V:2907` | なし | future_id,timeout deadline,owner用途。入口:future idだけ含む完成message | runtime専用定義表 |
| ProcessDown | target停止前reply無し、pending future resolve | `V:2898` / `V:2517` | なし | future_id,pid,process state/stopreason上流。入口:pid入り完成message | runtime専用定義表 |
| EmptyList | Forgeが生成したList/String head-tail Pattern失敗をMakeErrorが実行 | `F:10034` / `V:5430` | `lib/bootstrap.srt:373` `0引数` | input_sourceはstatic、元Pattern span。入口:固定message+diagnostic | Forge用途表 |
| IndexOutOfBounds | Forgeが生成したfixed List length失敗をMakeErrorが実行 | `F:10151` / `F:10169` / `V:5430` | `lib/bootstrap.srt:388` `(detail: String)` | expectedlen,actual_lenを元は持つ。入口:完成message+diagnostic | Forge用途表 |
| PatternMismatch | Forge literal/general mismatchをMakeErrorが実行 | `F:10066` / `F:10105` / `V:5471` | なし | literal lhs display,rhs inspect / generic rule、inputsource。入口:rhs String+diagnostic又は固定message | Forge用途表 |
| VariantMismatch | Forge Facet enum variant mismatchをMakeErrorが実行 | `F:9889` / `V:5430` | `lib/bootstrap.srt:402` `(detail: String)` | detailはForge側のvariant用途表示。入口:完成detail | Forge用途表 |
| InvalidMatchResult | Forgeが未知MatchResult tag failureをMakeErrorで実行しeprint | `F:11510` / `V:5430` | なし | invalid runtime outcome。入口:固定message | Forge用途表 |

`FileSystemIsDirectory` は B:4885 の message 分岐と標準定義を確認したが、それを選ぶ製品呼出しは検索で未検出である。上記46 kindへ加えず、標準表 #41 の定義として移行する。`ShellUnsupported` も標準表 #49 の扱いとする。存在する定義と現在使われる生成箇所の数を混同しない。

## runtime専用・追加定義の最終契約

以下は全て新しい `.srt` 定義を必要とする。`I=P` の行は入力と同じ名前・型・順序を保存する。内部不整合の行は言語Payloadを持たず、Rust RuntimeErrorの診断へ移す。process専用定義は `lib/Process.srt`、Uncons定義は `lib/kernel.srt`、深さ制限は `lib/types/json.srt` に置く。PID／futureの内部IDを保存する行では、内部の固定幅IDを通常のSurtr `Int`値へ明示的に変換する。内部ID表現をユーザーIntの制約として導入しない。

| 現行kind・用途 | 最終定義・入力署名 | 保存Payload | 定義側の文型・処置 |
|---|---|---|---|
| PatternMismatch・空List uncons | `UnconsEmptyList()` | `[]` | `cannot uncons empty list`。MatchResult::Errを生成 |
| PatternMismatch・空String uncons | `UnconsEmptyString()` | `[]` | `cannot uncons empty string`。MatchResult::Errを生成 |
| JsonParseError・深さ制限 | `JsonParseDepthLimitExceeded(line: Int, column: Int, depth: Int, limit: Int)` | I=P | `json parse depth #{depth} reached the limit #{limit} at #{line}:#{column}`。現行の `depth >= 127` を維持 |
| HandlerInitFailed・path namedarg無し | Rust RuntimeError | 対象外 | FileOutHandler metadataの不整合。空pathで救済しない |
| HandlerInitFailed・FileOutHandler open失敗 | `FileOutHandlerOpenFailed(path: String, detail: String)` | I=P | `FileOutHandler open failed for #{path}: #{detail}` |
| HandlerWriteFailed・FileOutHandler write失敗 | `FileOutHandlerWriteFailed(path: String, detail: String)` | I=P | `FileOutHandler write failed for #{path}: #{detail}` |
| UnknownHandlerTarget・未知target | Rust RuntimeError | 対象外 | OutHandler metadataの不整合 |
| SupervisorAdoptForbidden | `SupervisorAdoptForbidden(supervisor: String)` | I=P | `#{supervisor} does not allow adopt` |
| InvalidPid・adoptの未登録PID | `SupervisorAdoptUnknownPid(pid: Int, process: String, supervisor: String)` | I=P | `supervisor #{supervisor} cannot adopt unknown pid #{pid} for #{process}` |
| InvalidPid・stateの未登録PID | `ProcessStateUnknownPid(pid: Int, process: String)` | I=P | `state requested for unknown pid #{pid} for #{process}` |
| InvalidPid・storeの未登録PID | `ProcessStoreUnknownPid(pid: Int, process: String)` | I=P | `store requested for unknown pid #{pid} for #{process}` |
| InvalidPid・stateのPID型不一致 | `ProcessStatePidTypeMismatch(pid: Int, actual: String, expected: String)` | I=P | `state pid #{pid} belongs to #{actual}, not #{expected}` |
| InvalidPid・storeのPID型不一致 | `ProcessStorePidTypeMismatch(pid: Int, actual: String, expected: String)` | I=P | `store pid #{pid} belongs to #{actual}, not #{expected}` |
| InvalidPid・workers初期生成のPID結果なし | `WorkerCreationExpectedPid(actual: String)` | I=P | `worker creation expected Ok(PID(...)), got #{actual}`。元値のinspect部品を消費前に取得 |
| InvalidPid・refillのPID結果なし | `WorkerRefillExpectedPid(actual: String)` | I=P | `worker refill expected Ok(PID(...)), got #{actual}` |
| SupervisorAdoptInvalidPid・Workerでない | `SupervisorAdoptNonWorker(supervisor: String, pid: Int, process: String)` | I=P | `supervisor #{supervisor} cannot adopt non-Worker pid #{pid} (#{process})` |
| SupervisorAdoptInvalidPid・Workerがliveでない | `SupervisorAdoptWorkerNotLive(supervisor: String, pid: Int, process: String)` | I=P | `supervisor #{supervisor} cannot adopt non-live Worker pid #{pid} (#{process})` |
| ProcessStateUnavailable | `ProcessStateUnavailable(pid: Int, process: String)` | I=P | `pid #{pid} (#{process}) has no materialized state`。standby init未完了は既存RuntimeError |
| InvalidWorkerStrategy・schema／tag／field／型の破損 | Rust RuntimeError | 対象外 | 型検査済み表現の内部不整合。欠損specも同様 |
| InvalidWorkerStrategy・正規Intのi64非表現 | `WorkerStrategyFieldOutOfRange(field: String, value: Int)` | I=P | `worker strategy field #{field} exceeds runtime limit: #{value}`。fieldはinit／min／max／Fix sizeの対象を識別 |
| InvalidWorkerStrategy・initとtarget不一致 | `WorkerStrategyInitTargetMismatch(init: Int, target: Int)` | I=P | `worker strategy init #{init} differs from target #{target}` |
| InvalidWorkerStrategy・bounds条件違反 | `WorkerStrategyBoundsInvalid(min: Int, target: Int, max: Int)` | I=P | `worker strategy bounds must satisfy 0 <= #{min} <= #{target} <= #{max}` |
| Timeout・future deadline終了 | `FutureDeadlineExceeded(future: Int)` | I=P | `future #{future} timed out` |
| ProcessDown・reply前にtarget停止 | `ProcessReplyTargetStopped(future: Int, pid: Int)` | I=P | `target process #{pid} stopped before replying to future #{future}` |

worker生成結果の `InvalidPid` は、現行helperが元のResultを消費して置換する経路も含む。今回はその既存結果・cause規則を変えず、表示に必要な元値を消費前に取得する。型検査された戻り値の形状破損が確定する枝は内部不整合として分け、利用者の元Errまで一律にRuntimeErrorへ変えない。

Futureの元timeout量とtask／callの用途は現在の保存契約に揃っていない。今回の最終定義は確実に得られるfuture IDだけを保存する `FutureDeadlineExceeded` とし、架空のtimeout値・空の用途を追加しない。別用途へのさらなる分割には元contextの追加仕様が必要であり、本移行の前提にしない。

## Forgeが出力するError生成コード

コンパイル時にSurtrランタイムを動かす処理ではなく、実行時に不一致分岐からErrorコンストラクタを呼ぶコードが対象である。全 `deferror` を命令化する入口（F:7813）と、定義を通らない直生成入口（F:10444）を区別する。直生成入口の現行kindは5種、用途は次の9行である。

| ID | 発生条件・使用位置 | 現行kind / message | 現行の生成入力と保存情報 | 根拠 |
|---|---|---|---|---|
| F01 | Listの `[head, ..tail]` が空入力に不一致。保持consumerの SafeBind、partial `<-`、`apply_pattern` | `EmptyList` / `Empty List.` | 定義入力なし。Forgeが固定messageとruleを作り、diagnostic.input_source=`List`。元Listも長さもPayloadとして保存しない | `crates/forge/src/codegen.rs:10034`、同`:10080`、同`:11148` |
| F02 | 固定長List Patternより入力が短い | `IndexOutOfBounds` / `LHS.len(N) > RHS.len(M)` | expected length Nと、空tailが判明した位置Mは分岐コード内の定数。完成messageに埋込み、長さは保存しない | 同`:10151`、同`:10698`–10706 |
| F03 | 固定長List Patternより入力が長い | `IndexOutOfBounds` / `LHS.len(N) < RHS.len(M)` | Nと、実行時 `ListLen(remainder) + N` のMをForge命令で文字列化・連結。長さは保存しない | 同`:10169`–10225、同`:10708` |
| F04 | Int / String / Boolean / Duration literalと入力が異なる | `PatternMismatch` / `Pattern did not match.` | lhsはcompile-time display文字列。rhsは実行時 `inspect(input)`。diagnosticにlhs/rhsの文字列を保存するが、元の型付き値は保存しない | 同`:10105`–10148、同`:13002`、`crates/eldr/src/vm.rs:5471` |
| F05 | pinがEqで不一致 | `PatternMismatch` / `Pattern did not match.` | 期待値と実際値のlocal slotは存在するが、generic failure emitterには渡さない。diagnostic.rule=`pattern must match the SafeBind input`、input_sourceなし | `crates/forge/src/codegen.rs:10752`、同`:10094` |
| F06 | Resultまたは一般Enum constructorのtagが異なる | `PatternMismatch` / `Pattern did not match.` | typed Patternにexpected tag、入力localにactual valueがある。完成messageは汎用固定文。tag/variant名は保存しない | 同`:10830`–10841、同`:10094` |
| F07 | Facetの必須variant segmentの読み取りが不一致 | `VariantMismatch` / `Variant mismatch at segment N (.Variant) in facet path: expected variant Enum::Variant, but got a different variant` | compile-time segment番号・display・enum名・expected variant名からForgeが完成detailを作る。actual tagは比較するだけで保存しない | 同`:9778`–9816、同`:9889` |
| F08 | Facet更新の必須variant segmentが不一致 | `VariantMismatch` / F07と同じ形式 | F07と同様。optional segmentの不一致はそのsegmentをskipし、ここでErrorを生成しない | 同`:9545`–9557、同`:9889` |
| F09 | Extractor / ExtractorClosureがcanonical OK/Err以外のMatchResult tagを返す | `InvalidMatchResult` / `Extractor returned an unknown MatchResult tag.` | canonical success/err tagと実際のtagを比較するが、actual tagを保存しない。言語ErrorをeprintしてHaltする内部protocol違反 | 同`:11477`–11505、同`:11510`–11527 |


| 対応 | 最終具象名 | コンストラクタ入力署名 | 保存Payload | 定義側messageテンプレート |
|---|---|---|---|---|
| F01 | `EmptyHeadTailListPattern` | `()` | 空 | `head-tail list pattern requires a non-empty List` |
| F02 | `ListPatternTooShort` | `(expected_length: Int, actual_length: Int)` | 同名・同型の2field | `LHS.len(#{expected_length}) > RHS.len(#{actual_length})` |
| F03 | `ListPatternTooLong` | `(expected_length: Int, actual_length: Int)` | 同名・同型の2field | `LHS.len(#{expected_length}) < RHS.len(#{actual_length})` |
| F04 Int | `IntLiteralPatternMismatch` | `(expected: Int, actual: Int)` | 同名・同型の2field | `Int literal pattern #{expected} did not match #{actual}` |
| F04 String（Stringの `[]` を含む） | `StringLiteralPatternMismatch` | `(expected: String, actual: String)` | 同名・同型の2field | `String literal pattern #{inspect(expected)} did not match #{inspect(actual)}`。引用・escapeは通常の表示APIを定義本文で使う |
| F04 Boolean | `BooleanLiteralPatternMismatch` | `(expected: Boolean, actual: Boolean)` | 同名・同型の2field | `Boolean literal pattern #{expected} did not match #{actual}` |
| F04 Duration | `DurationLiteralPatternMismatch` | `(expected: Duration, actual: Duration)` | 同名・同型の2field | `Duration literal pattern #{expected} did not match #{actual}`。比較時の内部millisecondsをErrorの公開Intへ混同しない |
| F05 | `PinnedValuePatternMismatch` | `(value_type: String, expected_repr: String, actual_repr: String)` | 同名・同型の3field | `pinned #{value_type} value #{expected_repr} did not match #{actual_repr}`。任意型のpin値を固定スキーマへ保存するため、inspect表現という必要文字列を保持する |
| F06 Result | `ResultVariantPatternMismatch` | `(expected_variant: String, actual_variant: String)` | 同名・同型の2field | `Result pattern expected #{expected_variant}, got #{actual_variant}` |
| F06一般Enum | `EnumVariantPatternMismatch` | `(enum_name: String, expected_variant: String, actual_variant: String)` | 同名・同型の3field | `#{enum_name} pattern expected #{expected_variant}, got #{actual_variant}`。内部tag値ではなく宣言metadataからのvariant名を渡す |
| F07 | `FacetReadVariantMismatch` | `(segment_index: Int, segment: String, enum_name: String, expected_variant: String, actual_variant: String)` | 同名・同型の5field | `Facet read at segment #{segment_index} (#{segment}) expected #{enum_name}::#{expected_variant}, got #{actual_variant}` |
| F08 | `FacetUpdateVariantMismatch` | `(segment_index: Int, segment: String, enum_name: String, expected_variant: String, actual_variant: String)` | 同名・同型の5field | `Facet update at segment #{segment_index} (#{segment}) expected #{enum_name}::#{expected_variant}, got #{actual_variant}` |
| F09 | 言語deferrorを新設せず、内部 `RuntimeError` | expected OK/Err tagとactual tagを内部検証へ渡す | 言語Payloadの対象外 | 内部protocol違反の診断。現行 `InvalidMatchResult` の言語Error/eprint/Halt経路を削除する。新しいrecoverable failureへ変えない |


空List、短い／長い固定List、literal、pin、Result／Enumのvariant、Facet read／updateを別定義へ分ける。literalは型付きの期待値と実値、pinは任意型を無理に新しいgeneric Errorへせずtype名とinspect部品を保存する。variantの実名は比較時の値と既存宣言metadataから取得する実装変更であり、現在すでに保存されていると扱わない。

Stringのhead-tailは現行ScarでKernel::uncons Extractorへlowerされる（`crates/scar/src/checker/patterns.rs:1200`）。Forgeに残るString用EmptyList枝から新しい失敗定義を増やさず、Uncons専用定義へ移す。Record外枠の不正tag、literal display欠損、未知MatchResult tagは内部不整合としてRuntimeErrorへ分離する。子Patternの失敗を親の汎用PatternMismatchへ置き換えず、既存ExtractorのErrを保持する。

literalの現行MakeErrorでは、VMが固定messageを作り、スタックから得たinspect文字列をdiagnostic.rhsへ格納する（V:5471–5485）。Forgeの文型だけでなく、このVM側の固定messageも定義側へ移す。diagnosticのlhs／rhs／rule／input_sourceはstructured factsとして保持し、messageテンプレートと混同しない。

## 生成・伝播の実装方針と移行単位

1. コンパイラ出力コードは解決済み宣言identity・入力署名を用いて呼出し命令を生成する。builtinとVMも同じ定義参照・署名検査を使い、失敗分岐で `.srt` 本体を一度実行する。Rust helperからVMを同期再帰駆動せず、既存のCall／Resumeと継続処理へ載せる。未定義kindの直接生成fallbackは残さない。
2. JSON変換、VmFileError、worker strategy decode、IO helperが現在Stringへ潰す情報は、型付きの内部失敗データに残す。生成側は発生条件を選び、deferror側がmessageとPayloadを構築する。完成文を逆parseして引数を復元しない。
3. Error生成元のsource ID／spanをコンストラクタ実行へ渡す。`deferror` 宣言や内部Selfの位置を主キャプションにしない。call-site、Pattern、Facet、module、REPL、builtin／processの既存contextを維持する。cause、diagnostic、stack traceの統合は共通生成契約で行う。
4. Result wrap、map_err／cause／recover、GenServer／Worker stopの既存Error、ExtractorのErrは生成と運搬を区別する。移送・再格納・失敗伝播でPayloadを破棄しない。Alternativeの不一致やbranch consumerでは、従来不要なErrorを新たに生成しない。
5. 新規deferrorの配置と名前変更は、標準宣言・全呼出し・Result補助表記・Pattern・recover_kind・Testのkind判定・期待値・正本文書を同じ移行単位に含める。旧名を汎用入口や別名として常設しない。

| 段階 | 本一覧から適用する内容 |
|---|---|
| 0 | 本一覧の現行入力・発生条件・最終署名を実装入力とする。検索未検出の定義とfixture／exampleの独自Errorは別に利用確認 |
| 1 | 全実行時生成を定義へ接続。現行srt定義なしkindも宣言を追加。内部不整合を分離し、一時message責務を追跡 |
| 2 | 全標準・fixture／test宣言を新構文と単一Payload表現へ追従。旧ヘッダ解釈を削除 |
| 3 | bootstrap、List／Option／HashMapの欠落、Uncons、Forge Pattern／Facetの最終定義を適用 |
| 4 | Int、String、Duration、Generator、Regex、JSONを原因別に移行 |
| 5 | IO、File、FileSystem、Shell、Randomを操作・原因ごとに移行 |
| 6 | ProcessとTest、fixture／example、kind参照・残った生成テンプレートを移行 |
| 7 | 全体のPayload保持・位置・公開契約を統一し、旧生成経路と未移行項目を除去 |

## 調査の網羅性と未検証範囲

RichErrorの新規生成、kind分岐、MakeError、下位helperと継続を追跡した。`builtin/{generator,list_flat_map,map_values,list_builder}.rs`、`vm/{continuation,runtime_table,test_runner}.rs` の製品部分に追加の独立kind生成はない。`builtin/map_values.rs:89` はcfg(test)内の擬似Errorであり、本番一覧から除いた。File、FileSystem、Shell、JSONの処理は主にB／V内にある。Rune／Xldrの表示・decode・テスト用Error作成は新規の製品失敗kindではない。

この一覧はソースの静的追跡に基づく。OS失敗の実際の再現、新コンストラクタ呼出し、Payload保存、変更後表示・ソース位置は未検証である。製品コード・実行可能テストは変更せず、ビルド・コンパイラテスト・Surtr実行は行っていない。
