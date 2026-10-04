# Eldr VM 仕様書

> Surtr の実行層仕様。実装詳細（Rust の構造体定義や補助関数）はソースを正とし、
> 本書は VM の意味論と外部契約のみを定義する。

---

## 1. 目的と責務

Eldr は Surtr の Bytecode 実行エンジンであり、以下を担う。

- 命令列の逐次実行
- スタック/ローカル/関数テーブルの管理
- 組込み関数呼び出し
- ランタイムエラーの検出と停止
- 開発観測用の execution stats / trace 収集（有効時のみ）

Eldr は次を担わない。

- 構文解析（Spire）
- 名前解決（Sigil）
- 型検査（Scar）
- コード生成（Forge）

File / FS の host filesystem surface は `lib/File.srt` と `lib/FileSystem.srt` の
標準 module を正本とし、VM はその lower 先 builtin を実行する。path は実行時の
current working directory 基準で解決し、存在しない path や open/read/write failure は
`RuntimeError` ではなく user-facing `Result` の domain error として返す。

---

## 2. Bytecode 成果物

### 2.1 `Bytecode`

- ファイル実行と `.eldr` 入出力に使う完全な実行単位
- `opcodes`, `constants`, `type_registry`, `error_templates`, `functions` を持つ
- `source_map` は `Option<SourceMap>` で付与する
- `docs` は `@doc` 由来の symbol metadata を保持する
- `runtime_process_specs` と `runtime_boot_plan` は、compiler が
  process surface と `supervisor_init` を正規化した VM 入力である。
  surface 文法の正本は [ProcessRuntime spec](./ProcessRuntime_spec.md) とする。

### 2.2 `BytecodeChunk`

- REPL 増分実行に使う差分単位
- opcode は chunk 単位で生成される
- `const_base`, `type_registry_base`, `error_template_base`, `dbg_template_base` を持ち、VM 側で現在状態と照合してから絶対 index へ再配置する
- `type_registry_base` は chunk 生成時点の VM-wide TypeRegistry entry 数であり、preload/project chunk が古い registry 前提で append されることを禁止する

---

## 3. 実行モデル

### 3.1 構成

- Operand Stack
- Locals（現在フレームのローカルスロット）
- Call Frames（関数呼び出し境界）
- Constants（定数プール）
- Functions（関数テーブル）
- TypeRegistry（型タグ逆引き）

### 3.2 呼び出し規約

- 呼び出し時、実引数は stack から取り出し `locals[0..arity)` に配置する
- `Callable` は `lexical_captures` を保持する
- 関数呼び出し時、`locals` には `lexical_captures` → 実引数 の順で先頭から配置する
- `Call` 実行後は、呼び出し先がフレーム完成状態で開始する
- `FunctionEntry.arity`、`Call` / `CallClosure` / `CallBuiltin` の arity、closure capture 数、error template 引数数、`Dbg` 引数数は bytecode 上 `u8` であり、最大 255 とする。Forge は宣言・呼出し・生成 wrapper の隠れた引数を含め、表現できない個数を `CodegenError` で拒否し、切り詰めて命令を作らない
- tail-position は、現在の関数 / closure / extractor / process handler の返り値そのものになる式位置を指す
- user function への tail-position call は、次 opcode が `Return` の場合に限り current `CallFrame` を再利用してよい
- 対象は direct `Call` と、target が user function の `CallClosure` / `TailCallClosure` に限る
- builtin / template target の tail-position call は user-function TCO ではなく、必要なら圧縮 opcode の実行規約として扱う
- frame 再利用時も外部意味は変わらず、返り値 1 個・呼び出し元への復帰位置・operand stack 契約は維持する

### 3.3 返り値

- 関数は 1 値を返す
- 呼び出し元には返り値 1 つのみが push される
- tail call が最適化された場合、途中フレームの `Return` は省略されうるが、観測上は最終返り値だけが呼び出し元へ渡る

標準 `Result` / `Boolean` の constructor、capture、Pattern は通常 Enum の解決済み variant metadata を受け取る。
Result の runtime tag は Ok=0 / Err=1、Boolean は既存の primitive Boolean 表現を維持する。
これらの tag は compiler symbol UID や bytecode `fun_idx` の割り当てには使わない。
compiler symbol と関数は各登録処理の追加順で採番し、builtin runtime ID は callable metadata から参照する。
alias に追加の関数枠を作らず、生成する constructor capture / wrapper / closure も通常の関数 allocator を使う。

Extractor の返却は canonical `MatchResult::Ok` / `MatchResult::Err` の enum 表現を使う。
field 0 は variant discriminant、field 1 は payload とし、Err payload は runtime Error 値である。
`GetTag` / `GetField` は canonical MatchResult の field 数・discriminant・Err payload を検査し、
不正な表現を VM error にする。`Kernel::uncons` は通常 builtin として List / String を分解し、
空入力の PatternMismatch Error と元の source location を返す。Scar までに List と String の
concrete 静的契約を選択済みとし、Eldr が generic target や Union の型判定を提供するものではない。
同じ runtime primitive を共有しても consumer 側は型検査済み contract を受け取り、元 Error を保持または破棄する。

### 3.4 関数テーブル不変条件

- `fun_idx` は実行時の関数テーブル添字と一致する（`functions[fun_idx as usize]`）
- 欠番は作らない（holes 禁止）
- `FunctionEntry` の整列後は `entry.fun_idx == index` を満たす
- VM はこの不変条件を前提に O(1) 参照し、破綻時は `RuntimeError` とする

### 3.5 REPL 増分実行（`push_chunk`）契約

