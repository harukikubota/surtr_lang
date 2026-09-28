# Record の構造的分解と位置 path

Pattern の開発契約は [`docs/dev/Pattern_spec.md`](../docs/dev/Pattern_spec.md)、利用者向け surface は
[`docs/site/record.md`](../docs/site/record.md) と [`docs/site/pattern-matching.md`](../docs/site/pattern-matching.md) を正本とする。
この文書は実装・検証の対象を固定する作業入力である。

## 目的と対象

Record を、1個以上の名前付き public field を持つ nominal aggregate とする。構築と分解は全スコープで同じ field 構造を使う。Record の分解は compiler-owned な total structural Pattern であり、user-defined Extractor ではない。Struct の `new` / attached `deconstruct` と一般 Extractor の partial 契約は維持する。

field 名を省略する shorthand、field を省略する Pattern、Struct の Pattern 拡張は今回の対象外とする。

## 受入条件

```surtr
defrecord User(name: String, age: Int)
user = User(age: 20, name: "Ada")
User(name, age) = user
User(age: selected_age, name: selected_name) = user
match user { User(_, _) => "user" }
Facet::view(User._0, user) # User.name と同じ focus
```

- `defrecord Empty()` と `defrecord Empty( )` はいずれも定義時のエラー。`private` / `public` などの field 可視性指定と、位置 path と競合する `_N` 形式の field 名も定義時のエラー。
- Record Pattern は positional または全 field の named 形式に限る。named は順不同でよい。重複・未知・不足 field、両形式の混在を拒否する。子の照合と binding の順序は宣言順へ正規化する。
- Record head 内の `field: child` は常に名前付き field Pattern として解釈する。位置指定の子に型注釈を付ける場合は `(binding: Type)` のように括って区別する。
- Record の外枠は total。すべての子が total なら通常 `=` を許可する。literal や一般 Extractor など partial な子を含む Pattern 全体は `=` で拒否する。単一の `match` arm が catch-all かどうかも子を再帰的に判定する。複数 arm の部分 Pattern を合成した構造的網羅性解析は今回の対象外とする。
- `Type._N` / `value._N` は Record の field 順序で bounds check し、その field に対する名付きアクセスと同じ型・実行動作にする。`Tuple._N` を Record に適用せず、`Record._N` root を設けない。
- `:facet User._0` と位置 path を保持する binding の表示には、元の `._0` を表示する。フィールドアクセスの内部正規化で `User.name` に書き換えられても path origin を失わない。

## 影響範囲と順序

level 4。Spire の Record 宣言と Pattern argument、Sigil の Pattern head、Scar の型・totality・網羅性・Facet、Forge の分解と Facet metadata、Xldr の `:facet` 表示、関連仕様と利用者文書に及ぶ。

1. 正本を整合させ、Record 宣言の拒否境界を直接検証する。
2. positional / named Record Pattern の成功・拒否と totality を検証し、フェーズ間で宣言順 field metadata を保持する。
3. Record の位置 path を専用に検査し、名付き field との実行上の一致と origin 表示を検証する。
4. `rtk cargo nextest run --profile ci --workspace` と `cargo run -- test --quiet --all` を通し、独立レビューで仕様・最終差分・結果を確認する。
