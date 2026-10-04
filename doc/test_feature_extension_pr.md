# PR: Test の停止・未実装項目、名前フィルター、アサーションと CLI 拡張

## 1. 状態・目的・範囲

- 状態: 未実装の設計提案。ユーザーが採用した相談内容を実装入力として整理する。
- 作成日: 2026-10-04。
- 調査基準: `aa93b14106e5215e648e6cdff7c63261d602954b`。
- level: 4。Test の実行契約と、コンパイラ管理の `ErrorKind` 引数を受ける標準 API を拡張する。

テストの一時停止と未実装項目を区別し、名前で実行対象を選べるようにする。Result・Option・エラー種別などを直接検査するアサーションを追加し、一覧、実行時間、JSON 出力を提供する。新しい言語構文や暗黙変換は追加しない。

本 PR の対象は `xit`、`pend`、CLI フィルターと組み合わせ規則、追加アサーション、`--list`、`--timings`、`--format json`。以下は対象外とする。

- **失敗位置の正確な表示。現在対応中の別改修に任せ、本 PR には含めない。既存の位置検索処理の置換・削除も行わない。**
- 文末 `?` と未使用値の警告。別提案 [test_assertion_statement_question_pr.md](test_assertion_statement_question_pr.md) の範囲。
- before/after フック、再試行、並列実行、スナップショット、既知の失敗を許容する expected-failure 機能。相談時に後続検討とした機能であり、今回採用した追加機能には含めない。

本書の追加 API・CLI の例は実装後の目標。今回、製品コード、実行可能テスト、正本文書は変更しない。

## 2. 現状と実装の境界

- [lib/test.srt](../lib/test.srt): `test`・`describe` はスコープを積んで本文をその場で実行する。`it` は本文の `Result<()>` から一件の成否を記録する。
- [Eldr VM](../crates/eldr/src/vm.rs): test event は Passed / Failed。スコープは名前だけを保持し、push 時の kind は保持していない。
- [Rune test command](../crates/rune/src/commands/test.rs): ファイル一件または `--all` と `--quiet` を受理する。フィルター、一覧、停止・未実装、時間、JSON は未対応。
- 現行アサーションには真偽、Eq、Ok の内容、Err の表示文字列、StyledDoc、stdout/stderr がある。`assert_eq` の成否は Eq で決め、inspect は説明にのみ使う。
- `ErrorKind` は通常の値型ではなく、標準 builtin の直接引数に限定されたコンパイラ管理の型。`Result::recover_kind` は canonical deferror 宣言の identity を使う。

選択規則は一つの runner policy にまとめる。CLI は policy を構築し、VM はケース開始時に判定する。標準 Test 関数は本文の実行と Result の処理を担当する。表示処理から実行対象を逆算しない。

## 3. Test API とケースの状態

```surtr
def xit(name: String, reason: String, body: (-> Result<()>)) -> Result<()>
def pend(name: String, reason: String) -> Result<()>
```

| 宣言 | 通常実行 | `--include-xit` 付き実行 | 一覧 |
|---|---|---|---|
| `it(name) { body }` | 本文を一度実行 | 同左 | 本文を実行しない |
| `xit(name, reason) { body }` | Skipped を記録、本文は実行しない | 通常の it として一度実行 | 停止宣言と有効化後の実行予定を表示 |
| `pend(name, reason)` | Pending を記録 | 同左。本体は存在しない | 未実装項目として表示 |

フィルター判定は上表より先に行う。不一致なら Filtered とし、本文は実行しない。`--include-xit` はフィルターを迂回しない。

- `xit` は「本文が存在する一時停止」、`pend` は「本文が存在しない未実装項目」。pend を呼び出して現在の it を中断する API にはしない。
- `reason` は必須。空文字列・空白だけの理由は Test の宣言エラーとして扱い、成功・Skip・Pending に変換しない。宣言の引数検証はフィルターより前に行う。理由の不正は VM の実行異常としてファイルを失敗させ、無視された Result によって宣言エラーが消える経路を作らない。名前の空文字列は既存 it と同じ扱いを維持する。
- xit 本文を含め、対象ファイル全体を通常どおり構文解析・名前解決・型検査する。一覧や停止でコンパイルエラーを隠さない。
- `xit`・`pend` の正常な宣言処理は `Ok(())` を返す。通常の it と同様、記録済みのケース失敗で次のケースを止めない。
- `--include-xit` で実行した xit が成功してもエラーにはしない。expected-failure の意味を加えない。宣言種別と停止理由は結果に保持する。
- Test のケース宣言を実行中のケース本文に入れることは拒否する。it / xit / pend は test・describe の走査中、またはトップレベルで宣言する。これはケース間の IO 分離と一件の成否を守る境界である。
- トップレベルのケースは従来どおり受理する。test / describe がなくても動くが、それらのフィルターを指定すれば一致しない。

