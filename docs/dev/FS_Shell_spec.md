# FS / Shell spec

`FS` と `Shell` の標準 surface、runtime builtin 契約、既存 `File`
module との境界をまとめる開発者向け正本仕様。

この文書は次を扱う。

- 標準ライブラリ surface
- snapshot と host filesystem mutation の意味論
- builtin / VM 実装契約
- テストで固定すべき観点

利用者向け surface は [FS / Shell](../site/shell.md) と標準定義 source の `@doc`、
runtime 実行層の詳細は [EldrVM spec](./EldrVM_spec.md)、テスト配置方針は
[テスト方針](./テスト方針.md) を併読する。

---

## 1. Positioning

`FS` は、host filesystem を Surtr の構造化データとして観測し、
必要最小限の filesystem mutation を `Result` で扱う標準 module である。

`Shell` は、実行コンテキストの working directory と外部コマンド実行を扱う
標準 module である。通常のファイル操作は `FS` を優先し、
外部コマンド実行は `Shell::exec` に閉じ込める。

既存 `File` v1 は維持する。

- `File`: UTF-8 text file の内容 I/O と structured handle API
- `FS`: path / metadata / directory snapshot / 最小 mutation
- `Shell`: cwd / external command

`FS` v1 は text content API を重複提供しない。
`read` / `write` / `append` / `with_open` が必要な場合は `File` を使う。

`FS` と `Shell` は auto import しない。利用者は明示 `import` または
`FS::ls(path)` のような qualified call を使う。

---

## 2. Surface

### 2.1 Top-level types

`FilePath` は path を表す軽量 wrapper である。v1 では正規化済み、絶対 path、
存在確認済み path、file / directory の型分離を導入しない。

```surtr
defstruct FilePath {
  raw: String,
}
```

`FileSystemEntryKind` は host filesystem の種類を Surtr 側の最小カテゴリへ丸める。
OS 固有の詳細は v1 surface に露出しない。

```surtr
defenum FileSystemEntryKind {
  File,
  Directory,
  Symlink,
  Other,
}
```

`FileSystemPermissions` は portable な最小 view である。

```surtr
defstruct FileSystemPermissions {
  read_only: Boolean,
  executable: Boolean,
}
```

`FileSystemMetadata` は取得できない属性を `Option` で表す。
`DateTime` / `ByteSize` / `Bytes` は v1 では導入しない。size は byte count、
timestamp は Unix epoch milliseconds を `Int` で表す。

```surtr
defstruct FileSystemMetadata {
  size: Option<Int>,
  modified_at_epoch_ms: Option<Int>,
  accessed_at_epoch_ms: Option<Int>,
  created_at_epoch_ms: Option<Int>,
  permissions: Option<FileSystemPermissions>,
}
```

`FileSystemEntry` は観測時点の 1 entry を表す。file handle ではない。

```surtr
defstruct FileSystemEntry {
  path: FilePath,
  name: String,
  kind: FileSystemEntryKind,
  metadata: FileSystemMetadata,
}
```

`FileSystemSnapshot` は `ls` / `tree_depth` 実行時点の観測結果である。
snapshot を更新しても host filesystem は変更されない。

```surtr
defstruct FileSystemSnapshot {
  root: FilePath,
  entries: List<FileSystemEntry>,
}
```

`CommandResult` は、起動できた外部コマンドの終了状態と出力を表す。
非ゼロ終了は `Err` ではなく `exit_code` で表す。

```surtr
defstruct CommandResult {
  command: String,
  args: List<String>,
  exit_code: Int,
  stdout: String,
  stderr: String,
}
```

### 2.2 Error surface

FS / Shell 失敗は concrete `deferror` family として定義する。宣言・生成・Payload の保持は [Error spec](Error_spec.md) に従い、本節は操作ごとの失敗条件と入力を定める。
Rust の `std::io::ErrorKind` や OS error code をそのまま public contract にしない。

入力と同名・同型・同順の値を Payload に保存する。FileSystem と Shell の path は `FilePath`、不正 raw path は `String`、OS や decode の detail は外部原文として渡す。生成側は完成 message を作らず、標準定義本文が各操作の文型を持つ。署名と message の一次情報は [`lib/FileSystem.srt`](../../lib/FileSystem.srt) と [`lib/Shell.srt`](../../lib/Shell.srt) とする。

