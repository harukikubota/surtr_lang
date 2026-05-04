# Compile Time Dependency Gating

## 目的

- Rust 側のクリーンビルド時間を短縮する
- 通常の `surtr run/check/build/dump` で不要な対話 UI / schema 生成依存を default dependency graph から外す
- `cargo nextest run --workspace` と入口別の `run` / `xldr` 実行時間を before / after で記録する

## 実装日

- 2026-04-29 (Asia/Tokyo)

## 変更点

- `xldr`
  - `rustyline` を optional dependency に変更
  - `line-editor` feature を追加
  - default build では plain stdin/stdout REPL を使い、`--features line-editor` で履歴・補完つき REPL を有効化する
- `rune`
  - `line-editor = ["xldr/line-editor"]` feature を追加
- `sindr`
  - `schemars` を optional dependency に変更
  - `viewer-schema` feature を追加
  - `viewer_schema()` と schema derive を `viewer-schema` feature 配下へ移動
  - `export_viewer_schema` example に `required-features = ["viewer-schema"]` を追加
- `tests/profile/heavy_compile.srt`
  - `surtr run` 入口の測定用に、型検査・関数呼び出し・match・BigInt 実行が少し重い入力を追加

## 依存グラフ確認

default の `rune` dependency graph から消したもの:

- `rustyline`
- `fd-lock`
- `radix_trie`
- `nix`
- `schemars`
- `schemars_derive`

feature 有効時の確認:

- `cargo check -p rune --features line-editor`
- `cargo check -p sindr --features viewer-schema --example export_viewer_schema`

## 測定条件

- 比較対象:
  - `cargo clean && /usr/bin/time -p cargo nextest run --workspace`
- 入口測定:
  - `cargo run -p rune -- run tests/profile/heavy_compile.srt`
  - `printf ':quit\n' | cargo run -p rune -- repl --quiet`
  - `./target/debug/surtr run tests/profile/heavy_compile.srt`
  - `printf ':quit\n' | ./target/debug/surtr repl --quiet`

## Before / After

| case | before | after | note |
|---|---:|---:|---|
| clean nextest, Cargo build 表示 | 18.63s | 18.04s | `rustyline` / `schemars` 系の compile 行が消えた |
| clean nextest, nextest summary | 55.522s | 58.596s | テスト実行側の揺れが支配的 |
| clean nextest, `/usr/bin/time real` | 78.47s | 80.79s | 今回は wall-clock 全体では改善せず |
| clean nextest, `/usr/bin/time user` | 421.98s | 421.09s | ほぼ横ばい |
| clean nextest, `/usr/bin/time sys` | 15.05s | 16.05s | ほぼ横ばい |
| `cargo run ... run heavy_compile.srt` | 1.21s | 1.25s | cargo overhead 込み |
| `cargo run ... repl --quiet` | 1.02s | 1.13s | cargo overhead 込み |
| direct `surtr run heavy_compile.srt` | 0.66s | 0.62s | 実行入口のみ |
| direct `surtr repl --quiet` | 0.63s | 0.60s | 実行入口のみ |

## 読み取り

- 今回の効果は主に clean build の dependency graph 縮小で、実行入口の runtime にはほぼ影響しない
- `cargo nextest run --workspace` 全体は integration test の Surtr コンパイル・実行ワークロードに強く左右されるため、1 回測定の wall-clock だけでは改善判定に向かない
- build 表示では少し改善したが、`chumsky`, `regex`, `ariadne`, `serde`, `num-bigint` など通常入口に必要な依存がまだ大きい
- `sindr::viewer::viewer_schema` は default では利用できなくなったため、schema export が必要なときは `--features viewer-schema` を指定する

## 追加でプロファイルすべき項目

- `cargo build --timings`:
  - crate / dependency ごとの wall time と並列待ちを HTML で確認する
- `cargo nextest run --workspace --profile ci` 相当:
  - 並列数固定・fail-fast 条件固定で、テスト実行側の揺れを減らす
- `cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket_*`:
  - Surtr 仕様 fixture の実行時間が大きいので、bucket ごとの偏りを追う
- `cargo nextest run -p scar` / `cargo nextest run -p forge`:
  - 型検査・codegen の crate-local tests は 1 件あたり 0.7s 以上のものが多く、compiler pipeline 側の改善余地が見える
- `./target/debug/surtr build tests/profile/heavy_compile.srt /tmp/heavy.eldr` と `.eldr run`:
  - Surtr source compile と VM 実行を分離して、標準定義ソース load / typecheck / codegen / VM のどこが支配的かを見る
- `cargo llvm-lines` または `cargo bloat`:
  - monomorphization が重い型や関数を探す。特に parser / typechecker の generic-heavy な経路を見る
- incremental rebuild:
  - `touch crates/spire/src/parser/expr.rs`
  - `touch crates/sindr/src/viewer.rs`
  - `touch crates/xldr/src/repl/ui/cli.rs`
  - 変更対象ごとの再ビルド範囲を測る

## 次の改善候補

- `xldr` から loader / error display / REPL core をさらに分離し、`rune run/check/build` が REPL UI module を parse しない構造にする
- `sindr::viewer` を `sindr-viewer` crate に分離し、`dump --format json` だけが依存する形にする
- `rune` integration test の fixture bucket をさらに均等化する
- `scar` / `forge` の重い unit tests を fixture cache または shared bootstrap helper でまとめる