```surtr
import Test;

test("Parser") {
  xit("深いネストを処理する", "ネスト処理を修正中") {
    assert_eq(3, 1 + 2)
  }
  pend("不正な入力位置を報告する", "診断機能の実装待ち")
}
```

## 4. CLI 全体と引数解析

```text
surtr test (<lib-relative-name> | --all)
  [--test <text>] [--describe <text>] [--it <text>]
  [--include-xit] [--deny-pending]
  [--list] [--timings] [--quiet|-q] [--format human|json]
```

| 軸 | 選択・既定値 | 責務 |
|---|---|---|
| ファイル対象 | 名前一件 または `--all`。必須・排他 | コンパイル・走査するファイル |
| 動作 | 実行 / `--list`。既定は実行 | ケース本文を実行するか |
| 名前選択 | `--test` / `--describe` / `--it`。既定は制限なし | ケースの選択 |
| 一時停止 | `--include-xit`。既定は停止 | 選択された xit の実行可否 |
| 未実装の扱い | `--deny-pending`。既定は許容 | 選択された pend による終了コード |
| 詳細表示 | `--quiet` / `-q`。既定は通常 | 正常結果の詳細・サマリーを抑制 |
| 時間 | `--timings`。既定は計測しない | ケース本文とコマンドの実測時間 |
| 出力 | `--format human|json`。既定は human | 表現形式 |

引数順序は任意。値付き引数は `--it TEXT` と `--it=TEXT` の両方を受理し、format も同じ規則にする。空文字列・空白だけのフィルターは拒否する。それ以外のフィルターは前後の空白も含め、入力文字列をそのまま比較する。名前の既存のパス正規化・lib/tests 内への制限を保持する。

すべてのフラグは一回だけ指定できる。`--quiet -q`、`--format human --format json`、同種フィルターの重複は、同じ値でも usage error。未知の引数、欠落した値、不正な format、対象の欠落・複数指定も usage error。値位置に既知の別オプションが現れた場合は欠落として扱う。`-` から始まるフィルターは `--it=-name` のように等号形式で指定する。

`--` 以降は位置引数として扱い、フラグを解釈しない。例えば `surtr test -- --all` は `--all` というファイル名を選ぶ。位置引数は一件だけで、`--all` とファイル名の併用は常に拒否する。

## 5. 名前フィルターと走査

| 引数 | 比較対象 |
|---|---|
| `--test TEXT` | ケースが所属する test スコープの名前のいずれか |
| `--describe TEXT` | ケースが所属する describe スコープの名前のいずれか |
| `--it TEXT` | ケース自身の名前。it / xit / pend に共通 |

大文字小文字を区別するリテラルの部分一致とする。正規表現・glob・カンマ区切り OR は追加しない。指定されたフィルター種類間は AND、同種の祖先スコープ間は「いずれか一致」。祖先がなければその種類のフィルターは不一致とする。表示用の ` > ` 結合文字列では比較せず、種類付きのスコープ情報で比較する。

| 指定集合 | 選択条件 |
|---|---|
| なし | 全ケース |
| test | T |
| describe | D |
| it | I |
| test + describe | T AND D |
| test + it | T AND I |
| describe + it | D AND I |
| test + describe + it | T AND D AND I |

test / describe は階層を知るため本文を走査する。名前が不一致という理由でスコープ本文を丸ごと省略しない。ネストした同種スコープが一致する可能性を失わないためである。

**フィルターも一覧も、ファイルのトップレベルと test / describe 内に直接書いた処理は実行する。** `--list` を副作用のない静的検査として説明しない。ケース本文に入れた副作用だけが抑止される。スコープの Err、走査中の実行異常、コンパイル失敗は選択結果で隠さず報告する。

