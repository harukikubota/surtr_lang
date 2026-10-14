# OI-036: ready収集の確保要求と実時間

入力は[runtime一覧](../README.md)と[ready map借用の残件](future_ready_snapshot_borrow_2026_10_14.md#残件)。owned収集の`c76a0980`と借用収集の`1557b9a6`を比較する。製品コードは変更せず、Eldrのlevel2としてcfg(test)観測と手動測定を追加する。

## 測定条件

Future表とoperand stackの準備後、実際の`resolve_ready_pending_stack_values`一回だけを測る。256 / 4,096位置、0 / 64要素Tuple、同じIDの反復 / 全ID別、Ready / Running / Futureなしを使う。Futureなしでも非空のReady表を用意する。cleanup Taskは登録しない。

確保要求と時間は別のfresh processで測る。両版ともSystemへ委譲するtest専用allocatorを使い、明示したscope内の測定threadだけで、成功したalloc・alloc_zeroed・reallocの要求bytesと回数、解放bytesと回数を数える。reallocは増分ではなく新しい要求全量と旧layoutの解放を数える。準備済みのbufferの解放も観測できる。物理確保量・allocatorのusable size・RSS・peak保持量とは区別する。

時間系列では確保の記録を無効にするが、両版にallocator wrapperの判定と既存の論理counterが残る。Instant区間にprocess起動や準備を含めない。6 sampleずつをAB / BA同数で比較し、速度の閾値を受入テストにしない。全slotの内容と元のFuture値の確認は別のverify processで行う。

前後のrelease Cargo test executableを独立targetで保存する。基準はGit archiveへ同じ測定fixture・allocatorを加え、patch、全source hashとbinary hashを記録する。確保要求のscopeはalloc・zeroed・realloc・既存blockの解放を含む直接テストでも確認する。製品動作を変えない観測なので、人工的なRedは作らない。

## 結果

48 processの全内容確認と、確保・時間各288 process、計624 fresh processが成功した。各系列は前後6 sampleずつ、AB / BA同数。70個ずつのsource / manifest / lock hash、Cargo artifactと実行binary hashを照合し、同じfixture・allocatorとslot clone / map型サイズのcounter条件で比較した。比較中はbuild・他のtestを行っていない。

| 4,096位置、Ready | 確保要求 bytes 前→後 | 確保回数 前→後 | 解決時間中央値 ms 前→後 |
|---|---:|---:|---:|
| 全ID別、64要素Tuple | 73,678,364 → 35,930,140 | 8,204 → 4,108 | 15.851 → 6.633 |
| 同じID反復、64要素Tuple | 35,660,876 → 35,651,660 | 4,098 → 4,097 | 3.799 → 3.746 |
| 全ID別、0要素Tuple | 2,375,196 → 278,556 | 12 → 12 | 0.656 → 0.444 |
| 同じID反復、0要素Tuple | 588 → 76 | 1 → 1 | 0.100 → 0.071 |

64要素・全ID別の要求削減は37,748,224 bytesだった。このうち元のTuple bufferの収集用コピーが35,651,584 bytes、mapの要求差が2,096,640 bytes。0要素の対照はTuple bufferを確保せず、mapの要求差を確認できる。再確保は全条件で0回であり、mapの拡張は別blockのalloc / deallocとして記録された。各値の配送用コピーは省かれない。

64要素・全ID別の時間範囲は変更前14.874〜19.105 ms、変更後6.497〜7.517 msだった。同じID反復は変更前3.698〜4.013 ms、変更後3.658〜4.144 msと重なる。同じIDでは元々一回だけsourceをcloneするため、今回の削減量は9,216 bytesにとどまる。測定結果をすべての配置や通常CLIへ一般化しない。

4,096位置・Ready・64要素のscope内要求−解放は前後とも35,651,584 bytes（4,096 × 64 × Value 136 bytes）だった。これは配送後に残る要素bufferの要求layout分に一致する。準備済みのFuture表・値Vec・位置表やallocatorの実使用サイズを含むVM全体のlive量ではない。0要素ではscope内純増0だった。

RunningとFutureなしは全条件で要求0・alloc / realloc 0回。Futureなしの時間は0〜208 nsと時計の粒度に近く、0のsampleもあるため時間比を評価しない。Running・全ID別64要素の中央値は0.125→0.120 ms、範囲は0.123〜0.133 / 0.118〜0.127 msと重なる。未解決経路の確保や全体性能の改善は主張しない。

[原本](future_ready_snapshot_cost_2026_10_14.jsonl)・[集計](future_ready_snapshot_cost_2026_10_14.summary.json)・[変更前build](future_ready_snapshot_cost_2026_10_14_before_builds.json)・[変更後build](future_ready_snapshot_cost_2026_10_14_after_builds.json)。時間は既存のcfg(test)論理counterとallocator wrapperを含む内部一回の処理であり、製品CLIの測定ではない。Future表のsource cloneが減ることで、そのclone用counter呼出しも減る。cleanup-bearing Task、Cancelled、unknown、交互ID、入れ子の値種別、RSS・peak保持量は今回の費用測定に含めない。既存の受入テストと過去の操作数系列は維持する。

## 再現と検証

[基準patch](future_ready_snapshot_cost_2026_10_14_baseline.patch)をGit archiveの`c76a0980e736d71d80663717aa8cc5031b4a197f`へ適用し、変更前buildの70 source hashと一致することを確認した。測定fixture・allocator・既存のslot / layout counter hookだけを加え、製品のowned経路を改変していない。過去の借用受入期待値を含むため、この基準の全testをGreenとして扱わず、cost manual testだけを実行する。準備時にrevisionの記録誤りとcounter不足があったため、そのbuildは比較から除外し、修正・再構築した最終buildだけを原本に記録した。[再現手順](../../../../tests/profile/future_stack_scan/README.md#ready収集の確保要求と時間)。

現行製品への変更はなく、`#[cfg(test)]`のmodule、直接テストとmanual fixtureを追加した。最初の関連nextestは2 passed / 391 skipped / 終了コード0。Rust fmt・Python構文と基準patchの適用・source一致を確認した。同じ`CARGO_TARGET_DIR=/tmp/surtr-oi036-debug-target`で`rtk cargo nextest run --locked --profile ci --workspace`は2,573 passed / 69 binaries / 失敗・除外0（113.536秒）、`rtk proxy cargo run --locked --message-format=json -- test --quiet --all --format=json`は標準1,124件すべて成功。終了コードはともに0。標準のfailed / skipped / pending / filtered / policy_errors / scope_failures / script_errorsは0。[検証記録](future_ready_snapshot_cost_2026_10_14_validation.json)へcommand、結果と最終source・原本hashを保存した。

差分空白とローカルリンク、runner / builder / binary hash、原本624行と集計の一致も確認した。最終差分を自己点検し、allocatorの記録処理が割当・assert・ログを行わず、失敗した要求を数えないこと、reallocとscope終了・Dropの扱い、thread-local集計、製品buildへmoduleを含めないことを確認した。既存の製品経路・公開API・VM/schema version・標準sourceは変更していない。level2のため別エージェントは使っていない。

## 残件

今回確認した条件では収集用の確保要求が減り、全ID別のReady入力で時間も短くなった。mapの確保と配送用clone、未解決IDの再確認・Future / cleanup表検索は残る。通常CLIと他の値種別、cleanup Taskを含む時間・物理確保量・RSS、OI-037は未測定・未解決である。
