# Scar Staged Suffix Test Helper

## 目的と範囲

canonical builtin type 名の予約を検証する6ケースと process module を検証する11経路が、
stdlib 全体を再 typecheck する重複をなくす。変更対象は
`crates/scar/tests/support` と `typecheck_surface` の test helper 呼出しだけで、製品コード、
診断契約、言語意味論は変更しないため level 2 とする。

## 観測

- `typecheck_surface` は8 bucket / 8.352s（単独基準）。
- `SURTR_SCAR_PROFILE=1` で、process module 経路の呼出しごとに約330ms、550超
  statement の型検査が再実行されている。
- 各経路は cached std prelude を取得しているが、full resolved program を空の
  `ScarSession` から typecheck している。

## 変更

- user module を含む declaration precollect と full process specs / boot plan は維持する。
- resolved node は cached std prefix の件数を除いた suffix のみを取り出す。
- typecheck は cached `ScarCheckpoint` を復元した session で suffix のみ行う。
- canonical builtin type 名 helper は resolve も cached `ResolveResumeState` から開始する。
- 現行と同じ `TypecheckContext::default()` を使い、成功・拒否条件を変えない。

## 受入条件

- canonical builtin type 名の6拒否ケースと process module の11経路が成功する。
- `typecheck_surface` と Scar package が成功する。
- `typecheck_surface` の wall time が単独基準より短くなる。

## 結果

- `typecheck_surface`: 9 tests / 8.352s から 6.725s、再測定 6.674s
  （約20%短縮）。
- Scar package: `218 passed` / 9.252s。
