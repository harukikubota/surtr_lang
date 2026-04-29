# Compile Time Follow-up Profile

## 目的

- `002_compile_time_dependency_gating.md` の次段として、追加プロファイルを行う
- Rust build、Surtr source compile、VM 実行、fixture bucket、Scar unit tests を分けて見る
- 測定から見えた低リスクなテスト高速化を実施する

## 実装日

- 2026-04-29 (Asia/Tokyo)

## Cargo timings

コマンド:

```bash
cargo clean
/usr/bin/time -p cargo build --workspace --timings
```

結果:

- `real`: 13.32s
- `user`: 39.50s
- `sys`: 5.09s
- HTML report: `target/cargo-timings/cargo-timing-20260429T022448.720622Z.html`

上位 compile units:

| unit | duration |
|---|---:|
| scar | 2.87s |
| object | 2.20s |
| libc build script run | 2.13s |
| object build script run | 2.09s |
| serde_json build script run | 1.93s |
| zmij build script run | 1.80s |
| chumsky | 1.80s |
| sindr | 1.74s |
| syn | 1.67s |
| diagnostics | 1.67s |
| regex-syntax | 1.66s |
| regex-automata | 1.54s |
| aho-corasick | 1.46s |
| xldr | 1.42s |
| forge | 1.30s |

## Source compile と VM 実行の分離

測定入力:

- `tests/profile/heavy_compile.srt`

コマンド:

```bash
rm -f /tmp/surtr-heavy.eldr
/usr/bin/time -p ./target/debug/surtr build tests/profile/heavy_compile.srt /tmp/surtr-heavy.eldr
/usr/bin/time -p ./target/debug/surtr run /tmp/surtr-heavy.eldr
/usr/bin/time -p ./target/debug/surtr run tests/profile/heavy_compile.srt >/tmp/surtr-heavy-run.out
/usr/bin/time -p ./target/debug/surtr run /tmp/surtr-heavy.eldr >/tmp/surtr-heavy-eldr.out
```

結果:

| case | real | user | sys |
|---|---:|---:|---:|
| `.srt -> .eldr` build | 1.25s | 0.59s | 0.01s |
| direct `.srt` run | 0.61s | 0.58s | 0.01s |
| prebuilt `.eldr` run | 0.01s | 0.00s | 0.00s |

読み取り:

- この入力では VM 実行より source compile 側が支配的
- `.eldr` 実行はほぼ無視できるため、次の runtime 最適化では別の長時間 VM workload を用意する必要がある

## run_srt bucket

コマンド:

```bash
/usr/bin/time -p cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket_0
/usr/bin/time -p cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket_1
/usr/bin/time -p cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket_2
/usr/bin/time -p cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket_3
```

結果:

| bucket | nextest summary | real | user | sys |
|---|---:|---:|---:|---:|
| bucket 0 | 5.733s | 8.32s | 7.41s | 0.62s |
| bucket 1 | 5.861s | 6.14s | 5.88s | 0.10s |
| bucket 2 | 5.301s | 5.57s | 5.38s | 0.11s |
| bucket 3 | 5.336s | 5.57s | 5.40s | 0.09s |

読み取り:

- bucket 自体はおおむね均等
- workspace 全体の nextest で `run_srt` が 11s 以上に見えるのは、並列実行時の CPU 競合が大きい

## Scar unit tests

Before:

```bash
/usr/bin/time -p cargo nextest run -p scar
```

- build: 2.72s
- nextest summary: 11.443s
- `real`: 15.03s
- `user`: 67.69s
- `sys`: 1.79s

変更:

- `crates/scar/src/lib.rs` の `#[cfg(test)]` helper に `OnceLock` cache を追加
- 標準 module stages と `DeclarationIndex` をテストプロセス内で共有
- override を使う契約テストや user module を追加する helper は従来通り個別構築

After:

```bash
/usr/bin/time -p cargo nextest run -p scar
```

- build: 2.52s
- nextest summary: 9.187s
- `real`: 12.49s
- `user`: 65.16s
- `sys`: 2.54s

読み取り:

- Scar unit tests の summary は約 20% 短縮
- `safebind_*` 系の 2s 前後だったテストが 0.6s 前後まで落ちた
- まだ 0.6s 以上のテストが多いため、次は typecheck context / builtin env の共有可能性を調べる価値がある

## Workspace verification

コマンド:

```bash
/usr/bin/time -p cargo nextest run --workspace
```

結果:

- build: 3.97s
- nextest summary: 45.684s
- `real`: 53.46s
- `user`: 335.76s
- `sys`: 9.71s
- `756 passed`, `7 skipped`

補足:

- 直前の `cargo build --workspace --timings` 後の warm target で実行
- clean build 比較ではないため、`003` の workspace 値は「今回の追加変更が全体テストで通ること」と「warm target の目安」として扱う

## 次の候補

- `scar` の `TypeEnv` / builtin signatures 初期化をテスト専用 fixture として共有できるか確認する
- `rune` integration tests のプロセス起動回数を減らせるか確認する
- `heavy_compile.srt` とは別に、VM 実行が支配的な workload を追加する
- `cargo build --timings` の HTML を保存対象にするか、要約だけ doc に残す運用にするか決める
