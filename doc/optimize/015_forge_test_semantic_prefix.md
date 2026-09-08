# Forge Test Semantic Prefix

## 目的と範囲

Forge の source-to-bytecode 契約テストが各 case で stdlib の resolve / typecheck /
codegen を再実行する重複をなくす。製品 codegen と言語意味論は変更せず、Forge の
crate-local test helper だけを変更するため level 2 とする。

## 現状

- helper の `OnceLock` は stdlib parse と `DeclarationIndex` までしか保持しない。
- `codegen_source` と process module helper は毎回 stdlib を含む全 program を
  resolve / typecheck / codegen する。
- clean CI log では該当 15 case の大半が 2.0〜3.0s である。

## 変更

- stdlib の `ResolveResumeState`、`ScarCheckpoint`、base `Bytecode` を test process
  内で 1 回だけ構築する。
- user suffix だけを resolve / typecheck し、`ForgeSession::from_bytecode` から chunk
  を生成して base bytecode と compose する。
- process spec / boot plan を検証する module helper も同じ staged suffix 経路を使う。
- 15 case を 4 bucket に集約し、process ごとの prefix 構築を 15 回から 4 回に減らす。
- case inventory で名前・関数の重複と standalone `#[test]` への戻りを拒否する。

## 受入条件

- source-to-bytecode 15 case の assertion が維持される。
- Forge package がすべて成功する。
- 対象 test の compile 時間が短くなる。

## 結果

- 対象 15 case: `15 passed` / 12.727s から、4 bucket + inventory の
  `5 passed` / 3.816s（70.0% 短縮）。
- Forge package: `72 passed` / 1.540s。
- 製品 codegen、言語意味論、公開 API は変更していない。
