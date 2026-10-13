# RT-2: 短い suffix の保持方式比較

指定入力は [2026-10-11 の測定レポート](../../../doc/optimize/runtime/baseline/list_runtime_measurement_report_2026_10_11.md) の RT-2。
製品コード・通常の correctness test から独立した、level2 相当の手動測定である。
採用方式、実時間や RSS の合否閾値は決めない。

## 比較する方式

- `shared`: 公開 `ListHandle` を使う現行 Packed。tail は全 buffer を共有する。
- `chunk`: capacity 256 の Vec を、前方への `Rc` 鎖でつなぐ局所モデル。tail は offset か次 node を参照するだけで、payload をコピーしない。最後の所有者は chunk 鎖を反復解放する。
- `compact`: 現行 Packed から取り出した長さ1の suffix にだけ、`iter().collect()` と `from_items` を一回行う。元 suffix の破棄も compact 区間に含める。別の suffix や checkpoint は置換しない。

chunk の構築は payload を chunk 内へ直接生成する。現行の `from_items(Vec<Value>)` が
入力 buffer の所有権をそのまま移す契約を実装したものではない。製品への移行、Builder からの
所有権移動、cons/uncons、VM・REPL の継続、scheduler 予算は未実装・未検証である。
外側の chunk 内にある入れ子 List は現行 Packed のまま使う。

## 入力と受入条件

n=10,000 / 100,000、payload は `int` / `string` / `nested`。
各要素は位置番号を持ち、string は256 bytes、nested はその string を3段の singleton List で包む。
各要素の入れ子 buffer を独立に作る。nested の compaction は内部 ListHandle を共有する clone で、深いコピーではない。

ownership は次の3条件。方式・n・payload・ownership の直積54条件を各5回測る。

| 条件 | 保持する所有者 | 破棄順 |
|---|---|---|
| suffix | 元 List、末尾1要素 | 元 List → 末尾 |
| siblings | 元 List、中点 n/2 からの suffix、末尾1要素 | 元 List → 中点 suffix → 末尾 |
| checkpoint | 元 List、その clone、末尾1要素 | 元 List → clone → 末尾 |

checkpoint は元 List を保持する所有者の代理であり、実 VM の checkpoint を呼ばない。
複数の root handle は同じ元 List を共有する。独立した元 List 間の payload 共有は測らない。

- tail 全走査の allocation/reallocation は0であることを harness で確認する。
- 長さ、全要素の順序・内容、空の head/tail、各所有者の破棄後の suffix の有効性を別 process で確認する。
- 各区間の live delta の累積を workload の残存 requested bytes として記録し、最後の破棄で0へ戻ることを確認する。
- 改善する所有条件と改善しない所有条件を両方記録する。製品契約への適合や採用を性能値だけで判断しない。

## 測定区間と指標

build → advance → compact → drop_root → drop_sibling → drop_checkpoint → drop_last を測る。
存在しない所有者の drop と shared/chunk の compact は空操作で、時計分解能付近の値に意味を持たせない。
build は全 payload と backing storage の生成を含み、advance は head の読取り・clone を含まない。
compact は末尾1要素だけを処理する。全所有者を置換する compaction の費用は測らない。
各区間の結果は scalar で保持し、全破棄後に JSON を作る。ビルド、JSON 出力、検証は区間外である。

割当指標は System allocator への requested layout を数える。RSS や物理コピー量ではない。
realloc は新サイズを requested、旧サイズを freed に加える。peak_extra は区間開始時の論理 live
を超えた最大増分であり、realloc 内部の一時二重保持は観測しない。
`workload_retained_bytes` は build 以降の区間 live delta の和で、プロセス全体の live ではない。
`/usr/bin/time -l` の peak RSS はプロセス全期間の値で、区間別の保持量ではない。
カウンタなし系列の割当指標の0は未計測を表す。

## 再現

リポジトリルートで実行する。各 workload は独立 process で測り、count 有効/無効を別 binary にする。
lock は root lock を基に不要項目を除いて生成した。同じ依存版を使い、再現時は `--locked` を指定する。

```bash
cargo build --release --locked --manifest-path tests/profile/list_tail_retention/Cargo.toml --target-dir target/list-tail-retention-count
python3 tests/profile/list_tail_retention/run.py --binary target/list-tail-retention-count/release/list-tail-retention-probe --output doc/optimize/runtime/list/list_tail_retention_2026_10_11_count --samples 5
cargo build --release --locked --no-default-features --manifest-path tests/profile/list_tail_retention/Cargo.toml --target-dir target/list-tail-retention-time
python3 tests/profile/list_tail_retention/run.py --binary target/list-tail-retention-time/release/list-tail-retention-probe --output doc/optimize/runtime/list/list_tail_retention_2026_10_11_time --samples 5
python3 tests/profile/list_tail_retention/verify.py --binary target/list-tail-retention-count/release/list-tail-retention-probe
PROBE_VERIFY=1 target/list-tail-retention-count/release/list-tail-retention-probe chunk 100000 nested siblings
```

runner は macOS 専用。性能系列では `PROBE_VERIFY` を unset する。
出力 `{prefix}.jsonl` と `{prefix}.summary.json` に全サンプル、中央値・範囲と
commit / source・lock・binary の SHA-256、toolchain / OS を保存する。
