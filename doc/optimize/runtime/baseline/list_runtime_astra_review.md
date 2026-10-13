> 取り込み元: `/Users/haruca/.codex/worktrees/4c18/surtr`（detached、HEAD `ee13e448`）。資料の集約。製品変更を伴わない基準測定。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](../README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# List / VM 追加調査に対する Astra レビュー

更新日: 2026-10-11。状態: 読み取り調査と測定結果の独立照合を完了。改善方式の決定・製品実装は未実施。

対象は [List / VM の残課題](../list_runtime_followups.md) の RT-2〜4 に関連して見つかった性能候補である。
本レビューでは製品コード・実行可能テストを変更せず、ビルド・テスト・性能測定も実行していない。
初期調査のコピー回数と保持量は現行コードから導いたもので、実時間・RSS の実測値ではない。
その後に親エージェントが取得した結果は [測定報告](list_runtime_measurement_report_2026_10_11.md)にあり、原本との照合結果を末尾の最終確認に記載する。
言語仕様や表現の変更は決定しない。後続実装の level は採用方式の影響範囲を確定してから判断する。

## 初期調査時の判定

| 候補 | コードから確認できること | 未確認のこと |
| --- | --- | --- |
| `Regex::find_all` | n byte の入力を m 個の match がそれぞれ String として複製・保持する | 実時間、allocator overhead、RSS、実利用での頻度 |
| HashMap の反復構築 | distinct n key なら既存 entry を合計 n(n−1)/2 回 clone する | payload ごとの実コピー量、allocator と hash の寄与 |
| `HashMap::map_values` | 開始時に全 entry を clone・sort し、各成功再開時に既存 output 全体を clone する | 開始・再開・callback・解放の時間比率、実時間上の停止時間 |

いずれも性能改善候補として成立する。今回の確認だけでは、直近変更によるデグレとは判定できない。
List の仮想計算量や callback の論理契約が破れたことを示すものでもない。

## Regex::find_all

### 根拠と成立条件

- [builtin.rs](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs) の `builtin_regex_find_all`（2965 行付近）は `find_iter(input)` の各結果で `input.clone()` を実行する。
- [runtime.rs](/Users/haruca/.codex/worktrees/4c18/surtr/crates/sindr/src/runtime.rs) の `RegexMatchHandle`（287 行付近）は `input: String` と `start` / `end` を保持する。`Value` と handle の `Clone` は derive されており、match 値の複製でも入力文字列が複製される。
- 同ファイルの `ListHandle::from_items` は Vec の所有権を Packed へ移す。結果の List を保持している間、各 match の入力コピーは残る。List handle 自体の clone は buffer を共有するため、match 値の clone と区別する。

入力長 n byte、結果数 m の場合、各 match が保持する文字列の内容だけで合計 n×m byte になる。
Vec、各 Value、allocator の管理情報、呼出し元に残る入力などは別に加わる。
例えば ASCII の `"a"` を n 個並べ、pattern を `"a"` にすれば m=n となる。
同じ入力に `"a+"` を使う場合は m=1 なので、この保持量は n byte になる。
入力が大きいだけでは二次増加せず、match 数も増えることが条件である。

Packed の短い suffix を残すと元の buffer 全体が残るため、RT-2 と組み合わせた保持も起きる。
一方、`head_value` 等で match 値を一つ取り出して元の List をすべて破棄した場合は、取り出した match の入力コピーだけが残る。
結果 List のコピー、要素の取り出し、`RegexMatch::text` が作る部分文字列を同じ費用として数えない。

### 最小測定案

1. pattern `"a"` と、ASCII 入力 n=1,024 / 2,048 / 4,096 byte を使い、結果を生存させて測る。理論上の入力コピー保持量はそれぞれ 1 / 4 / 16 MiB。これは実測値ではない。
2. 同じ n に対し pattern `"a+"`（1 match）と `"b"`（0 match）を対照にする。n 固定で match 数を変える系列も追加できる。
3. 生成前、結果保持中、結果破棄後の requested live bytes と生成・解放時間を分ける。結果数は `len` で確認し、全要素の表示や text 取得を測定区間に入れない。
4. UTF-8 入力と空文字に一致する pattern は意味論の確認用に分ける。ASCII 密一致の性能系列に混ぜない。

`compile_cached_regex`（builtin.rs の 4267 行付近）は名前に反して毎回 `Regex::new(pattern)` を呼ぶ。
builtin 全体の測定には正規表現コンパイルも入るため、その条件を記録する。
外部の一時 crate で handle を直接組み立てた測定はモデル測定であり、VM builtin の実測とは区別する。
VM の公開入口で builtin を実行する場合も、入力構築・bytecode 構築・結果保持場所の clone を別途確認する。

### 次の仕様入力に必要な受入条件

[標準定義](../../../../lib/types/regex.srt) と [標準テスト](../../../../lib/tests/basic_types/regex.srt) は、入力順の全 match、text、元入力に対する start / end の byte offset、0 match の空 List を契約としている。
修正時はこれらに加え、UTF-8 の境界、空 match、入力を破棄した後の match の有効性を保つ。
`find` と `find_all` の match を同じ API で読めること、内部の不正 span を RuntimeError とする境界も維持する。

候補には入力の共有と一致部分だけの保持があるが、ここでは採用しない。
入力共有は全 match の重複保持を減らせる一方、単一 match が大きな元入力を保持するケースは残る。
一致部分だけの保持は元入力 offset と内部 `PartialEq` の扱いも検討が必要になる。
`RegexCaptures` の `input: String` は [Eldr VM 仕様](../../../../docs/dev/EldrVM_spec.md) に明記されているため、今回の match 最適化に混ぜて変更しない。
受入仕様では改善対象の保持量、改善しないケース、clone 時の費用、正本の更新範囲を明記する。

## HashMap の反復構築と map_values

### 根拠と成立条件

[runtime.rs](/Users/haruca/.codex/worktrees/4c18/surtr/crates/sindr/src/runtime.rs) の `HashMapHandle::insert`（759 行付近）は、毎回 `self.entries.clone()` を行ってから一件を追加する。
`HashMapHandle::from_entries`、[builtin.rs](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs) の `builtin_map_from_entries`（2532 行付近）、JSON object 変換（4583 行付近）は、この insert を反復する。

distinct n key を一件ずつ追加すると、既存 entry の clone 回数は 0+1+…+(n−1)=n(n−1)/2 となる。
同じ少数 key の上書きだけなら map の件数は増えないため、この n² の説明はそのまま当てはまらない。
String key は複製され、Value の payload は型ごとに費用が異なる。String や入れ子の HashMap は深いコピーを伴い、List handle は共有される。
古い map は各代入後に破棄されるため、累積割当量が二次でも、結果保持中の live bytes が二次になるとは限らない。

[map_values.rs](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin/map_values.rs) は、開始時に `sorted_entries()` で全 key / Value を clone して sort する。
その後は callback ごとに continuation へ戻るが、成功した `resume` は `output.insert` により、それまでの output をすべて clone する。
したがって開始の準備と各再開の出力更新に、別々の未分割処理が残る。
callback の RuntimeError は `result?` で伝播する。失敗を部分 map の成功に置き換える問題は確認していない。

### 最小測定案

- distinct key 数 n を倍増させ、小さい Value と固定長 String payload を分ける。入力を区間外で作り、構築時間、累積 requested bytes、peak live bytes、最終 live bytes、解放時間を測る。
- 同一 key を繰り返す対照を置く。key 長、payload 長、出力件数を記録する。異なる hash seed の変動があるため複数回測る。
- `from_entries` の公開 Rust API、VM の `map_from_entries`、JSON parse は別系列にする。JSON では parser と再帰変換の時間も入る。
- `map_values` は開始、callback、各再開、完了、解放を区別する。callback 回数 n と決定的な key 順を別途確認する。公開 VM 入口だけで段階を観測できない場合は、全体値を段階別測定として報告しない。

### 次の仕様入力に必要な受入条件

完成前の構築物を内部だけで可変にする方式なら、immutable な公開 HashMap 契約を保つ改善候補になる。
ただし公開 `insert` の永続性、duplicate key の後勝ち、key 順を必要とする API の決定性、入力エラーの位置と伝播を保つ。
JSON の duplicate key と変換エラーの扱いも別途固定する。

`map_values` は全 key の保持、決定的な順序で callback を一度ずつ呼ぶこと、空 map で呼ばないこと、言語の `Err` 値を通常の値として保持すること、RuntimeError 後の未評価を維持する。
構築状態を可変にする場合は、通常切替えで所有権を移し、REPL checkpoint と live 状態が独立することを確認する。
rollback、取消、再開後の結果配送で callback や副作用を重複させない。
開始時の sort や出力更新を分割する採用案では、その範囲と一単位の処理量を仕様へ記載する。
単にコピー量が減っただけで、VM 全体の実時間上限や RT-4 全体の解決を宣言しない。

## RT-2 / RT-3 の測定上の注意

- global allocator で集計する requested bytes は、アプリケーションが要求した allocation layout の byte 数である。allocator の丸め、管理情報、保持 arena、未接触 page、stack、mmap 等をすべて表す値ではなく、RSS と同義ではない。
- 累積割当量、現在の live bytes、区間 peak live bytes を分ける。realloc は成功時の旧 size / 新 size と失敗時の扱いを定義する。累積量に新 size 全体を足す方式と増分だけ足す方式を混ぜない。
- 区間前から存在する入力を区間内で解放する場合、区間差分が負になることはある。カウンタを単純にゼロへ戻して後続 dealloc を引く方式は underflow や誤集計を招く。開始値との差分と全期間の live 管理を使う。
- RSS は観測時点と取得方式、最大 RSS は process 全期間の高水位かを明記する。ケースごとに新 process を使い、allocator の前ケース由来の保持と高水位を切り分ける。解放直後に RSS が下がらないことだけで生存オブジェクトの残存と断定しない。
- RT-2 は元 Packed、短い tail、要素を取り出したコピー、checkpoint の各所有者を区別する。元 handle を落として短い tail だけを残す区間と、最後の参照を落とす区間を分ける。
- RT-3 は B / E / callback 回数と物理費用を分け、小さい値、String、List、入れ子の payload を同一視しない。観測のための全走査・表示・clone が測定値を増やさないようにする。
- 同じ build profile、最適化条件、入力、payload、結果の保持期間で比較する。allocator 計測を有効にした時間は計測 overhead を含むため、可能なら時間だけの系列も分ける。
- 旧 Cons 方式との比較が必要でも、production の旧経路を復活させない。独立した一時 harness のモデル比較であることを明記し、現行 VM 全体の改善率へ読み替えない。
- 実時間や RSS の閾値を correctness test の合否へ混ぜない。今回の検討は既存の正しさのテストが未達であるという指摘ではない。

## 初期調査の検証範囲と次の一手

このレビューは対象実装、標準 API、関連正本、既存テストの読み取りに限定した。
テストの実行結果、RSS、割当量、実時間、scheduler 公平性の測定結果は含まない。
まず上記の小さい系列で現行基準値を取り、改善対象を選んで受入仕様を確定する。
内部表現・checkpoint・継続を変える実装は、その仕様入力と回帰条件に基づく別の作業とする。

## 追補: 手動測定ハーネスの読み取りレビュー

対象: [main.rs](/Users/haruca/.codex/worktrees/4c18/surtr/tests/profile/list_runtime/src/main.rs)、[Cargo.toml](/Users/haruca/.codex/worktrees/4c18/surtr/tests/profile/list_runtime/Cargo.toml)。
この追補も読み取りレビューであり、ハーネスのビルド・実行は行っていない。

allocator は LIVE を全期間保持し、区間前後の差分を取る。realloc は成功時だけ旧 size を引き、新 size を加えるため、区間内で既存入力を解放する場合も符号付き live 差分として表せる。
`requested_bytes` は成功した要求の size 全体を累積し、realloc の新 size も全体を加える定義である。
`freed_bytes` は realloc の旧 size を論理的な解放として含む。
`peak_extra_bytes` は区間開始時の live bytes を基準とした高水位の増分であり、System allocator 内部で realloc の旧 buffer と新 buffer が同時に存在する瞬間は測れない。
これらの値は単一実行を前提とした診断用の集計であり、物理メモリや未計測時間の推定値へ読み替えない。

RT-2 の root 構築時 `live_delta_bytes` を保持 storage の基準にする方法は妥当である。
長さ 1 の tail を残した後、root と最後の tail を別区間で破棄するため、共有参照の解放と buffer 全体の解放を分けられる。
kernel 比較は reverse Cons と forward Cons の構築モデルであり、旧 VM の計測ではない。
現行 VM の flat_map 系列は singleton、空結果、巨大 mapper 結果、三段の nested bind、多段の直列 bind を区別する。
三段系列の B=n+n²+n³、E=3n³、直列 depth 段の B=E=n×depth は bytecode の構造と整合する。
E は期待する Builder 追加数であり実測カウンタではない。callback の観測方法は下記の再確認結果に従う。
三段系列は同じ payload を増幅するもので、三要素 tuple を構築する完全な直積例の費用は含まない。

測定結果には以下の範囲を併記する必要がある。

- VM の `run` は入力 List の生成と mapper の実行を含み、bytecode 構築と `VM::new` は含まない。observer と counting allocator の費用は含む。
- `drop_vm` は結果だけでなく、bytecode・定数・observer 等を含む VM 全体の解放である。
- 小さい値の Int は BigInt であり、無割当の scalar と同義ではない。
- Regex 系列には `Regex::compile` と `find_all` 内の再コンパイルの両方が含まれる。
- `ps` の RSS は結果保持時等の一点での process 全体値であり、allocator 区間の peak ではない。出力用 JSON や RSS 取得用処理の影響も含む。

初回レビューでは、測定後の検証が RSS を変える問題を指摘した。
Regex の `matches.iter()` は各 match の String を再度 clone し、kernel の `assert_eq!(output, input)` も両 List の要素を clone する。
時間・requested bytes の区間外でも、その後の RSS と process 最大 RSS、および解放前の allocator 状態へ影響する。
全要素検証は別の非測定実行へ分け、測定実行では `len` 等の複製しない検証を使うことを推奨する。
修正後のコードでは `PROBE_VERIFY` が設定された検証実行だけで全要素を検査し、測定実行では長さと期待保持量を使うことを確認した。
`PROBE_VERIFY` を設定した実行の数値を測定結果に混ぜない。
`count-alloc` feature により、allocator 計測を外した時間測定も可能になった。計測無効時の allocation 各値は実測のゼロではなく未測定として扱う。

親エージェントの smoke 実行で、callback が完了しても `function_calls` が 0 となり、ハーネスの assertion が失敗した。
原因は [continuation.rs](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/vm/continuation.rs) の `callable_invocation` が、関数 callback 用 frame を直接作り、`observe_call_event` を呼ばないことである。
`closure_calls` に変更してもこの入口は計測されない。
このハーネスでは各 callback が `Return` を一度実行し、他の user function を呼ばないため、成功した実行の `return_count` または `per_opcode["Return"]` を callback 完了数の代理として B と照合できる。
これは当該 bytecode に限定した代理指標であり、一般的な callback 回数の観測契約ではない。
今回の assertion 失敗はハーネスの観測指標選択の誤りとして扱い、製品の機能デグレとは判定しない。
修正後のハーネスが `per_opcode["Return"]` を使い、この入口の条件をコメントで説明していることを読み取りで確認した。

## 追補: FileOutHandler の資源寿命

[Process Runtime 仕様](../../../../docs/dev/ProcessRuntime_spec.md) の `FileOutHandler`（1076 行付近）は、handler init 時に open し、handler lifecycle 中は開いたまま保持し、shutdown で flush / close すると定めている。
現行 [vm.rs](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/vm.rs) の `out_handler_write`（2174 行付近）は、write ごとに局所変数の `File` を `OpenOptions::create(true).append(true).open(path)` で作り、`write_all` 後に破棄する。
`process_context_handler` は metadata から handler PID を作るだけで、この file を open しない。
write 内の file は `open_files` に登録されず、VM の通常 file resource の shutdown 経路では扱われない。

したがって正本との相違は、write ごとの open 費用だけでなく、open 失敗の発生時点と資源寿命にもある。
局所 `File` の drop により close は行われるが、正本に定めた handler shutdown 時の flush / close を実装しているとはいえない。
通常の `File` を使っているため、この相違から直ちに「write_all 成功時の内容が失われる」とは判断しない。
既存テスト `out_handler_write_appends_to_file_target` と `out_handler_write_reports_invalid_file_and_unknown_targets_as_result_errors` は append と write 時の open failure を確認しており、init・保持・shutdown の契約を固定していない。

最小の確認案は、存在しない親 directory の path を handler に設定して write 前の init の結果を確認するケースと、二回の write の間に path を rename / replace するケースである。
後者は OS の open file の振る舞いに依存するため、対象 OS とファイル操作の成功を記録する。
保持した同じ file へ書くのか、毎回 path を開き直して別 file へ書くのかを比較できる。
これらの再現は今回実行していない。

対応時は init の成功・失敗の入口、file sink の所有者、取消・shutdown の後処理を仕様入力として確定する。
正本にある canonical path ごとの sink 正規化と mailbox 順序も受入範囲に含める必要がある。
現行 `handler_target_identity` は named args の文字列から identity を作っているため、同一 file を指す異表記の統合をこの関数だけでは保証できない。
資源寿命とエラー時点の相違を性能改善へ混ぜて暗黙に変えず、RT-4 の一処理分割とは区別して管理する。

## 最終確認: 測定原本と報告

[測定報告](list_runtime_measurement_report_2026_10_11.md)、harness / runner / README、
RT-2〜5 の進捗、OI-037 / OI-038 を最終差分として確認した。
製品コードの変更はなく、改善方式は未確定、RT-2〜5 は未解決のまま記録されている。

- counted / time-only の原本は各 460 区間行、92 集計。各条件・区間の sample は 0〜4 の5件である。
- 全集計の最小・中央値・最大を raw JSONL から再計算し、一致を確認した。
- 保存された binary / harness source / lock の SHA-256 を現存ファイルと照合し、一致を確認した。
- 報告の全26表行について、時間系列の ns と process peak RSS、割当系列の requested / live / freed、期待操作数と callback 完了数を照合した。単位変換と小数第3位への丸めも一致した。
- counted 原本の全行で `requested_bytes - freed_bytes == live_delta_bytes` が成立した。time-only の割当欄は未測定として区別されている。
- nested 系列の B=k / E=kn と、直列 chain の B=E=kn は bytecode と整合する。`Return` の回数を callback 完了数とみなす条件も記載されている。
- 検証専用 process を性能測定から分離し、runner は `PROBE_VERIFY` が環境にあれば拒否する。旧方式は構築モデルに限定し、production 経路を復活させていない。

報告本文には、workload 自体とその事前準備の区別、observer が RT-3 系列だけで有効であること、
RSS 表値が time-only 系列であること、callback 単独の費用は分離していないことを明確にするよう指摘した。
この指摘は説明の範囲に関するもので、測定値の再取得を要求するものではない。
最終本文で4点の修正を確認した。レビューで挙げた測定・説明上の指摘は解消しており、未解決の追加指摘はない。

ビルドと測定の終了コード、10件の内容検証、既存 nextest の42 passed / 401 skipped / 失敗0は親エージェントの実行報告に基づく。
独立レビューではこれらのコマンドを再実行していない。workspace 全体 gate、標準 Surtr 全件、
実際の generic do と REPL / Task / process 全入口の性能、RT-5 の性能、HashMap の性能、FileOutHandler の最小再現は未検証である。
この範囲を越える性能保証、正しさの全体検証、最適化完了を表す結論は採用しない。
