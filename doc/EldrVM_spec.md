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

Eldr は次を担わない。

- 構文解析（Spire）
- 名前解決（Sigil）
- 型検査（Scar）
- コード生成（Forge）

---

## 2. Bytecode 成果物

### 2.1 `Bytecode`

- ファイル実行と `.eldr` 入出力に使う完全な実行単位
- `opcodes`, `constants`, `type_registry`, `error_templates`, `functions` を持つ
- `source_map` は `Option<SourceMap>` で付与する
- `docs` は `@@doc` 由来の symbol metadata を保持する

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
- `Callable` は `lexical_captures` と `partial_args` を別々に保持する
- 関数呼び出し時、`locals` には `lexical_captures` → `partial_args` → 実引数 の順で先頭から配置する
- `Call` 実行後は、呼び出し先がフレーム完成状態で開始する

### 3.3 返り値

- 関数は 1 値を返す
- 呼び出し元には返り値 1 つのみが push される

### 3.4 関数テーブル不変条件

- `fun_idx` は実行時の関数テーブル添字と一致する（`functions[fun_idx as usize]`）
- 欠番は作らない（holes 禁止）
- `FunctionEntry` の整列後は `entry.fun_idx == index` を満たす
- VM はこの不変条件を前提に O(1) 参照し、破綻時は `RuntimeError` とする

### 3.5 REPL 増分実行（`push_atomic`）契約

- `BytecodeChunk` の `LoadConst` / `MakeError` は chunk-local index で生成される
- `push_atomic()` は `const_base` / `error_template_base` により絶対 index へ再配置する
- `push_atomic()` は jump 先も append 後の opcode 位置へ再配置する
- `chunk.const_base` / `chunk.error_template_base` が VM の現在プール長と一致しない場合は `RuntimeError` とする
- Forge の chunk codegen は top-level 末尾へ必ず `Halt` を 1 つ挿入する
- top-level 実行は append された `code_base` から開始し、最初の `Halt` で停止する
- 関数本体は top-level `Halt` 後ろに配置され、top-level からは到達不能であり、`Call` / `CallClosure` でのみ到達する
- 実装は VM 全体 clone ではなく、append した bytecode 断片と実行時状態の checkpoint / rollback で原子性を保つ
- `push_atomic()` の返り値は chunk 実行終了時点の stack top 1 値のみとする。stack が空なら `Unit` を返す
- `push_atomic()` 完了後、VM の operand stack は空に戻す。REPL は前回 chunk の stack 内容を次回 chunk へ持ち越さない
- `push_atomic()` は chunk 実行を原子的に扱い、失敗時は VM 状態を更新しない

### 3.6 トップレベル名衝突ポリシー（コンパイラ契約）

- 同一モジュール（REPL セッションを含む）で、トップレベル定義名の重複は禁止する
- 対象: `def` / `defstruct` / `defrecord` / `deferror`（trait は将来仕様）
- 本規約はファイル実行と REPL で同一に適用する

---

## 4. Value モデル

Eldr が扱う値の概念カテゴリ:

- プリミティブ: `Int`, `Float`, `String`, `Boolean`, `Unit`
- コンテナ: `List`
- タグ付き値: `Tagged { tag, fields }`
- runtime 内部 tag 値: `Tag(u32)`（user-visible `Int` と分離）
- 呼び出し可能値: `Callable`
- 言語エラー値: `Error(RichError)`

### 4.1 RichError（V9確定）

`RichError` は次を保持する。

- `kind`
- `message`
- `location`

`cause` チェーンは V9 範囲外とする。

---

## 5. 命令体系

Opcode は以下のカテゴリを持つ。

- 定数/ローカル操作（Load/Store）
- 算術・比較
- 文字列結合
- リスト/タグ付き値操作
- 呼び出し（`Call`, `CallClosure`, `CallBuiltin`）
- 制御フロー（`Jump`, `JumpIf*`）
- スタック操作（`Pop`）
- 関数復帰（`Return`）
- 停止（`Halt`）

補足:

- `MakeFrame` / `PopFrame` は互換目的の命令で、新規 codegen は emit しない
- `CallBuiltin` は `builtin_id` ベースでディスパッチする

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

`Value::Error` は正常なデータフローであり、`RuntimeError` と混同しない。

---

## 7. 組込み関数と型情報

- 組込み関数メタデータは単一テーブルで管理する
- `Bootstrap` module の `@@builtin` 宣言はこの共有テーブルに対応する宣言層であり、builtin の追加起点ではない
- VM は `builtin_id` により実装関数をディスパッチする
- `eprint` は `Error` 値を診断表示し、それ以外の値への適用は VM 側ガード対象とする
- `Int` は `BigInt` を用い、tag/builtin/function ID などの runtime 内部値とは分離する
- `Float` の厳密契約は `doc/float.md` を参照する

組込み宣言の読み込み順序は compile 側で `Bootstrap -> [Kernel + 他標準モジュール] -> ユーザ拡張` に固定される。Eldr はこの順序で解決済みの bytecode を受け取る前提とし、VM 内で追加の import 解決は行わない。

### 7.1 TypeRegistry

- `tag -> 型名/フィールド名` の逆引きを提供
- 表示 (`to_string`) と診断表示で参照される
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

- `VM::step()` / `VMSnapshot`
- Bytecode verifier
- 値表現最適化（clone 削減、共有構造）

---

*Surtr — 既存の妥協を、型で焼き払う。*
