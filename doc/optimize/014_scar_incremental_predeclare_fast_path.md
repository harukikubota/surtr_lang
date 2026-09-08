# Scar Incremental Predeclare Fast Path

## 目的と範囲

stdlib checkpoint から user suffix を型検査するとき、suffix に trait / trait impl が
ないにもかかわらず既存 trait 全体の closure と impl metadata を再走査する固定費を
除く。宣言の検査順、型検査結果、診断は変えないため level 2 とする。

## 観測

`SURTR_SCAR_PROFILE=1` で `typecheck_surface` の通常 case を見ると、1 statement の
suffix でも毎回およそ次の固定費があった。

- `predeclare_traits`: 1.6ms 前後
- `predeclare_functions`: 0.75ms 前後
- `specialize_program`: 1.4ms 前後

`predeclare_traits` は新しい `TraitDef` がなくても
`resolve_trait_constraint_closure()` を呼び、新しい `TraitImplDef` がなくても
persistent state 内の全 impl の parent chain を再検証していた。
`predeclare_functions` も新しい `TraitImplDef` がなくても全 impl を clone / sort
していた。

変更前の対象実測:

```text
rtk cargo nextest run -p scar --test typecheck_surface
9 passed / nextest 8.159s / real 8.36s / user 59.53s / sys 0.82s
```

## 変更

- この suffix で `TraitDef` を追加した場合だけ trait constraint closure を再解決する。
- この suffix で `TraitImplDef` を追加した場合だけ parent impl chain を再検証する。
- この suffix で `TraitImplDef` を追加した場合だけ impl method の function metadata を
  persistent impl registry から抽出する。
- trait / impl を含む compile unit の既存経路は変更しない。
- specialization の固定費は別原因なので、この変更には含めない。

## 受入条件

- `typecheck_surface` と Scar package がすべて成功する。
- trait parent / impl method を含む既存テストが成功する。
- 対象の wall / user time が変更前より短くなる。

## 検証結果

- trait / impl を持たない通常 case の profile:
  - `predeclare_traits`: 約 1.5ms から 0.000ms
  - `predeclare_functions`: 約 0.75ms から 0.000ms
- `rtk cargo nextest run -p scar --test typecheck_surface`:
  - before: 9 passed / nextest 8.159s / user 59.53s
  - after: 9 passed / nextest 8.000s / user 58.08s
- `rtk cargo nextest run -p scar`: 218 passed / nextest 10.272s