- 公開境界は batch 実行用の `VM` と、REPL/対話実行用の `InteractiveVm` に分ける
- `VM` は完全な `Bytecode` の `run()` と opcode / process runtime 実行を担う
- `InteractiveVm` は `VM` を内包し、`BytecodeChunk` の原子的 append 実行、interactive policy 検証、REPL host I/O buffering、`last_result`、`.eldr` 保存用 `snapshot_bytecode()` を担う
- Xldr は source-level REPL policy を持つが、Eldr は `SourceKind::ReplChunk` や暗黙モジュールを解釈しない
- `InteractiveVm` の公開 API は `push_chunk(chunk, policy)` の 1 入口とし、policy は少なくとも `ReplAppendOnly` と `Preload` を持つ
- `InteractiveChunkPolicy` は Eldr 内で `RuntimeAppendPolicy` に写像し、関数 table prefix、type registry entry、runtime process metadata、boot plan の append 可否だけを扱う。これは staged compile prefix/suffix aggregate や compile-space symbol capability とは別の runtime 境界である
- `BytecodeChunk` の `LoadConst` / `MakeError` は chunk-local index で生成される
- `push_chunk()` は `const_base` / `error_template_base` / `dbg_template_base` により絶対 index へ再配置する
- `push_chunk()` は jump 先も append 後の opcode 位置へ再配置する
- `chunk.const_base` / `chunk.type_registry_base` / `chunk.error_template_base` / `chunk.dbg_template_base` が VM の現在プール長と一致しない場合は `RuntimeError` とする
- Forge の chunk codegen は top-level 末尾へ必ず `Halt` を 1 つ挿入する
- top-level 実行は append された `code_base` から開始し、最初の `Halt` で停止する
- 関数本体は top-level `Halt` 後ろに配置され、top-level からは到達不能であり、`Call` / `CallClosure` でのみ到達する
- 実装は VM 全体 clone ではなく、append した bytecode 断片と実行時状態の checkpoint / rollback で原子性を保つ
- `InteractiveVm::push_chunk()` の返り値は `ChunkExecution` とし、chunk 実行終了時点の stack top 1 値を `value` に保持する。stack が空なら `Unit` を返す
- `last_result` はユーザー言語の通常 binding ではなく、直近の batch 実行または committed chunk の結果を保持する REPL-facing session property とする
- `push_chunk()` 完了後、VM の operand stack は空に戻す。REPL は前回 chunk の stack 内容を次回 chunk へ持ち越さない
- `push_chunk()` は chunk 実行を原子的に扱い、失敗時は VM 状態を更新しない
- `policy = ReplAppendOnly` のとき、`InteractiveVm` は公開 REPL 境界として append-only function table を強制し、`fun_idx < current_function_len` の `FunctionEntry` を含む chunk を拒否する
- `policy = ReplAppendOnly` のとき、`type_entries`、`runtime_process_specs`、`runtime_boot_plan` の追加を拒否する
- `policy = Preload` のとき、Xldr が構築した preload/bootstrap chunk を live REPL 開始前に適用できる
- `fun_idx` は VM 内の物理 slot であり、言語上の安定 identity ではない。Forge/Eldr は shared verifier により function table prefix 不変、dense append、function ref 範囲を検査する
- rollback 対象は bytecode append 分、function overwrite、locals、operand stack、call frames、pc、process runtime、exit code、標準 I/O / REPL host I/O / test event cursor、`last_result` とする

### 3.6 トップレベル名衝突ポリシー（コンパイラ契約）

- 同一モジュール（REPL セッションを含む）で、トップレベル定義名の重複は禁止する
- 対象: `def` / `defstruct` / `defrecord` / `deferror` / `deftrait`
- 本規約はファイル実行と REPL で同一に適用する

### 3.7 Process Runtime 入力契約

Eldr は `defagent`、`defgenserver`、`defsupervisor`、`supervisor_init` などの
surface syntax を直接読まない。Compiler はこれらを immutable な
`RuntimeProcessSpec` table と `RuntimeBootPlan` に正規化してから VM に渡す。
`Bytecode` はこの正規化結果を `runtime_process_specs` と `runtime_boot_plan`
として保持し、VM は surface syntax や source-level boot DSL を再解釈しない。

VM 側の責務は次の通り。

- `RuntimeProcessSpec` に基づく process instance / singleton slot の管理
- `RuntimeBootPlan` に基づく standard singleton と user singleton の boot
- `RuntimeHandlerSpec` に基づく Agent / GenServer / Supervisor / Task handler dispatch
- Standby init の retry、deadline、Ready 待ち caller の管理
- `Process::sleep`、Task、call timeout の scheduler-backed waiting / wakeup
- process-local handler dependency (`ctx.<slot>`) の runtime context 解決
- `StdIn` / `StdOut` / `StdErr` builtin handler と handler override の適用
- observability / snapshot 用に singleton slot、process table、waiting state、
  deadline queue などの runtime state を正規化済み VM 構造として保持する

`Process::sleep(duration)` は host thread 全体を block せず、呼び出した process だけを
`Waiting(Timer)` に移す。Ready 前の process への call は Ready 待ちに入り、
call timeout は Ready 待ち時間を含む。

標準 I/O は VM 内部の stdout/stderr/stdin バッファへ直接触る契約ではなく、
`StdIn` / `StdOut` / `StdErr` builtin handler への message call として扱う。
Rust tests と Pure Surtr `Test` DSL は、この handler backend を差し替えて同じ
buffer semantics を観測できなければならない。

### 3.8 Step / ExecutionContext / quantum

VM の互換 entrypoint は引き続き `VM::run()` / `InteractiveVm::push_chunk()` だが、
内部実行は `ExecutionContext` を介した step 単位に分ける。

- `ExecutionContext` は `pc`、operand stack、call frames、実行 target と、未完了の builtin / callback の継続状態を持つ。
- `VM` は bytecode、constant/function/type table、boot plan、process runtime、
  I/O、observer、file resource を所有し続ける。
- `step_context(ctx)` は `ctx.pc` の opcode 1 個、またはそれに相当する小さな VM 実行単位だけを進める。
- batch / REPL の外部 API は結果まで待つ。内部では共通の予算付き driver を使い、Task / process を含む他の runnable な処理へ実行を切り替える。
- `run_quantum(ctx, budget)` は reduction budget が切れた時点で scheduler 境界へ戻る。
- opcode 1 個につき 1 reduction とする。tail-call frame reuse も `Call` opcode の step として 1 reduction を消費する。builtin の再開は有界の状態遷移単位で課金し、callback の opcode と同じ予算を使う。予算 0 では進めず、新しい予算は scheduler が実行対象を選び直したときに与える。
- `StepOutcome::Pending` は future id と resume 用 `ExecutionContext` を保持する。

