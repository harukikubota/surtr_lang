# HashMap v1 Design

> 目的: Surtr に immutable な string-keyed map を最小コストで導入するための詳細設計。
> 本ドキュメントは実装前の詳細設計メモであり、正本仕様へ昇格する前の叩き台として扱う。
> `HashMap` の導入後、確定事項は `doc/要件定義v9.md` と `doc/EldrVM_spec.md` へ反映する。

最終更新日: 2026-04-14

---

## 1. 方針

`HashMap` の v1 は次の制約で導入する。

- immutable value とする
- key は `String` に固定する
- value だけを generic parameter `$V` にする
- v1 では専用 literal syntax を導入しない
- v1 では path sugar / index sugar / pattern matching は導入しない
- v1 では `Opcode` を増やさず builtin 群で実装する

この方針により、`HashMap` は runtime-facing builtin type として追加しつつ、
parser / resolver / codegen / VM 命令体系への影響を最小化する。

---

## 2. Surface Type

v1 の surface type は次で固定する。

```surtr
HashMap<$V>
```

補足:

- surface 上は value type のみ generic とする
- key type parameter は持たない
- key は API 契約として常に `String` とする

### 2.1 `HashMap<String, $V>` を採らない理由

現行の `@@builtin type` canonical head は `name + type parameter names` だけを前提にしている。
したがって `HashMap<String, $V>` のように concrete な `String` を head に埋め込む形式は、
v1 の最小導入方針と相性が悪い。

そのため v1 は `HashMap<$V>` とし、key 固定は各 builtin 関数シグネチャで表す。

---

## 3. Standard Module Surface

標準モジュールは新規 `lib/hash_map.srt` を想定し、公開 surface は次で固定する。

```surtr
@@builtin type HashMap<$V>

defmod HashMap {
  def empty() -> HashMap<$V>
  def from_entries(entries: List<(String, $V)>) -> HashMap<$V>
  def len(map: HashMap<$V>) -> Int
  def contains_key(map: HashMap<$V>, key: String) -> Boolean
  def get(map: HashMap<$V>, key: String) -> Result<$V, NoneError>
  def insert(map: HashMap<$V>, key: String, value: $V) -> HashMap<$V>
  def remove(map: HashMap<$V>, key: String) -> HashMap<$V>
  def keys(map: HashMap<$V>) -> List<String>
  def values(map: HashMap<$V>) -> List<$V>
}
```

### 3.1 API の意味論

#### `empty`

- 空 map を返す

#### `from_entries`

- `List<(String, $V)>` から map を構築する
- 左から右へ順に適用する
- duplicate key は後勝ちとする
- duplicate key の再挿入でも key の表示順は元の位置を維持する

#### `len`

- entry 数を返す

#### `contains_key`

- key の存在判定を返す

#### `get`

- hit 時は `Ok(value)` を返す
- miss 時は `Err(NoneError)` を返す

#### `insert`

- 新しい map を返す
- 既存 key がある場合はその値を上書きする
- 既存 key の順序は維持する
- 新規 key は末尾へ追加される

#### `remove`

- 新しい map を返す
- key が存在しない場合は no-op とする

#### `keys`

- key 一覧を insertion order で返す

#### `values`

- `keys(map)` と同じ順序の value 一覧を返す

---

## 4. Display / Inspect Contract

`HashMap` の `inspect` / `to_string` は、`HashMap(...)` 形式で表示する。

v1 の表示形式は次で固定する。

```text
HashMap("name" => 1, "age" => 2)
```

契約:

- 先頭は `HashMap(`、末尾は `)`
- entry 区切りは `, `
- key と value の区切りは ` => `
- key は必ず quoted string とする
- key の quoting は Surtr の string literal に準じる
- value は既存の `inspect` 表示をそのまま埋め込む
- 空 map は `HashMap()`

### 4.1 key の quoting

key は常に `String` なので、表示では常に quoted にする。

例:

```text
HashMap("a" => 1)
HashMap("first-name" => "haruca")
HashMap("line\nfeed" => 1)
```

最小 escaping 契約:

- `\\`
- `\"`
- `\n`
- `\t`

必要なら既存 string literal display helper と整合させる。

### 4.2 順序契約

`inspect` は insertion order を使って描画する。

したがって次は同じ表示になる。

```surtr
m = HashMap::empty()
m = HashMap::insert(m, "a", 1)
m = HashMap::insert(m, "b", 2)
inspect(m) == 'HashMap("a" => 1, "b" => 2)'
```

また duplicate key 更新でも順序は維持する。

```surtr
m = HashMap::from_entries([("a", 1), ("b", 2), ("a", 3)])
inspect(m) == 'HashMap("a" => 3, "b" => 2)'
```

---

## 5. Runtime Representation

v1 の runtime 表現は、まず単純さを優先する。

候補は次の 2 つがある。

### 5.1 実装候補 A: `Vec<(String, Value)>`

```rust
pub struct HashMapHandle {
    pub entries: Vec<(String, Value)>,
}
```

利点:

- 実装が単純
- insertion order を自然に保持できる
- `inspect` 実装がわかりやすい
- duplicate key 更新時の順序維持ロジックを明示しやすい

欠点:

- lookup / contains / remove が線形時間になる

### 5.2 実装候補 B: `order + HashMap<String, Value>`

```rust
pub struct HashMapHandle {
    pub order: Vec<String>,
    pub values: std::collections::HashMap<String, Value>,
}
```

利点:

- lookup が平均 O(1)
- insertion order も保持できる

欠点:

- 更新時に `order` と `values` の整合管理が必要
- v1 としてはやや実装量が増える

