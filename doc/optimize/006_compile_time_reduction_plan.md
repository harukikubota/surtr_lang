# Compile Time Reduction Plan

## 目的

- Rust 側の clean build / test build を短縮する
- 日常開発の編集対象ごとの再コンパイル波及を小さくする
- 依存 feature 調整だけでなく、`scar` / `diagnostics` の中期的な test / module 構造も整理する

## 作成日

- 2026-04-29 (Asia/Tokyo)

## 現状計測

計測は既存 `target/` を避け、別 `CARGO_TARGET_DIR` で実施した。

```bash
CARGO_TARGET_DIR=/tmp/surtr-cargo-timing-base /usr/bin/time -p cargo build --workspace --timings
CARGO_TARGET_DIR=/tmp/surtr-cargo-timing-testbuild /usr/bin/time -p cargo test --workspace --no-run --timings
```

### clean build

| case | real | user | sys |
|---|---:|---:|---:|
| `cargo build --workspace --timings` | 11.09s | 39.82s | 4.89s |

上位 compile units:

| unit | duration | 主な理由 |
|---|---:|---|
| `object` | 2.6s | `spire -> chumsky(default) -> stacker -> psm` 経由 |
| `scar` | 2.1s | 型検査器本体が最大の内部 crate |
| `serde_core` | 1.9s | `sindr` の bytecode / JSON 系 |
| `regex-syntax` | 1.8s | `eldr` の regex builtin |
| `chumsky` | 1.8s | parser |
| `sindr` | 1.7s | IR / runtime / serde が集中 |
| `regex-automata` | 1.6s | regex builtin |
| `diagnostics` | 1.1s | 大きな単一 module と `scar` / `spire` 依存 |
| `spire` | 1.0s | parser crate |
| `sigil` | 1.0s | resolver crate |

### clean test build

| case | real | user | sys |
|---|---:|---:|---:|
| `cargo test --workspace --no-run --timings` | 15.50s | 65.55s | 8.16s |

上位 compile units:

| unit | duration | 読み取り |
|---|---:|---|
| `scar lib(test)` | 4.8s | unit test harness 生成込みで最大 |
| `scar` | 3.2s | 通常 lib としても最大 |
| `sindr lib(test)` | 2.7s | IR / runtime / serde 系 |
| `diagnostics lib(test)` | 2.3s | diagnostics unit tests が重い |
| `spire lib(test)` | 2.2s | parser unit tests |
| `eldr lib(test)` | 2.2s | VM / builtin tests |
| `xldr lib(test)` | 2.2s | REPL / loader tests |
| `sigil lib(test)` | 2.1s | resolver tests |
| `forge lib(test)` | 2.0s | codegen tests |
| `rune integration(test)` | 1.9s | integration harness |

## 実施方針

### 1. 低リスク feature 調整

#### 1.1 `chumsky` から `stacker` を外す

Status: Done (`codex/compile-time-reduction-1`)

変更候補:

```toml
# crates/spire/Cargo.toml
chumsky = { version = "0.12.0", default-features = false, features = ["std"] }
```

効果見込み:

| case | before | after |
|---|---:|---:|
| clean build | 11.09s | 10.46s |

意図:

- `object`, `psm`, `cc`, `ar_archive_writer` などの C / object 系 build dependency を default build から外す
- parser の機能要件は `std` のみで満たす

確認:

- `cargo test --workspace`
- 深いネスト入力の parser regression を追加または既存 fixture で確認
- stack overflow が再発する入力がある場合は rollback し、`chumsky/stacker` を feature opt-in にする

実施結果:

- `crates/spire/Cargo.toml` を候補どおり変更した
- `Cargo.lock` から `stacker`, `psm`, `object`, `cc`, `ar_archive_writer` が外れた
- `cargo tree -p spire -e features` で `chumsky feature "std"` のみを確認した
- `crates/spire/src/parser/tests.rs` に 16 段の括弧ネスト regression を追加した
- 64 / 256 段の括弧ネストは現行 parser 側の再帰で stack overflow するため、`doc/open-issues.md` の OI-017 として分離した

#### 1.2 `regex` feature を絞る

Status: Done (`codex/compile-time-reduction-1`)

変更候補:

```toml
# crates/eldr/Cargo.toml
regex = { version = "1.11.1", default-features = false, features = ["std", "unicode"] }
```