### 3.9 Builtin / callback の継続実行

`HashMap::map_values` は builtin として元 map の全キーを保ち、決定的なキー順で各値へ callback を一度ずつ適用する。空 map では callback を呼ばない。map の内部異常と callback の RuntimeError は伝播し、部分 map を成功として返さない。callback が返す言語の Result 値（`Err` を含む）はそのまま新しい値として格納する。

builtin の内部結果は、完了、継続可能、callback 要求、Future 待機、RuntimeError を区別する。
継続状態は VM の実行コンテキストが所有し、利用者の `Value` や bytecode に格納しない。
即時完了する builtin も同じ dispatch へ接続する。

- direct / closure / tail closure / partial / inject / compose と、Task / process handler は共通の実行契約を使う。
- callback 要求では親の復帰先を保存し、子を通常の VM engine で進める。子の完了結果は親へ一度だけ渡す。引数評価や callback を再開時にやり直さない。
- 予算切れは Runnable として保存し、Future 待機は Waiting として保存する。CPU の yield に Future を作らず、待機中の再実行や queue の二重登録を許さない。
- 実行中の状態は切替え時に移動する。REPL checkpoint は独立した保存状態を持ち、rollback はその位置へ戻す。新規継続や失敗した chunk の進捗を破棄し、保存状態を二重実行しない。外部 I/O の巻き戻しは保証しない。
- `__recover_kind` や `file_with_open` など、callback 後に処理がある wrapper は後処理も継続状態に保持する。ファイルは中断中も開いたまま保持し、完了・失敗時に所定の flush / close を一度だけ行う。
- ファイルの所有権を含む継続は、開始直後の中断や timeout による取消でも後処理を失わない。取消中に後処理が Result のエラーを返しても、利用者 callback の後続命令は再開しない。
- REPL checkpoint は開いているファイル資源も保持する。失敗した chunk が既存 handle を閉じた場合、rollback は保存した資源から handle の対応を復元する。パスを開き直したり、ファイルを切り詰めたり、OS の読み書き位置を巻き戻したりしない。
- batch のトップレベルが完了しても background task が残る間はファイル資源を保持し、background task の終了後に shutdown を行う。
- 失敗時は未完了状態を破棄し、呼出し元の source location と call trace を保つ。欠落した復帰先などの不正状態は RuntimeError とする。

利用者 callback を同期ループで完走させる旧経路は残さない。入力サイズに比例する既存の純粋な Rust loop や、regex / JSON / OS I/O の一回の外部呼出しの分割は別途扱う。
要素の clone / drop と allocator の時間も reduction の実時間上限には含めない。
この契約は VM 全体の実時間の公平性上限を保証しない。

---

### 3.10 Test のケース実行

VM の実行時設定が test / describe / it の名前フィルター、一覧、xit の有効化、時間計測を保持する。
スコープは Test / Describe の種類と名前を保持し、ケースは It / Xit / Pend の宣言種別、ファイル内の宣言順 index、理由、選択状態を保持する。
不正な種類・空白だけの理由・実行中ケース内のケース宣言は VM 実行異常とする。理由はフィルター判定前に検証する。

ケース開始処理は選択を判定し、非実行ケースを Filtered / Skipped / Pending / Runnable として記録する。
実行ケースは一つの active case とし、本文の Ok / Err を Passed / Failed として確定する。
VM 実行異常で中断した active case も一件の Failed に確定する。スコープの Err は ScopeFailed として別に記録する。

実行するケースの開始時だけ stdout / stderr / stdin を分離し、終了時に走査側のバッファへ戻す。
非実行ケースは IO を初期化・消費しない。走査出力をケース出力に含めない。
計測を指定した実行ケースだけ単調時計の経過 ns を保持する。それ以外は未計測とする。
選択・表示の設定は VM の実行時設定であり、コンパイルキャッシュのキーに加えない。

## 4. Value モデル

Eldr が扱う値の概念カテゴリ:

- プリミティブ: `Int`, `Float`, `String`, `Boolean`, `Unit`
- コンテナ: `List`, `HashMap`
- opaque runtime 値: `Regex`, `RegexCaptures`, `RegexMatch`, `RandomGenerator`, `Generator`, `InfiniteGenerator`
- opaque runtime 値として見える `Duration` は source 上では private field を持つ struct として扱い、表示は `100ms` 形式にする
- タグ付き値: `Tagged { tag, fields }`
- runtime 内部 tag 値: `Tag(u32)`（user-visible `Int` と分離）
- 呼び出し可能値: `Callable`
- 言語エラー値: `Error(RichError)`
- process capability: `PID`（runtime が発行する opaque handle）

値の表示と `inspect` は、未知の tag、reserved Result（tag 0/1）の payload 数不一致、既知の struct / record / enum variant のフィールド数不一致を RuntimeError とする。enum variant の先頭フィールドは Int の discriminant を必須とし、runtime のフィールド数は payload を記録する `TypeEntry.field_names` の数に 1 を足した値とする。比較 builtin が生成する `Ordering` も通常の enum 表現に従い、Less / Equal / Greater はそれぞれ discriminant 0 / 1 / 2 を持つ。未知 tag の `Tagged(...)` 表示、欠損 payload の `Ok()` 表示、空文字による救済は行わない。正規の reserved Result は registry entry がなくても表示できる。
List / tuple / HashMap / tagged value のフィールドも再帰的に検証し、内部値の表示失敗を保持する。Sindr の表示エラーは Eldr の境界で RuntimeError に変換する。`print` / `inspect` / REPL / `dbg!` / runtime snapshot の呼出し側まで伝播し、言語の `Err` 値や panic で代用しない。

