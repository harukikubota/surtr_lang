# `.srt` コンパイル時間の基準値

2026-10-15、commit `a0d430a609da73f7f35bbba7a01f41f9b28dfc09` の実測。最適化前の観測であり、短縮効果の検証ではない。製品コードと入力ソースは変更していない。

本書は初回調査時点の記録。後続の追加依頼による複数入力の整備と `heavy_compile.srt` の修正は [05_compile_inputs.md](05_compile_inputs.md) に記録した。本書の失敗記録・入力 hash・測定値は初回時点のまま保持する。

## 条件と証拠

- macOS 26.6.2 / arm64、Rust 1.94.1、release build、既定 feature。
- `cargo build --release -p rune --bin surtr --target-dir /tmp/surtr-scar-investigation-a0d430a6` は終了コード 0。Rust のビルド時間を `.srt` の計測に含めていない。
- バイナリ、入力の SHA-256、commit、実行時の cwd（`execution_cwd`）と測定出力先（`work_directory`）は [manifest.json](evidence/manifest.json) に保存した。
- 3 入力 × 2 cache 条件 × 5 回、計 30 回を直列実行。各入力・各反復で未使用の cache directory を割り当て、cold の直後に同じ directory の disk hit を測った。毎回、新しい CLI process を起動した。
- 全件終了コード 0、`cache.stdlib` が指定条件と一致。`cache.artifact=not_used`、`execute.status=skipped`。VM は実行していない。
- `SURTR_SCAR_PROFILE` は時間比較用の 30 回では無効。原因調査用には別途 6 回実行し、全件終了コード 0。
- 生の stdout / stderr、実行引数、cache directory、JSON は [baseline.jsonl](evidence/baseline.jsonl)、集計値は [summary.json](evidence/summary.json)、追加 profile の実行記録は [profile_runs.json](evidence/profile_runs.json) に保存した。

ここでの cold は **Surtr の disk semantic cache が空**という意味。OS のページキャッシュ、CPU 周波数、他プロセスは制御していない。全反復で cold → disk hit の順序を固定した初期調査であり、性能比較実験では A/B の実行順も交互にする。

## 結果

単位は ms、中央値（5 回）。typecheck は標準定義の復元後の user suffix に対応する。

| 入力 | stdlib cache | stdlib load | resolve | typecheck | codegen | compile total |
|---|---|---:|---:|---:|---:|---:|
| `stdlib_prewarm.srt` | cold | 407.508 | 1.898 | 1.613 | 0.565 | 416.884 |
| `stdlib_prewarm.srt` | disk hit | 15.702 | 1.992 | 2.126 | 0.648 | 25.728 |
| `monad_fail_priority.srt` | cold | 410.448 | 3.548 | 20.645 | 1.033 | 441.927 |
| `monad_fail_priority.srt` | disk hit | 15.709 | 3.716 | 21.243 | 1.067 | 48.192 |
| `generic_extractor_specialization.srt` | cold | 409.162 | 5.826 | 7.569 | 1.773 | 432.348 |
| `generic_extractor_specialization.srt` | disk hit | 15.975 | 6.109 | 8.237 | 1.776 | 40.079 |

disk hit の typecheck の最小〜最大は、小入力 2.004〜2.208 ms、MonadFail 21.108〜21.330 ms、generic extractor 8.025〜8.393 ms。compile total の最小〜最大は、それぞれ 25.369〜26.262 / 48.056〜48.403 / 39.915〜41.058 ms。全条件の幅は集計 JSON にある。

入力:

- [小入力](../../tests/profile/stdlib_prewarm.srt): `prewarm = 0`。標準定義の構築・読込と suffix の固定費を見る。
- [MonadFail](../../tests/fixtures/script/pass/stdmod/monad_fail_priority.srt): 独自 carrier と複数 Trait impl、`do`、失敗経路の型検査。
- [generic extractor](../../tests/fixtures/script/pass/patterns/generic_extractor_specialization.srt): generic と pattern specialization。

`compile total` は Scar 単体ではなく、cache 読込、parse、resolve、codegen、encode、書込などを含む。`typecheck` も [Rune の計測区間](../../crates/rune/src/compile.rs) では ScarSession の復元から含む。cold の stdlib load には標準定義自体の各コンパイルフェーズが含まれる。表の中央値同士を足しても、個別実行の合計や内訳の網羅値にはならない。

