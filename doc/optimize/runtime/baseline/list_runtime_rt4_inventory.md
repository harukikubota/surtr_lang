> 取り込み元: `/Users/haruca/.codex/worktrees/4c18/surtr`（detached、HEAD `ee13e448`）。資料の集約。製品変更を伴わない基準測定。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](../README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# RT-4: 未分割 builtin と外部呼出しの棚卸し

調査日: 2026-10-11。状態: 現行コードの読み取り調査。製品コード・テストは変更していない。

入力は [List / VM の残課題](../list_runtime_followups.md) の RT-4。
処理量と中断できる境界を整理し、次に仕様を確定する対象を選ぶための資料とする。
continuation、ライブラリの分割 API、非同期 I/O、worker の採用方式は未確定。
実装へ進む場合は共通実行・取消契約に触れるため level4 として扱う。

## 現行の実行境界

- [Eldr VM 仕様](../../../../docs/dev/EldrVM_spec.md) の第3.8・3.9節は opcode と builtin の状態遷移を共通予算で実行し、予算切れと Future 待機を区別する。入力サイズに比例する既存 Rust loop、regex / JSON / OS I/O の分割は別課題として明示されている。
- [`run_quantum`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/vm.rs#L4361) は `step_context` が戻った後に reduction を消費する。同期 builtin の内部で長い loop や OS 呼出しが続いても、その途中では予算切れを確認できない。
- [`callable_invocation`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/vm/continuation.rs#L187) は builtin を同期 Rust 関数として呼び、その戻り値を次の invocation として保存する。`Complete` / `Call` / `Resume` / `Wait` は区別されるが、builtin 関数の内部へ自動で割り込む仕組みではない。
- [`BuiltinContinuation`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L56) にある FlatMap、MapValues、Generator、Runtime、FileWithOpen は既存の再開状態。`FileWithOpen` は body callback の成功・失敗後に flush と close を行う。open / flush / close 自体は同期呼出しである。
- [`cancel_execution_context`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/vm/continuation.rs#L68) は開始直後に保持した continuation も cleanup 経路へつなぐ。同期 builtin が Rust 内で処理している間に、この取消経路を実行することはできない。
- [Process Runtime 仕様](../../../../docs/dev/ProcessRuntime_spec.md) の timeout 契約では、Task 自身の取消と call timeout を区別する。call timeout は開始済み callee を取り消さず、Task の取消は cleanup 後に結果を配送する。RT-4 の移行でこの違いを変えない。

以下の「未分割」は、一回の同期呼出し内の入力処理が共通予算に分割参加していないことを指す。
dispatch 自体が予算付き driver に接続されていることとは両立する。
処理量はコード構造からの評価であり、実時間・RSS の測定値ではない。

## 純粋な処理

`N` は要素数、`B` は入力 bytes、`M` は出力数。値の clone / equality の内部処理量は要素数とは別に数える必要がある。
各行の分割候補は検討案であり、採用済み仕様ではない。

| 対象と根拠 | 一回の同期処理量・予算参加 | 取消・後処理の境界 | 分割候補と維持する条件 |
|---|---|---|---|
| [`List::zip`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L2511) | 短い方の全要素を走査し Tuple と出力 Vec を作る。未分割 | 外部資源なし。取消時は部分出力を破棄。値の解放量は別評価 | 二つの cursor と Builder を continuation が所有し、一組ずつ進める。短い側で終了し論理順を保つ |
| [`List::group_count`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L2489) | 各入力について既存 groups を線形探索。全て異なる値では比較回数が最悪 O(N²)。出力変換も一括。未分割 | 外部資源なし。値の比較そのものが大きい入力を含み得る | 入力 cursor と比較中の group index、部分 groups を保存する。初出順と equality の意味を保つ。入力一件だけを分割単位にしても比較量は有界にならない |
| [`Facet` List get / set / slice get / slice set](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L2651) | get は指定位置まで、slice get は開始位置の skip と選択範囲を走査。set は全 List を Vec 化し、slice set は splice する。未分割 | 外部資源なし。元 List の永続性を保ち、部分変更を公開しない | cursor と出力 Builder、位置、置換 cursor を保存。範囲検査・負 index・両端を含む範囲の契約を維持 |
| [`HashMap::from_entries`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L2532) | 全入力を検査・追加。[`HashMapHandle::insert`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/sindr/src/runtime.rs#L755) は既存 map 全体を clone するため、異なるキーの逐次構築で最悪 O(N²) 個の既存 entry をコピー。未分割 | 外部資源なし。途中の内部異常は RuntimeError。重複キーの既存更新規則を保つ | 入力検査と追加を分割するだけでは一追加の全 map clone が残る。構築用可変 map と最終 freeze の可否を別途定義 |
| [`HashMap` insert / remove / keys / values_list、Facet map set](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L2586) | 更新は map 全 clone、keys / values は [`sorted_entries`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/sindr/src/runtime.rs#L784) の全 clone と sort。未分割。get も返す値の clone は payload サイズに依存 | 外部資源なし。永続性と決定的なキー順を維持 | map 表現・構築・sort の契約を先に整理。要素ごとの continuation だけでは sort、値 clone の一回の量を制限できない |
| [`HashMap::map_values`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin/map_values.rs#L15) | callback は共通予算で再開する。開始時に全 entry の clone + sort、各 resume は output 全 clone を伴う insert。これらは未分割 | callback RuntimeError を伝播し、部分 map を成功にしない。取消後に次の callback を呼ばない | callback の分割済み範囲と前処理・出力構築の未分割範囲を分ける。出力構築方式の決定は from_entries と合わせる |
| [`String::len / contains / starts_with / ends_with / split / replace`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L2069) | len は全 chars、検索は入力文字列、split / replace は全結果を処理。`expect_string_pair` は両引数を先に clone するため starts/ends の比較が短くても入力全体コピーがある。未分割 | 外部資源なし。UTF-8 と空 separator / 空 from の既存規則を維持 | byte/char offset、検索状態、出力を保存。検索境界をまたぐ needle と UTF-8 を正しく扱う。引数 clone は別途評価 |
| [`codepoints / from_codepoints`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L2214) | 全 bytes / 全 List の走査、値変換、UTF-8 検証。未分割 | 外部資源なし。最初の不正位置で既存の言語 Error を生成し、後続を評価しない | 入力位置、部分 bytes / Builder、encoding を保存。文字 index と byte index を混同しない。最後の UTF-8 一括検証も分割範囲に含めるか確定する |
| [`String` eq / neq / concat](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L1945)、[`uncons` の String 分岐](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L4802) | 比較は内容長、concat は両文字列、String uncons は残り全 chars を新 String に収集。未分割。List uncons は handle 操作なので同じ走査ではない | 外部資源なし。String の tail のコピーと List の tail の共有を区別 | 文字列所有・共有表現を変えるか、コピーを continuation に分けるかは別途判断 |
| [`inspect / to_string`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L1086)、[`render_value`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L3767)、Error format / chain | 入れ子を再帰走査し、文字列化・join。HashMap の表示は clone + sort。Int 表示は桁数に依存。Error の chain / trace も件数・message 長に依存。未分割 | 外部資源なし。ただし print / eprint は後続に出力副作用を持つ | 明示的な表示 stack と出力 buffer を保持。未知 tag 等の RuntimeError と canonical 表示を保つ。結果が確定する前の部分表示を外部へ出すかは未決定 |
| [`Int` 演算・比較・bit 操作・shift](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L1616) | BigInt の桁数、指定 bit 位置、結果の桁数に依存。Rust 数値ライブラリ内の一回の演算は未分割 | 外部資源なし。Int を固定幅へ狭めない | 通常の loop と同じ分割方式を前提にしない。多倍長演算の分割 API または実行先の検討が必要 |
| [`Random` の整数範囲生成](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L3993) | 範囲の bit 数だけ bytes を生成し、棄却時は再試行。試行数に固定上限がない。未分割 | 外部資源なし。generator 状態、host seed、返す次状態を保持し、再開で同じ乱数を生成し直さない | bytes の生成 offset と棄却 loop の状態を保存。一試行完了時だけ yield しても巨大範囲の処理量は残る |
| [`project_args`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L3062)、テスト用 capture / stdin 等 | 全引数や capture buffer / 注入文字列のコピー量に依存。未分割 | 外部資源なし。テスト境界と製品の入力境界を区別 | 主処理の性能評価で補助コピーを分けて記録 |

## regex と JSON

| 対象と根拠 | 一回の同期処理量・予算参加 | 取消・後処理の境界 | 分割候補と制限 |
|---|---|---|---|
| [`regex compile / matches / captures / find`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L2818) | `Regex::new`、`is_match`、`captures`、`find` を同期呼出し。使用時の `compile_cached_regex` も現行コードでは `Regex::new` を呼ぶ。pattern / input / group 数に依存し未分割 | 外部資源なし。Rust 呼出し完了前に VM 取消を観測しない | worker 移管、探索状態を保存できる API 等を比較。単に一回の find 後に yield しても一探索の時間上限は得られない |
| [`regex find_all / split / replace / replace_all`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L2965) | 全 match / split / replacement を一括収集。find_all は match ごとに input 全 String を clone する。input B bytes、M matches なら検索以外に B×M bytes のコピー・保持が発生する構造。未分割 | 外部資源なし。部分結果を公開しない | match cursor と出力を再開状態にする候補。ただし一探索の時間と input 全 clone は残る。共有所有表現の変更は別に評価 |
| [`regex escape / group_names / whole / get / get_name / text`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L2876) | escape は入力全体、group_names は compile と全 group、抽出は substring のコピー。capture_count / start / end はこの走査を持たない | 外部資源なし。capture の存在・未一致エラー、span の整合を維持 | 分割対象の走査・コピーと固定量 getter を分ける |
| [`json_parse`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L4600)、[`json_value_to_surtr`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L4523) | serde_json で RawValue を検証後、各 container の RawValue を再 decode し、子を再帰変換。object は HashMap の逐次 insert。階層深度・token 長・entry 数に依存し未分割 | 外部資源なし。深度127、巨大 Int、位置付き parse Error の既存規則を保つ。内部不整合は RuntimeError | parse と Surtr 変換のどちらを分割するか確定する。変換だけ分割しても初回検証・container decode・map clone の量は残る |
| [`json_stringify`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L4760)、[`surtr_json_to_serde`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L4667) | 入れ子全体を serde_json::Value に変換し、最後に to_string。object は sorted_entries。未分割 | 外部資源なし。Int 範囲拒否、finite-only Float、内部 arity 検査を保つ | 変換 stack、serializer、出力 buffer を保存する案。最終一括 stringify を残す場合は分割した範囲を明記 |

## OS I/O と副作用

入力処理量が小さくても、OS や子プロセスが戻るまでの時間は別の変数となる。
CPU の進捗保存だけでは、この待機中に runnable task を実行できるとは限らない。

| 対象と根拠 | 一回の同期処理量・予算参加 | 取消・後処理の境界 | 移行前に決める条件 |
|---|---|---|---|
| [`print / eprint`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L1077)、[`emit_stdout/stderr`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/vm.rs#L1693) | 表示生成、output event / capture の文字列保存、passthrough / tee の host 出力。入力 bytes と I/O の待ち時間に依存。未分割 | 出力済み bytes は rollback しない。capture / event / host を再開で二重出力しない | 表示生成と出力 commit を分け、順序・newline・tee・失敗の扱いを定義 |
| [`__out_handler_write`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/vm.rs#L2174) | StdOut/StdErr の同期出力、FileOutHandler の open + write_all。本文 bytes と I/O 待ちに依存 | File は局所所有で呼出し終了時に drop。既存 event と結果の配送を重複させない | Future 待機に移す場合は本文・書込 offset・資源所有者・終了済み状態を保存。取消済み caller と開始済み callee の契約を維持 |
| [`io_get / io_get_line`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L3072) | prompt 出力後に端末イベント / stdin を同期 read。get は最大4 bytes の復号でも入力待ちは無制限。get_line は一行全体 | 読取済み入力と prompt は巻き戻さない。端末 raw mode は [`read_terminal_char`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L3678) で復元。中断時も復元する所有権が必要 | 入力待ちを Future 待機にするか worker に移すかを決定。raw mode、stdin lock、入力の一度限りの消費を定義 |
| [`file_read / file_write / file_append`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L3120) | read/write はファイル全体、append は open + write_all + close。UTF-8 検証を含め未分割 | write / append の済んだ副作用は巻き戻さない。append は write の成否にかかわらず close を呼ぶ | byte offset、部分読取 / 書込、UTF-8 の跨ぎ、open handle、エラーと close の優先順位を保存 |
| [`file_with_open / read_chunk / write_chunk / flush`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L3190) | body callback は予算参加済み。open / flush / close は同期。read_chunk は [`read_utf8_chunk`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/vm.rs#L6586) で利用者指定 max_chars 全体を loop、write_chunk は write_all。chunk 名でも scheduler の有界単位ではない | handle を FileWithOpen continuation が所有し、callback のエラー・取消でも cleanup。read/write の進捗は既に host file へ作用 | callback の既存所有権を保ち、read/write の途中状態を追加するか確定。flush/close 待機まで扱わなければ実時間上限は得られない |
| [`filesystem_ls / tree_depth`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L3315)、[`filesystem_snapshot / collect_filesystem_entries`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L4958) | 再帰 read_dir、children sort、全 path sort、各 entry の metadata を一括処理。entry 数・深度・path 長・OS 待ちに依存 | ReadDir 等は局所所有で drop。失敗時に部分 snapshot を成功にしない。外部 FS は途中で変化し得る | 探索 stack、dir iterator、並べ替え、metadata のどこを分割するか定義。既存の並び順、symlink、depth の扱いを維持 |
| [`filesystem_cp / mkdir_all`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L3354) | copy はファイル全体、mkdir_all は path 階層、各 OS 呼出しの待ち時間に依存。未分割 | 作成済み directory / 書込済み destination を自動 rollback しない。既存の directory copy 拒否を保つ | 分割 copy の資源所有・offset・部分 destination の扱い、非同期 OS 待機を定義 |
| [`file_exists / delete`、filesystem exists / stat / mkdir / rm / mv、`shell_cd`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L3176) | 一件の path を処理しても metadata / open / remove / rename / canonicalize が同期で待つ。固定サイズ入力でも停止時間は有界とはいえない | 実行済み副作用・cwd 更新は二重実行しない | 一件の OS 操作を分割 loop とみなさず、待機と結果 commit の契約を定義 |
| [`shell_exec`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin.rs#L3499) | argv 全体を decode、`Command::output()` が子終了と stdout/stderr の全収集を待つ。その後 UTF-8 検証。子の実行時間・出力 bytes に依存 | 現行は同期 call の内部で子を待ち、VM が取消できる Child を continuation に保持しない | 子の所有者、kill 方針、wait/reap、pipe の並行 drain、終了結果の一度限りの配送を確定。VM の Value/Rc をそのまま別スレッドへ渡す前提を置かない |

## 先に検証する候補

1. **外部待機を伴わない線形処理**: `zip`、codepoints、from_codepoints から候補を絞る。cursor と Builder の所有権を既存 continuation に接続する案を比較し、予算1・取消・再開で論理順とエラー位置を確認する。方式はまだ確定していない。
2. **分割前に構築コストを測る処理**: HashMap の全 clone、group_count の比較回数、regex find_all の入力全コピーを小 payload / 大 payload で分けて測る。yield の追加だけでコピー量の改善を報告しない。
3. **ライブラリ内処理**: regex / JSON / BigInt は一呼出しの停止時間と変換・収集の loop を分ける。ライブラリ API の現行能力を確認してから方式を決める。
4. **OS 待機**: stdin、Shell、File の順序・所有権・取消・部分副作用の条件を定義し、Future 待機と CPU yield のテストを分ける。

コードから得た新規の性能候補は親エージェントへ伝え、Astra への相談対象とした。
デグレの発生や実時間の悪化をこの調査で確認したものではない。

FileOutHandler の資源所有には正本との差異候補もある。
[Process Runtime 仕様](../../../../docs/dev/ProcessRuntime_spec.md) の第3.19節は init 時に open して lifecycle 中保持し、shutdown で flush / close すると定義する。
現行の `out_handler_write` は write ごとに局所 File を open して終了時に drop する。
親エージェントから Astra へ相談し、[独立レビュー](list_runtime_astra_review.md)で正本との相違を確認した。最小再現はまだ実行しておらず、新規デグレとは断定しない。
本資料の OS I/O 表は現行コードの所有形態を記載しており、正本をこのコードへ合わせる決定は行っていない。

## 受け入れ条件と検証案

- 移行対象ごとに、予算0で進まないこと、予算1等の小さい予算で完了前に他の runnable task へ切り替わることを確認する。結果・callback・副作用・結果配送の各回数を数える。
- 再開境界と Future 待機境界で取消し、所有資源を一度だけ cleanup する。取消後に利用者 callback の後続を再開しない。
- 純粋処理は logical order、永続性、最初の不正入力、空入力、深い値と大 payload を確認する。sort / clone / equality を未分割のまま残す場合はその範囲を記録する。
- I/O は短い read/write、途中エラー、UTF-8 の境界、EOF、失敗時 cleanup、子の reap、stdin raw mode 復元を直接検証する。済んだ外部 I/O を rollback 成功として報告しない。
- batch / REPL / Task / process の通常入口で切替えと配送を確認する。REPL は checkpoint より前の callback・副作用を再実行しない。Task timeout と call timeout の既存契約をそれぞれ確認する。
- 対象を決めた後、最も直接的な Eldr テストを先に追加して TDD を行う。候補は [`list_flat_map_tests`](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/vm/list_flat_map_tests.rs)、file continuation cancellation tests、各 builtin の crate-local tests。必要な通常入口を Rune / Xldr テストへ配置する。
- 実装時の level4 全体 gate は `rtk cargo nextest run --profile ci --workspace` と `cargo run -- test --quiet --all`。最終差分は独立レビューへ渡す。新規問題や予期しない失敗は原因を固定してから Astra に相談する。

本調査ではビルド、テスト、性能測定を実行していない。
lib の List / String の builtin 宣言、EldrVM / ProcessRuntime の正本、builtin・continuation・VM・runtime の現行ソースを確認した。
全 builtin の定数項、compiler 側処理、allocator・Value clone/drop、host OS の時間上限は未検証。
RT-4 は棚卸し段階であり、解決済みとは扱わない。
