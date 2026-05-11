# Scar Surface Test Harness Cleanup

## 目的

- `cargo nextest run -p scar` / workspace 全体実行時に、Scar surface tests が libtest subprocess を大量生成する問題を抑える。
- clean test build で `scar lib(test)` に巨大な surface harness を抱え込まない構造へ寄せる。
- Surtr source (`*.srt`) の仕様 fixture は変更せず、Rust crate / test harness 側だけを整理する。

## 作成日

- 2026-05-10 (Asia/Tokyo)

## 根本原因

`crates/scar/src/typecheck_surface_tests.rs` には 212 個の `#[test]` があり、各 test が `parse -> resolve -> typecheck` helper を通る。

helper には `OnceLock` による std prelude cache があるが、`cargo nextest` は通常 `#[test]` ごとに test binary を別 subprocess として実行する。そのため cache は test process 内でしか効かず、surface test ごとに std module stages / declaration index / `ScarCheckpoint` の構築を繰り返していた。

また public surface test が `src/**` の unit test として置かれていたため、clean test build では `scar lib(test)` が大きくなりやすかった。

## 実施内容

- `crates/scar/src/typecheck_surface_tests.rs` を `crates/scar/tests/typecheck_surface.rs` へ移動した。
- `crates/scar/src/test_support/mod.rs` を `crates/scar/tests/support/mod.rs` へ移動した。
- `crates/scar/src/lib.rs` から surface test 用 `#[cfg(test)]` module を削除した。
- 212 個の surface case は関数として残し、登録 test は `typecheck_surface_suite` 1 件に集約した。
- suite 内では `SURFACE_WORKER_COUNT = 8` の worker で case を並列実行し、std prelude cache は同一 process 内で共有する。
- `std_module_stages()` は通常 caller では cached prelude の stages clone を返し、override test だけ uncached builder を使う形にした。

## 計測

計測は 2026-05-10 に実施した。`HEAD` は作業前の `9c2e5e4d` を一時 worktree で測った。

### warm test run

| case | command | tests | real | user | sys |
|---|---|---:|---:|---:|---:|
| before | `cargo nextest run -p scar` | 221 | 48.97s | 348.43s | 11.42s |
| after | `cargo nextest run -p scar` | 10 | 24.06s | 169.63s | 5.14s |

読み取り:

- wall-clock は約 51% 短縮した。
- CPU time もほぼ半減した。
- surface case の意味は維持しているが、nextest 上の test 数は suite 集約により減る。

### clean test build

| case | command | real | user | sys |
|---|---|---:|---:|---:|
| before | `CARGO_TARGET_DIR=/tmp/surtr-head-scar-target cargo test -p scar --no-run --timings` | 11.68s | 26.20s | 2.79s |
| after | `CARGO_TARGET_DIR=/tmp/surtr-scar-clean-target cargo test -p scar --no-run --timings` | 12.31s | 24.02s | 2.81s |

読み取り:

- `scar lib(test)` から巨大 surface module は外れた。
- 一方で `typecheck_surface` integration test target が増えたため、今回の単体 clean `--no-run` wall-clock は微増した。
- clean build の主目的に対しては、今後さらに surface case の削減または fixture 化が必要。

## 残った課題

- `typecheck_surface.rs` はまだ 212 case を保持している。実行時間の大部分は各 case が小さな source snippet を個別に parse / resolve / typecheck することに残っている。
- Facet / Process / trait helper 周辺には、`tests/fixtures/**` / integration fixture と重複する legacy surface tests が多い。
- clean build 短縮をさらに進めるなら、Scar に残すのは typed IR / metadata / private invariant を直接見る case に絞り、user-visible behavior は既存 fixture 層へ寄せる。

## 今後の方針

- `crates/scar/tests/typecheck_surface.rs` は「公開 API で見たい Scar 固有の typed result」に限定する。
- user-visible 成功 / 失敗は `tests/fixtures/script/**` と `tests/fixtures/modules/**` を正本にする。
- 新規型機能追加が薄い前提では、型周りの surface regression を増やすより、既存 case の重複削除を優先する。
- clean timing は workspace 全体と package 単体を分けて記録し、warm nextest の改善と混同しない。

## 検証コマンド

```bash
cargo fmt -p scar
cargo nextest run -p scar
rm -rf /tmp/surtr-scar-clean-target
CARGO_TARGET_DIR=/tmp/surtr-scar-clean-target cargo test -p scar --no-run --timings
cargo nextest run --workspace
```

2026-05-10 の最終確認:

- `cargo nextest run -p scar`: 10 passed
- `cargo nextest run --workspace`: 1064 passed
