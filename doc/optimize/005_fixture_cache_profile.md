# Fixture Cache Profile

## 目的

- 既存の `SURTR_TEST_CACHE=1` が integration fixture にどれだけ効くか確認する
- nextest profile で cache を opt-in 化できるか確認する

## 実施日

- 2026-04-29 (Asia/Tokyo)

## 測定対象

```bash
cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket_0
```

## 結果

cache cold:

```bash
rm -rf target/test-fixture-cache/eldr
SURTR_TEST_CACHE=1 /usr/bin/time -p cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket_0
```

- build: 1.04s
- nextest summary: 5.865s
- `real`: 7.73s
- `user`: 6.30s
- `sys`: 0.68s

cache warm:

```bash
SURTR_TEST_CACHE=1 /usr/bin/time -p cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket_0
```

- build: 0.03s
- nextest summary: 0.132s
- `real`: 0.36s
- `user`: 0.23s
- `sys`: 0.07s

## nextest profile 試行

`.config/nextest.toml` に次の profile を一時的に試した。

```toml
[profile.cached]
fail-fast = false
status-level = "pass"
final-status-level = "slow"

[profile.cached.env]
SURTR_TEST_CACHE = "1"
```

nextest 0.9.132 では次の warning が出て、env 設定は無視された。

```txt
warning: in config file .config/nextest.toml, ignoring unknown configuration key: profile.cached.env
```

そのため、現時点では cache は README 通りに env var で明示する。

## 読み取り

- warm cache は非常に効く
- cold cache は通常 run とほぼ同じなので、CI の fresh workspace では効果が薄い
- local iterative run では `SURTR_TEST_CACHE=1` を使う価値が高い
- nextest profile では env を直接設定できなかったため、必要なら shell alias / task runner / cargo xtask で包むのが安全
