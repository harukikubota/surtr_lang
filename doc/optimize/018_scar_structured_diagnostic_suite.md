# Scar Structured Diagnostic Suite

## 目的と範囲

structured diagnostic 16ケースの assertion と case inventory を維持したまま、test
process ごとの cached std prelude 構築を減らす。変更対象は Rust test harness の実行単位
だけで、製品コードと診断契約は変更しないため level 2 とする。

## 観測

- 温間 default workspace では 4 buckets + inventory が累積 7.036 test-seconds。
- 各 bucket は別 process なので、同じ stdlib の parse / resolve / typecheck checkpoint を
  4回構築する。
- 16ケースはいずれも `support::typecheck` が新しい cached checkpoint session を作り、
  case 間で変更可能な状態を共有しない。

## 変更と受入条件

- 4 buckets を1 suiteへまとめ、case名を失敗出力に残す。
- inventory test は独立して維持し、未登録・重複 case を拒否する。
- 対象 target と Scar package が成功する。
- 対象 wall timeを悪化させず、累積 test-secondsを短縮する。効果がなければ戻す。

## 結果

- `structured_diagnostics`: 5 tests / 累積 7.036 test-seconds から 2 tests /
  1.135s、再測定 1.157s。inventoryを除く suiteは1.134–1.156s。
- Scar package: `215 passed` / 6.028s。
