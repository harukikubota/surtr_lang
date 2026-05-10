# File I/O

`File` は Surtr の UTF-8 テキスト専用ファイル I/O モジュールです。  
source 上の正本は [../../lib/file.srt](/Users/haruca/work/rust/surtr/lib/file.srt) にあります。

`File` は auto import されません。使うときは明示 `import File` するか、`File::read(...)` のように修飾して呼びます。

## 何ができるか

- テキストファイル全体の読み書き
- 追記
- 存在確認と削除
- `with_open` を使った chunked read/write
- callback 終了時と VM 終了時の確実な resource cleanup

v1 の範囲は intentionally small です。

- UTF-8 text only
- binary API はまだない
- user-visible `close` はない
- append-only な process runtime sink は `FileOutHandler` の責務で、`File` とは分離される

## 最初の例

```surtr
import File

def main() -> Result<()> {
  path = "./tmp/sandbox/note.txt"
  _ =? File::write(path, "hello")
  text =? File::read(path)
  print(text)
  Ok(())
}
```

## one-shot helpers

### `File::read`

ファイル全体を UTF-8 として読み込みます。

```surtr
note =? File::read("./tmp/sandbox/note.txt")
```

主な失敗:

- `Err(FileNotFound(path))`
- `Err(FilePermissionDenied(path))`
- `Err(FileEncodingError(...))`
- `Err(FileIoError(...))`

### `File::write`

ファイルを作成または truncate して、与えた text 全体を書き込みます。

```surtr
_ =? File::write("./tmp/sandbox/note.txt", "alpha")
```

### `File::append`

ファイルがなければ作成し、末尾へ追記します。

```surtr
_ =? File::append("./tmp/sandbox/note.txt", "\nbeta")
```

### `File::exists`

パスが存在すれば `True` を返します。

```surtr
if(File::exists("./tmp/sandbox/note.txt"), print("found"), print("missing"))
```

存在確認だけなので、「読める」「書ける」「通常ファイルである」ことまでは保証しません。

### `File::delete`

1 つのファイルを削除します。

```surtr
_ =? File::delete("./tmp/sandbox/note.txt")
```

## `with_open` と `FileHandle`

複数回の read/write をまとめたいときは `File::with_open` を使います。

```surtr
import File

def write_report(path: String) -> Result<()> {
  File::with_open(path, Write, fn(file) {
    _ =? File::write_chunk(file, "title: demo\n")
    _ =? File::write_chunk(file, "status: ok\n")
    File::flush(file)
  })
}
```

`FileHandle` は opaque です。user code で new したり close したりはできません。  
この制約は不便さではなく、「cleanup を runtime が責任を持つ」ための design choice です。

### cleanup guarantee

`with_open` の callback では次を保証します。

- callback が `Ok(...)` を返しても close される
- callback が `Err(...)` を返しても close される
- REPL の interactive chunk rollback 時も orphan handle を残さない
- VM run 終了時に残存 handle を shutdown cleanup する

つまり、Surtr user code は通常の `open -> try -> finally -> close` を毎回書きません。

## `FileMode`

`with_open` の mode は次の 5 種類です。

- `Read`
  読み取り専用
- `Write`
  作成または truncate して先頭から書く
- `Append`
  末尾へ追記する
- `ReadWrite`
  読み書き両用
- `ReadAppend`
  読みつつ追記する

`Read` で開いた handle に `File::write_chunk` すると `Err(FileIoError(...))` になります。

## chunked read

`read_chunk` は raw bytes ではなく UTF-8 文字数で上限を取ります。

```surtr
import File

def read_prefix(path: String) -> Result<String> {
  File::with_open(path, Read, fn(file) {
    File::read_chunk(file, 5)
  })
}
```

EOF では `Ok("")` を返します。

```surtr
import File

def drain_two(path: String) -> Result<(String, String)> {
  File::with_open(path, Read, fn(file) {
    left =? File::read_chunk(file, 5)
    right =? File::read_chunk(file, 5)
    Ok((left, right))
  })
}
```

## エラーハンドリング

`File` の失敗は VM panic ではなく、recoverable な `Result` error として返ります。

```surtr
import File

def load_note(path: String) -> Result<String> {
  match File::read(path) {
    Ok(text) => Ok(text),
    Err(FileNotFound(_)) => Ok(""),
    Err(err) => Err(err),
  }
}
```

`=?` を使えば、伝播だけしたい失敗は短く書けます。

## `IO` との違い

- `IO`
  標準入力専用
- `File`
  ホストファイルシステム上の UTF-8 text file 専用

`IO::get_line(...)` で stdin を読み、`File::write(...)` で保存する、という組み合わせが基本です。

## テストでの使い方

file I/O テストは working directory に依存するので、Surtr では `./tmp/sandbox/` 配下を明示して使うのがおすすめです。

```surtr
import File

def test_write_and_read() -> Result<()> {
  path = "./tmp/sandbox/test-note.txt"
  _ =? File::write(path, "hello")
  File::read(path)
}
```

## 関連ページ

- [標準定義ソース](./standard-modules.md)
- [標準ライブラリ全体ガイド](./standard-library.md)
- [エラーハンドリング](./error-handling.md)

## 確認したソース

- ソース
  - `../../lib/file.srt`
  - `../../lib/IO.srt`

## 躓きやすいポイント

- `File` は auto import されないので、bare `read(...)` ではなく `File::read(...)` が必要です。
- `read_chunk` の上限は byte 数ではなく UTF-8 文字数です。
- `exists` は権限やファイル種別までは保証しません。
- `with_open` の handle は callback の外へ持ち出す前提ではありません。
