# Semantic Cache Single-Flight

## 目的と範囲

`std.test.semantic` と test-related semantic prefix の cold miss が複数の
nextest process で同時に発生したとき、同じ resolve / typecheck / codegen を
重複実行しないようにする。cache の key、payload、言語意味論は変更しないため
level 2 とする。

## 現状

- cache write は一時 file と atomic rename で破損を防いでいる。
- cache miss から build までを直列化していないため、同じ path の miss を観測した
  process はすべて build へ進む。
- `profile ci` の setup は default variant だけを prewarm するため、test-enabled
  stdlib と Project semantic prefix はこの重複の影響を受ける。

## 変更

- cache file と別の安定した `.lock` file に OS の排他 file lock を取る。
- 最初の load が miss した場合だけ lock を取り、取得後に同じ key でもう一度
  load する。
- 再 load も miss した process だけが build / store し、store 完了まで lock を
  保持する。
- cache directory 作成または lock が失敗した場合は、既存契約どおり cache を
  correctness 条件にせず通常 compile を行う。
- corrupt / schema mismatch / key mismatch も既存どおり rebuild する。

## 受入条件

- 同じ cache path へ同時に入った writer の build は 1 回だけになる。
- 待機した process は lock 取得後の再 load から payload を得る。
- default / test-enabled stdlib snapshot、Rune test prefix、integration fixture prefix
  の既存成功・corrupt rebuild 契約が維持される。
- 対象 test と CI profile の実測で失敗がなく、cold 並列実行の重複 write が減る。

## 実装・検証結果

- `std::fs::File::lock` を使う cache path 単位の guard を Xldr に追加した。
- 4 thread の同一 prefix cache miss は build 1 回に収束した。
- test-enabled stdlib の 2 process cold 同時起動は `cold` 1 件、lock 待機後の
  `disk_hit` 1 件になった。
- `rtk cargo nextest run -p xldr semantic_cache`: 5 passed
- `rtk cargo nextest run -p xldr`: 80 passed
- `SURTR_TEST_CACHE=1 rtk cargo nextest run -p rune --test integration run_srt`:
  27 passed
- `SURTR_TEST_CACHE=1 rtk cargo nextest run -p rune --test integration module_import_fixtures`:
  11 passed
- `rtk cargo nextest run --profile cold -p rune --test integration test_command`:
  15 passed
