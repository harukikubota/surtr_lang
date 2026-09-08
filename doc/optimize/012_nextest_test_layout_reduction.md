# Nextest Test Layout Reduction

## 目的と範囲

`nextest.log` の 2031 tests / 65.523s を基準に、テストの意味を変えず、nextest subprocess と process-local bootstrap の重複を減らす。ログは `cargo clean && cargo build && cargo nextest run --all --profile ci` の clean 実行で採取されたものとする。

今回変更するのは Rust test harness、bucket、inventory、nextest 設定に限る。compiler / runtime / standard library の製品コードは変更しない。製品コード側の高速化候補は末尾のメモに分離する。

言語意味論と製品コードを変えず Rust test harness 内で契約が閉じるため level 2 とする。

## 観測

- nextest が先行 `cargo build` 後に追加で行った test profile build: 13.05s
- CI setup (`ci-stdlib-prewarm`): 2.468s
- nextest: 2031 passed / 65.523s
- duration 累積上位:
  - Scar: 333 tests / 246.468 CPU-seconds 相当
  - Rune: 287 tests / 123.540 CPU-seconds 相当
  - Forge: 82 tests / 38.056 CPU-seconds 相当
  - Xldr: 155 tests / 30.210 CPU-seconds 相当
- `scar::typecheck_surface`: 104 tests / 184.370 CPU-seconds 相当
- 対象単独の修正前実測: 104 passed / nextest 24.609s / real 39.23s / user 192.04s / sys 6.43s

`typecheck_surface.rs` は process-local `OnceLock` で stdlib checkpoint を共有する既存構成だが、後から追加された 72 case が `SURFACE_CASES` に登録されず、個別の `#[test]` として残っている。このため nextest は case ごとに subprocess を作り、checkpoint 構築を共有できない。

## 実装順序

### 1. Scar surface case の再集約

- 個別 `#[test]` 72 件を `SURFACE_CASES` に登録し、8 bucket から実行する。
- case 名と関数 pointer の重複を inventory test で拒否する。
- test target に `dead_code` deny を置き、case の登録漏れを compile 時に検出する。
- 既存の case 本体、assertion、worker 数は変更しない。

受入条件:

- `typecheck_surface` の全 case が従来どおり実行される。
- nextest 上の test 数が 104 から 9（8 bucket + inventory）へ減る。
- 対象単独の nextest summary と CPU time が修正前より短くなる。

### 2. 次点の test binary 集約

1 の再計測後、`structured_diagnostics`、`closed_diagnostic_schema`、`common_constructor_invocation` の個別 subprocess が次の支配項なら、同じ process-local prelude を共有できる suite/bucket へ集約する。失敗時の captured stderr に case 名を残す。

### 3. `surtr test --all` CLI 契約の集約

- `--all` discovery、stdout capture、stderr capture、stdin、`it` 間 I/O 分離の fixture を同じ一時 `lib/tests/**` に置く。
- 5回の `surtr test` CLI processを1回にし、各 pass 行と合計8 caseを従来どおり assertion する。
- missing script、failure、quiet、color、cache、file I/O など独立した境界は集約しない。

### 4. Rune / Xldr の bucket balance

Scar 改善後の全体ログでクリティカルパスを再確認する。既に 8 bucket 化されている `language_features` と `repl_core` は、単一ログの揺れだけで再配置しない。継続して偏る場合だけ stable case assignment を調整する。

## 不採用にした仮説

`language_features` の通常 helper を Project mode から Script mode へ変える案を試したが、8 bucket 中4 bucketで `top-level definition cannot appear after top-level expression` になった。process declaration 1件だけでなく、複数の既存 case が Project source policy を必要としている。sourceを見て自動 fallback する経路は設けず、変更を戻した。復元後は 8 passed / 195 skipped。

## 局所実測

| 対象 | before | after |
|---|---:|---:|
| `typecheck_surface` | 104 tests / 24.609s / user 192.04s | 9 tests / 8.648s / user 59.92s |
| `structured_diagnostics` | 16 tests / 3.747s / user 27.49s | 5 tests / 1.292s / user 7.14s |
| schema / constructor / callback 3 targets | 15 tests / 3.439s / user 19.89s | 6 tests / 1.191s / user 6.24s |
| Scar package | 元ログ 333 tests | 218 passed / 11.039s |
| 統合後の `test --all` case | 5 CLI tests / 元ログ 9.640 test-seconds | 1 test / cold 2.529s / warm 1.074s |
| `test_command` filter | 元ログ 19 tests / 13.309 test-seconds | 15 passed / 1.743s (warm) |

## 検証

```bash
cargo fmt -p scar
rtk cargo nextest run -p scar --test typecheck_surface
rtk cargo nextest run -p scar
rtk cargo nextest run --profile cold -p rune --test integration test_command
zsh -lic 'surtr_test'
```

対象単独で Red / Green と効果量を確認してから、test-only 変更が全 test discovery を壊していないことを Scar package、最後に CI profile で確認する。

## 次タスク向け製品コード改善メモ

- Scar の各 source case 自体の parse / resolve / typecheck cost は今回変更しない。最終 clean logでも `typecheck_surface` 8 bucketは合計58.615 test-secondsを占めるため、次は `SURTR_SCAR_PROFILE=1` で型関係・trait selection の支配区間を特定する。
- Xldr の `std.test.semantic` と integration Project semantic prefix は process間 single-flightを持たない。cache path lock取得後に再loadし、winnerだけがbuild/storeする方式を検討する。atomic renameは破損を防ぐが重複compileは防がない。
- Forge の重い15 caseは最終 clean logでも大半が2秒以上。現test helperの cacheはparse/declarationまでで、stdlib resolve/typecheckを毎回行う。Scar checkpoint / resolve resume / Forge bytecode prefixをtest helperに持たせてから集約する。単純bucket化は並列性を落とすため行わない。
- Rune REPL CLIはXldr coreと意味論が重複するcaseを監査し、CLIにはstdio / PTY / ANSI / exit / preload orderingだけを残す候補がある。削除前に1対1の正本対応を記録する。
- clean aliasのdev build 20.18s、追加test build 12.02sはtest executionとは分離してCargo timingで調べる。
- timeout 延長、cache miss の成功条件化、テスト除外で時間を隠さない。

## 最終 clean 検証

ユーザ環境の `surtr_test='cargo clean && cargo build && cargo nextest run --all --profile ci'` を最終差分で実行した。

- exit code: 0
- dev build: 20.18s
- nextest用 test profile build: 12.02s
- setup: 2.276s
- nextest: 1912 passed / 0 skipped / 40.859s
- command total: real 97.37s / user 333.23s / sys 22.77s

元の clean logと比較可能な nextest summaryは 65.523s から 40.859sへ24.664s（37.6%）短縮した。test profile buildは13.05sから12.02sへ短縮した。元ログには先行dev buildとcommand totalがないため、その2項目はbefore/after比較しない。
