# Mahjong Example

Surtr の非自明なサンプルプロジェクトです。
麻雀の手牌文字列を parse し、検証済みドメイン値へ変換して、和了形と役を判定します。

## Run

```bash
cargo run -p rune -- run examples/mahjong/run.srt
```

既定入力は `123m456789p567s11w` です。
CLI 入口は `examples/mahjong/src/6_cli.srt` にあります。

## Test

```bash
cargo run -p rune -- run examples/mahjong/test/yaku_pure.srt
```

役判定と面子分解の代表ケースを Surtr のコードだけで確認します。

## What To Read

- `src/2_normalize.srt`: `MahjongDomain::parse_valid_hand14` が `>=>` で `parse -> normalize -> validate` をつなぐ中心のドメイン変換です。
- `src/9_extractor.srt`: `defextractor sequence` / `triplet` が `ExtractedMeld` を pattern-side API として分解します。
- `src/4_judge.srt`: Extractor pattern と List/Result 系の関数演算子を使って役候補を組み立てます。
- `test/yaku_pure_impl.srt`: サンプルの読みやすい利用例です。

設計上の正本は `doc/example_project_mahjong.md` です。
