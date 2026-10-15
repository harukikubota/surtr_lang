# Error 宣言の本体検査を局所化する

2026-10-16。[03_compile_time.md](03_compile_time.md) の P1 を実施する。
開始時の HEAD は `6a9529cd`。既存の型規則・診断・公開 API・保存形式を保つ level2 の内部最適化とする。

## 対象と手順

`checker/definitions/nominal.rs` の `check_deferror_def` が宣言ごとに行う子 Checker の metadata 複製を減らす。
変数表 `env.vars` の保存 clone は維持し、型定義・関数署名等の read-only metadata と区別する。
String 本体を含め、すべての本体を通常どおり検査する。
P2a / P2b、trait probe の子 Checker、永続 cache、session をまたぐ summary は今回変更しない。

通常のdebug stackでの確認では、用意済みのgeneric chain 64段が変更前後ともstack overflowになった。
16 / 32段は双方で成功した。[03の採用条件7](03_compile_time.md#6-比較採用条件) を満たすため、
[既存のスタック使用量修正](../optimize/018_scar_specialization_stack_usage.md) と同じ方針で、
specializationの定義ノード書換えを小さいhelperへ移す。固定点計算・key・予約順・生成内容は変えない。
これはP2aの走査削減とは分け、通常の8176 KiB stackで64段の正順・逆順が成功することを確認する。

1. 現行 child の状態継承・成功時反映・失敗時破棄を整理する。
2. 宣言間の状態分離、推論の反映、失敗後の復元を直接検証するテストを先に固定する。
3. 本体検査に閉じた scope または状態の移動で複製を減らし、置換した経路を削除する。
4. Scar、関連する Xldr / Rune、CI profile の workspace と標準テストを確認する。
5. 同一入力の before / after を測定し、最終差分を別エージェントでレビューする。

正本は [Error_spec](../../docs/dev/Error_spec.md)、session の契約は [Xldr_spec](../../docs/dev/Xldr_spec.md)、
観測区間は [Rune_observability](../../docs/dev/Rune_observability.md) に従う。意味論は変更しない。

## 測定と採用条件

開始時点の clean HEAD を専用の一時ソースディレクトリへ保存し、before / after の release target を分ける。
測定中は binary を更新しない。入力・標準ソース、commit / diff、binary hash、toolchain、host、電源条件を保存する。

[cases.json](../../tests/profile/scar_compile/cases.json) の全24入口を、各 binary・各 cache 条件で10回ずつ測る。
入力ごとに before / after の順序を交互にし、各試行で専用 cache の cold と、その直後の disk hit を組にする。
profiler は無効にする。終了コード、cache 状態、artifact 不使用、execute skipped を検証し、生値と中央値・最小最大を保存する。
内部費用の帰属は別走行の profiler で確認する。

目標は cold の小入力で `stdlib_load` と `compile_total` の中央値を10%以上、
Error 64 / 128 / 256 宣言の disk-hit `typecheck` を20%以上減らすこと。
これは実装前に定めた採用目安とし、達成状況は後述の実施結果に記す。
他の機能別・規模別入力で `compile_total` の5%超の悪化が繰り返す場合は原因を確認してから採否を決める。
差が1ms未満の条件はばらつきも確認し、小さい差だけから効果や回帰を断定しない。

process warm / incremental 専用の性能 harness は対象外とする。checkpoint / rollback の正しさは既存・追加テストで確認する。

## 本体検査の状態境界

保存地点は入力型の解決後、本体検査の直前とする。入力型の解決中に親へ生じた進行は、
本体の失敗時にも巻き戻さない。旧実装の環境複製は入力型の解決前なので、
本体の `next_tyvar` は複製時点の値で開始し、成功時だけ親の値との最大値を保持する。

| 状態 | 本体へ渡す値 | 成功時 | 失敗時 |
|---|---|---|---|
| `env.vars`、入力束縛 | 変数表を保存 clone して入力を追加 | 保存した表へ復元 | 保存した表へ復元 |
| `concrete_error_bindings` | 既存の scope が保存して親を継承 | scope 前へ復元 | scope 前へ復元 |
| `error_definition_uid`、`function_return_ty` | 対象 UID、`Error` | 元へ復元 | 元へ復元 |
| substitutions、type-variable bounds、process-marker type variables | 親を継承 | 本体の進行を保持 | 保存値へ復元 |
| `env.next_tyvar`、`next_tag` | 旧 child の開始値 | 親の保存値との最大値 | 親の保存値へ復元 |
| active capabilities | 親を継承 | 既存の各 capability へ consumed だけを OR | 保存値へ復元 |
| pending trait obligations | 親を継承 | 保存値へ復元 | 保存値へ復元 |
| Facet・Lazy capture・constructor capability・explicit closure parameter・constructor witness・Pattern alias・safe-operator result の局所 map | 親を継承 | 保存値へ復元 | 保存値へ復元 |
| warnings | 空から開始 | 親の既存 warning の後ろへ一度追加 | 本体の warning を破棄 |
| alias expansion stack、constructor mapping resolution、direct Pattern summary、boot plan | 旧 child と同じ空の状態 | 親の状態へ復元 | 親の状態へ復元 |
| 関数等の lexical context、active Lazy capture | 親を継承 | 親の状態を保持 | 親の状態を保持 |
| declaration metadata、process specs と dependency | 親の値を参照 | 変更しない | 変更しない |
| profiler | 同じ集計器 | 検査イベントを保持 | 検査イベントを保持 |

Spire は式 block 内の宣言を拒否するため、正当な Spire→Sigil 出力の本体から declaration metadata は変更されない。
公開 API に手作業で渡した不正な `Resolved` の宣言配置を、新たに受理・拒否する仕様は追加しない。
`TypeEnv` の scope は `bind_var` の変更を記録する。通常束縛から呼ぶ環境正規化は既存の直接書込みを保ち、
型が変わらない項目の書込みだけを省く。通常の do / match / closure では、この正規化が外側の変数型へ
反映される既存の推論規則を変更しない。deferror 本体では push/pop の後、保存した変数表を完全に戻す。
変数表 clone の削減は別の仕様で検討し、今回の最適化へ含めない。

変数表、本体の局所 map と失敗時に復元する推論状態は保存するが、型定義・関数署名・trait 定義等の大きい
read-only metadata は複製しない。通常関数の本体 helper と、trait probe の child 経路は置き換えない。

## 実施結果

P1の子Checkerを専用scopeへ置き換え、宣言metadataの複製と、子から親への再複製を除いた。
使用先がなくなった `absorb_child_progress` と、そのprofilerイベントも削除した。
変数表と局所推論状態の保存は維持している。通常scopeの環境正規化をundo対象にすると既存のdo推論が変わるため、
deferror境界で保存した変数表を完全に戻す。型が同じ項目への書込みだけは省略する。

64段のgeneric chainは、変更前の通常debug CLIで正順・逆順ともabortし、16 / 32段は成功した。
定義ノードの書換えを小さいhelperへ移した最終CLIでは、全6入力が通常の8176 KiB stackで成功した。
stack増量、再試行、旧経路への切替は追加していない。
キー生成・再帰用function index予約・生成順・固定点計算は維持した。
debug CLIの静的なフレームサイズは、従来のother処理76,720 bytesに対し、新しい定義専用処理は5,312 bytesだった。
[変更前](evidence/p1/stack_before_cli_frame.log)・[変更後](evidence/p1/stack_after_cli_frame.log)の逆アセンブルと
[binary情報](evidence/p1/stack_frame_manifest.json)を保存した。これはreleaseの実行時間やメモリ使用量の値ではない。

### 性能

Apple M2 / macOS 26.6.2 / Rust 1.94.1、同じrelease設定で測定した。
beforeは開始時HEADのソース保存、afterはP1と上記stack修正を含む最終ソースである。
各binaryのtargetとcacheを分離し、埋め込み元の標準ソース130ファイルとCargo設定の一致を確認した。
AC電源で、他のagentのビルド・テスト・LLDBを停止した。OSのpage cache、CPU周波数、他アプリの負荷は固定していない。

全24入力 × before / after × cold / disk hit × 10回、計960回が成功した。
全走行でcache条件、artifact不使用、execute省略を検査し、測定前後の入力・標準ソース・binary hashも一致した。
profilerはこの測定で無効にした。単位はms、各10回の中央値である。

| 入力・条件 | 指標 | before | after | 短縮 |
|---|---|---:|---:|---:|
| 小入力・cold | stdlib load | 418.740 | 344.089 | 17.83% |
| 小入力・cold | compile total | 428.596 | 354.490 | 17.29% |
| Error 64宣言・disk hit | typecheck | 65.145 | 33.341 | 48.82% |
| Error 128宣言・disk hit | typecheck | 130.569 | 64.403 | 50.68% |
| Error 256宣言・disk hit | typecheck | 269.130 | 128.872 | 52.12% |
| 小入力・disk hit | compile total | 26.492 | 26.553 | -0.23% |

採用目安を満たした。coldのcompile totalは全入力で13.78〜31.66%短縮した。
disk hitのcompile totalの変化は46.91%短縮〜1.17%増加で、5%超の悪化はなかった。
小入力のdisk hitの差は0.061msであり、改善とは判断しない。
各回の値と最小最大は[生値](evidence/p1/runs.jsonl.gz)と[全入力の集計](evidence/p1/summary.json)に保存した。
生値はstdout / stderrを内包したJSONLのgzipで、cache payloadと生成 `.eldr` は一時測定先に残した。
source / diff / binary / 入力hash、toolchain、電源と実行順は[manifest](evidence/p1/manifest.json)を参照する。

別走行の[profiler記録](evidence/p1/profiles.json)では、標準定義の `DeferrorDef` は174件を保ち、
child spawnは174→0回、本体の集計時間は168.670→88.912msだった。
256宣言のdisk hitでも256件を保ち、child spawnは256→0回、266.392→125.079msだった。
これらはprofiler有効の各1回であり、通常測定と混ぜず、入れ子のタイマーも加算しない。
上表は最終変更全体の比較であり、stack修正単独の速度効果を分離した値ではない。

### 検証とレビュー

| 検証 | 結果 |
|---|---|
| deferrorの成功・失敗、metadata共有、変数表復元、do推論のfocused test | 5件成功。元実装と途中実装での意図したRedを記録 |
| `trait_method_instantiation` | 16件成功。正逆64段・8 MiBの追加テストは修正前SIGABRT、修正後成功 |
| `rtk cargo nextest run -p scar` | P1段階で409件成功。後続stack修正と追加テストは最終workspace検証に含む |
| `rtk cargo nextest run --profile ci --workspace` | 最終ソースで2628件成功 |
| `cargo run --release -p rune --bin surtr -- test --quiet --all` | after専用targetで終了コード0。quiet出力に件数はない |
| 通常debug CLIのgeneric chain正逆16 / 32 / 64段 | 最終ソースで全6入口成功。`RUST_MIN_STACK`未指定 |
| 独立レビュー | do推論への影響を修正後、stack修正も含めた最終差分を確認。未解決指摘なし |

[検証一覧](evidence/p1/validation.json)、[CIログ](evidence/p1/workspace-ci.log)、
[標準テストログ](evidence/p1/stdlib-tests.log)、[debug CLI比較](evidence/p1/debug-chains.json)、
[ビルド時の差分](evidence/p1/after-source.diff)を保存した。
[測定スクリプト](../../tests/profile/scar_compile/measure.py)で同じ手順を再実行できる。

P2a / P2bの走査削減、P3の一般化、P4のidentity index、変数表clone削減、process warm / incremental専用性能harnessは未実施。
公開API・checkpoint形式・言語仕様・schema / VM versionは変更していない。
