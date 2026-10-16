# Scar の大規模リファクタリング調査

2026-10-15 / 調査対象 commit: `a0d430a609da73f7f35bbba7a01f41f9b28dfc09`

先に既存の型検査契約をテストで固定し、責務単位のファイル移動と、処理量を減らす最適化を別々に進める。現時点では、初回コンパイルの Error 宣言検査と、キャッシュ利用時の宣言・具体化処理が有力な調査対象である。

初回調査では製品コード・実行可能テストを変更せず、調査と実装計画を作成した。サブエージェントがテスト、分割、コンパイル性能を分担し、親エージェントが基準値の実測と全体の整合確認を行った。

**追加依頼への対応:** 計測入力の整備は実施済み。[05_compile_inputs.md](05_compile_inputs.md) に記録した。機能別・規模別・複数 module の新規 22 入口を追加し、既存 2 入力と合わせて cold / disk hit 各 1 回のコンパイルを確認した。

**テスト計画の実施（2026-10-16）:** P0 / P1 の T-01〜T-12 を追加・拡張した。対応箇所と検証結果は [01_test_plan.md の実施記録](01_test_plan.md#8-テスト計画の実施2026-10-16) を参照。

**ファイル分割の実施（2026-10-16）:** `02_file_split` の移動を実施した。既存の `Checker` と公開 API を保ち、式、session・状態、宣言・型・具体化を責務別に配置した。実際の配置と検証結果は [02_file_split.md の実施記録](02_file_split.md#実施記録2026-10-16) を参照。処理回数・コピー範囲・型規則は変更しておらず、分割単独による速度改善は主張しない。

**性能改善の実施（2026-10-16）:** P1のError本体検査を専用scopeへ置き換え、宣言metadataの複製を減らした。64段generic chainのdebug stack修正も実施。全24入力を各条件10回、計960回測定し、小入力coldのcompile totalは17.29%、Error 64 / 128 / 256宣言のdisk-hit typecheckは48.82〜52.12%短縮した。最終CI profileは2628件成功。条件と記録は [06_deferror_scope.md](06_deferror_scope.md) を参照。

**P2の実施（2026-10-16）:** specializationのcompile内summary共有・逆辺worklistと、trait前宣言の候補・宣言検索の局所化を実施した。別の960回比較で、disk-hit typecheckは逆順generic chain 64段で18.88%、trait impl 64件で12.99%短縮。正順64段は4.97%で10%目安未達。最終CI profileは2635件成功。合算比較と採用判断は [07_specialization_predeclare.md](07_specialization_predeclare.md) を参照。

**P3の実施（2026-10-16）:** `deferror` scopeの変数表全コピーを変更差分のundoへ置き換えた。全24入口・960回の比較で、Error 256宣言のdisk-hit typecheckは78.61%、小入力coldのcompile totalは19.72%短縮した。最終CI profileは2,640件成功。初回Scar検証の2件timeoutと後続全件成功も区別して [08_state_copy.md](08_state_copy.md) に記録した。

## 調査結果

| 項目 | 結論 | 詳細 |
|---|---|---|
| テストケースの追加 | 既存の成功・拒否に加え、nested probe、失敗後の再試行、通常／in-place API、full／prefix compile の比較を補った。P0 の7群、P1 の5群を追加・拡張済み | [01_test_plan.md](01_test_plan.md) |
| ファイル分割 | 責務別の移動を実施済み。`checker/mod.rs` は553行、`expr/mod.rs` は1,018行となり、入口と共通型を残した。状態所有者の再設計は行っていない | [02_file_split.md](02_file_split.md) |
| `.srt` コンパイル時間 | P1のmetadata複製削減、P2のTrait前処理・specialization走査削減、P3の変数表複製削減を実施。それぞれ全24入力でbefore / afterを測定 | [03_compile_time.md](03_compile_time.md)、[P1結果](06_deferror_scope.md)、[P2結果](07_specialization_predeclare.md)、[P3結果](08_state_copy.md) |
| 基準値 | release binary で 3 入力 × cold / disk hit × 5 回を計測。追加 profile 6 回と生ログを保存 | [04_baseline.md](04_baseline.md) |
| 測定入力の整備 | 現行の機能別入力、規模系列、宣言順比較、複数 module を追加。計測対象 24 入口の一覧を保存 | [05_compile_inputs.md](05_compile_inputs.md) |

初回調査時の Scar の `src` は 24 Rust ファイル、61,572 行（空行・内部テストを含む）。テストは既に多数あり、`typecheck_surface` だけでも登録関数が 430 件あった。これらを「既存テストが少ない」と扱うのではなく、今回触る状態・走査の境界に追加した。分割後の規模は `02_file_split.md` の実施記録に記載する。

disk hit の中央値は次のとおり。これは速度改善前の基準値であり、リファクタリングによる短縮値ではない。

| 入力 | user suffix の typecheck | compile total |
|---|---:|---:|
| 小入力 `stdlib_prewarm` | 2.126 ms | 25.728 ms |
| 独自 carrier / Trait impl / `do` | 21.243 ms | 48.192 ms |
| generic extractor | 8.237 ms | 40.079 ms |

cold の標準定義 load は約 407〜410 ms（各条件の中央値）。別の profile 走行では標準定義の Scar 処理 287.595 ms のうち、`DeferrorDef` の検査が 174 件で 163.654 ms だった。ここには child の構築・反映なども含まれ、各タイマーは重複する。Error 宣言のどのコピー・走査を減らせるかが次の調査点となる。

## 目的と維持する契約

- 型検査の責務を追いやすくし、局所変更で確認すべき状態とテストを明確にする。
- 現行仕様の成功・拒否、型、callable identity、diagnostic reason / source span、warning、process metadata を維持する。
- `.srt` の parse → resolve → typecheck → codegen と、Rust 自体の build、テストハーネス、VM 実行を分けて測る。
- 置き換えた実装は削除する。エラーを握りつぶす fallback、旧仕様の成功経路、互換スイッチを追加しない。
- schema / VM version を上げない。ファイル移動のために checkpoint や公開 IR の形を変えない。

対象外は、言語構文・型規則・暗黙変換の追加、別の checker backend、並列型検査の導入、VM の高速化、Rust build 自体の高速化、CLI の新機能、テスト bucket 数の調整。最適化の検証に必要な既存 CLI は利用する。

## 実装順序

各段階はレビューと検証が独立する単位に分ける。最適化のために分割案の全項目を先に完了させる必要はなく、測定で選んだ責務の移動までで着手できる。

| 段階 | 作業 | 依存・完了条件 |
|---|---|---|
| S0: 基準の固定 | 対象 commit・入力 hash・cache 条件を固定。既存 focused test を実行し、触る契約に対応する追加ケースを書く | 既存失敗と新規失敗を区別。時間計測は Rust build と分離。今回取得した値は出発点で、実装開始時の HEAD でも再確認 |
| S1: 最初の分割 | profiler、builtin bootstrap、function-index rewrite、内部テストなどから一単位を選ぶ | 計算手順・公開 path・可視性・保存形式を維持。元実装を削除。対応テストと Scar を確認 |
| S2: 式・宣言の分割 | Facet / process / call、型構文 / inference、定義 / 前宣言 / 具体化を責務単位で順に移動 | [分割表](02_file_split.md) の関数群を依存ごと移す。巨大な共通 helper や新旧経路を作らない |
| S3: 性能改善 | 最も効果が期待できる 1 候補を選び、コピー数・走査数を調べ、最小の変更で比較する | [性能計画](03_compile_time.md) の候補ごとのテストと before / after。全候補を一括で実装しない |
| S4: 統合検証 | focused test 後に影響する fixture / REPL、最終 workspace 検証、同条件の性能比較 | 機能・診断・状態の維持を確認。改善した入力と未改善の入力を分け、未実行や失敗を残したまま完了としない |

最初の具体的な着手単位には **profiler / bootstrap の分割と既存テストの維持**を勧める。推論状態に手を入れる前に、移動だけの差分を完結させられる。その後、Error 宣言の child 状態を調べる場合は T-03〜T-05、specialization の再走査を変える場合は T-06 / T-07 / T-12 を優先する。これらは変更対象に応じて選び、全ケース追加を機械的な前提にしない。

## level と検証

既存仕様と公開契約を保つファイル移動・内部アルゴリズム改善は **level2** とする。行数の大きさだけでは level4 にしない。型規則、評価規則、フェーズ間契約の変更が必要になった場合は **level4 の別仕様**とし、この計画に混ぜて実装しない。現段階で新しい言語仕様の未確定事項は提案していない。

実装では変更対象のテストから始め、`rtk cargo nextest run -p scar` を通す。session / checkpoint は Xldr、source / module / diagnostic は Rune の該当 fixture へ広げる。最終統合前は `rtk cargo nextest run --profile ci --workspace` を一度実行する。標準ソースへの影響を確認する場合は `cargo run -- test --quiet --all` も実行し、level4 に進んだ場合は両方と独立レビューを必須にする。default profile を全件検証と呼ばない。

性能の受入条件は、対象処理の実行回数・コピー対象などの削減根拠と、同じ条件の `.srt` コンパイル比較を揃えること。単なる行数減少、nextest の高速化、final artifact cache hit は Scar の速度改善の証拠にしない。比率の目標値を今回の 3 入力だけから決めず、変更候補を選んだ時点で規模系列と代表入力を固定する。

## 関連する正本

- [Trait system](../../docs/dev/Trait_system_spec.md): Trait head、constructor capability、候補選択、型推論。
- [Error](../../docs/dev/Error_spec.md): Error 値と ErrorKind、宣言・呼出しの契約。
- [Pattern](../../docs/dev/Pattern_spec.md)、[do](../../docs/dev/Do_intrinsic_spec.md)、[Lazy](../../docs/dev/Lazy_spec.md): 分割する式の成功・拒否と metadata。
- [診断](../../docs/dev/diagnostics.md): reason、source facts、表示境界。
- [Xldr](../../docs/dev/Xldr_spec.md): incremental session、失敗と再試行。
- [観測](../../docs/dev/Rune_observability.md)、[テスト方針](../../docs/dev/テスト方針.md): 計測条件と回帰の配置。

純粋な分割ではこれらの仕様を変更しない。説明が必要なら [Scar README](../../crates/scar/README.md) に実装後の責務配置を反映する。標準 API や `lib/**/*.srt` の `@doc` は現行仕様の参照先であり、この整理を理由に書き換えない。

## 初回調査の検証と現在の残件

- release compiler の build 成功。時間比較用の `.srt build` 30 回、別の内部 profile 6 回は成功。
- 初回の `heavy_compile.srt` は現行 Error 仕様と不一致で失敗。ファイルは変更せず、失敗ログを保存して代替 fixture で測定した。
- 文書の相対リンク 125 件、JSON の整合、入力・バイナリ hash、変更範囲を確認した。
- 別エージェントのレビューを実施。既に scope 化済みの経路を再提案していた箇所と、rollback 後の関数番号再割当を禁止しかねないテスト条件を修正した。
- 初回調査では Scar / workspace の nextest、標準テスト、生成 bytecode の実行は未実施。後続の入力整備では標準テスト全件の成功を確認した。コンパイル成功をリファクタリング後の回帰検証とは扱わない。

回帰テスト追加、ファイル分割、P1 / P2a / P2b / P3の性能改善と全24入口の反復比較は2026-10-16に実施した。残件はP4、session transactional cloneと診断用child probeの最適化、process warm / incremental専用の性能測定。公開契約を保ち、P1では宣言metadataの複製、P2では走査と検索、P3では変数表の複製を減らした。