効果見込み:

| case | before | after |
|---|---:|---:|
| clean build (`chumsky` 調整込み) | 10.46s | 9.72s |
| clean test build (`chumsky` 調整込み) | 15.50s | 13.11s |

意図:

- Surtr の regex builtin の Unicode 意味論は維持する
- `perf-*` 系 feature を外し、compile cost を下げる

確認:

- `eldr::builtin` の regex unit tests
- `tests/spec` / `tests/integration` の regex 関連 fixture
- runtime regex workload がある場合は before / after benchmark を取る

rollback 条件:

- regex builtin の実行時間が実用上問題になる
- `Regex::find_all` / `split` / `replace_all` などの標準関数で明確な regression が出る

実施結果:

- `crates/eldr/Cargo.toml` を候補どおり変更した
- `cargo tree -p eldr -e features` で `regex` の `std` / `unicode` feature を確認した
- `perf-*` feature が default graph に出ないことを確認した
- `cargo test -p eldr regex` で regex builtin unit tests が通ることを確認した
- `cargo test -p rune language_features_bucket_` で surface 経由の regex wrapper を含む language feature bucket が通ることを確認した

#### 1.3 `dev` / `test` profile の debug 情報を軽くする

Status: Done (`codex/compile-time-reduction-1`)

変更候補:

```toml
# Cargo.toml
[profile.dev]
debug = 1

[profile.test]
debug = 1
```

効果見込み:

| case | before | after |
|---|---:|---:|
| clean build (`chumsky` + `regex` 調整込み) | 9.72s | 9.83s |
| clean test build (`chumsky` + `regex` 調整込み) | 13.11s | 12.65s |

読み取り:

- wall-clock への効果は小さい
- CPU 時間と生成物サイズには効く可能性がある
- debug 情報を使う調査が多い場合は導入を分けて判断する

実施結果:

- workspace root `Cargo.toml` に `[profile.dev] debug = 1` と `[profile.test] debug = 1` を追加した
- `cargo nextest run --workspace` で全体回帰を確認した

### 1.4 実施時の検証ログ

2026-04-29 に 1.1〜1.3 を同一 branch で実施した。

```bash
cargo test -p spire test_deep_parenthesized_expression_parses_without_stacker_feature
cargo test -p eldr regex
cargo test -p rune language_features_bucket_
cargo nextest run --workspace
```

結果:

- `cargo test -p spire test_deep_parenthesized_expression_parses_without_stacker_feature`: pass
- `cargo test -p eldr regex`: pass
- `cargo test -p rune language_features_bucket_`: pass
- `cargo nextest run --workspace`: 757 passed, 7 skipped

未解決事項:

- 深い `Grouped` 式の stack overflow は OI-017 として追跡する
- clean build / clean test build の before / after 再計測は、main 統合後に別 `CARGO_TARGET_DIR` で実施する

## 2. `scar` test build の中期改善

`scar lib(test)` が 4.8s と最大のため、テスト配置を整理する。

### 2.1 テスト分類

`scar` tests を次の 2 層へ分ける。

| 層 | 配置 | 対象 |
|---|---|---|
| public API tests | `crates/scar/tests/*.rs` | `typecheck(...)` で検証できる言語 surface |
| private invariant tests | `crates/scar/src/**` の `#[cfg(test)]` | env / checker 内部状態 / 型環境の局所不変条件 |

意図:

- surface の型検査テストを integration test crate へ移し、`scar lib(test)` の巨大化を抑える
- private API に依存する少数のテストだけを unit test に残す
- `cargo nextest run -p scar` の実行意味は維持する

手順:

1. `crates/scar/src/lib.rs` の大きな `#[cfg(test)]` module を棚卸しする
2. `parse -> resolve -> typecheck` helper を `crates/scar/tests/support.rs` に作る
3. public API だけで書けるテストを `crates/scar/tests/typecheck_*.rs` へ移す
4. private invariant test は最小限だけ残す
5. 移動後に `cargo test -p scar --no-run --timings` で `scar lib(test)` の縮小を確認する

受け入れ条件:

- 移動前後で `cargo test -p scar` のテスト件数と意味が維持される
- `scar lib(test)` の compile duration が有意に下がる
- private helper を `pub` 化しない

### 2.2 標準 prelude 初期化の共有