### 5.3 v1 推奨

v1 は候補 A から始める。

理由:

- feature 導入初期の正しさ確認がしやすい
- API と表示契約を先に固めるほうが重要
- 内部表現は後で差し替え可能

ただし、実運用で `HashMap` が頻出になった段階では候補 B への移行を再検討する。

---

## 6. Compiler / Runtime Change Scope

### 6.1 追加が必要な箇所

#### 仕様・設計ドキュメント

- `doc/要件定義v9.md`
- `doc/EldrVM_spec.md`
- `doc/テスト方針.md`
- `doc/HashMap_v1.md`（本ファイル）

#### 標準モジュール

- `lib/hash_map.srt`

#### builtin 正本

- `crates/sindr/src/builtin.rs`

#### 型表現

- `crates/scar/src/types.rs`
- `crates/scar/src/checker/types.rs`
- `crates/scar/src/checker/mod.rs`

#### runtime 値表現

- `crates/sindr/src/runtime.rs`
- `crates/eldr/src/builtin.rs`

#### 標準モジュール読み込み

- `crates/xldr/src/loader.rs`
- `crates/scar/src/lib.rs`
- `crates/forge/src/lib.rs`
- builtin alignment test を持つ `crates/eldr/src/builtin.rs`

### 6.2 v1 では不要な箇所

- 新規 `Opcode`
- `crates/forge/src/codegen.rs` の命令追加
- `crates/eldr/src/vm.rs` の opcode 実行分岐追加
- parser / resolver への syntax sugar 追加

---

## 7. Builtin Naming Strategy

surface 名と runtime builtin 名は分離して考える。

surface:

- `HashMap::empty`
- `HashMap::from_entries`
- `HashMap::len`
- `HashMap::contains_key`
- `HashMap::get`
- `HashMap::insert`
- `HashMap::remove`
- `HashMap::keys`
- `HashMap::values`

runtime builtin 名の候補:

- `empty_map`
- `map_from_entries`
- `map_len`
- `map_contains_key`
- `map_get`
- `map_insert`
- `map_remove`
- `map_keys`
- `map_values`

この方針により、既存 module surface の一般名と衝突しにくくする。

---

## 8. Future Syntax Plan

v1 では literal syntax を入れないが、将来は次を導入候補とする。

```surtr
hash!["k1" => 2, "k2" => 3]
```

この syntax は v1 API へ次のいずれかで lower できる。

### 8.1 `from_entries` へ lower

```surtr
hash!["k1" => 2, "k2" => 3]
```

↓

```surtr
HashMap::from_entries([("k1", 2), ("k2", 3)])
```

### 8.2 `empty + insert` 連鎖へ lower

```surtr
m0 = HashMap::empty()
m1 = HashMap::insert(m0, "k1", 2)
m2 = HashMap::insert(m1, "k2", 3)
```

v1 で `from_entries` を入れておく理由は、この lower 先を単純に保つためである。

### 8.3 `=>` を採る理由

- `:` は Surtr ですでに型注釈・field・引数宣言で多用されている
- `=>` は `match` / `cond` で「対応づけ」を表しており、key-value に意味が近い
- `{}` は block として使っているため、map literal に流用しない方針と整合する
- `array![...]` と `hash![...]` の対になる macro-like literal syntax にしやすい

---

## 9. Tests

### 9.1 spec

追加する機能ケースの最小セット:

- empty map
- insert + get
- duplicate key overwrite
- remove existing key
- remove missing key
- from_entries basic
- from_entries duplicate key overwrite
- keys / values keep insertion order
- inspect rendering with quoted keys

### 9.2 compile_errors

v1 では主に次を固定する。

- `HashMap::get(map, 1)` のように key が `String` でない
- `HashMap::from_entries([(1, 2)])` 相当で entry key が `String` でない
- `HashMap<Int>` のように key type parameter を取れると誤解したコードを reject する

### 9.3 unit

- builtin meta と std-module 宣言の整合
- runtime handle の duplicate key overwrite 契約
- inspect rendering の escape 契約
- insertion order 保持契約

---

## 10. Open Questions

### OQ-001 runtime 内部表現の初期選択

- v1 は `Vec<(String, Value)>` で十分か
- 早めに `HashMap<String, Value> + order` にするか

### OQ-002 `entries(map)` を v1 に入れるか

`from_entries` があるため対になる `entries(map) -> List<(String, $V)>` も自然だが、
v1 の最小 API には必須ではない。

### OQ-003 display の string escape をどこまで既存 `String` 表示と共有するか

- `inspect(String)` の契約とどこまで共通 helper にするか
- quoting の詳細を `String` 表示契約と同じ文書に寄せるか

---

## 11. 推奨実装順

1. `doc/HashMap_v1.md` を基に API と表示契約を確定する
2. `doc/要件定義v9.md` に `HashMap<$V>` と標準モジュール順を反映する
3. `lib/hash_map.srt` を追加する
4. `sindr::builtin` に builtin type / builtin functions を追加する
5. `scar` に `Ty::HashMap` と型解決を追加する
6. `sindr::runtime` / `eldr::builtin` に runtime 実装を追加する
7. std module loading と test harness の source list を更新する
8. spec / compile_errors / unit tests を追加する

---

## 12. Summary

v1 `HashMap` は次で固定するのが最も安全である。

- type: `HashMap<$V>`
- key: `String` 固定
- semantics: immutable
- display: `HashMap("key" => value)`
- future literal: `hash![...]`
- v1 lowering base: `HashMap::from_entries(...)`

この設計により、将来 syntax sugar を導入しても API と runtime 契約を大きく崩さずに拡張できる。
