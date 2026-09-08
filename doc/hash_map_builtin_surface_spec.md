# HashMap builtin surface への統一

## 目的

`HashMap` は `lib/types/hash_map.srt` の `impl HashMap` に、Eldr builtin の宣言と
Surtr で合成した高水準関数を持つ。現在は builtin へ単純転送するラッパーを別名で
公開しているため、同じ操作に二つの名前が存在する。この重複を除き、HashMap の
操作呼び出しを builtin 宣言の名前へ統一する。

## 現状調査

次の 9 関数は本体が対応する builtin の呼び出しだけであり、独自の意味論を持たない。

| 削除するラッパー | 置換先 builtin |
| --- | --- |
| `empty` | `empty_map` |
| `from_entries` | `map_from_entries` |
| `len` | `map_len` |
| `contains_key` | `map_contains_key` |
| `get` | `map_get` |
| `insert` | `map_insert` |
| `remove` | `map_remove` |
| `keys` | `map_keys` |
| `values` | `map_values_list` |

`entries` は `keys` と `values` を zip する合成関数、`map_values` はキー列の走査と
値変換を行う合成関数であるため削除しない。これらの内部参照だけは、削除後の
builtin 名へ置換する。

builtin の正本である `crates/sindr/src/builtin.rs` の metadata、Eldr の実装表と
関数本体、HashMap literal の Forge lowering は既に `empty_map` / `map_*` 名を
使っている。したがって今回の変更は標準定義、標準ライブラリ利用箇所、テストと
現行ドキュメントの API 表記に閉じ、builtin 実装自体は変更しない。

## 変更後の振る舞い

- `HashMap` の直接操作は上表の builtin 名を使う。
- `HashMap::entries` と `HashMap::map_values` の意味論、deterministic なキー順、
  duplicate key の後勝ち、`map_get` の miss 時 `Err(NoneError)` は維持する。
- `hash![...]` の lowering は `map_from_entries` builtin のまま維持する。
- 旧ラッパー名は宣言・標準ライブラリ利用コード・現行ドキュメントから除去する。
- `HashMap::empty_map` と `hash![]` は、型注釈などの expected type があれば成立する。
  型注釈なしで空 map だけを評価した場合に型引数を推論できない規則は変更しない。
  そのためテストと REPL 例では `HashMap<Int>` の型注釈を付ける。

## 対象範囲

- `lib/types/hash_map.srt` の 9 ラッパー削除と残存合成関数の内部呼び出し置換
- 標準ライブラリ、examples、fixture、Rust の source-string test、現行仕様・開発
  ドキュメントにある旧呼び出しの置換
- builtin metadata、Eldr runtime、Forge lowering、HashMap の型／runtime 表現は対象外

## 受入条件

1. `impl HashMap` に上表のラッパー宣言が残っていない。
2. 旧ラッパー名を呼ぶ `.srt` の利用箇所がなく、必要な箇所は対応する builtin 名へ
   置換されている。
3. `entries` と `map_values` が builtin 名だけを参照して従来の結果を返す。
4. HashMap の成功・拒否境界、および JSON／Facet／fixture／examples の利用が
   置換後も型検査できる。

## 検証

- `cargo run -- test --quiet lib/tests/hash_map.srt`
- `cargo run -- test --quiet --all`
- `rtk cargo nextest run -p rune --test integration run_srt`

Rust の builtin 実装を変更しないため、最終検証は変更した標準定義と、その標準定義を
読み込む Rune の script fixture に限定する。失敗・除外・未実行があれば完了報告で
明記する。

## level

level 1。既存の HashMap 言語仕様の範囲で、標準定義とその呼び出し名を整理する変更
であり、型規則・評価規則・フェーズ間契約は変更しない。