既存の `003_compile_time_followup_profile.md` / `004_forge_test_prelude_cache.md` と同じ方向で、`scar` test helper の標準 module parse / precollect を `OnceLock` で共有する。

対象:

- `scar` の unit / integration test helper
- 標準 module stages
- `DeclarationIndex`
- builtin prelude 相当の初期環境

受け入れ条件:

- production path に影響しない
- failed test が共有状態を汚さない
- `cargo nextest run -p scar` の summary / real が before より下がる

## 3. `diagnostics` の中期改善

`crates/diagnostics/src/lib.rs` は約 6,261 行あり、test build でも上位に出ている。

### 3.1 module 分割

候補分割:

```text
crates/diagnostics/src/
├── lib.rs
├── source.rs
├── report.rs
├── render.rs
├── parse.rs
├── resolve.rs
├── typecheck.rs
├── runtime.rs
├── spans.rs
└── tests/
```

意図:

- incremental compilation の再利用粒度を改善する
- review / debugging 時の編集対象を小さくする
- `lib.rs` を公開 API と module wiring に寄せる

受け入れ条件:

- public API の破壊を避ける
- error output の snapshot 的な期待値を変えない
- `cargo test -p diagnostics` が移動前後で同じ意味を保つ

### 3.2 diagnostics tests の外出し

`diagnostics` も `scar` と同じく、public API で検証できるものは `crates/diagnostics/tests/*.rs` へ移す。

残す unit tests:

- private span helper の境界値
- renderer 内部の正規化
- source registry の局所不変条件

外へ移す tests:

- parse / resolve / typecheck / runtime の error rendering surface
- serializable report の公開契約
- CLI 相当の human-readable output 契約

## 4. 依存境界の見直し

### 4.1 `diagnostics -> scar` 依存の圧縮

現状 `diagnostics` は `scar` / `spire` に直接依存する。これにより `scar` 変更時に `diagnostics` 以降も再ビルドされる。

中期候補:

- phase error 型から diagnostics 表示に必要な最小 report 型へ変換する境界を明確化する
- `diagnostics` が重い phase crate の内部型に触る範囲を限定する
- ただし phase 固有エラー型を維持する AGENTS.md の方針は守る

受け入れ条件:

- エラー型の所有 crate は変えない
- human / JSON output の構造は維持する
- 依存境界変更で診断品質を落とさない

### 4.2 `rune` / `xldr` の default dependency graph 維持

既に `line-editor`, `tui`, `viewer-schema`, `bench` は opt-in 化されている。今後も default build に対話 UI / schema / benchmark 専用依存を戻さない。

確認:

```bash
cargo tree -p rune -e features
cargo tree -p xldr -e features
cargo tree -p sindr -e features
```

## 5. 計測と完了条件

各段階で同じ条件の before / after を残す。

必須計測:

```bash
rm -rf /tmp/surtr-cargo-timing-*
CARGO_TARGET_DIR=/tmp/surtr-cargo-timing-build /usr/bin/time -p cargo build --workspace --timings
CARGO_TARGET_DIR=/tmp/surtr-cargo-timing-test /usr/bin/time -p cargo test --workspace --no-run --timings
cargo nextest run --workspace
```

重点確認:

- `cargo tree -p spire -e features`
- `cargo tree -p eldr -e features`
- `cargo test -p spire`
- `cargo test -p eldr`
- `cargo test -p scar --no-run --timings`
- `cargo test -p diagnostics --no-run --timings`
- `cargo nextest run -p rune --test integration`

Definition of Done:

- clean build が 11.09s より短い
- clean test build が 15.50s より短い
- `object` が default graph の上位 compile unit から消えている
- `regex-automata` / `regex-syntax` の feature list から不要な `perf-*` が消えている
- `scar lib(test)` の duration が 4.8s から有意に下がっている
- `diagnostics lib(test)` の duration が 2.3s から有意に下がっている
- `cargo nextest run --workspace` が通る

## 実装順

1. `chumsky` feature 調整
2. `regex` feature 調整
3. `dev` / `test` profile 調整は単独 commit で判断可能にする
4. `scar` tests の public / private 分類と外出し
5. `scar` test helper の prelude cache
6. `diagnostics` module 分割
7. `diagnostics` tests の外出し
8. 依存境界の追加見直し
9. before / after を本ファイルまたは後続 `007_*` 実装記録へ反映する
