# 未確定 Enum コンストラクタ型の check 拒否

## 状態

実装仕様。型検査完了時の未確定型変数の扱いを変更するため level 4 とする。

正本は `doc/要件定義v9.md`、診断契約は `docs/dev/diagnostics.md` とし、本書と整合させる。

## 目的と対象外

`surtr check` が、payload や expected type から確定できない Enum constructor の型変数を
成功として通さず、型検査エラーにする。

対象は既存の bare enum constructor である。新しい expression syntax、型引数の部分指定、
構造体 constructor、builtin-special `Result` constructor、callable の return type argument、
Trait dispatch の規則は変更しない。
既存の `Enum<_, ...>::Variant` も同じ finalization を通るが、`_` を payload または expected
type から決定する既存規則を変更するものではない。

## 現状

次のように constructor の結果を Boolean 消費 call に渡しても、Enum の一部の型引数は制約を
得ない。しかし現行の `surtr check` は成功する。

```surtr
Option::is_some(Option::None)
Either::is_left(Either::Left("term"))
```

これは `Option::None` の element type、および `Either::Left("term")` の right type が
未確定のまま型検査を通過する問題である。

## 変更後の規則

Enum constructor の Enum application に含まれる inference variable は、型検査完了前に
payload、expected type、binding annotation、関数の引数・戻り値などの通常制約で全て
解決されなければならない。未解決の variable が一つでも残れば error とする。

```surtr
none: Option<Int> = Option::None
left: Either<String, Int> = Either::Left("term")
```

上記は expected type により全て決まるため成功する。

次は失敗する。

```surtr
Option::is_some(Option::None)
Either::is_left(Either::Left("term"))
value = Option::None
```

消費側の戻り値が型変数を観測しないこと、値が unused であること、候補の宣言順は型を
決定する根拠にならない。`Unit` その他への既定化や runtime representation による選択は
行わない。

## 診断

未確定 Enum constructor 型は構造化された型診断とする。診断には constructor の source
span、未確定の型引数 ordinal、確定に使える制約が不足していることを含める。既存の
callable return type argument 専用診断を名前だけ流用せず、Enum constructor を origin として
区別できる構造データを持たせる。

## フェーズ別の責務

- Scar: 通常 Enum の constructor instantiate 後の型変数を追跡し、program specialization / finalization で
  未解決なら失敗する。候補探索の fallback を追加しない。
- Forge / Eldr: 型検査を通過した constructor の tag/payload lowering は変更しない。
- Diagnostics: Enum constructor の未確定型を source span と ordinal を持つ構造化診断として
  出力する。

## 受入条件

1. `Option::is_some(Option::None)` と
   `Either::is_left(Either::Left("term"))` を `surtr check` が拒否する。
2. expected type で全 type parameter が決まる `Option::None` と `Either::Left("term")` は通る。
3. unused binding の `value = Option::None` も通さない。
4. 未確定型を `Unit`、候補順、runtime representation で解決しない。
5. 診断は Enum constructor と未確定 type argument ordinal を構造化データとして示す。
6. 関連する Scar の成功・拒否境界を検証する。実装時の level 4 検証は
   `cargo run -- test --quiet --all` と
   `rtk cargo nextest run --profile ci --workspace` を含む。
