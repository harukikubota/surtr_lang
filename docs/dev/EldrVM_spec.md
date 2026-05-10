# Eldr VM 仕様書（V9）

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

File v1 の host filesystem surface は `lib/file.srt` の `File` module を正本とし、
VM はその lower 先 builtin を実行する。path は実行時の current working directory
基準で解決し、存在しない path や open/read/write failure は `RuntimeError` ではなく
user-facing `Result` の domain error として返す。

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
- `const_base` と `error_template_base` を持ち、VM 側で絶対 index へ再配置する

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
- user function への tail-position call は、次 opcode が `Return` の場合に限り current `CallFrame` を再利用してよい
- frame 再利用時も外部意味は変わらず、返り値 1 個・呼び出し元への復帰位置・operand stack 契約は維持する

### 3.3 返り値

- 関数は 1 値を返す
- 呼び出し元には返り値 1 つのみが push される
- tail call が最適化された場合、途中フレームの `Return` は省略されうるが、観測上は最終返り値だけが呼び出し元へ渡る

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
- `BytecodeChunk` の `LoadConst` / `MakeError` は chunk-local index で生成される
- `push_chunk()` は `const_base` / `error_template_base` により絶対 index へ再配置する
- `push_chunk()` は jump 先も append 後の opcode 位置へ再配置する
- `chunk.const_base` / `chunk.error_template_base` が VM の現在プール長と一致しない場合は `RuntimeError` とする
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
- Lazy init の retry、deadline、Ready 待ち caller の管理
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

- `ExecutionContext` は `pc`、operand stack、call frames、実行 target を持つ。
- `VM` は bytecode、constant/function/type table、boot plan、process runtime、
  I/O、observer、file resource を所有し続ける。
- `step_context(ctx)` は `ctx.pc` の opcode 1 個、またはそれに相当する小さな VM 実行単位だけを進める。
- `run_until_outcome` は `step_context` の loop として扱い、既存の batch / REPL 契約を保つ。
- `run_quantum(ctx, budget)` は reduction budget が切れた時点で scheduler 境界へ戻る。
- 初期 cost は opcode 1 個につき 1 reduction とする。tail-call frame reuse も `Call` opcode の step として 1 reduction を消費する。
- `StepOutcome::Pending` は future id と resume 用 `ExecutionContext` を保持する。

この段階では user-facing `yield`、新 opcode、bytecode format 変更、builtin continuation
は導入しない。重い builtin はまだ分割不能な 1 step として扱い、後続フェーズで
continuation / dirty worker へ移行する。

---

## 4. Value モデル

Eldr が扱う値の概念カテゴリ:

- プリミティブ: `Int`, `Float`, `String`, `Boolean`, `Unit`
- コンテナ: `List`, `HashMap`
- opaque runtime 値: `Regex`, `RegexCaptures`, `RegexMatch`, `RandomGenerator`
- opaque runtime 値として見える `Duration` は source 上では private field を持つ struct として扱い、表示は `100ms` 形式にする
- タグ付き値: `Tagged { tag, fields }`
- runtime 内部 tag 値: `Tag(u32)`（user-visible `Int` と分離）
- 呼び出し可能値: `Callable`
- 言語エラー値: `Error(RichError)`
- process capability: `PID`（runtime が発行する opaque handle）

`inspect` / `to_string` における `Callable` 表示は、bare callable
（`lexical_captures == 0`）かつ runtime metadata から
`module` / `name` / `signature` を復元できる場合、
`FnCapture(module: M, name: f, signature: sig)` を返す。
それ以外の callable は実装定義の fallback 表示を使う。

### 4.1 RichError（V9確定）

`RichError` は次を保持する。

- `kind`
- `message`
- `location`
- `cause: Option<RichError>`

`cause` は runtime 管理の線形 chain とする。

compile / surface 契約との対応は次のとおり。

- source に現れる `Error` は abstract failure view であり、runtime 実体は常に concrete `deferror` 由来の `RichError`
- user code は `Error` を一般の first-class data として保持しない
- `Error` が surface 上で生存するのは `Err(Error)`、`match` の `Err(err)` で束縛された局所スコープ、標準定義ソース内の `Error` 観測 helper の引数位置に限る
- `Result::map_err` / `Result::cause` / `assert` / `ensure` は、この既存 `Error` 値を forward してよい
- `Result::recover_kind` だけは existing `Error` value ではなく concrete `deferror` kind marker surface を受ける

