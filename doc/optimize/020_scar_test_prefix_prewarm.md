# Scar のテスト時間短縮

## 目的と方針

Scar の言語仕様・診断・テストケースを維持し、標準前置状態のプロセスごとの再構築と
Pattern 要件伝播の反復 AST 走査を減らす。言語の意味論を変えない level 2 の改善とする。
今回はユーザーが仕様ファイルの入力なしで変更まで進めることを明示している。

## 現状の実測

貼付ログは default profile、331 件成功、経過 26.209 秒、各テスト時間の合計
196.636 秒。nominal_constructor_parameters / lazy_capture_revision /
grouped_contextual_callables / lazy_capture_diagnostics の 53 個別テストが合計
133.088 秒を占める。各 nextest プロセスは support の OnceLock を持つため、同じ
標準ライブラリの resolve / typecheck を繰り返している。

変更直前の同一 checkout での測定は 331 件成功、経過 27.572 秒、各テスト時間の合計
209.120 秒。ビルド時間は nextest Summary の経過時間に含まれない。

## 変更の契約

### コード側

- DirectExpression の Pattern 要件を集める最初の走査と不変条件検査は維持する。
- 初期要件が空なら、その後の要件伝播は終了する。
- 要件があれば呼出しの置換情報を一度抽出し、固定点計算では抽出した参照だけを走査する。
- 署名が所有する型変数だけに伝播する制限、置換の照合、診断は変えない。

### 事前準備

- Scar の test support 用 prefix を nextest の setup で準備し、各テストへ渡す。
- 保存する状態は ScarCheckpoint、ResolveResumeState、process specs、boot plan。
  AST と declaration index はテストバイナリが自身の入力から構築する。
- ビルド時の compiler / source 指紋はテスト専用の補助 crate に閉じ込める。
  関係するソース、標準入力、manifest、Cargo.lock、ビルド条件を鍵へ含める。
- 古いバイナリは自身のコンパイル時指紋を使う。実行時の現行ソースから鍵を作らない。
- 指名された準備状態の欠落、指紋や schema の不一致、破損はエラーにする。
  不正な状態を再構築で隠さない。事前準備なしの通常 cargo test は同じ指紋の状態を読み、
  miss 時だけ排他制御して構築する。
- 各ケースは復元した独立 session で実行する。ケースの除外、タイムアウト延長、ignored 化をしない。
- cold profile でも既存の default stdlib 準備を実行する。CLI 固有の temp cache や
  test-enabled stdlib、最終 fixture のキャッシュはこの準備で代用しない。

## 受入条件と検証

- 多段の generic forwarding で Int の成功と外部 thunk の DirectExpression 拒否を維持する。
- キャッシュの復元、指紋・schema・checksum・欠落の拒否、同時初期化を直接検証する。
- Scar 全体、cold CLI 境界、workspace CI、標準 SRT テストを通す。
- 準備時間・テスト時間・各テスト時間の合計を分け、同じ profile の変更前後を比較する。
- 最終差分を別エージェントがレビューする。新規問題は独立コンテキストの Astra に相談する。

## Astra の設計助言

実行時のソースだけで作る鍵では、古いバイナリが現在のソースの鍵に古い状態を保存し得る。
また run 固有の鍵だけでは producer と consumer の compiler の一致を保証できない。
setup 内で別途 cargo run する場合もビルド条件の不一致を検出する必要がある。
製品側の build.rs を増やさず、テスト専用 helper のコンパイル時指紋を使い、復元時に
一致を独立検証する方式を採用した。

## 性能測定結果

同じ checkout、default profile の Scar 全体で測定した。経過時間は setup を含み、
Cargo のビルド時間は含まない。テスト時間の合計は、各プロセスの実行時間を足した値である。

| 段階 | 成功件数 | 経過時間 | setup | テスト時間の合計 |
|---|---:|---:|---:|---:|
| 変更前 | 331 | 27.572s | なし | 209.120s |
| コード側のみ | 339 | 27.073s | なし | 203.327s |
| 事前準備込み、prefix 初回構築 | 339 | 8.837s | 2.040s | 46.891s |
| 事前準備込み、再実行 | 339 | 8.055s | 0.739s | 51.018s |

主な短縮は標準 prefix の再構築削減による。初回構築を含め約68%、再実行で約71%の
経過時間短縮を確認した。コード側のみの全体差は約2%であり、単回測定では揺らぎと
厳密に分離できない。固定点中の AST 再走査を除いたことはコードと境界テストで確認した。

独立レビューと Astra 顧問レビューに阻害指摘はなかった。compile 時の feature 集合は
指紋へ直接含めておらず、profile は helper のビルド条件である。現在 prefix 意味論を
変える feature はない。将来それを追加する場合は失効鍵の契約も更新する。

## 検証結果

- `cargo nextest run -p scar`: 事前準備あり初回・再実行とも339件成功。
- `SURTR_TEST_CACHE=1 cargo nextest run --profile ci --workspace`: 2,076件成功、45.271秒。
- `cargo nextest run -p scar --test std_prelude_cache`: 追加したschema・末尾データ拒否を含む10件成功。
- `cargo nextest run --profile cold -p rune --test integration`: 77件成功、55件はcold対象外、9.788秒（準備1.019秒を含む）。
- `cargo run -- test --quiet --all`: 終了コード0。
- 変更したRustファイルの`rustfmt --check`、`sh -n`、`git diff --check`: 成功。
- `cargo fmt --all -- --check`は、変更前からある`crates/sigil/src/resolver/pattern_consumers.rs`の整形差分で不成立。
  今回の変更範囲から外れているため、そのファイルは変更していない。

workspaceの全件確認後に追加した2件はschema・末尾データの拒否テストだけであり、
製品コード・support・失効鍵は全件確認後に変更していない。

## 全体用 prewarm への統合

`scripts/scar-test-prewarm.sh` と Scar 専用の setup 定義を削除し、
`scripts/ci-stdlib-prewarm.sh` に stdlib と Scar の準備をまとめた。
`ci` / `cold` は既存の全体用 setup を一度実行する。`default` は全体・部分実行とも setup
を省き、テスト側の同じ排他キャッシュ構築を使う。スクリプトを直接実行した場合も
両方の準備を行い、nextest の環境ファイルがある場合だけ prefix のパスを通知する。
上記の性能表は統合前の測定結果である。

統合後の検証は、スクリプト単独実行が終了コード0、setup なしの Scar が341件成功
（6.925秒）、workspace CI が2,078件成功（42.692秒、全体用 setup 1回の2.246秒を含む）。
`sh -n`、変更したexampleの整形確認、`git diff --check`も成功した。
独立レビューに阻害指摘はなかった。