`--all` では走査対象は現行と同じファイル集合・ソート順。補助定義等の除外規則を保持する。フィルター不一致のファイルもコンパイル・走査する。明示的フィルターがあり、全対象を通じた選択件数がゼロなら失敗とする。ファイルごとにはゼロ件をエラーにしない。

同名ケースは同じフィルターで複数選択される。結果の識別はファイルと、そのファイル内の宣言遭遇順の case index、種類付きのスコープ列、ケース名を使う。同名だから統合・上書きしない。

## 6. CLI の組み合わせ契約

第4節の軸は独立して組み合わせる。次表を合成して意味を決め、特定の引数の順番や早期 return で他の引数を落とさない。

| 組み合わせ | 振る舞い |
|---|---|
| 名前 / all × フィルター8集合 | ファイル選択後、全体で同じ名前選択規則を適用 |
| 実行 × include-xit | 選択された xit を it と同じ経路で実行 |
| list × include-xit | 本文は実行しない。選択された xit に実行予定を表示 |
| 実行 / list × deny-pending | 選択された pend が一件でもあれば失敗 |
| include-xit × deny-pending | xit の扱いと pend の判定を独立に適用 |
| フィルター × deny-pending | Filtered の pend はポリシー違反に数えない |
| 実行 × quiet | Passed / Skipped / 許容 Pending / Filtered の詳細を省略。成功時のサマリーも省略 |
| list × quiet | 選択された全項目は表示し、Filtered の詳細とサマリーを省略 |
| quiet × deny-pending | 拒否された Pending とその理由を表示し、失敗サマリーも表示 |
| 実行 × timings | 本文を実行したケースの時間とコマンド全体の時間を計測 |
| list × timings | ケース本文の時間は存在しない。コマンド全体の時間だけ計測 |
| quiet × timings | 実行時は表示が残る失敗ケース・失敗サマリーに時間を付与。一覧は項目を維持 |
| human × その他 | 上記の詳細抑制を反映し、既存の色設定を適用 |
| json × その他 | 第9節の一つの JSON 文書に出力。色・human 行は混在させない |
| json × quiet | 正常な詳細を同じ規則で省略。サマリーと終了理由は必ず保持 |
| list × json × quiet | 選択された全項目を保持。Filtered の詳細だけを省略 |

名前 / all、実行 / list、quiet、include-xit、deny-pending、timings、human / json の7つの二択と、フィルター8集合の **1,024 通りをすべて受理**する。値の正当性、重複、対象の排他は別の構文検証とする。意味のないフラグとして拒否する組み合わせは設けない。

```sh
surtr test --all --test String --describe strip_prefix --it missing
surtr test string --include-xit --deny-pending --timings
surtr test --list --all --it=missing --include-xit --deny-pending --quiet --timings --format=json
surtr test --format json --quiet string --test String --describe=strip_prefix --it=missing --timings --include-xit --deny-pending
```

最後の二例を含め、オプションの全効果を合成する。一覧は include-xit 付きでも本文を実行せず、deny-pending は一覧でも有効。

## 7. 件数・終了コード・異常

ケース件数と、スコープ・ファイルの異常を分ける。

- `discovered`: 遭遇した it / xit / pend の数。
- `selected`: 名前フィルター一致数。`discovered = selected + filtered`。
- 実行時: `selected = passed + failed + skipped + pending`。
- 一覧時: `selected = runnable + skipped + pending`。runnable は実行予定であり成功数ではない。
- `executed = passed + failed`。pending を failed に二重計上しない。
- `scope_failures`: test / describe の本文が Err を返した件数。ケースの failed に加えない。
- `script_errors`: 読み取り・コンパイル・VM 実行異常など、ファイル処理の異常件数。ケース数を捏造して加算しない。
- `policy_errors`: deny-pending 違反は選択 Pending 一件ごとに一件、明示フィルターの一致ゼロはコマンド全体で一件。ケース状態は変更しない。

ケース本文の Ok / Err は通常の Passed / Failed。VM 実行異常でケースが中断した場合は、そのケースを Failed として一件確定し、ファイルの実行異常を script_errors にも記録する。異なる分類のため合算してケース件数にしない。到達しなかった宣言は discovered に含めない。エラーがあれば「全件の一覧が得られた」と扱わない。

