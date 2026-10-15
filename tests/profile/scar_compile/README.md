# Scar のコンパイル計測入力

現行機能の組合せと、定義数・依存の深さを分けて測るための手動入力。通常の nextest gate には登録しない。**測定入口は `surtr build`** とし、VM の実行時間を含めない。

全入口は [cases.json](cases.json) に列挙した。パスはリポジトリルートからの相対パス。新規 22 入口と、既存の小入力・具体型中心の入力 2 件を合わせた 24 件である。module の補助ファイル 2 本は `dependencies` に置き、単独コンパイルしない。

## 機能別の入力

| 入力 | 主に通す経路 |
|---|---|
| [error_constructors.srt](error_constructors.srt) | Error の message、Payload、constructor 引数、body 内の局所 binding、ErrorKind、payload pattern |
| [generic_calls.srt](features/generic_calls.srt) | 同じ generic の同型／異型の具体化、struct、RTA、返却関数、capture、Trait dispatch |
| [patterns.srt](features/patterns.srt) | generic Extractor、match、SafeBind、`do`、capture、複合 payload |
| [facet_paths.srt](features/facet_paths.srt) | record / List / HashMap / Option の path、合成 path、動的 index、capture、更新 |
| [monad_carriers.srt](features/monad_carriers.srt) | Result / Option / List の `do`、StateT / OptionT、generic bind、constructor slot |
| [closures_lazy.srt](features/closures_lazy.srt) | capture、入れ子 closure、遅延分岐、関数の保存、部分適用、Extractor capture |
| [modules/entry.srt](modules/entry.srt) | 追加 module の staged compile、import、module をまたぐ generic と Trait impl。依存は [Operations](modules/Operations.srt) と [Values](modules/Values.srt) |

これらは複数の現行機能を通す固定サイズの入力。ある入力の時間が減っただけでは、その中の特定の処理が速くなったとは断定しない。対応する profile の内訳と併せて判断する。

## 規模別の入力

ファイルは生成済みで、そのまま `build` できる。生成元は [generate.py](generate.py)。系列ごとに表の条件を固定し、記載した規模を変える。

| 系列 | 規模と入力 | 比較する負荷・限界 |
|---|---|---|
| `error_definitions` | [64](generated/error_definitions_064.srt) / [128](generated/error_definitions_128.srt) / [256](generated/error_definitions_256.srt) | Payload なしの Error 宣言数。constructor 呼出しは先頭・末尾の 2 箇所で固定。body 検査の費用を見る |
| `generic_chain` | [16](generated/generic_chain_016.srt) / [32](generated/generic_chain_032.srt) / [64](generated/generic_chain_064.srt) | generic forwarding の深さ。末端は `Monad::bind`、呼出しは Int / String / Int の 3 箇所で固定 |
| `generic_chain_reverse` | [16](generated/generic_chain_reverse_016.srt) / [32](generated/generic_chain_reverse_032.srt) / [64](generated/generic_chain_reverse_064.srt) | 上記と同じ定義・body・呼出しで宣言順だけ逆転。function index に沿った固定点走査の順序依存を比較する |
| `trait_impls` | [16](generated/trait_impls_016.srt) / [32](generated/trait_impls_032.srt) / [64](generated/trait_impls_064.srt) | 一つの Trait に対する非重複 impl 数。dispatch は先頭・末尾の 2 箇所。nominal 型と method 本体も増えるため、候補検索だけの時間とはしない |
| `nominal_chain` | [16](generated/nominal_chain_016.srt) / [32](generated/nominal_chain_032.srt) / [64](generated/nominal_chain_064.srt) | record 型の依存、field access の深さ。構築を平坦な binding に分け、構文の括弧の深さを増やさない。型依存と式検査の合計を測る |

`generic_chain` の順方向は、現在の昇順走査で要求が伝わりやすい順序。逆順も組にして、短い固定点処理だけを測らないようにする。ただし、宣言順で変わる時間は前宣言や別の走査も含む。固定点の更新回数は別に計測する。

Trait の探索 index だけを評価する場合は、同数の型・method を持つ対照などを追加して切り分ける。今回の系列をそのまま個別アルゴリズムの benchmark と呼ばない。

生成内容の確認と再生成は、リポジトリルートで実行する。

```sh
python3 tests/profile/scar_compile/generate.py --check
python3 tests/profile/scar_compile/generate.py
```

`--check` はファイルを書き換えず、欠落・差分・想定外の生成 `.srt` があれば失敗する。生成入力を直接修正せず、生成元と出力を一緒に変更する。系列や規模を増やした場合は `cases.json` も更新する。

## 対照入力

- [stdlib_prewarm.srt](../stdlib_prewarm.srt): 一つの binding。stdlib と suffix の固定費。
- [heavy_compile.srt](../heavy_compile.srt): 初期の具体型中心の入力を継続利用。Error constructor を現行仕様に合わせた。現行機能全体の代表にはせず、過去の小さい機能集合との対照にする。

`heavy_compile.srt` は繰り返し二乗を含み、VM で実行すると巨大な BigInt を作りうる。入力確認も `run` ではなく `build` で行う。

## 計測例

リポジトリルートで、まずバイナリを用意する。Rust build の時間は `.srt` の計測から外す。

```sh
export CARGO_TARGET_DIR=/tmp/surtr-scar-compile-target
cargo build --release -p rune --bin surtr
measure_dir=$(mktemp -d /tmp/surtr-scar-compile.XXXXXX)
SURTR_STDLIB_CACHE_DIR="$measure_dir/cache" "$CARGO_TARGET_DIR/release/surtr" build \
  tests/profile/scar_compile/generated/generic_chain_032.srt \
  "$measure_dir/cold.eldr" --phase-times-json 2> "$measure_dir/cold.json"
SURTR_STDLIB_CACHE_DIR="$measure_dir/cache" "$CARGO_TARGET_DIR/release/surtr" build \
  tests/profile/scar_compile/generated/generic_chain_032.srt \
  "$measure_dir/disk.eldr" --phase-times-json 2> "$measure_dir/disk.json"
```

時間比較では `SURTR_SCAR_PROFILE` を unset する。profiler は別走行で有効にする。cold / disk hit は出力 JSON の `cache.stdlib` で確認し、`cache.artifact=not_used`、`execute.status=skipped`、終了コード 0 を条件にする。入力・反復・before/after ごとに cache を分ける。既存 cache をまとめて削除しない。

一括処理では `cases.json` の `entry` だけを使う。`*.srt` の再帰 glob は補助 module も拾うため使わない。入力と依存ソースの hash、compiler の commit・差分・binary hash、cold / disk hit、各回の時間を保存する。少なくとも複数回を測り、cold と disk hit を別集計にする。

追加時の確認は [入力整備と検証記録](../../../doc/scar_optimize/05_compile_inputs.md) に記載した。24 入口の cold / disk hit 各 1 回はコンパイル可否と cache 経路の確認であり、安定した性能基準値や機能の実行結果を保証する測定ではない。
