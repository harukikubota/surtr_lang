# Pattern 拡張の残実装計画

入力仕様: [pattern_extension_spec.md](pattern_extension_spec.md)。level 4。
基点は `be365e2f`。capture index 制限、binding operator の OR 拒否、branch consumer の binding OR は実装済みとして維持する。

## 実装とコミットの単位

1. **Extractor の MatchResult 更改**（仕様 §§3, 5.2, 7, 8）
   - canonical MatchResult 型・二状態の variant と利用位置制限を追加する。
   - named / builtin Extractor、標準定義、既存成功例を一括移行し、Option 専用の解釈と未使用 no-match metadata を削除する。
   - 共通 Pattern engine で Error の保持 / 破棄を分け、Extractor 本文の SafeBind と UnitOnly を接続する。
   - `uncons` の失敗 Error は builtin の契約に置き、Forge で名前から生成しない。
   - 検証: Scar の carrier / constructor 制限、Forge の保存・短絡、Eldr の builtin、既存 script / module Extractor fixtures。
2. **事前引数と Pattern の scope**（仕様 §§2, 5, 6.3）
   - 宣言の入力を複数にし、最後を照合対象にする。
   - signature 解決まで未分類の適用引数を保持し、事前引数 Expr と子 Pattern を確定する。arity 不一致は先に拒否する。
   - Pattern 開始時の外側 scope から事前引数 / pin を解決し、新規 binding は全成功後だけ公開する。
   - 検証: 0 / 1 / 複数の事前引数、Unit 省略、型・arity 拒否、外側同名変数、到達時の単一評価と短絡。
3. **ExtractorClosure**（仕様 §4.1–4.3, §8.2）
   - literal、専用 signature 型、lexical head identity、capture、通常推論と値の受け渡しを追加する。
   - callable kind を明示し、通常 Closure に MatchResult の構築 / failure target 権限を継承しない。
   - 既存 Closure の生成・呼出表現を使い、通常 call / named Extractor の値化 / 即時 Pattern head を拒否する。
   - 検証: capture、helper、generic、if / match 選択、local shadowing、nested callable / do、REPL relocation と signature。
4. **apply_pattern と projection**（仕様 §§6, 9）
   - canonical Kernel consumer として Pattern 引数を解析・検査し、Result input はそのまま照合する。
   - 番号順の projection、注釈、alias / tail、0 / 1 / 複数 slot を実装する。
   - index、利用位置、予約名、OR、pipe を検査し、Regex::is_match の通常 call / capture は canonical identity で維持する。
   - 検証: projection の成功・拒否境界、元 Error 保存、通常 Pattern Error、binding 非公開、pipe と事前引数の分離。
5. **Extractor 標準 API とソースドキュメント**（仕様 §4.4, §10.3）
   - `lib/extractor.srt` と loader 登録を追加し、通常 SRT の `Extractor::from_result` を実装する。
   - module `@doc` に named / closure / 事前引数 / Unit / projection / SafeBind / from_result の REPL サンプルを置く。
   - 要件定義・診断・do・Xldr・利用者向け説明を最終契約に整合する。
   - 検証: from_result の一段 unwrap、capture、Error 保存、生成時未評価と occurrence ごとの一回評価、拒否境界、文書サンプルの実行。

各単位では契約を直接検証するテストを先に実行して Red の理由を確認し、実装後に対象を Green にする。後続単位が未実装の間は、入力仕様の完了記録と現行ドキュメントを区別する。機能単位のコミットは root が行い、サブエージェントは共有ワークツリーの担当ファイルのみ編集する。

## 分担と統合

- frontend: Sindr の型 metadata、Spire、Sigil。適用引数・lexical identity・公開 IR の変更を backend と先に共有する。
- typecheck: Scar の型、typed contract、推論、consumer / callable policy、SafeBind。
- standard/runtime: 標準 SRT、Eldr、builtin 正本、fixtures・examples の移行。
- root: Forge、Rune / Xldr、正本・計画の整合、統合検証、コミット。

公開 IR は identity / 確定 signature / 型検査済み事前引数 / payload shape / canonical tags / projection / failure policy と source origins を保持する。Forge は表示名や raw signature から契約を復元しない。壊れた metadata / tag / payload は即時の内部 failure にし、不一致や Alternative empty に落とさない。

## 最終受入

仕様 §11.1 の17項目を最終差分と照合する。独立レビューには会話履歴を渡さず、仕様・差分・検証結果を渡し、意味論、旧経路、fallback、テスト不足を確認する。妥当な指摘の修正後に最終 revision で以下を実行する。

```sh
rtk cargo nextest run --profile ci --workspace
cargo run -- test --quiet --all
```

既知失敗、未実行、timeout が残る場合は全工程完了としない。除外 / ignored 化 / timeout 延長で Green にしない。完了した機能と実測結果だけを入力仕様 §1.1 と本計画に記録する。main への merge は依頼に含まれないため、本ワークツリーの機能コミットを成果物にする。

## 実行記録

- 第1単位: named / builtin MatchResult 更改、UnitOnly、本文 SafeBind を実装。独立レビュー完了（指摘は全件解消）。最終差分で `rtk cargo nextest run --profile ci --workspace` は1954件成功、`cargo run -- test --quiet --all` は終了コード0。作業用ビルドキャッシュには `CARGO_TARGET_DIR=/Users/haruca/work/rust/surtr/target`、CIには `SURTR_TEST_CACHE=1` を使用した。
- 初回全体検証で検出した型queryのAnnotatedWildcard追従漏れ、REPL署名期待値、SafeBindの投影後入力型再注入、generic固定shapeの過剰拒否を修正し、最終全件Greenで確認した。

- 第2単位: 複数入力、signature に基づく事前引数、外側 scope、遅延する構文診断を実装。レビューで見つかった通常 Closure の prearg capture 漏れと Forge の nominal 型表記差に対する過剰拒否を回帰テストとともに修正。最終 CI workspace 1962件成功（45 binaries）、標準 SRT 全件は終了コード0。旧診断期待2件と追加parserテストの入力を整合した後の最終差分で確認した。Extractor 標準テスト13件には複数事前引数・generic shape・capture・Unit・評価順・短絡を含む。

- 第3単位: ExtractorClosure の専用 literal / 型・first-class 値・local Pattern head・推論 / capture・本文 SafeBind を実装。引数の両候補を lexical identity と元診断で保持し、signature 選択後の canonical UID だけを Typed IR へ渡す。REPL の capture・署名・doc target、通常 generic trait API の往復も検証。独立レビューの OR binding 順序と canonical 逆変換の指摘、既存 Facet 動的パスの capture 回帰を解消した。最終 CI は1968件成功（45 binaries）、標準 SRT 全件は終了コード0。専用 SRT 17件、拒否18例と REPL 回帰を含む。

- 第4単位: canonical `apply_pattern`、projection、予約名・OR・pipe・Regex通常call/captureを実装。共通 Pattern engine の明示consumer policyで local Result join を作り、既存のliteral/list Error規則と最初の失敗を維持する。専用SRT19件、拒否28fixture、REPLのscope・capture・doc/signature、capture合成を検証。独立レビューで指摘されたcapture rewrite漏れとKernel consumer値化の名前fallbackを解消。特殊化の再帰frameはhelper抽出で縮小し、既存8MiB/16段stack回帰をそのまま成功させた。fixture runnerは従来phase診断優先を保ちつつScarで確定する元Parse/Resolveエラーを検証する。最終CI1972件成功（45 binaries）、標準SRT全件終了コード0、未解消review指摘なし。