終了コードは既存 Rune に合わせ、成功 0、それ以外 1。使用法エラー、ケース失敗、scope_failures、script_errors、policy_errors のどれかがあれば 1。一覧では本文の成否を作らず、走査の失敗とポリシー違反を同じ規則で判定する。

選択ゼロでもフィルター指定がなければ現行どおり成功。選択対象が xit / pend だけでも、宣言が一致しているためフィルター一致ゼロにはしない。deny-pending がなければ pending の存在だけで失敗にしない。走査自体が失敗した場合、一致ゼロを確定した診断は追加しない。

all 実行ではファイル異常があっても次のファイルへ進む。ケース本文の Err 後も次のケースへ進む。実行異常が発生した VM は再利用せず、そのファイルを終了する。リトライ・成功へのフォールバックを追加しない。

## 8. 追加アサーション

すべて `Result<()>` を返す。契約に合わなければ `Err(TestAssertionFailed(detail))`、合えば `Ok(())`。アサーション自身はケースの成否を記録せず、本文の Result を既存の it に返す。途中の Result を捨てない書き方は既存 do / SafeBind を使い、別改修の文末 `?` に依存しない。

| API | 成功条件・型制約 |
|---|---|
| `assert_ne(expected: $A, actual: $A)` | 同一静的型、`$A: Eq`。Eq::eq が False |
| `assert_ok(result: Result<$A>)` | Ok。中身を比較しない |
| `assert_err(result: Result<$A>)` | Err。エラー種別を比較しない |
| `assert_err_kind(marker: ErrorKind, result: Result<$A>)` | Err かつ指定 deferror 宣言と同じ identity |
| `assert_some_eq(expected: $A, option: Option<$A>)` | Some かつ Eq で expected と等価。`$A: Eq` |
| `assert_none(option: Option<$A>)` | None。`$A` に Eq を要求しない |
| `assert_contains(fragment: String, actual: String)` | String::contains(actual, fragment) が True |
| `assert_approx(expected: Float, actual: Float, tolerance: Float)` | 非負の tolerance、絶対誤差が tolerance 以下 |
| `fail(detail: String)` | 常にアサーション失敗。detail はそのまま保持 |

assert_ne / assert_some_eq で Eq がなければ型エラー。inspect 比較にフォールバックしない。assert_ok / assert_err は Result の枝だけを検査し、値に Eq を要求しない。assert_none の失敗では Some の実値を表示する。

assert_contains は大小文字を区別する部分一致。正規表現・正規化を加えない。空 fragment は既存 String::contains と同じ成功条件。失敗では fragment と actual を表示する。

assert_approx は絶対誤差だけを扱い、相対誤差・暗黙の Int → Float 変換は加えない。許容誤差ちょうどは成功、0 は Float の等価検査、負の tolerance は不正な許容誤差として TestAssertionFailed。finite-only の Float 契約を守る。差の計算が有限 Float に収まらない組は、有限の許容誤差を超えるためアサーション失敗として扱い、計算途中の overflow で VM を終了させない。非有限値を Surtr の値として作らず、overflow を成功に変換しない。

### ErrorKind の契約

assert_err_kind の marker は具体的 deferror 宣言名だけを受理する。payload を持つ deferror でも、インスタンス構築やダミーの引数は不要。

```surtr
assert_err_kind(NoneError, result)
```

同名の別宣言を同一視しない。message・cause・位置・表示は比較条件に含めない。文字列、error インスタンス、変数、通常の型名などの marker はコンパイルエラー。Ok を渡す場合と、異なるエラー種別の Err を渡す場合はアサーション失敗。

現行 ErrorKind は通常関数で自由に転送できないため、assert_err_kind は標準のコンパイラ管理 API として追加する。Result::recover_kind と具体的宣言名の解決・identity 検証を共有し、API 名ごとの重複した型解決経路や文字列比較を作らない。利用者が ErrorKind を束縛・格納・一般の関数引数にする能力は増やさない。標準 API の canonical identity に基づいて認識し、同名の利用者定義関数を特別扱いしない。

### 既存 API との関係

assert_eq / assert_ok_eq / assert_err_contains、StyledDoc・入出力アサーションは、それぞれ既存の契約として維持する。assert_err_contains は表示文字列を検査する明示的な API であり、assert_err_kind の代替経路にはしない。

