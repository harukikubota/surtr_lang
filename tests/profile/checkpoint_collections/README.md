# RT-5 管理表・queue の checkpoint profile

Eldr の private checkpoint を測るため、crate-local の
`vm::checkpoint_collections_tests::checkpoint_collections_profile` を release test binary から単独実行する。
製品 API や feature は追加しない。通常のテストでは小さい条件で内容を確認し、
手動測定では runner が `SURTR_COLLECTION_PROFILE=1`、`SURTR_COLLECTION_N`、
`SURTR_COLLECTION_REPEATS` を指定する。速度の合否閾値は設けない。

```sh
cargo test --release --locked -p eldr --lib --no-run --message-format=json > /tmp/eldr-collections-build.jsonl
```

build の JSON にある `compiler-artifact` の `target.name == "eldr"` の `executable` を
別名に保存し、ビルド直後のソースで runner を実行する。変更前後はそれぞれ保存したバイナリと
別の output 名を使う。前回の metadata 測定原本は上書きしない。

```sh
python3 tests/profile/checkpoint_collections/run.py --binary /absolute/path/to/saved-eldr-test-binary --output doc/rt5_collections_before --samples 5 --repeats 32
```

N = 16 / 128 / 1,024 の条件で、次の合成状態を用意する。

- N 個の process 定義。各定義は2 handler、1 dependency handler と256-byteの名前付き引数を持つ。
- handler context、singleton slot、flight、refilling ID、reply、waiting の各表に N entry。
- deadline / run / output queue に各 N entry。output は256-byteの文字列を持つ。
- supervisor の boot failure / effective policy / child 表に各 N entry。failure は256-byteの文字列、child は3要素の Vec を持つ。
- worker set に N entry。各 entry は3 member を持ち、同一 function callable を clone して使う。

実際に稼働する process、Builder、future は0件。合成状態の ID を実行へ渡さず、
容器の所有と変更を直接測る。compiler、REPL の入力処理全体、実際の process 稼働費用は測らない。
`init` と `chunk_*` は、上記の可変状態を入れず process 定義だけを持つ VM を使う。

| 区間 | 測定範囲 |
|---|---|
| `init` | process 定義だけを持つ bytecode から `VM::new` で初期化・登録する。bytecode の準備と clone、VM の破棄は区間外 |
| `save` | 合成状態を持つ VM の checkpoint 構築。checkpoint 用 chunk の準備は区間外 |
| `drop` | 未変更 checkpoint の破棄。元 VM は生存 |
| `first_write` | 合成状態を持つ VM clone と保存側が生存する live 側で、各可変表の1 entry の内側を更新し、各 queue の末尾へ1 entry 追加する。worker は ID 0 の entry だけを変更する |
| `second_write` | 同じ live 側へ2回目の変更を行う。間に新しい checkpoint は作らない |
| `rollback` | 2回変更した live 状態を破棄して、保存した checkpoint へ復元する |
| `chunk_shared` | process 定義だけを持つ元 VM clone と追加の保存側を保持し、定義を追加しない `Halt` chunk の `push_atomic` 全体を実行する |
| `chunk_unique` | process 定義だけを持つ独立 VM で同じ `push_atomic` を実行する。VM は区間外で構築し、実行中の内部 checkpoint とだけ共有する |

各反復で新しい条件を準備し、各区間を1回観測する。1 process 内で32反復し、
`ns` に区間の合計、`samples_ns` に32個の個別値を保存する。各条件で5 process を順に起動する。
集計は process ごとの合計値の最小・中央値・最大とする。runner は8区間の過不足、
N、反復数、個別値の件数と合計を検証する。各系列は120行の原本、24個の集計になる。

VM clone、bytecode の準備、assert、全要素の走査、JSON 出力は区間外。
区間外の準備や先行区間の allocator 状態は後続区間の時間へ影響し得るため、
独立 kernel の費用や実利用全体の改善率として扱わない。
8区間の測定は wall time。macOS の `/usr/bin/time -l` から process 全体の最大 RSS を
`process_peak_rss_bytes` として取得し、原本と集計へ保存する。macOS の出力単位は byte。
これは VM の初期化、区間外の準備、8区間、JSON 出力、test harness 全体を含む process の高水位であり、
各区間の peak や現在の live bytes ではない。同じ process の8行へ同じ RSS 値を付けるため、
区間ごとの RSS 値を加算しない。集計も対応する process 高水位の最小・中央値・最大であり、
phase のメモリ使用量を示さない。requested allocation、論理 live bytes、物理コピー byte 数は未測定。
この RSS 取得方法は macOS 専用とし、別の OS ではエラーにする。

`.summary.json` は実行時の root、HEAD、`git status --porcelain`、
`git diff HEAD --binary` の SHA-256、Eldr の runtime source と probe、manifest、
root の Cargo.lock、binary、runner の hash、rustc、platform と条件を記録する。
前回の改善を含む dirty な変更前状態を、HEAD だけで識別しない。
差分 hash は追跡済みファイルが対象であり、未追跡 probe は別の source hash に含む。
source hash はビルド証明ではなく runner 実行時のソースを表すため、
記録したソースのビルド直後にバイナリを保存して測定する。実行中に source または binary が変われば
集計を保存せずエラーにする。原本 `.jsonl` は中断時にも残るため、不完全な原本を完成系列に混ぜない。

変更前を5 process で測り、その後に変更後を5 process で測る。両系列を保存後、
以下で実行順を偶数 sample では AB、奇数 sample では BA として再測定する。
各条件は before / after 各10 process、合計480行の原本と48個の集計になる。

```sh
python3 tests/profile/checkpoint_collections/recheck.py --before doc/rt5_collections_before.summary.json --after doc/rt5_collections_after.summary.json --output doc/rt5_collections_recheck --samples 10
```

再測定は元の summary の build metadata を継承し、保存済み binary hash、
lock、toolchain、platform、runner、条件の一致を確認する。各 binary の出力についても
8区間、N、反復数と個別値の合計、RSS 出力の存在を毎回確認する。
変更前のソース情報を現在のソースから取り直さない。
probe は plain 容器から共有容器への移行で接続箇所の型が変わり得るため、
前後の probe hash は個別に保存する。区間と準備条件の意味が同じであることは、対応する source 差分で確認する。
ビルドやテストと重ねて測らず、両バイナリは同じ release 設定で作成する。
