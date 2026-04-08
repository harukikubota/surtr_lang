# sindr クレート コードレビュー

対象: `crates/sindr/src/{builtin.rs, ir.rs, runtime.rs}`

---

## 1. バグ・潜在的な問題

### [重要] `ir.rs` — `CallBuiltin` / `Call` / `CallClosure` のタプルフィールドが無名

**場所:** `ir.rs` l.73–81

```rust
CallBuiltin(u16, u8, u32, u32),
Call(u32, u8, u32, u32),
CallClosure(u8, u32, u32),
```

各フィールドが何を意味するかコード上で判別できない。
`Forge` や `Eldr` 側でパターンマッチするとき、位置だけで意味を読み取ることになり
フィールド順の変更や誤った実装を静的に検出できない。

**対応案:** 構造体バリアントに切り替える。

```rust
CallBuiltin { builtin_id: u16, arity: u8, span_start: u32, span_end: u32 },
Call        { fun_idx: u32,    arity: u8, span_start: u32, span_end: u32 },
CallClosure { arity: u8, span_start: u32, span_end: u32 },
```

同様に `MakeError(u32)` / `MakeErrorLiteral(u32, u32)` / `StructNew(u32)` / `GetField(u32)` /
`ListNew(u32)` / `ListFromItems(u32)` なども、u32 が何を示すか名前だけでは自明でない。

---

### [重要] `ir.rs` — `decode_payload` のレガシーフォールバックがコラプションを隠蔽する

**場所:** `ir.rs` l.367–381

```rust
fn decode_payload(payload: &[u8]) -> Result<Bytecode, BytecodeFormatError> {
    match bincode::deserialize::<Bytecode>(payload) {
        Ok(bytecode) => Ok(bytecode),
        Err(err_new) => {
            match bincode::deserialize::<LegacyBytecode>(payload) {
                Ok(legacy) => Ok(legacy.into()),
                Err(err_legacy) => Err(BytecodeFormatError::DecodeFailed(...)),
            }
        }
    }
}
```

現フォーマットのデシリアライズに失敗した場合、常にレガシー解釈を試みる。
しかし `.eldr` ファイルが破損した場合でも、偶然レガシーとして解釈できてしまうと
誤ったバイトコードが無エラーで実行される危険がある。

また `VERSION` フィールドは `parse_container` でチェックしているが、
将来 v2 フォーマットを追加した際にレガシーへの誤フォールバックが起きうる。

**対応案:** バージョンを `.eldr` コンテナヘッダで管理し、バージョンに応じてどの
デシリアライザを使うかを明示的に分岐させる。

---

### [重要] `ir.rs` — `encode()` でペイロード長を `u32` にキャストする際に上限チェックなし

**場所:** `ir.rs` l.342

```rust
bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
```

`payload.len()` が `u32::MAX`（約 4 GB）を超えた場合、サイレントにトランケートされる。
現実的には発生しないが、ファイル破損に繋がるため明示的にチェックすべき。

**対応案:**

```rust
let payload_len = u32::try_from(payload.len())
    .map_err(|_| BytecodeFormatError::EncodeFailed("payload too large".into()))?;
bytes.extend_from_slice(&payload_len.to_le_bytes());
```

---

## 2. 設計上の指摘

### [中] `ir.rs` — `align4` がインライン重複している

**場所:** `ir.rs` l.343–345、l.443–445

```rust
// encode()
while bytes.len() % 4 != 0 {
    bytes.push(0);
}

// parse_container()
while offset % 4 != 0 && offset < bytes.len() {
    offset += 1;
}
```

`align4` 関数 (l.458) が定義されているにもかかわらず、両箇所でインラインの while ループが使われている。
`parse_container` 側の条件 `&& offset < bytes.len()` は `align4` と等価でなく、
バイト列末尾での動作が微妙に異なる。

**対応案:** 両箇所とも `align4` を使い統一する。

```rust
// encode()
bytes.resize(align4(bytes.len()), 0);

// parse_container()
offset = align4(offset);
```

---

### [中] `ir.rs` — `decode()` が `inspect()` を経由して不要なアロケーションを行う

**場所:** `ir.rs` l.362–364

```rust
pub fn decode(bytes: &[u8]) -> Result<Self, BytecodeFormatError> {
    Self::inspect(bytes).map(|inspected| inspected.bytecode)
}
```

`inspect()` は `EldrHeader` と `Vec<EldrChunkInfo>` を構築するが、
`decode()` はそれらをすぐ捨てる。デコードだけが目的なら直接 `parse_container` +
`decode_payload` を呼ぶほうが効率的。

---

### [中] `runtime.rs` — `TypeRegistry::lookup` が O(n) 線形探索

**場所:** `runtime.rs` l.37–39

```rust
pub fn lookup(&self, tag: u32) -> Option<&TypeEntry> {
    self.entries.iter().find(|entry| entry.tag == tag)
}
```