標準テストにある inspect 比較を調査し、枝だけ・種別だけ・Some の内容などを意図する箇所は新 API に移す。表示文字列そのものを検査するケースは文字列比較のまま残す。今回の移行を理由に別 PR の失敗伝播や診断位置の処理を改修しない。

## 9. 一覧・時間・出力形式

### 一覧

選択ケースを宣言順に表示し、it / xit / pend、名前、種類付きスコープ、ファイル、case index、理由、実行予定を保持する。本文は実行しないが走査処理は実行する。通常 human 出力では Filtered も明示し、quiet で省略する。

### 時間

単調時計で計測する。ケース時間は本文の評価開始から Result の返却または実行異常まで。IO キャプチャの初期化、結果の整形・出力は含めない。Skipped / Pending / Filtered / 一覧のケースに時間 0 を捏造せず、未計測として扱う。

コマンド時間は引数検証成功後から、全ファイルの処理と集計終了まで。読み取り、コンパイル、走査、ケース評価を含め、最終出力を含めない。再試行やウォームアップはしない。human は ms、JSON は非負整数の ns を使う。性能の合否閾値は設けない。

### human

実行時は PASS / FAIL / SKIP / PENDING / FILTERED、一覧時は LIST と宣言種別・選択状態を表示する。停止・未実装の理由を併記する。サマリーは第7節の件数を実行／一覧に応じて表示し、ケース件数とスコープ・ファイル異常を分ける。quiet でも失敗とポリシー違反は省略しない。色の既存設定を保持する。

### JSON

`--format json` は stdout に一つの JSON 文書だけを出力する。対象ファイル・ケースの順序は human と同じ。schema version や VM version の更新は行わない。

トップレベルは `command`、`mode`、正規化した `options`、`scripts`、`cases`、`errors`、`summary`、`exit_code`、`duration_ns`。`options` は file target、三種の filters、include_xit、deny_pending、quiet、timings、format を保持する。使用法エラーで正規化を完了できなければ options は null とする。

ケースには `file`、`case_index`、`scopes`（kind/name の配列）、`name`、`declaration`（it/xit/pend）、`selected`、`status`、`reason`、`detail`、`duration_ns` を含める。status は実行時 passed/failed/skipped/pending/filtered、一覧時 runnable/skipped/pending/filtered。スコープ・宣言 kind は閉じた enum とし、不正な値はエラーにする。

既存のキャプチャ情報は `io.stdout` / `io.stderr`、既存診断は `diagnostic` に保持する。新しい位置情報は生成しない。診断がなければ null。未計測時間、存在しない reason / detail は null。一覧のケース IO も null とし、本文を実行していないことを空配列と混同しない。

errors は usage / scope / script / policy の分類と message を保持する。script の既存 compile/runtime 診断を利用できる場合は diagnostic に保持する。走査処理の出力はケースの IO に混ぜず、ファイルごとのキャプチャとして別の `scripts` 配列に保持する。scripts は処理したファイル、完了・中断状態、走査処理の stdout/stderr を含む。入力取得と IO キャプチャの既存契約を変えず、JSON の stdout に走査中の生出力を混在させない。

quiet の JSON は実行時 passed/skipped/許容 pending/filtered のケース詳細を省略する。一覧では選択項目をすべて保持する。失敗・拒否 pending の詳細はどちらでも保持し、summary は常に全件を集計する。cases 配列の長さから総数を逆算しない。

引数解析に失敗しても、一意な有効 `--format json` 指定を認識できる場合は usage error を同形式で出す。format が重複・不正・欠落して出力形式を一意に定められない場合は stderr の通常 usage error。format の抽出は共通の引数トークン化を使い、別 parser の緩い再解釈を追加しない。JSON で処理した診断を stderr に重複表示しない。

## 10. 実装順序・正本