| Error | コンストラクタ入力・保存 Payload |
|---|---|
| `FileSystemParentMissing` | `path: FilePath` |
| `FileSystemNameMissing` | `path: FilePath` |
| `FileSystemMoveFailed` | `from: FilePath, to: FilePath, detail: String` |
| `FileSystemMoveNotFound` | `from: FilePath, to: FilePath, detail: String` |
| `FileSystemMovePermissionDenied` | `from: FilePath, to: FilePath, detail: String` |
| `FileSystemMoveAlreadyExists` | `from: FilePath, to: FilePath, detail: String` |
| `FileSystemMoveInvalidPath` | `from: FilePath, to: FilePath, detail: String` |
| `FileSystemCopyFailed` | `from: FilePath, to: FilePath, detail: String` |
| `FileSystemCopyNotFound` | `from: FilePath, to: FilePath, detail: String` |
| `FileSystemCopyPermissionDenied` | `from: FilePath, to: FilePath, detail: String` |
| `FileSystemCopyAlreadyExists` | `from: FilePath, to: FilePath, detail: String` |
| `FileSystemCopyInvalidPath` | `from: FilePath, to: FilePath, detail: String` |
| `FileSystemNotFound` | `path: FilePath` |
| `FileSystemAlreadyExists` | `path: FilePath` |
| `FileSystemPermissionDenied` | `path: FilePath` |
| `FileSystemNotDirectory` | `path: FilePath` |
| `FileSystemIsDirectory` | `path: FilePath` |
| `FileSystemInvalidPath` | `raw: String` |
| `FileSystemInvalidDepth` | `depth: Int` |
| `FileSystemDirectoryCopyUnsupported` | `from: FilePath, to: FilePath` |
| `FileSystemStatFailed` | `path: FilePath, detail: String` |
| `FileSystemListFailed` | `path: FilePath, detail: String` |
| `FileSystemTreeReadFailed` | `path: FilePath, detail: String` |
| `FileSystemMkdirFailed` | `path: FilePath, detail: String` |
| `FileSystemMkdirAllFailed` | `path: FilePath, detail: String` |
| `FileSystemRemoveFailed` | `path: FilePath, detail: String` |
| `ShellSpawnResourceNotFound` | `command: String, args: List<String>, cwd: FilePath, detail: String` |
| `ShellSpawnFailed` | `command: String, args: List<String>, cwd: FilePath, detail: String` |
| `ShellWorkingDirectoryNotFound` | `path: FilePath` |
| `ShellWorkingDirectoryNotDirectory` | `path: FilePath` |
| `ShellWorkingDirectoryInspectFailed` | `path: FilePath, detail: String` |
| `ShellUnsupported` | `feature: String` |
| `ShellStdoutEncodingError` | `command: String, detail: String` |
| `ShellStderrEncodingError` | `command: String, detail: String` |
| `ShellWorkingDirectoryResolveFailed` | `path: FilePath, detail: String` |

```surtr
deferror FileSystemNotFound(path: FilePath) {
  |path: FilePath|
  Self(message: "filesystem path not found: #{to_string(path)}", path)
}
```

move / copy は from・to・OS detail を保存し、失敗 endpoint を推測しない。Shell の host NotFound は command 欠落を確定せず、`ShellSpawnResourceNotFound` に command・args・cwd・OS detail を渡す。汎用の旧 detail 入口を残さない。

関数定義の直接の戻り値位置に限り、詳細な error contract 表記として `Result<T, Error>` を使ってよい。
値やその他の型注釈で使う型 head は既存方針どおり `Result<T>` である。

### 2.3 `FS` operations

`FS` v1 は path helper、観測、最小 mutation を持つ。

```surtr
defmod FS {
  @builtin def path(raw: String) -> Result<FilePath, Error>
  @builtin def join(base: FilePath, child: String) -> Result<FilePath, Error>
  @builtin def parent(path: FilePath) -> Result<FilePath, Error>
  @builtin def name(path: FilePath) -> Result<String, Error>
  @builtin def extension(path: FilePath) -> Option<String>

  @builtin def exists(path: FilePath) -> Result<Boolean, Error>
  @builtin def stat(path: FilePath) -> Result<FileSystemEntry, Error>
  @builtin def ls(path: FilePath) -> Result<FileSystemSnapshot, Error>
  @builtin def tree_depth(path: FilePath, depth: Int) -> Result<FileSystemSnapshot, Error>

  @builtin def mkdir(path: FilePath) -> Result<Unit, Error>
  @builtin def mkdir_all(path: FilePath) -> Result<Unit, Error>
  @builtin def rm(path: FilePath) -> Result<Unit, Error>
  @builtin def mv(from: FilePath, to: FilePath) -> Result<Unit, Error>
  @builtin def cp(from: FilePath, to: FilePath) -> Result<Unit, Error>
}
```