`Value::to_display_string` はリスト要素の再帰表示などで lookup を繰り返し呼ぶ。
型数が増えると表示コストが O(型数 × 値の深さ) になる。

**対応案:** `HashMap<u32, usize>` でタグ→インデックスを保持するか、
`indexmap` クレートを使う。ただし現在の Surtr の規模では許容範囲内。

---

### [軽微] `runtime.rs` — `ListNode` が単一バリアントの enum

**場所:** `runtime.rs` l.67–69

```rust
pub enum ListNode {
    Cons(Value, ListRef),
}
```

バリアントが一つしかない enum は struct で代替できる。
`match` 時に常に全バリアントが一致するため、enum の恩恵がない。

---

### [軽微] `runtime.rs` — `tail_handle()` の `saturating_sub` は不要

**場所:** `runtime.rs` l.202–209

```rust
Some(node) => match node.as_ref() {
    ListNode::Cons(_, next) => Some(Self {
        head: next.clone(),
        len: self.len.saturating_sub(1),
    }),
},
```

このアームには `self.head` が `Some` のとき（非空リスト）しか到達しないため、
`self.len >= 1` が保証されている。`saturating_sub` は不要。

---

### [軽微] `builtin.rs` — `BUILTIN_UID_BASE = 2` にコメントがない

**場所:** `builtin.rs` l.14

```rust
pub const BUILTIN_UID_BASE: u32 = 2;
```

なぜ 0 や 1 ではなく 2 から始まるのかが不明。
予約されている UID 0 / 1 が何を指すかのコメントがあると、
`sigil` や `scar` を読む際の手掛かりになる。

---

### [軽微] `runtime.rs` — `RichError::to_display_string` がメッセージを `{:?}` でフォーマット

**場所:** `runtime.rs` l.248–250

```rust
pub fn to_display_string(&self) -> String {
    format!("{}({:?})", self.kind, self.message)
}
```

`{:?}` はデバッグ表記のため、メッセージが `"boom"` の場合
`TestError("boom")` と引用符付きで表示される。
ユーザー向けエラーメッセージとしては `{}` の方が自然。
テスト (`display_for_rich_error_uses_message`) がこの挙動を確認しているため、
意図的であれば `/// NOTE:` コメントで理由を明示することを推奨する。

---

## 3. テスト網羅の不足

現在 13 テスト（builtin: 1、ir: 8、runtime: 4）で主要パスはカバーされているが、
以下が未テスト。

| モジュール | 未テスト項目 | 優先度 |
|---|---|---|
| `ir.rs` | `BytecodeFormatError::UnsupportedVersion` — version != 1 で失敗するか | 高 |
| `ir.rs` | `BytecodeFormatError::TruncatedChunkHeader` — ヘッダ途中で切れたバイト列 | 高 |
| `ir.rs` | `BytecodeFormatError::TruncatedChunkData` — チャンクデータが宣言サイズより短い | 高 |
| `ir.rs` | `inspect()` — chunks / header フィールドの内容確認 | 中 |
| `ir.rs` | `line_column_for_offset` — マルチバイト UTF-8 文字を含む行でのカラム計算 | 中 |
| `builtin.rs` | `builtin_meta_by_id` — 存在しない ID (`None` が返るか) | 中 |
| `builtin.rs` | `builtin_meta_by_name` — 存在しない名前 (`None` が返るか) | 中 |
| `runtime.rs` | `ListHandle::head_value()` / `tail_handle()` を空リストに呼んだとき `None` | 中 |
| `runtime.rs` | `Value::Callable` の `to_display_string` | 低 |
| `runtime.rs` | `Value::Tagged` でフィールド数と `field_names` 数が一致しない場合の表示 | 低 |

---

## 4. 推奨対応まとめ

### 優先度 高

1. **`CallBuiltin` / `Call` / `CallClosure` を構造体バリアントに変更**
   — 無名フィールドによる可読性の低下と誤実装リスクを排除する。

2. **`encode()` でペイロード長を `u32::try_from` でチェック**
   — サイレントトランケーションを防ぐ。

3. **`UnsupportedVersion` / `TruncatedChunk*` のエラーパステスト追加**
   — 既定義エラーが実際に返るか確認する。

### 優先度 中

4. `decode_payload` のレガシーフォールバックをバージョン番号で制御する
5. `align4` のインライン重複を解消し関数呼び出しに統一
6. `builtin_meta_by_id` / `builtin_meta_by_name` の境界値テスト追加
7. `ListHandle::head_value()` / `tail_handle()` の空リストテスト追加

### 優先度 低

8. `ListNode` を単一バリアント enum から struct へ変換
9. `BUILTIN_UID_BASE` にコメントで予約 UID の意味を記載
10. `RichError::to_display_string` の `{:?}` → `{}` 変更、または意図をコメントで明示