- parallel error は持たない
- `Result::cause(result, err)` は `err` chain の末尾に既存 error chain を付ける
- `Result::chain(head, tail)` は右 error chain の末尾に左 error chain を付ける
- `Result::map_err(result, err)` は既存 error chain を捨てて `err` chain で置き換える

表示契約は次で固定する。

- `inspect(Error)` / `to_string(Error)` は head-first tree 表示を返す
- 先頭行は `Kind("message")`
- cause がある場合、次行以降を `|_ ...` でネスト表示する
- `inspect(Result::Err(...))` も同じ tree を使うが、先頭行だけ `Err(...)` で包む
- `inspect(Struct)` / `to_string(Struct)` は `Type(field: value, ...)` を返し、内部専用の `Type { ... }` 構造体リテラルは表示しない
- `inspect` は再帰的に string literal を quote し、`to_string` は素の string 値を使う
- private field を含む named-field 値は公開 field のみを表示し、hidden 部分を `..private` として要約する
- `inspect(HashMap)` / `to_string(HashMap)` は `HashMap("key" => value, ...)` 形式で、key は `String` literal と同じ escaping で表示する
- `inspect(HashMap)` / `to_string(HashMap)` / `map_keys` / `map_values_list` はキー昇順の deterministic order を使う
- `eprint(Error)` は先頭行を `Error: Kind: message`、以降を `Caused by: Kind: message` で出力する
- `Error::kind(Error)` は `RichError.kind`、`Error::message(Error)` は `RichError.message` を `String` として返す
- `Error::format(Error)` は `eprint(Error)` と同じ行列を stderr へ出さず、`\n` join した `String` として返す

---

## 5. 命令体系

Opcode は以下のカテゴリを持つ。

- 定数/ローカル操作（Load/Store）
- 算術・比較・bitwise
- 文字列結合
- 文字列分解
- リスト/タグ付き値操作
- 呼び出し（`Call`, `CallClosure`, `CallBuiltin`）
- 制御フロー（`Jump`, `JumpIf*`）
- スタック操作（`Pop`）
- 関数復帰（`Return`）
- 停止（`Halt`）

補足:

- `CallBuiltin` は `builtin_id` ベースでディスパッチする
- `BitNotInt` / `BitAndInt` / `BitOrInt` / `BitXorInt` は `Int::bit_not` / `bit_and` / `bit_or` / `bit_xor` の direct call を対象にした monomorphic fast-path とする
- `StoreConstLocal { const_idx, local_idx }` は `LoadConst(const_idx); StoreLocal(local_idx)` と同じ意味の圧縮 opcode とする。operand stack へ中間値を push せず、定数値を現在フレームの local slot に直接保存する。`const_idx` は `LoadConst` と同じ relocation / verifier 規則に従う
- `CopyLocal { src_local_idx, dst_local_idx }` は `LoadLocal(src_local_idx); StoreLocal(dst_local_idx)` と同じ意味の圧縮 opcode とする。operand stack を経由せず、現在フレーム内で local 値を clone して保存する

実 opcode 一覧とオペランドは `crates/forge/src/opcode.rs` を正とする。

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
- `Facet::view` / `Facet::set` / `Facet::over` / `Facet::over_result` / `Facet::compose` / Facet `/` compose は compile-time lowering 対象であり、runtime builtin として直接到達した場合は防御的に `RuntimeError` とする
- Facet の variant mismatch は `Err(VariantMismatch(detail))` で返し、`detail` には失敗 segment（index と path 表示）を含める
- `eprint` は `Error` 値を診断表示し、それ以外の値への適用は VM 側ガード対象とする
- `Error::kind` / `Error::message` / `Error::format` は `Error` 値を introspection / 表示文字列化する runtime builtin とし、それ以外の値への適用は VM 側ガード対象とする
- `Result::recover` は compiler が lowering する special form であり、runtime builtin としては持たない
- `Int` は `BigInt` を用い、tag/builtin/function ID などの runtime 内部値とは分離する
- `HashMap` の runtime 表現は immutable map を基準にし、duplicate key 更新時は後勝ちで値を上書きする
- process / task / duration 系の hidden builtin は owner module (`Process`, `Task`, `Duration`) 側の `@hidden @builtin ...` 宣言に対応し、`CallBuiltin` で実装する。VM は process table / PID capability / handler callable invocation を経由する。詳細な process runtime 契約は [ProcessRuntime spec](./ProcessRuntime_spec.md) を正とする。
- `__supervisor_workers` は `(supervisor, worker_init, WorkerStrategy)` を受け取る。Eldr v1 は `WorkerScale::Fix(n)` のみ実行し、`init == n` かつ `0 <= min <= n <= max` を満たさない場合は `Err(InvalidWorkerStrategy)` を返す。
- process runtime snapshot は `worker_sets` を含む。各要素は `id`, `worker_process`, `supervisor`, `target`, `min`, `max`, `member_pids`, `live_count` を持つ。
- `Process::sleep(duration)` は runtime builtin とし、`Duration` 値を受け取って `Result<Unit>` を返す。
- process / workers / task await timeout は `@timeout(100ms)` literal から hidden builtin 呼び出しへ lower し、dynamic timeout は初期フェーズでは許可しない。
- regex 系は Rust `regex` crate のラッパーとして builtin 実装し、regex 未サポート構文は `RegexCompileError` として返す
- `RegexCaptures` の runtime 表現は `groups: Vec<Option<(start, end)>>`, `name_to_index: HashMap<String, usize>`, `input: String` を保持する
- random 系は `CallBuiltin` で実装し、Opcode は追加しない。`RandomGenerator` は opaque な seedable state として保持し、半開区間が空の場合は `InvalidRandomRange` を `Result` の `Err` として返す
- `Float` の厳密契約は `doc/float.md` を参照する

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

