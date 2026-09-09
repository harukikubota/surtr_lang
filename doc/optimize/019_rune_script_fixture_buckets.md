# Rune Script Fixture Buckets

## 目的と範囲

script pass / compile-error fixture の契約を維持しつつ、同じ Script compile prefix を使う
test process 数を減らす。変更対象は Rust integration test harness の bucket 構成だけで、
fixture、製品コード、言語意味論は変更しないため level 2 とする。

## 観測

- 温間 `run_srt` filter は 27 tests / 1.928s、累積 13.911 test-seconds。
- pass 8 buckets と compile-error 16 buckets は別 process で、各 process が同じ stdlib
  Script compile prefix を process-local cacheへ読み込む。
- pass bucket は最大0.875s、compile-error bucketは最大0.789sで、各caseの本処理より
  processごとの共有初期化が目立つ。

## 試行と受入条件

- pass と compile-error を同じ stable 8 buckets で順番に実行し、24 processを8へ減らす。
- fixture、期待値、phase check、timing report、独立した cache / strict parsing testsは維持する。
- `run_srt` filter と default Rune integration が全件成功する。
- 対象 wall timeを大きく悪化させず、累積 test-secondsまたは workspace wall timeを短縮する。
  効果がなければ戻す。

## 結果

- `run_srt` filter: 27 tests / 1.928s / 累積 13.911 test-seconds から
  11 tests / 1.801s / 累積 11.256、再測定 1.679s / 累積 10.716。
- default Rune integration: `55 passed, 76 skipped` / 5.822s、累積
  43.969 test-seconds。変更前は `71 passed, 76 skipped` / 5.898s、累積45.946。
