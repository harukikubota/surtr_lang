> 取り込み元: `/Users/haruca/.codex/worktrees/list-runtime-remaining/surtr`（detached、HEAD `72154707`）。資料の集約。集約時点では、記載された製品変更・harnessはmain未統合。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](../README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# RT-4: Regex の入力全文を共有する

集約後に[main作業treeへの反映・再測定](regex_input_sharing_adoption_2026_10_13.md)を実施した（未コミット）。以下は先行worktreeでの実装・測定記録として保持する。

入力は [2026-10-11 の指定測定レポート](../baseline/list_runtime_measurement_report_2026_10_11.md) と [現在の残課題](../list_runtime_followups.md)。main `72154707399d6cdaf7190ab75007f2049ab010c6` を基準とする。RT-3、HashMap のコピー削減、RT-5 の管理表共有は統合済みで、別 worktree `9ea8` の Builder 改善も重複して実装しない。

## 変更の範囲と受入条件

`RegexMatchHandle` / `RegexCapturesHandle` の入力全文だけを不変の `Rc<String>` で保持する。`find_all` の一呼出しは一つの入力 buffer を所有し、各一致と clone した handle はそれを共有する。`find` / `captures` の成功時も所有した引数 String を移す。通常の String 表現や capture group / name table は変更しない。

これは既存意味論を維持する内部所有表現の選択であり、Sindr と Eldr の境界に及ぶ level3 とする。Astra と現行の equality / span / schema 契約を確認した。一致部分だけの保存は入力全文による equality を変えるため採用しない。言語構文、標準 API、builtin ID、VM / schema version は変更しない。

- 同じ `find_all` の一致間と handle / Value clone の間で入力 buffer を共有し、成功 producer では所有 String の buffer を移す。
- 入力全文＋span の内容比較を保つ。同じ全文の独立 allocation は等しく、同じ一致 text / span でも全文が異なる値は等しくない。
- `éあ` 等の UTF-8 の byte offset、空一致、無一致、空入力を保つ。
- 元の引数・結果 List を落とした後も抽出した handle と clone を使用できる。
- `text` / `whole` / `get` / `get_name` は逆順、範囲外、UTF-8 の途中の span を既存 RuntimeError として拒否する。`start` / `end` に新しい検証や fallback を加えない。
- 正規表現の compile・探索・収集は同期処理のまま。continuation、fairness、Regex cache、入出力の非同期化は対象外。

正本は [EldrVM 仕様](../../../../docs/dev/EldrVM_spec.md) の builtin 契約を整合させる。runtime handle は bytecode Constant のシリアライズ表現とは別であり、同一 VM 内の不変値共有を使う。

## 実装・検証の手順

1. 入力 buffer 共有を直接検証する回帰テストで現行の全文コピーを Red として確認する。
2. main の未変更製品ソースを使う manual probe の baseline binary を保存し、同じ入力・lock・release 条件で比較する。
3. 入力所有を共有化し、UTF-8 / lifetime / equality / 不正 span の境界を固定する。
4. Eldr / Sindr の対象テスト、workspace CI と標準 Surtr 全件を確認する。最終差分と測定の限界を Astra にレビュー依頼する。

## 測定条件

独立 manual harness [regex_input_cost](/Users/haruca/.codex/worktrees/list-runtime-remaining/surtr/tests/profile/regex_input_cost/README.md) を使う。`run` は実 VM の compile / find_all と、LoadConst による runtime String のコピーを含む。元文字列の生成・Constant の作成は区間外に置く。区間外で VM を用意し、結果の handle 取得、handle の追加 clone、VM と各 handle 群の破棄を別区間で測る。内容・所有者破棄後の span 検証は別 process とする。性能原本では結果全走査による追加検証を行わない。

requested allocation、論理 live、process peak RSS、実時間は別指標とする。論理的に参照された入力 byte 数 B×M を、共有後の固有 buffer 保持量と読み替えない。性能の合否閾値を correctness test に追加しない。

## 結果

### 実装と前後測定

入力所有の移譲と共有化を実装した。`find_all` の一致間、`find` / `captures` を含む Value clone の間で入力全文を共有する。captures の groups / name table は従来どおり clone する。

各10条件を5回、割当有効・無効と変更前後の4系列で測った。計200 process、1,200区間行、240集計で、実行順を sample ごとに AB / BA とした。release / System allocator、依存版は root lock と一致する。両版の harness ソースと lock は同一。ビルド・テストとは重ねていない。

以下の時間は割当 counter 無効系列の中央値。requested allocation は有効系列で、いずれも5 processの中央値である。

| bytes / 一致数 | VM run requested MiB 変更前→後 | VM run ms 変更前→後 | process peak RSS MiB 変更前→後 |
|---:|---:|---:|---:|
| 1024 / 1024 | 1.281 → 0.281 | 0.307 → 0.210 | 6.531 → 3.562 |
| 2048 / 2048 | 4.547 → 0.547 | 0.631 → 0.270 | 15.953 → 3.938 |
| 4096 / 4096 | 17.081 → 1.081 | 1.836 → 0.453 | 52.906 → 4.750 |

4,096件の `collect_handles` / `clone_handles` は、それぞれ requested allocation が16.531 → 0.531 MiBとなった。これは外側 Vec の確保も含む。時間の中央値は collect が1.750 → 0.092 ms、clone が1.746 → 0.046 msだった。コピーした物理byte数を直接数えたものではない。

同条件の `run` の論理 live 増分は16.534 → 0.538 MiB。変更後は長さ4,096の全文 buffer を共有するが、値・Vec・VM等の保持分も含むため、run の live 増分を入力全文だけの保持量とは呼ばない。結果から取り出した handle の寿命検証では、VM破棄後も入力全文にアクセスできた。

無一致は `Rc<String>` の容器に40 requested bytesが加わる。空 List が入力を残すことはない。1一致対照では4,096bytesのrun requestedが25,664 → 21,608bytesとなったが、時間は0.187083 → 0.187292msであり、一律の高速化を主張しない。無一致や1一致の小さい条件のRSSには約0.05〜0.08MiBの増加もあり、binary layoutやallocator等の寄与は未分離。密一致の改善を全入力へ一般化しない。

RSSはprocess全期間の高水位で、初期化、6区間、区間外処理、JSON出力を含む。各区間へ同じ値を付けるが加算しない。request/liveとRSSは別指標であり、区間のsumを全VM所有物の解放量検証には使わない。

[生データ](regex_input_cost_2026_10_13.jsonl)・[集計](regex_input_cost_2026_10_13.summary.json)・[build記録](regex_input_cost_2026_10_13_builds.json)を保存した。[別processの検証結果](regex_input_cost_2026_10_13_verify.json)は52件で、入力全文・一致text・byte offset・空一致・VM破棄後の有効性を確認し、baselineは一致数個、共有後は非空なら1個、無一致なら0個の入力buffer pointerを確認した。空Stringはheap bufferを持たないため、このpointer数を実allocation数とは扱わない。検証系列は性能原本に混ぜていない。

### 通常の batch 入口

既存APIで `do → Regex::compile → Regex::find_all → List::map(RegexMatch::text) → Ok(件数)` を実行した。1,024/4,096bytes、密一致/1一致/無一致の6入力を両binaryでwarmし、各5process、合計60sampleを交互に測った。全sampleはartifact cache `hit`で、各binary固有のfingerprint keyを同じcache directoryに保存する。cold compiler比較ではない。observer/traceは無効で、requested allocationは取得していない。

4,096bytes密一致のCLI `execute` 中央値は54.511 → 50.502ms、process peak RSSは46.828 → 30.844MiBだった。`execute`はVM構築、通常関数呼出し、background drain、出力処理等を含み、最終VM破棄前に計測される。cache読込み等を含むCLI `total`は82.167 → 77.944ms。別のparent側process wallには起動・最終破棄・time wrapperも含むため、混同しない。

1,024bytes密一致は13.112 → 12.857ms。1一致/無一致の微小な実行差は改善の根拠にせず、通常入口の全条件が速くなるとは主張しない。出力件数は全sampleで一致した。textの全文・span・lifetimeは別のunit/manual検証で確認し、このbatch出力自体は件数だけを検証する。

[採用した通常入口の生データ](regex_input_cost_batch_2026_10_13_rechecked.jsonl)・[集計](regex_input_cost_batch_2026_10_13_rechecked.summary.json)・[独立buildのartifact記録](regex_input_cost_batch_2026_10_13_isolated_builds.json)。

#### 初回 binary の取り違えを補正

最初のbatch系列は共有targetを使った。保存binaryの全体SHAは前後で異なったが、AstraのMach-O照合で実行コード`__TEXT,__text`は同一だった。after buildではEldr/Sindrが再コンパイルされず、変更後runtimeの由来を確認できなかった。UUID・fingerprint・署名だけが違うbinaryから性能効果を判断しない。

[初回原本](regex_input_cost_batch_2026_10_13.jsonl)・[無効として保持した集計](regex_input_cost_batch_2026_10_13.summary.json)は結論から除外した。前後を明示manifest・独立targetで再buildし、Cargo JSONのSindr/Eldr/Rune manifestとexecutable、saved binaryのbyte一致を照合した。独立afterの実行コードhashはbeforeと異なることもAstraが確認した。full gateと標準Surtr全件も独立targetで再確認した。

### 検証とレビュー

- 入力共有契約2件のRed: 旧Value cloneのinput pointer不一致で失敗。環境のPATH不足による初回起動失敗はRedと扱わない。
- `rtk cargo nextest run -p sindr -p eldr -E 'test(/regex_/) | test(opaque_error_schema_checks_all_builtin_value_kinds)'`: 8 passed、468 skipped、失敗0。
- 初回の共有targetによるCIは2,528 passed。後述のbinary疑義を受け、独立target・明示manifestで `rtk cargo nextest run --manifest-path <task>/Cargo.toml --target-dir /tmp/surtr-regex-20261013-target --profile ci --workspace` を再実行した。最終結果は2,528 passed、69 binaries、失敗0、終了コード0、nextest部分129.656秒。原本は `/tmp/surtr-regex-20261013-ci-isolated.log`。
- 初回の標準Surtr全件は終了コード0。独立target・明示manifest・Cargo JSON artifactで `rtk proxy cargo run --locked --manifest-path <task>/Cargo.toml --target-dir /tmp/surtr-regex-20261013-target --message-format=json -- test --quiet --all` を再実行し、最終終了コード0。Sindr/Eldr/Runeのmanifestが全てtask worktreeで、実行exeが独立targetであることも確認した。原本は `/tmp/surtr-regex-20261013-stdlib-isolated.log`。
- `cargo fmt --all -- --check`、manual harnessのfmt、`git diff --check`: 成功。
- Astraが最終製品差分と性能原本を静的レビューし、修正必須指摘なし。無一致エラーの入力保持と測定区間の説明を補正し、1,200行/240群の集計・4binary hash・52検証を照合した。Astraはビルド・テストを再実行していない。

### 残件

Regex compile・探索・収集は未分割で、Task / processのfairnessや実時間上限を保証しない。短い一致だけでも元入力全文を保持する。一致部分のみ保存への変更は全文によるequalityの扱いを含む別仕様が必要である。

FileOutHandlerの既存契約不一致は[現行mainの再現報告](../file-out/file_out_handler_lifecycle_reproduction_2026_10_13.md)へ分けた。RT-2の採用方式、RT-3の非空Future走査、RT-5の他worktreeの残件は今回の製品変更に含めない。