## 内部 profile から分かったこと

以下は profiler 有効の別走行で各 1 回の観測。上表との直接比較やタイマーの単純加算はしない。親子の処理時間は重複し、profiler 自体の測定負荷もある。

| 観測対象 | 観測 | 次に確認する箇所 |
|---|---|---|
| stdlib cold | Scar total 287.595 ms、statement loop 252.028 ms、`DeferrorDef` 174 件で 163.654 ms | `check_deferror_def` の child 作成・body 検査・状態反映 |
| stdlib cold | child spawn 174 回 / 42.582 ms、absorb 174 回 / 15.516 ms | コピー対象の内訳、共有済み metadata と可変状態の境界 |
| MonadFail の disk hit | Scar total 20.627 ms、predeclare traits 5.548 ms、specialize 2.449 ms | impl 追加時の既存 impl 再検査、suffix ごとの解析再走査 |
| generic extractor の disk hit | Scar total 8.436 ms、child spawn / absorb は 0 回 | child 最適化だけで全入力が改善するとはいえない |

証拠: [stdlib cold](evidence/profile_stdlib_prewarm_cold.log)、[MonadFail disk hit](evidence/profile_monad_fail_priority_disk_hit.log)、[generic extractor disk hit](evidence/profile_generic_extractor_specialization_disk_hit.log)。

小入力の suffix は 5 ms 未満なので、[TypecheckProfiler::print_summary](../../crates/scar/src/checker/mod.rs) の条件により詳細 summary が出ない。ログの不在を処理時間 0 と扱わない。

これにより、cold の調査では Error 宣言と child 状態、warm suffix では predeclare / specialization を分けて追う方針を採る。どのデータ構造やコピーが支配的か、どの変更が有効かまでは未確認である。

## 既存 heavy 入力の失敗

最初に [heavy_compile.srt](../../tests/profile/heavy_compile.srt) を試したが、8 行目の `Err(NoneError)` が終了コード 1 になった。

```text
TypeError: Err(...) requires an Error value.
Help: Call `NoneError()` to construct an Error; the bare name is an ErrorKind.
```

[現行 Error 仕様](../../docs/dev/Error_spec.md) では裸名が ErrorKind、`NoneError()` が Error。これは旧形式の測定入力との不一致であり、コンパイラの回帰とは判断しない。入力は変更せず、成功時の時間集計から除外した。失敗を含む初回の 3 実行は [initial_attempt.jsonl](evidence/initial_attempt.jsonl) に残した（小入力の cold / disk hit 成功 2 件と heavy の失敗 1 件）。

後続の実装段階では、この入力を現行仕様に修正してから全体がコンパイルできることを確認する。今回の 1 箇所の診断だけで、他の行もすべて有効とは保証しない。

## 再実行例

リポジトリルートから、ビルド済みの同一バイナリで実行する。測定データや生成物は専用ディレクトリへ出し、既存 cache を削除しない。

```sh
measure_dir=$(mktemp -d /tmp/surtr-scar-measure.XXXXXX)
scar_bin=/tmp/surtr-scar-investigation-a0d430a6/release/surtr
SURTR_STDLIB_CACHE_DIR="$measure_dir/cache" "$scar_bin" build \
  tests/fixtures/script/pass/stdmod/monad_fail_priority.srt \
  "$measure_dir/out.eldr" --phase-times-json 2> "$measure_dir/cold.json"
SURTR_STDLIB_CACHE_DIR="$measure_dir/cache" "$scar_bin" build \
  tests/fixtures/script/pass/stdmod/monad_fail_priority.srt \
  "$measure_dir/out.eldr" --phase-times-json 2> "$measure_dir/disk-hit.json"
```

終了コードと cache 状態を確認する。時間比較では `SURTR_SCAR_PROFILE` を unset し、profile 付きの実行は別ログにする。5 回の再現には反復ごとに新しい `measure_dir` を作る。入力別にも cache を分け、同じバイナリを使う。

未検証: process-local warm / REPL の連続入力、追加 module prefix、失敗入力の所要時間、入力規模に対する増加率、メモリ使用量、最適化前後比較、Scar / workspace の全テスト、生成物の実行結果。これらは [測定計画](03_compile_time.md) と [テスト計画](01_test_plan.md) に引き継ぐ。
