# RT-5 process metadata checkpoint profile

Eldr の private checkpoint を測るため、crate-local の
`vm::checkpoint_metadata_tests::process_metadata_profile` を release test binary から単独実行する。
製品 API や feature は追加しない。通常のテスト実行では小さい条件（4定義、2反復）で内容を確認し、
手動測定では runner が環境変数で条件を指定する。速度の合否閾値は設けない。

```sh
cargo test --release --locked -p eldr --lib --no-run --message-format=json > /tmp/eldr-profile-build.jsonl
```

build の JSON にある `compiler-artifact` の `target.name == "eldr"` の `executable` を
別名に保存して、変更前後を分ける。runner は同じ probe をそれぞれ新しい process で5回実行する。

```sh
python3 tests/profile/checkpoint_metadata/run.py --binary /absolute/path/to/saved-eldr-test-binary --output doc/optimize/runtime/checkpoint/rt5_metadata_sample --samples 5 --repeats 32
```

16 / 128 / 1024 個の Worker 定義を持つ VM を用意する。定義ごとに2 handler、
1 dependency handler と256-byte引数を持つ。起動中の process / future / Builder は0個。
spec table とその lookup index、handler 設定以外の大きい状態を混ぜない。
この条件では通常の compiler、source による定義登録、REPL の入力処理全体は測らない。

| 区間 | 測定範囲 |
|---|---|
| `save` | bytecode 追加のない checkpoint の構築。checkpoint 用 chunk の準備は区間外 |
| `drop` | 未変更 checkpoint の破棄。元 VM は生存 |
| `register` | 元 checkpoint と VM clone が生存する live 側へ、定義を1個追加した table を登録。新 registry の構築と前の handler 設定等の解放を含む |
| `rollback` | 登録後の live 状態を元 checkpoint へ復元。失敗進捗に相当する registry / handler 設定の解放を含む |
| `chunk_shared` | 元 VM clone が registry を保持する live 側で、新定義を追加せず `Halt` で終わる `push_atomic` 全体。検証・保存・既存定義の再登録・boot・実行・保存側破棄を含む |
| `chunk_unique` | registry を単独所有する VM を区間外で新しく準備し、同じ `push_atomic` を実行。保存側破棄で旧 registry の最終解放も含む |

1 process 内で各区間を32回観測し、`ns` に区間の合計、`samples_ns` に個別値を保存する。
条件ごとに5 process を起動し、集計は process ごとの合計値の最小・中央値・最大とする。
VM clone、置換 table の準備、assert、JSON 出力は区間外。
区間外の準備や先行区間の allocator 状態は後続区間の時間へ影響し得るため、
独立 kernel の処理費用や実利用の全体改善率とは扱わない。

測定は wall time のみ。requested allocation、live bytes、RSS、物理コピー byte 数は未測定。
反復や条件は時間の合否基準に使わない。metadata の source/lock/binary hash、
toolchain、platform、反復数と原本を `.summary.json` / `.jsonl` に保存する。
`probe_source_sha256` と production hash は runner 実行時のソースを表すため、
binary は記録したソースをビルドした直後に保存する。

初回の5区間系列（`chunk` は現在の `chunk_shared`）は別名の原本として保持する。
6区間系列の変更前後を保存後、順序の偏りを抑えて再測定する。以下は before/after の順序を
偶数 sample では AB、奇数 sample では BA にして、各条件を10 processずつ測る。
記録済み binary hash を確認し、build metadata は対応する元の summary から引き継ぐ。

```sh
python3 tests/profile/checkpoint_metadata/recheck.py --before doc/optimize/runtime/checkpoint/rt5_metadata_2026_10_12_before_v2.summary.json --after doc/optimize/runtime/checkpoint/rt5_metadata_2026_10_12_after_v2.summary.json --output doc/optimize/runtime/checkpoint/rt5_metadata_2026_10_12_recheck --samples 10
```

ビルドやテストと重ねて測らない。JSON 出力前の全要素検査や表示で計測を汚さない。
比較する保存済み binary は、それぞれ記録したソースから同じ release 設定でビルドする。
同じ probe の差は3表のアクセス経路だけとし、製品側に旧経路を残さない。