`FS::ls` は直下 entry だけを取得する。root の前提判定は symlink を辿る metadata で不在・権限拒否・非 directory・その他取得失敗を分ける。その他取得失敗は ls なら `FileSystemListFailed`、tree なら `FileSystemTreeReadFailed` とする。tree の途中の失敗はその対象 path を保存する。子 entry の symlink 判定は従来の意味を維持する。
`FS::tree_depth(path, depth)` は root 配下を depth まで再帰的に取得する。
`depth < 0` は `Err(FileSystemInvalidDepth(depth))` を返す。
探索中の深さと指定した深さは `Int` のまま比較し、固定幅整数の上限を設けない。

`FS::rm` は file または空 directory を 1 つ削除する。危険な再帰削除
`rm_all` は v1 に含めない。

`FS::cp` は v1 では file copy を対象にする。directory copy は
`Err(FileSystemDirectoryCopyUnsupported(from, to))` を返す。

### 2.4 Snapshot querying

`FS` は sort / filter / query option を持たない。
取得後の加工は Surtr の通常言語機能で行う。

```surtr
import FS

def srt_files(root: FilePath) -> Result<List<FileSystemEntry>> {
  snapshot =? FS::ls(root)
  Ok(
    snapshot.entries
    |> List::filter({|entry|
      FS::extension(entry.path) == Some("srt")
    })
    |> List::sort_by({|left, right|
      compare(left.name, right.name)
    })
  )
}
```

field 取得や構造化更新には `Facet` を使ってよい。ただし snapshot は観測値であり、
`Facet::set` で entry を更新しても host filesystem は変更されない。

### 2.5 `Shell` operations

`Shell` v1 は execution context の cwd と外部コマンド実行を扱う。

```surtr
defmod Shell {
  @builtin def pwd() -> Result<FilePath, Error>
  @builtin def cd(path: FilePath) -> Result<Unit, Error>
  @builtin def exec(command: String, args: List<String>) -> Result<CommandResult, Error>
}
```

`Shell::cd` は Surtr VM / 実行コンテキストの working directory を変更する。
host parent process の cwd を変更する契約ではない。
path の前提判定は symlink を辿る metadata を使い、成功して非 directory の場合だけ `ShellWorkingDirectoryNotDirectory` とする。不在は `ShellWorkingDirectoryNotFound`、その他取得失敗は `ShellWorkingDirectoryInspectFailed` とする。directory として受理した path の canonicalize に失敗した場合は
`Err(ShellWorkingDirectoryResolveFailed(path, detail))` を返し、VM context cwd は変更しない。

`Shell::exec(command, args)` は shell parser を通さず、`command` と `args` を
分離して実行する。起動できた場合、process の終了 code が非ゼロでも
`Ok(CommandResult)` を返す。起動前後の失敗だけを `Err(Shell*)` にする。

`CommandResult::require_success` のような helper は v1 に含めない。
後続で source helper として追加する場合も、`Shell::exec` 自体の失敗意味論は変えない。

---

## 3. Runtime contract

### 3.1 Builtin names

public surface から runtime builtin への対応は `crates/sindr/src/builtin.rs` の
`BUILTIN_METAS` を正本にする。builtin id は定義順で割り当てる。

推奨 runtime builtin name:

- `FS::path` -> `filesystem_path`
- `FS::join` -> `filesystem_join`
- `FS::parent` -> `filesystem_parent`
- `FS::name` -> `filesystem_name`
- `FS::extension` -> `filesystem_extension`
- `FS::exists` -> `filesystem_exists`
- `FS::stat` -> `filesystem_stat`
- `FS::ls` -> `filesystem_ls`
- `FS::tree_depth` -> `filesystem_tree_depth`
- `FS::mkdir` -> `filesystem_mkdir`
- `FS::mkdir_all` -> `filesystem_mkdir_all`
- `FS::rm` -> `filesystem_rm`
- `FS::mv` -> `filesystem_mv`
- `FS::cp` -> `filesystem_cp`
- `Shell::pwd` -> `shell_pwd`
- `Shell::cd` -> `shell_cd`
- `Shell::exec` -> `shell_exec`

FS / Shell は side-effectful builtin なので専用 opcode を追加しない。
Forge は通常どおり `CallBuiltin` を emit し、Eldr が builtin id で dispatch する。

### 3.2 Runtime representation