`inspect` における `Callable` 表示は runtime metadata に従い、closure は
`Closure(sig)`、capture は `FnCapture(module: M, name: f, sig: sig)` を返す。
callable 値の signature の `Result` は `Result<T>`、ExtractorClosure 内の `MatchResult` は `MatchResult<T>` と表示する。Error 位置は値の表示に含めない。ExtractorClosure は `ExtractorClosure<(A -> MatchResult<P>)>` と表示する。REPL の束縛表示も同じ inspect 処理を使い、単独評価やコンテナ内の表示と揃える。定義に明記された Error 名を表示するのは、REPL コマンドによる定義シグネチャの照会である。
所属 namespace を保ちながら暗黙の `Global::` を省略する。
この表示規則はコンテナ内の callable にも再帰的に適用する。
部分適用した capture とそれを変数経由で再 capture した callable は capture の由来を保ち、
残りの引数 signature を表示する。signature は共有関数宣言ではなく、callable value の生成位置で
解決済みの callable 型から得る。closure literal で包んだ callable は `Closure(sig)` とする。
List / HashMap / tuple / tagged value の payload と field でも同じ callable 表示を再帰適用する。
Callable 自身の runtime metadata を origin と signature の正本とし、closure body、parameter 名、lexical
capture から Capture / Closure origin を推測しない。metadata を復元できない user-facing callable は汎用表示とし、
内部の function / template / builtin ID を出力しない。詳細な callable 生成契約と受入 inventory は
[`callable-display-origin-spec.md`](../../doc/callable-display-origin-spec.md) を参照。
直接 binding と nested value は同一の runtime metadata を使い、REPL binding metadata が表示 origin を上書きしない。
演算子 capture は lower 後の対応 Trait method の module / name を表示する。二要素 tuple の ``&`(,)` `` は
`Bootstrap` / `(,)` を表示し、どちらも capture 作成位置で解決した実際の callable signature を使う。
Facet API capture は module を `Facet`、name を対象 API 名、sig を型検査で確定した関数型とする。
`&Type.path`、`&p`、`_.path` の読み取りも `view` の同じ metadata を使う。たとえば
`FnCapture(module: Facet, name: view, sig: (Duration -> Int))` と表示する。
通常の closure literal は既存どおり `Closure(sig)` とする。capture できない `compose` や演算子を
表示のために許可しない。
`to_string` は文字列値を引用せず、`inspect` は文字列literalとして引用する。
Sindrの `quote_surtr_string_literal` を共通引用処理とし、`\\`・`\"`・`\n`・`\t`、
その他のC0・DEL・C1制御文字の `\u{...}`、文字としての `#{` の `\#{` を使う。
Unicodeエスケープは小文字16進数・不要な先頭ゼロなしとする。String単体の引用表示を通常式として
再評価すると元の値になることを保証する。List・Tuple・Result・struct内のStringとHashMapキーにも
同じ引用処理を適用する。構造体の構築経路やError表示など、inspect全体のソース化は保証しない。
`print(String)` の明示的な生出力は維持する。`eprint(String)` は `inspect` と同じ引用表示を使う。入力との対応は
[文字列リテラルの実装契約](./String_literal_spec.md#共通の引用表示)を参照する。

### 4.1 RichError

`RichError` は次を保持する。

- `kind`
- `message`
- `location`
- `cause: Option<RichError>`

`cause` は runtime 管理の線形 chain とする。

`location` は Error を生成したソース位置を保持し、主キャプションもこの位置を使う。
明示的な Error の構築はその構築式、構文 Pattern の不一致は失敗した子 Pattern、
構造自体の不一致はその構造 Pattern を指す。既存 Error の carrier への格納や伝播で位置を更新しない。
新しい Error による wrap は、新しい Error の構築位置と元 Error の cause を保持する。
`stack_trace` は呼出し経路の追跡用であり、先頭 frame の位置を Error の生成位置として使わない。
Source map から確定した位置を受け渡し、表示文や Error 名から位置を推測しない。

compile / surface 契約との対応は次のとおり。

- source に現れる `Error` は abstract failure view であり、runtime 実体は常に concrete `deferror` 由来の `RichError`
- user code は `Error` を一般の first-class data として保持しない
- `Error` が surface 上で生存するのは `Err(Error)`、`match` の `Err(err)` で束縛された局所スコープ、標準定義ソース内の `Error` 観測 helper の引数位置に限る
- `Result::map_err` / `Result::cause` / `require` / `ensure` は、この既存 `Error` 値を forward してよい
- `Result::recover_kind` の marker は標準引数専用の `ErrorKind` とし、具体的な `deferror` 型名だけを受ける。Sigil は修飾名を含む canonical 型 identity を確定し、Forge はその `fq_name` を静的 metadata から hidden builtin `__recover_kind` へ渡す。Eldr は内部 ABI の kind 文字列を照合して handler を呼び出す。marker は Error の生成や constructor 呼び出しを行わず、利用者が任意の文字列を渡せる surface は提供しない。
- `Test::assert_err_kind(marker, result)` も同じ ErrorKind の宣言 identity 解決・検証を使う。Err の kind が一致すれば `Ok(())`、Ok または異種の Err なら `TestAssertionFailed`。payload や表示文字列は比較しない。marker を一般の値として束縛・転送する能力は追加しない。
- `Test::assert_cause_chain(expected, result)` は最外側の Err から cause へ辿る kind の列を、静的な宣言 identity の列と比較する。順序・長さ・重複を含め完全一致なら `Ok(())`、不一致・Ok・空の期待列なら `TestAssertionFailed`。payload・message・位置は比較せず、result は一度だけ評価する。診断は期待列・実際列と、最初の不一致位置（0始まり）または長さの違いを含める。Forge は列を hidden builtin の `List<String>` metadata へ lower する。
- Lazyの正規化とeager入力の評価順は[Lazy spec](Lazy_spec.md)に従う。VMへLazy markerは渡さず、確定した分岐と通常call命令を実行する。branchをruntime callableとして表す場合も呼び出しは一回とし、戻り値がcallableでも追加で実行しない。

- parallel error は持たない
- `Result::cause(result, err)` は `err` chain の末尾に既存 error chain を付ける
- `Result::chain(head, tail)` は右 error chain の末尾に左 error chain を付ける
- `Result::map_err(result, err)` は既存 error chain を捨てて `err` chain で置き換える

表示契約は次で固定する。

- `inspect(Error)` は head-first tree 表示を返す。Error 自体には Show を提供せず、`to_string(Error)` は許可しない
- 先頭行は `Kind("message")`
- cause がある場合、次行以降を `|_ ...` でネスト表示する
- `inspect(Err(...))` も同じ tree を使うが、先頭行だけ `Err(...)` で包む
- `inspect(Struct)` は `Type(field: value, ...)` を返し、内部専用の `Type { ... }` 構造体リテラルは表示しない。`to_string(Struct)` はその型の明示的な Show または derive の契約に従う
- `inspect` は再帰的に string literal を quote し、標準の `Show for String` は素の string 値を返す
- 構造体の inspect は private を含む全フィールドの名前と値を定義順に表示し、入れ子でも同じ規則を使う。呼び出し側のスコープや Show の有無で分岐せず、Show を自動で呼び出さない
- `private_flags` はアクセス検査用の可視性情報として保持し、表示の省略判定には使わない。private は値の秘匿を保証せず、inspect の String はフィールドへの操作権限を含まない
- `to_string` は Show の契約に従う。Show 不足は型検査で拒否し、型検査後の不正な内部状態も inspect や汎用表示へのフォールバックで隠さない。標準型の明示的な Show と契約を保つ最適化は維持し、手書きの Show が明示的に inspect を呼ぶことは許可する
- `inspect(HashMap)` は生成可能な literal 形式 `hash!["key" => value, ...]` で、空 map は `hash![]`、key は `String` literal と同じ escaping で表示する
- `inspect(HashMap)` / `map_keys` / `map_values_list` はキー昇順の deterministic order を使う
- `eprint(Error)` は先頭行を `Error: Kind: message`、以降を `Caused by: Kind: message` で出力する
- `Error::kind(Error)` は `RichError.kind`、`Error::message(Error)` は `RichError.message` を `String` として返す
- `Error::same_kind(Error, Error)` は先頭の具象 `RichError.kind` だけを比較する。`Result` の `Err` 同士の `Eq` が利用し、message・cause・location・診断情報は判定に含めない。壊れた Error 表現や非 Error 値を `False` にせず runtime invariant failure とする
- `Error::format(Error)` は `eprint(Error)` と同じ行列を stderr へ出さず、`\n` join した `String` として返す

### 4.2 List の内部表現と flat_map

公開型は常に `List<$A>` とし、runtime は空の `Empty`、先頭と tail を持つ `Cons`、
共有 buffer と開始位置を持つ `Packed` を使う。`ListHandle` の表現と長さは private field とし、
利用側は `len`、`head_value`、`tail_handle`、`iter` を通して論理順の要素を読む。

- 空 List は `Empty` にそろえる。非空の `from_items(Vec<Value>)` は buffer の所有権を移して `Packed` を作る。
- `cons` は tail の表現を保持したまま一つの `Cons` を作る。Packed の tail は buffer を共有し、開始位置だけを進める。
- `len` は保存した長さを返す。長さと開始位置の加減算は checked API を使い、overflow や不正な状態を補正しない。
- `len == 0` と Empty は同値。Cons の長さは tail の長さ + 1、Packed は `offset < items.len()` かつ `len == items.len() - offset` を満たす。非空の表現だけが Cons / Packed になる。
- equality、iteration、display、inspect、Debug、List pattern は論理順を使い、内部表現を表示しない。
- List は immutable である。process の payload でも backing storage を共有でき、受信側から送信側の List を変更できない。

`List::flat_map(values, f)` は通常の `CallBuiltin(list_flat_map)` で実行する。
継続は入力 cursor、mapper、出力 cursor、`ListBuilder` を所有する。入力一要素の処理と出力一要素の追加で
共通予算を消費し、callback の命令も同じ予算で進める。mapper は入力順に一度だけ呼ぶ。
mapper の結果が List でない不正な bytecode は RuntimeError とし、後続の mapper を実行しない。
完了時にだけ Builder の buffer を ListHandle へ移す。中断状態や Builder は利用者の Value に追加しない。
REPL checkpoint は Builder を独立に保存するが、通常の実行切替えでは所有権を移し、複製しない。

要素操作を単位とする仮想計算量では、`cons` / `uncons` / `len` / `head` / `tail` は `O(1)`、
全走査は `O(n)`、`append` は左辺の長さに比例する。flat_map は入力長を N、mapper が返す全要素数を M とすると
`O(N + M + callback のコスト)` である。display と equality は要素の比較・表示のコストを別に加算する。
`Vec::push` は償却 O(1) であり、再確保を含む一回の処理の実時間上限を意味しない。

`Monad::bind` の source 定義と generic do lowering は維持し、各段は完成した List を返す。
全 bind の入力訪問数の合計を B、出力要素数の合計を E とすると、構造的な処理量は
`O(B + E)` に callback のコストを加えたものになる。flatten の構築は旧実装の `2E` 個の Cons から
`E` 回の Builder 追加と各 bind 一回の finish へ減る。
右辺や通常の束縛に別の bind / do を含まず、後続 continuation として直列に入れ子になった
k 個の bind が末尾の M 要素を順に平坦化して返す場合、E = kM になる。
一般の nested do は B / E で評価し、常に E = kM と仮定しない。
三段の各入力が10要素、末尾が singleton なら B = 1110、M = 1000、E = 3000 となる。
この構築操作の削減は、多段に残る kM の計算量クラスを変えない。

これらは要素の物理的な clone / drop、allocator、実時間、RSS の改善を保証しない。
Packed の tail は参照中の buffer 全体を保持し、処理済みの先頭部分を自動で縮めない。
最後の参照の破棄では元の buffer 全体を解放し得る。Cons の長い鎖の反復解放と、
checkpoint の一般的なコピー削減も未実装である。

---

### 4.3 Generator と InfiniteGenerator

公開型は `Generator<Item>` と `InfiniteGenerator<Item>` とし、内部 state の型を公開型引数に含めない。
有限の producer は `Unfold { state, step }`、整数・文字の range、`Terminal` を持つ。
無限の producer は `Unfold { state, step }` を持つ。両方とも生成と List の取得を担当し、取得後の変換・選別・集計は List モジュールに任せる。

- 有限の unfold step は `State -> Option<(Item, State)>`。`None` だけが正常終端で、item 自体の `Result` / `Option` はデータとして保つ。
- private な有限 pull builtin `gen_step` は `Option<(Item, Generator<Item>)>` を返す。公開 `Generator::next` は標準定義でこれを `Ok((item, rest))` / `Err(NoneError)` に変換する。step の終端 protocol と公開 API の戻り値は別の契約とする。
- 無限の unfold step は `State -> (Item, State)`。`InfiniteGenerator::next` は `(Item, InfiniteGenerator<Item>)` を直接返す。
- handle は不変で、構築や opaque 表示では step を呼ばない。進行は返された rest を使う。同じ handle を新しい呼出しで再利用すると、保存 state から再評価する。
- take 系は `(List<Item>, rest)` を返す。非正 count は無評価で、要求件数の次を先読みしない。有限終端を実際に観測した rest は Terminal とし、公開 next は callback を呼ばず `Err(NoneError)` を返す。
- 条件停止した item は List に含めず、その生成前の handle を rest とする。再利用ではその位置を再評価する。step の RuntimeError や不正 carrier を終端や途中までの成功 List に変換しない。
- callback、条件判定、List への一件追加は共通の builtin continuation と VM 予算で進める。一回の取得の中断・再開で callback を重複実行しない。通常の切替えでは状態を移動し、checkpoint は独立した状態を保持する。
- materialize は flat_map と同じ private `ListBuilder` を使い、完了時だけ buffer を ListHandle に移す。非空は既存の Packed、空は Empty になる。巨大な count の初期予約容量を制限しても、BigInt の要求件数と結果は切り詰めない。

公開 API、生成・取得、入力検証エラーの契約は `lib/types/generator.srt` と `lib/types/infinite_generator.srt` の `@doc`、
利用例は [Generator](../site/generator.md) を参照する。専用 Opcode、公開 mutable cursor、新しい serialization / equality 契約は追加しない。

---

## 5. 命令体系

Opcode は以下のカテゴリを持つ。

- 定数/ローカル操作（Load/Store）
- 算術・比較・bitwise
- 文字列結合
- 文字列分解
- リスト/タグ付き値操作
- 呼び出し（`Call`, `CallClosure`, `CallBuiltin`）
- callable metadata 操作（`SetCallableSignature`, `SetCallableOriginSource`, `SetCallableDelegateFunction`）
- 制御フロー（`Jump`, `JumpIf*`）
- スタック操作（`Pop`）
- 関数復帰（`Return`）
- 停止（`Halt`）

補足:

- `CallBuiltin` は `builtin_id` ベースでディスパッチする
- `BitNotInt` / `BitAndInt` / `BitOrInt` / `BitXorInt` / `ShlInt` / `ShrInt` / `TestBitInt` / `SetBitInt` / `ClearBitInt` / `ToggleBitInt` は `Int::bit_not` / `bit_and` / `bit_or` / `bit_xor` / `shl` / `shr` / `test_bit` / `set_bit` / `clear_bit` / `toggle_bit` の direct call を対象にした monomorphic fast-path とする
- `ShlInt` / `ShrInt` は負 shift count を `RuntimeError` ではなく `Err(NegativeShiftCount(...))` の `Result` 値として返す
- `TestBitInt` / `SetBitInt` / `ClearBitInt` / `ToggleBitInt` は負 bit index を `RuntimeError` ではなく `Err(NegativeBitIndex(...))` の `Result` 値として返す
- `StoreConstLocal { const_idx, local_idx }` は `LoadConst(const_idx); StoreLocal(local_idx)` と同じ意味の圧縮 opcode とする。operand stack へ中間値を push せず、定数値を現在フレームの local slot に直接保存する。`const_idx` は `LoadConst` と同じ relocation / verifier 規則に従う
- `CopyLocal { src_local_idx, dst_local_idx }` は `LoadLocal(src_local_idx); StoreLocal(dst_local_idx)` と同じ意味の圧縮 opcode とする。operand stack を経由せず、現在フレーム内で local 値を clone して保存する
- `EqLocalTag { local_idx, tag_const_idx }` は `LoadLocal(local_idx); GetTag; LoadConst(tag_const_idx); EqTag` と同じ意味の圧縮 opcode とする。`tag_const_idx` は `Constant::Tag` を指し、`LoadConst` と同じ relocation / verifier 規則に従う
- `EqPid` は 2 つの `Value::Pid` を消費する。登録済みの同一 process type であることを検査し、Singleton は常に等しく、Worker は instance ID で比較する。未登録・異種 process type・非 PID 値は runtime invariant failure とする。`!=` は結果に `NotBool` を適用する
- `MakeOk` は stack top の payload を `Tagged { tag: 0, fields: [payload] }` に包む Result 専用 constructor opcode とする
- `MakeErr` は stack top の `Error` payload を `Tagged { tag: 1, fields: [payload] }` に包む Result 専用 constructor opcode とする。payload が `Error` でない bytecode は runtime error とする
- `JumpIfLocalTagEq { local_idx, tag_const_idx, target_pc }` と `JumpIfLocalTagNe { local_idx, tag_const_idx, target_pc }` は `EqLocalTag` の直後に続く `JumpIfTrue` / `JumpIfFalse` を 1 opcode に畳み込む branch-fused fast-path とする。どちらも判定後の operand stack に Bool 中間値を残さない
- `JumpIfLocalTagEq` / `JumpIfLocalTagNe` の `tag_const_idx` は `Constant::Tag` を指し、`LoadConst` と同じ relocation / verifier 規則に従う。`target_pc` は `Jump*` と同じ jump-target verifier / relocation 規則に従う
- `TailCallClosure { arity, span_start, span_end }` は tail position の `CallClosure { arity, span_start, span_end }; Return` と同じ意味の圧縮 opcode とする。callable / argument / lexical capture の評価規約は `CallClosure` と同じで、結果は現在フレームの呼び出し元へ直接返る。target が user function の場合だけ user-function TCO として `tail_calls_optimized` を増やす。builtin / template target は圧縮実行として現在 frame の caller へ返るが、user-function TCO 観測値には含めない
- `SetCallableSignature(signature)` は stack top の `Callable` に capture site で解決済みの signature を設定し、既存 origin と canonical identity を保つ。stack top が `Callable` でなければ runtime error とする
- `SetCallableOriginSource(capture_index)` は stack top の `Callable` に、Callable 値である lexical capture の index を明示する。renderer はこの link がある場合だけ origin / identity を参照し、単に lexical capture が存在することから origin を推測しない。index が範囲外、または Callable でなければ runtime error とする
- `SetCallableDelegateFunction(function_index)` は stack top の生成 Callable に直接委譲する user function index を記録し、process initializer の実体を追跡する。Callable の display origin は変えず、stack top が `Callable` でなければ runtime error とする

実 opcode 一覧とオペランドは `crates/sindr/src/ir.rs` の `Opcode` を正とする。
`crates/forge/src/opcode.rs` は Forge 側の再エクスポート層であり、定義の正本ではない。

---

## 6. エラー体系

### 6.1 種別

- `RuntimeError`: VM 不正状態または実行不能状態（継続不能）
- `Value::Error`: 言語レベルの失敗値（`Result<T>` のデータ）

### 6.2 不正状態の扱い

次は即時 `RuntimeError` とする。

- stack underflow
- invalid jump（PC 範囲外）
- unknown function index
- locals 範囲外アクセス
- invalid tag
- top-level `Return`
- `RuntimeBootPlan` と singleton slot の不整合
- process init timeout (`ProcessInitTimeout`)
- process init が `Err` を返した場合の init failure (`ProcessInitFailed`)
- handler init failure
- handler write / read が VM 継続不能な形で失敗した場合

`Value::Error` は正常なデータフローであり、`RuntimeError` と混同しない。

---

## 7. 組込み関数と型情報

- 組込み関数メタデータは単一テーブルで管理する
- `Bootstrap` module の `@builtin` 宣言はこの共有テーブルに対応する宣言層であり、builtin の追加起点ではない
- VM は `builtin_id` により実装関数をディスパッチする
- `Facet<K, S, A, T, B>` は compile-time capability であり runtime value を持たない。`Facet::view` / `Facet::preview` / `Facet::put` / `Facet::set` / `Facet::over` / `Facet::over_result` / `Facet::case_set` / `Facet::case_over` / `Facet::compose` / Facet `->` 合成 は compile-time lowering 対象で、runtime builtin として直接到達した場合は防御的に `RuntimeError` とする
- Facet API が `Result<S, E>` source を受ける場合、VM は `Err(E)` に対して traversal、rebuild、mapper を実行せず同じ error を返す。これは API-level lift であり Facet slot `S` を `Result<S, E>` に変更しない
- Facet の variant mismatch は `Err(VariantMismatch(detail))` で返し、`detail` には失敗 segment（index と path 表示）を含める
- Facet の fallible container path segment は internal polymorphic helper `__facet_list_get` / `__facet_list_set` / `__facet_map_get` / `__facet_map_set_existing` に lower し、list miss は `IndexOutOfBounds`、map miss は `KeyNotFound` を `Result` で返す
- `eprint` は `Error` 値を診断表示し、それ以外の値は `inspect` 経由で標準エラー出力へ書き出す
- `Error::kind` / `Error::message` / `Error::format` / `Error::same_kind` は `Error` 値を introspection / 表示文字列化・kind 比較する runtime builtin とし、それ以外の値への適用は VM 側ガード対象とする
- `Result::recover` は compiler が lowering する special form であり、runtime builtin としては持たない
- `Int` は `BigInt` を用い、tag/builtin/function ID などの runtime 内部値とは分離する
- `HashMap` の runtime 表現は `HashMap<String, Value>` の immutable map を基準にし、duplicate key 更新時は後勝ちで値を上書きする
- process / task / duration 系の hidden builtin は owner module (`Process`, `Task`, `Duration`) 側の `@hidden @builtin ...` 宣言に対応し、`CallBuiltin` で実装する。VM は process table / PID capability / handler callable invocation を経由する。詳細な process runtime 契約は [ProcessRuntime spec](./ProcessRuntime_spec.md) を正とする。
- `__supervisor_workers` は `(supervisor, worker_init, WorkerStrategy)` を受け取る。Eldr v1 は `WorkerScale::Fix(n)` のみ実行し、`init == n` かつ `0 <= min <= n <= max` を満たさない場合は `Err(InvalidWorkerStrategy)` を返す。
- process runtime snapshot は `worker_sets` を含む。各要素は `id`, `worker_process`, `supervisor`, `target`, `min`, `max`, `member_pids`, `live_count` を持つ。
- `Process::sleep(duration)` は runtime builtin とし、`Duration` 値を受け取って `Result<Unit>` を返す。
- process / workers / task await timeout は `@timeout(100ms)` literal から hidden builtin 呼び出しへ lower し、dynamic timeout は初期フェーズでは許可しない。
- regex 系は Rust `regex` crate のラッパーとして builtin 実装し、regex 未サポート構文は `RegexCompileError` として返す
- `RegexCaptures` の runtime 表現は `groups: Vec<Option<(start, end)>>`, `name_to_index: HashMap<String, usize>`, `input: String` を保持する
- random 系は `CallBuiltin` で実装し、Opcode は追加しない。`RandomGenerator` は opaque な seedable state として保持し、半開区間が空の場合は `InvalidRandomRange` を `Result` の `Err` として返す
- `Float` は finite-only の `f64` ラッパーとして扱う
- VM は `LoadConst`, float arithmetic opcode, float builtin helper, `safe_div`, JSON bridge の各経路で non-finite value を user-visible `Float` として生成しない
- `Float` helper surface では `abs`, `min`, `max`, `floor`, `ceil`, `round`, `trunc`, `pi`, `e` を提供する

`__test_approx_equal` は有限 Float の絶対誤差比較を行う。差の overflow は False とし、非有限値を Surtr の値として返さない。
`Test::assert_approx` が比較結果と負の許容誤差を `TestAssertionFailed` に変換する。

### 7.1 Json builtins

- `Json::parse` は `json_parse` builtin に解決され、`CallBuiltin` で実行される
- `Json::stringify` は `json_stringify` builtin に解決され、`CallBuiltin` で実行される
- Json 用 opcode は追加しない
- Eldr は `serde_json` を使って text JSON と Rust `serde_json::Value` を相互変換する
- Surtr runtime value への変換では `TypeRegistry` から `JsonValue` variant tag を名前で解決し、tag 番号をハードコードしない
- `Object` は `HashMapHandle` に変換する。duplicate key は JSON parser 側の後勝ち値を採用する
- `json_stringify` は `HashMapHandle` の deterministic key order を使って object を出力する
- malformed JSON は `Err(JsonParseError(line, column, detail))` を返し、`RuntimeError` にしない
- `JsonValue` 以外の値が `json_stringify` に渡った場合は `Err(JsonEncodeError(detail))` を返す。`TypeRegistry` 不整合や variant arity 不整合は VM 内部不整合として `RuntimeError` でよい

標準モジュールの inventory、順序、stage 分割は compile 側の
[`STDLIB_MODULE_SPECS`](../../crates/xldr/src/loader.rs) を正本とする。同一 stage 内の import は
file 読み込み順に依存せず compile 側で解決され、later stage 参照は compile error になる。
Eldr は解決済みの bytecode を受け取り、VM 内で追加の import 解決を行わない。

### 7.2 TypeRegistry

- `tag -> 型名/フィールド名` の逆引きを提供
- 表示 (`to_string`) と診断表示で参照される
- 実装は deterministic な entry 列を保持したまま、内部 index により O(1) 相当 lookup を行ってよい
- `Ok=0`, `Err=1` は予約 tag
- `TypeRegistry` mutation は検証 API を経由し、予約 tag、duplicate tag、duplicate type name、`field_names` と `private_flags` の長さ不一致を拒否する
- runtime tag は user-visible `Int` に乗せ替えない

---

## 8. `.eldr` 形式（実行入力）

- マジック: `ELDR`
- ヘッダ: `magic/version/debug_level/num_chunks`
- ヘッダ `version` は現行 `3` とする
- 意味的 bytecode 版は `CInf.bytecode_version` に保持し、現行は `3`
- `.eldr` は単一バイナリ実行物であり、チャンク分割の主目的は実行時ロード都合ではなく viewer / disasm / 診断 / 比較の観測性にある
- 必須チャンク:
  - `Code`
  - `Cnst`
  - `Func`
  - `Type`
  - `ErrT`
  - `DbgT`
  - `CalT`
  - `CInf`
  - `LblT`
  - `ImpT`
  - `ExpT`
  - `LitT`
  - `Line`
  - `SpnT`
  - `SrcP`
  - `PcSp`
  - `Proc`
  - `Boot`
- 任意チャンク:
  - `Docs`
  - `SigT`
- `Code` は opcode 列のみを持つ
- `num_locals` は `CInf` に保持する
- `Cnst` は実行用 constant pool の正本
- `LitT` は viewer / 比較用の literal table
- `Func` は関数境界と viewer 用 flag / span を持つ
- `LblT` / `PcSp` / `Line` / `SpnT` / `SrcP` は viewer 向け索引・source 対応情報である
- `DbgT` は `dbg!` 表示 template、`CalT` は callable template、`Proc` / `Boot` は process runtime metadata を持つ
- `SigT` は REPL / docs 用 signature table であり、存在しない bytecode も受理する

### 8.1 `Func` と `LblT` の役割分離

- `Func` は人間が読む単位であり、関数一覧・関数ビューの正本とする
- `LblT` は制御フロー単位であり、jump target と function entry を `label -> pc` で引くために使う
- viewer は関数一覧から `Func` を起点に表示し、命令列や branch 追跡では `LblT` を補助的に使う

### 8.2 `ImpT` / `ExpT` / `LitT`

- `ImpT` は builtin / function / runtime 呼び出し先を viewer 用に正規化した import table である
- `ExpT` は公開シンボルと function ref の対応を持つ export table である
- `LitT` は `Cnst` の差分と viewer 表示を分離するための literal table である

### 8.3 source 対応

- `Line` は軽量な行ビュー用テーブル
- `SpnT` は span 正本
- `PcSp` は `pc -> span id`
- `SrcP` は path / normalized path / content hash / optional source text を持つ
- `.eldr` の `span_start` / `span_end` と line / column 算出は character offset 契約に従う

module の span は登録済み source ID ごとの範囲へ符号化する。VM は符号化した ID と
`SrcP` の `source_id` を厳密に照合し、該当ファイルのローカル span と行・列へ変換する。
script、include、標準定義、REPL の入力単位を呼出し側の単一 source へ割り当て直さない。
`.eldr` の encode / decode は元の source ID を維持し、ID の欠番を詰めない。
対話 VM はソースを不変の ID で追加登録し、同一 ID の別ファイル・本文への置換を拒否する。
入力の失敗後も診断用ソースを保持し、現在入力の更新で以前の定義位置を変更しない。
`dbg!` は式と各引数の source ID を照合し、該当ソース内のローカル span で描画する。

現行の符号化では、各 source の Unicode scalar value 数を `MODULE_SPAN_STRIDE`（1,000,000）未満に
制限する。CLI は script と include module を含む各 source をコンパイル前に検査し、
この上限以上の入力を `LoadError` で拒否する。他 source の ID と重なる位置を推測して解決しない。

source metadata は省略可能である。符号化した ID の登録がなければ、その ID と元の符号化 span を
保持し、ファイル表示を `<source:ID>`、行・列を 0 にする。符号化した module span に対応する
source text がない場合も行・列は 0 とする。符号化していない span は Error template の保存済み座標を使える。
別ファイルの本文や外側の call span から生成位置を推測しない。
`Line` / `SpnT` / `PcSp` は viewer 用の索引として、この runtime の位置契約と区別する。

### 8.4 `CInf`

`CInf` は少なくとも以下を保持する。

- `bytecode_version`
- `debug_level`
- `num_locals`
- optional compiler / target / build profile
- optional source hash / module hash

詳細なエンコード/デコード仕様は `crates/sindr/src/ir.rs` を正とする。
`crates/forge/src/bytecode.rs` は Forge から使うための re-export 層である。

---

## 9. 将来拡張

- public `VM::step()` / `VMSnapshot`
- Bytecode verifier
- 値表現最適化（clone 削減、共有構造）

補足:

- 開発観測機能は実行意味を変更しない read-only 計測とする
- stats / trace は CLI 等の上位層が opt-in で有効化する

---

*Surtr — 既存の妥協を、型で焼き払う。*
