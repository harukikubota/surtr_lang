# Surtr source compile measurement options audit

## 結論

現行の計測手段は、Rust の build、Surtr の fixture 実行、Scar の型検査内部を
それぞれ見るには足りている。しかし、標準定義と user source の compile
regression を継続的に追うための CLI 計測としては不十分である。

最大の問題は `surtr run --phase-times` である。出力項目には
`parse` / `resolve` / `typecheck` / `codegen` があるが、現行コードでこれらへ
時間を代入している箇所がない。そのため実測値は `n/a` になり、実際に測れるのは
compile 全体、execute、total の粗い時間だけである。

## 現行の計測手段

| 手段 | 見える範囲 | 評価 |
|---|---|---|
| `surtr run <file.srt> --phase-times` | compile 全体、execute、total | phase 欄が未実装。compile-only ではない |
| `SURTR_SCAR_PROFILE=1` | Scar の型検査イベント、statement / kind 集計 | 型検査内部には有効。ただし parse / resolve / codegen / stdlib cache 状態は見えない |
| `SURTR_TEST_TIMING=1` | `run_srt` / module fixture の fixture 総時間、遅い fixture、cache counters | fixture 単位の回帰検出には有効。compile phase の切り分けはない |
| `SURTR_TEST_CACHE=1` | integration の final `.eldr` cache | 測定条件を変える要因。hit/miss は test report でのみ確認できる |
| `/usr/bin/time -p` | CLI process の wall/user/sys | 外側の総時間のみ。標準定義と user source を分離できない |
| `cargo build --timings` / `cargo test --no-run --timings` | Rust crate / dependency の build time | Rust 側の回帰には必要だが、Surtr source compile の計測ではない |

## 現行実装との照合

- `crates/rune/src/commands/run.rs` の `PhaseTimes` は phase フィールドを持つが、
  `compile_ms` / `decode_ms` / `execute_ms` / `total_ms` 以外は設定されていない。
- `compile_ms` は source read 後の compile plan 作成、module source collection、
  cache lookup、stdlib snapshot、parse、resolve、typecheck、codegen、bytecode
  合成をまとめた値である。
- `run` は既定で final run cache を使用できるため、同じ入力の再測定では cache hit
  が compile を隠す。`SURTR_RUN_CACHE=0` は用意されているが、CLI の計測結果に
  hit/miss が出ない。
- stdlib は `xldr` の process-local snapshot と disk semantic cache を通る。
  `SURTR_SCAR_PROFILE=1` は Scar の内部値を詳しく出せるが、stdlib cold build と
  user suffix のどちらの結果かを一つの標準レポートとして識別できない。
- integration timing は fixture と cache layer を集計できるが、`run_srt` と
  `module_import_fixtures` に限定され、phase ごとの時間や `check` / `build` の
  compile-only 結果を直接返さない。

## 2026-09-08 の実測

```text
./target/debug/surtr run tests/profile/stdlib_prewarm.srt --phase-times
  parse: n/a
  resolve: n/a
  typecheck: n/a
  codegen: n/a
  compile: 245ms
  execute: 1ms
  total: 247ms
```

`heavy_compile.srt` でも phase 4 項目は `n/a` だった。一方、同じ build に
`SURTR_SCAR_PROFILE=1` を付けると、Scar の user suffix について
`total=63.392ms`、`check_stmt_loop=50.436ms`、`isolated_body=33.437ms` などは
取得できた。これは Scar 内部の原因調査には使えるが、compiler pipeline 全体の
代替にはならない。

また、`SURTR_TEST_TIMING=1 SURTR_TEST_CACHE=1` の fixture 実行では、例えば
`script pass bucket 0 fixtures=10 total=0.606s` と final cache の
`hit/miss/write`、slowest fixture が取得できた。この層は回帰の発見には有効だが、
遅い fixture が compile 起因か VM 起因かは直接分からない。

## 必要な最小補強仕様

これは今回実装する最小補強仕様である。

### 1. 共通の compile timing payload

`run` と `build` が同じ compile path の結果を返せるよう、次の情報を共通 payload
として定義する。

- `source_read`
- `compile_plan`
- `stdlib_load` と stdlib cache 状態（cold / process hit / disk hit）
- `parse_user` と追加 module の parse
- `resolve`
- `typecheck`
- `codegen`
- bytecode encode / output write
- final artifact cache lookup / store 状態
- `compile_total` と `total`

stdlib の内部 phase を常に再実行する必要はない。cache hit の場合は実時間を
`disk_hit` 等として記録し、未実行の phase は 0 と偽装せず `skipped` とする。

### 2. `--phase-times` の実装範囲

- `surtr build <file.srt> --phase-times` を compile-only の基準入口にする。
- `surtr run <file.srt> --phase-times` は compile と execute を分けて表示する。
- `check` と source を受け取る `dump` も同じ共通 payload を利用できる形にする。
- `.eldr` の run は decode / execute の計測として従来どおり別扱いにする。

出力は人間向け text に加えて、CI と before/after 比較用の JSON 形式を用意する。
JSON には schema version、入力 path、cache 状態、各 phase の duration と
`skipped` を含める。ミリ秒整数だけでなく、少なくともマイクロ秒精度を保持する。

### 3. 測定条件の固定

Rust 変更の影響を追う標準シナリオを次の 3 層に固定する。

1. Rust build: `cargo build --workspace --timings` と test build。
2. Surtr compile: `build` で stdlib cold / warm と user source を測る。
3. end-to-end: `run` で compile と VM execute を分離して測る。

Surtr compile の cold / warm は、既存の `SURTR_STDLIB_CACHE_DIR` と
`SURTR_RUN_CACHE=0` を使い、cache 状態をレポートへ必ず併記する。
fixture 全体の傾向は既存の `SURTR_TEST_TIMING=1` を使い、個別 phase の原因調査
では共通 compile timing payload と `SURTR_SCAR_PROFILE=1` を使う。

### 4. fixture と受入条件

- 標準定義の cold / warm を表す `stdlib_prewarm.srt`。
- user source の型検査・resolve・codegen を十分に含む `heavy_compile.srt`。
- 小さい user source を基準値として、stdlib cache の有無で user phase が変わらない
  ことを確認する。
- `--phase-times` の phase が `n/a` ではなく、実行または `skipped` として報告される。
- `build` と `run` の compile phase が同じ入力条件で一致する。
- cache hit / miss を変えた測定で、compile total の意味が明確に変わる。
- text 出力を壊さず、JSON は schema version を持つ。

## 対象外

- compiler pipeline の意味論変更
- stdlib snapshot / prefix cache の設計変更
- VM opcode stats の追加
- Rust build time と Surtr source compile time の一つの数値への合算

`--phase-times` は既存の text 出力を維持し、`--phase-times-json` を機械向けの
追加入口とする。未実行 phase は `skipped` とし、compile cache の hit/miss により
内訳が隠れる場合も計測結果から判別できるようにする。