`FilePath`、`FileSystemEntry`、`FileSystemMetadata`、`FileSystemSnapshot`、
`CommandResult` は user-visible struct value として生成する。
VM 実装は tag 番号をハードコードせず、`TypeRegistry` から type / constructor を
名前で解決する。

`FileSystemEntryKind` は user-visible enum value として返す。
host の file type を次に丸める。

| Host observation | Surtr value |
|---|---|
| regular file | `FileSystemEntryKind::File` |
| directory | `FileSystemEntryKind::Directory` |
| symlink | `FileSystemEntryKind::Symlink` |
| other / unknown | `FileSystemEntryKind::Other` |

metadata は取得に失敗した項目だけ `None` にする。`stat` / `ls` 全体が失敗した場合は
`Err(FileSystem*)` を返す。

`ls` / `tree_depth` の entry order は deterministic にする。最低限、path または name の
昇順で返し、OS directory iteration order に依存しない。

### 3.3 Working directory

VM は `Shell::pwd` / `Shell::cd` 用に execution context cwd を持つ。
`FS` と `File` の relative path 解決は、この cwd に従う。

`Shell::cd` は並列 test や複数 VM 実行に干渉しないよう、process-wide
`std::env::set_current_dir` へ直接寄せない。実装上どうしても host cwd を使う場合でも、
公開契約は VM context cwd とし、テストで漏れを検出する。
directory 判定後の canonicalize が失敗した場合、`Shell::cd` は
`Err(ShellWorkingDirectoryResolveFailed(path, detail))` を返して現在の VM context cwd を保持する。

### 3.4 Shell execution

`Shell::exec` は command shell を挟まない。

- Unix: `std::process::Command::new(command).args(args)`
- Windows: 同等の host process API

stdout / stderr は UTF-8 text として返す。UTF-8 変換不能時は lossless binary API を
v1 に導入せず、`Err(ShellStdoutEncodingError(command, detail))` / `Err(ShellStderrEncodingError(command, detail))` を返す。

environment override、stdin input、timeout、shell parser 付き実行は v1 に含めない。

---

## 4. Stdlib load order

標準定義ソースに `lib/FileSystem.srt` と `lib/Shell.srt` を追加する。
どちらも auto import しない。

load order は、既存 effect / runtime-facing module 群の近くに置く。

```text
Project, Random, File, FS, IO, Shell, StyledDoc
```

`FS` と `Shell` は同一 standard stage に属するため相互に明示 import できる。
ただし v1 surface では `Shell` が `FS` の薄い wrapper を公開しないため、
直接依存は持たせない。

---

## 5. Testing contract

最低限、次を回帰基準にする。

- `unit/sindr`
  - `BUILTIN_METAS` の id が定義順に一致する
  - `FS::*` / `Shell::*` の qualified surface が runtime builtin name に解決される
  - signature string が標準定義 source の `@builtin def` と一致する
- `unit/scar`
  - `FS` / `Shell` 標準定義 source が typecheck できる
  - `FS` / `Shell` が auto import されない
  - `FileSystemSnapshot.entries` に `List::filter` / `List::sort_by` を適用できる
- `unit/eldr`
  - `FS::path` / `join` / `parent` / `name` / `extension`
  - `FS::stat` / `ls` / `tree_depth`
  - `FS::mkdir` / `mkdir_all` / `rm` / `mv` / `cp`
  - `Shell::pwd` / `cd`
  - `Shell::exec` が launched non-zero exit を `Ok(CommandResult)` として返す
- `lib/tests`
  - `file_system.srt`: snapshot acquisition、`List::filter`、`List::sort_by`
  - `shell.srt`: `Shell::pwd()` と portable command execution
- `integration`
  - `surtr test lib/tests/modules/file_system.srt`
  - `surtr test lib/tests/modules/shell.srt`
  - cwd isolation が他 test case に漏れないこと

実装後の最小 verification:

```bash
cargo nextest run -p sindr
cargo nextest run -p scar
cargo nextest run -p eldr
cargo nextest run -p rune --test integration test_command
cargo nextest run --workspace
```

---

## 6. Non-goals

v1 では次を導入しない。

- binary read / write API
- `Bytes` / `ByteSize` / `DateTime`
- recursive delete (`rm_all`)
- directory copy
- file handle API の `FS` 側移植
- `Shell::exec` の shell parser 付き string 実行
- stdin / env / timeout 付き command spec
- sandbox / capability policy の完成形
- sort / filter / glob option 付き `ls`

これらは必要になった段階で `doc/open-issues.md` または別 spec へ切り出す。