1. CLI の typed options と共通トークン化、usage、組み合わせ・終了コードの契約を先に固定する。`crates/rune/src/commands/test.rs`、`crates/rune/src/error.rs` が主対象。
2. VM の種類付きスコープ、ケースの開始・選択・終了、イベント種別、一覧・時間を実装する。it / xit を同じ実行経路に統一し、停止専用の旧経路を残さない。IO の分離は実行されるケースに対してだけ初期化する。
3. Test の xit / pend と通常関数で表現できるアサーションを追加する。必要な runtime 操作だけ builtin とし、正本 `sindr::BUILTIN_METAS` と Eldr の実装を対応させる。
4. assert_err_kind の canonical contract と既存 ErrorKind の共有解決を整合させる。Sigil・Scar・Forge・Eldr の成功／拒否境界を揃える。専用の言語構文・opcode は追加しない。
5. 一つの集計結果を human / JSON に整形する。出力経路だけで走査・実行を再度行わない。runner policy は VM の実行時設定とし、CLI フィルターや表示の違いでコンパイルキャッシュを分けない。標準ソースの変更は既存のコンパイル入力 fingerprint に反映する。
6. 目的に合う標準テストを新アサーションに移行し、正本文書とサンプルを同期する。

正本の変更対象は `lib/test.srt` の @doc、`docs/dev/Rune_cli_spec.md`、`docs/dev/テスト方針.md`、必要な `docs/dev/EldrVM_spec.md` / `docs/dev/Xldr_spec.md` の runner・IO 境界、`crates/rune/README.md` の synopsis。ErrorKind を説明する現行正本を特定し、assert_err_kind の受理位置を追記する。利用者向けの関連 docs/site にフィルター、停止・未実装、一覧・JSON、アサーションの使い方を反映する。

失敗位置改善と文末 `?` の PR が先に統合された場合も、本書の変更対象は上記のまま。実装時に最新ソースへ照合し、その別機能の変更を本 PR に取り込まない。

## 11. 受入条件・検証

- CLI の1,024組み合わせを typed options の表駆動テストで網羅する。各引数が options に残ることを確かめる。独立フラグの組み合わせごとにプロセス起動を1,024回行う必要はない。
- parser の値形式・引数順序・`--`・欠落・重複・未知引数・対象排他と、三種のフィルター全8集合を直接検証する。
- CLI integration は単一 / all × 実行 / list × human / json の8境界を固定し、全フラグを併用する例も検証する。quiet、deny-pending、timings、include-xit の個別境界と重なるものは既存ケースへ統合する。
- フィルターがケース本文の副作用を止め、スコープ走査を止めないこと、ネストした test/describe の選択、トップレベルケース、同名ケース、all での全体一致ゼロ判定を固定する。
- xit の本文は通常・一覧では0回、有効化された実行では1回。pend に本文を渡す呼び出しは拒否する。停止本文の型エラー、空理由、ケース内のケース宣言を拒否する。
- Filtered pend を deny-pending 違反に数えず、選択 Pending は一覧・quiet・JSON でも理由と終了コードに残す。Skipped / Pending だけの選択を一致ゼロにしない。
- 選択・状態件数の恒等式、スコープ失敗、ファイル異常、中断ケース、all の続行を検証する。失敗を既知の停止や未実装へ変換して成功扱いにしない。
- it / 有効化 xit で stdout/stderr/stdin の既存分離を守り、非実行ケースが実行ケースの IO バッファを消費しないことを固定する。一覧でケース本文の IO を発生させない。
- 新アサーションの成功・失敗、Eq 不足、Result/Option の枝、文字列境界、Float の誤差境界・負の許容誤差・差の overflow を検証する。
- ErrorKind の具体的宣言 identity、payload あり、同名別宣言、非 deferror marker、通常値への逃がしを検証する。inspect/メッセージ一致で種別判定しない。
- 時間の値そのものを固定せず、未計測 null、実行されたケースだけの非負時間、一覧の全体時間を検証する。サマリーに測定を加えても終了コードが変わらないことを確かめる。
- JSON は parse 可能な単一文書、全フラグの options、quiet の詳細抑制と完全な summary、一覧の selected 全件、診断・走査出力の非混在、usage error の形式選択を検証する。
- 診断の失敗位置を新しく特定するテストは本 PR で追加しない。既存診断情報の転送だけを検証する。

実装時は対象単位の TDD、標準 SRT と CLI 境界を検証した後、level4 として `rtk cargo nextest run --profile ci --workspace`、`cargo run -- test --quiet --all` を実行する。最終差分に対する別エージェントレビューで、選択・実行・表示の境界、旧経路、フォールバック、CLI 組み合わせの不足を確認する。

今回は提案書だけを作成し、ビルド・実行テスト・コミットは行わない。