組込み宣言の読み込み順序は compile 側で `Bootstrap -> [SpecialTypes, Function, Kernel, Add, Sub, Mul, Eq, Neq, Compare, Lt, Lte, Gt, Gte, Concat, Numeric, Show, Ordering, Tuple, Ord, From, TryFrom, Encode, Decode, Functor, Chainable, PipeApply, Compose, Composable, LiftComposable, KleisliComposable, Int, String, Regex, Boolean, Error, List, Option, Generator, HashMap, Result, Duration, Process, Facet, Float, Json, Config, Project, Random, File, FS, IO, Shell, StyledDoc] -> [Test] -> ユーザ拡張` に固定される。同一 stage 内の import は file 読み込み順に依存せず compile 側で解決され、later stage 参照は compile error になる。Eldr はこの順序で解決済みの bytecode を受け取る前提とし、VM 内で追加の import 解決は行わない。

### 7.2 TypeRegistry

- `tag -> 型名/フィールド名` の逆引きを提供
- 表示 (`to_string`) と診断表示で参照される
- 実装は deterministic な entry 列を保持したまま、内部 index により O(1) 相当 lookup を行ってよい
- `Ok=0`, `Err=1` は予約 tag
- runtime tag は user-visible `Int` に乗せ替えない

---

## 8. `.eldr` 形式（実行入力）

- マジック: `ELDR`
- ヘッダ: `magic/version/debug_level/num_chunks`
- ヘッダ `version` は現行 `1` を維持する
- 意味的 bytecode 版は `CInf.bytecode_version` に保持し、現行は `1`
- `.eldr` は単一バイナリ実行物であり、チャンク分割の主目的は実行時ロード都合ではなく viewer / disasm / 診断 / 比較の観測性にある
- 必須チャンク:
  - `Code`
  - `Cnst`
  - `Func`
  - `Type`
  - `ErrT`
  - `CInf`
  - `LblT`
  - `ImpT`
  - `ExpT`
  - `LitT`
  - `Line`
  - `SpnT`
  - `SrcP`
  - `PcSp`
- 任意チャンク:
  - `Docs`
- `Code` は opcode 列のみを持つ
- `num_locals` は `CInf` に保持する
- `Cnst` は実行用 constant pool の正本
- `LitT` は viewer / 比較用の literal table
- `Func` は関数境界と viewer 用 flag / span を持つ
- `LblT` / `PcSp` / `Line` / `SpnT` / `SrcP` は viewer 向け索引・source 対応情報である

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

現行実装では Surtr の span 自体は source id を持たないため、`Line` / `SpnT` / `PcSp` / `SrcP` は単一 source を主対象にした viewer 情報として扱う。  
call opcode / error template / function span を使って source 対応を補完する。

### 8.4 `CInf`

`CInf` は少なくとも以下を保持する。

- `bytecode_version`
- `debug_level`
- `num_locals`
- optional compiler / target / build profile
- optional source hash / module hash

詳細なエンコード/デコード仕様は `crates/forge/src/bytecode.rs` を正とする。

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
