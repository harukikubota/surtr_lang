# Rune REPL Test Buckets

## 目的と範囲

Rune REPL の CLI 契約を変えず、nextest が起動する外側の test process を減らす。
変更対象は `tests/integration/repl.rs` の test layout だけで、製品コード、言語意味論、
各 case の assertion は変更しないため level 2 とする。

## 観測

- warm CI profile: 57 tests / 合計 8.361 test-seconds。
- cold profile の対象単独: 57 passed / 3.422s。
- cold / CI profile では REPL test が `threads-required = 2` であり、8 thread 環境の
  同時実行数は4件。
- 各 case は独立した `surtr repl` process を起動し、filesystem を使う case は
  一意 temp dir を使う。process-global environment の書換えはない。

## 変更

- 57 case を名前と関数 pointer の inventory に登録する。
- 4 bucket が inventory を round-robin で直列実行する。
- Unix 専用 PTY case は inventory entry にも同じ `cfg(unix)` を付ける。
- inventory test で名前・関数の重複と standalone `#[test]` の再導入を拒否する。
- case 開始時に名前を stderr へ出し、bucket failure から対象 case を特定できるようにする。

## 受入条件

- Unix では57 caseすべて、非Unixでは従来の非PTY caseすべてが登録される。
- cold profile の REPL filter と Rune integration target が成功する。
- 対象単独の wall time が基準より悪化しない。

## 結果

- REPL filter: `57 passed` / 3.422s から、4 bucket + inventory の
  `5 passed` / 2.916s、再測定 2.941s（約14%短縮）。
- Rune integration: `147 passed` / 15.843s。
