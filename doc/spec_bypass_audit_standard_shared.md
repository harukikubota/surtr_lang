# 標準定義・共有表現の暗黙処理とフォールバック調査・修正方針

調査基準: `7458fe8d8d16aa795d038345d7f86b9d6209ccee`（2026-10-03）。調査結果の行番号・実測値はこのコミットのもの。

修正方針追記: 2026-10-03。利用者の判断を各項目に追記した。追加のソース確認基準は `d96fc81e`。項目番号は元の調査との対応のため維持し、削除した項目は欠番とする。元の観測結果と、修正後に採用する期待値を区別する。この文書の移動・追記時点では製品コード・テスト・正本文書を変更していない。

**実測**、**既存テストの期待値**、**静的読解**を区別する。各項目の「確定した修正方針」「確定した扱い」が今回の判断である。SD-06 の実装案と SD-10 の LSP 詳細は、確定した言語仕様ではなく検討事項を含む。

## 実施状況（2026-10-04）

修正基準: `c71f510c`。以下の完了記録を除き、各項目の観測例は修正前の記録である。テストは現在の `lib/tests/` 配置を使う。

| 項目 | 状況 |
|---|---|
| SD-03 | 実装・対象テスト完了（level1）。全体検証は他項目の統合後に記録する |
| SD-04 | 実装・対象テスト完了（level3）。全体検証は統合後に記録する |
| SD-07 | 実装・対象テスト完了（level1）。全体検証は統合後に記録する |
| SD-09・11 | 実施中 |
| SD-05・08 | 確定方針どおり現行仕様を維持 |
| SD-06 | 簡素化案のまま保留 |
| SD-10 | 別ドラフトの未確定事項として維持 |

## SD-03 `StyledDoc::indent` が負の幅の Error を空 prefix に変える

- 性質: 公開入力で可達の成功フォールバック。確度: 実測。
- 実装: `lib/styled_doc.srt:161-165,421-422`。`String::repeat(" ", width)` の `Err(_)` を `""` にする。
- 正本: `lib/styled_doc.srt:413-419` は「各行を width 個の空白で prefix」とだけ記載。負の幅の no-op は明記されていない。`lib/types/string.srt:14-20,273-286` は負の repeat を `NegativeRepeatCount` として返す。

```surtr
StyledDoc::plain(StyledDoc::indent(StyledDoc::text("x"), -1))
# 現行実測: "x"
# 下位の String::repeat(" ", -1) は Err(NegativeRepeatCount(...))。
```

- 既存テスト: `lib/tests/styled_doc.srt:41-49` は幅 2 の期待値 `"  one\n  \n  two"`。`lib/tests/string.srt:41-45` は負数 Error を検証する。indent 側の負幅は固定されていない。

### 確定した修正方針

分類: **ソースコードと文書を修正する**。負の幅を成功として扱わず、`String::repeat` の `Err` をそのまま返す。新しい Error への変換や、空文字による救済はしない。戻り型と呼出し側も合わせて変更する。

```surtr
def indent(doc: StyledDocDoc, width: Int) -> Result<StyledDocDoc, NegativeRepeatCount>

StyledDoc::indent(StyledDoc::text("x"), -1)
# 修正後の期待値: Err(NegativeRepeatCount(-1))

StyledDoc::indent(StyledDoc::text("x"), 0)
# 修正後の期待値: Ok(元の内容を保つ StyledDocDoc)

StyledDoc::indent(StyledDoc::text("one\n\ntwo"), 2)
# 修正後の期待値: Ok(doc)。その doc を plain に渡すと "  one\n  \n  two"。
```

`lib/styled_doc.srt` の宣言・実装・`@doc`、呼出し側、`lib/tests/styled_doc.srt` を整合させる。負数・0・正数と空行の境界を検証する。冒頭の実測例は変更前の記録であり、変更後は `indent` の Result を処理してから `plain` に渡す。

### 実施記録（2026-10-04）

`indent` を `Result<StyledDocDoc, NegativeRepeatCount>` に変更し、`String::repeat` の Error をそのまま返す。空 prefix に置換する `_spaces` は削除した。`@doc` と呼出し側は Result を処理する形へ更新した。

`lib/tests/basic_types/styled_doc.srt` で負幅、0、正幅、空行、空文書を検証した。変更前は Result として扱えず `MissingTypeConstructorCapability: StyledDocDoc must implement Monad` で失敗し、修正後の `target/debug/surtr test --quiet lib/tests/basic_types/styled_doc.srt` は exit 0。対象バイナリは `cargo run` で標準定義更新後に再ビルドした。

## SD-04 `HashMap::map_values` が内部 lookup 失敗を欠落キーへ変える

- 性質: 内部不変条件破損を部分成功へ変える経路。正常な公開入力からの到達は確認できない。確度: 静的読解。
- 実装: `lib/types/hash_map.srt:35-46,143-144`。`map_keys(map)` で取得したキーを、同じ immutable source に問い合わせる。`Err(_)` ならキーを無言で捨てて続行。
- 正本: 同ファイル `@doc:1-8,85-91,110-124,136-141` は immutable map、keys/values の決定的順序、**same keys and transformed values** を明記する。

```surtr
HashMap::map_values(hash!["a" => 1, "b" => 2], {|n: Int| n + 1})
# 正常入力の期待値: hash!["a" => 2, "b" => 3]
# 内部条件: map_keys(source) にある "a" について map_get(source,"a") が Err。
# 現行の静的期待値: "a" を含まない部分 map。公開ソースだけでこの条件は作れない。
```

- 既存テスト: `lib/tests/hash_map.srt:45-78` は正常入力・ユーザー型の変換を検証する。

### 確定した修正方針

分類: **ソースコードと文書を修正する**。PureSurtr の内部 lookup に対する到達不能な `Err` ケースを握りつぶさずに表すため、`map_values` をビルトイン化する。公開シグネチャは維持する。

```surtr
@builtin def map_values(map: HashMap<$A>, f: ($A -> $B)) -> HashMap<$B>

HashMap::map_values(hash!["a" => 1, "b" => 2], {|n: Int| n + 1})
# 修正後の期待値: hash!["a" => 2, "b" => 3]
```

- 全キーを保持し、元 map の決定的なキー順で各値へ callback を1回ずつ適用する。空 map では callback を呼ばない。
- map の内部異常や callback 実行の RuntimeError は、そのエラーとして伝播する。キーを捨てた部分 map を成功として返さない。
- `$B` が Result の場合、その `Err` は普通の値として map に格納できる。VM 実行失敗と、言語上の Result 値を混同しない。
- `lib/types/hash_map.srt` の `_map_values_go` など旧実装を削除する。ビルトイン登録は Sindr の `BUILTIN_METAS` を正本とし、Eldr の実装を対応させる。

正常値・空 map・callback の順序と回数・Result 値の保持を検証する。内部異常の拒否は、公開入力で作れない条件を crate 内部のテストで確認する。

### 実施記録（2026-10-04）

`map_values` を Sindr の正本へ登録し、Eldr の callback 継続として実装した。元 map のソート済みエントリを直接走査するため、旧 `_map_values_go` とキーの再 lookup は不要になった。公開シグネチャを保ち、`@doc` と VM 正本へ callback の順序・回数・失敗伝播を明記した。

TDD では新 builtin の metadata 不在による Red を確認後、`rtk cargo nextest run -p eldr map_values` の4件、metadata 順序の1件、`rtk cargo nextest run -p sindr builtin` の36件が成功した。`rtk proxy cargo run -- test --quiet lib/tests/basic_types/hash_map.srt` も exit 0。callback の RuntimeError は部分成功へ変えず、言語の `Err` 値はそのまま保持する。HashMap の内部 storage は非公開の immutable 表現であり、破損したキー一覧と source の不一致は新経路では構築しない。crate 内部テストでは不正な map 引数の拒否も確認した。

## SD-05 Int 固定幅演算が「起こらないはずの除算 Error」を 0 に変える

- 性質: 内部成功フォールバック。正常入力では分母が正であり、ZeroDivisionError への到達根拠はない。確度: 静的読解。
- 実装: `lib/types/int.srt:489-493,516-523`。`_positive_mod` と `_shr_logical_bits` の `Err(_) => 0`。
- ガード: `width_bits:534-543` は正の幅のみ。`wrap_unsigned:564-566` は分母 `2^bits`。`shr_logical_in:679-683` は非負 shift を検証後に `2^shift` で割る。rotate の `_positive_mod` の分母も検証済み width (`693-716`)。
- 正本: 同ファイル `@doc:527-562,672-677,686-707` は invalid width / negative shift を Error として表す。内部除算失敗を 0 に畳む理由は明記されていない。

```surtr
Int::wrap_unsigned(-1, BitWidth::W8)       # 期待値: Ok(255)
Int::shr_logical_in(-1, 1, BitWidth::W8)  # 期待値: Ok(127)
Int::shr_logical_in(1, -1, BitWidth::W8)  # 期待値: Err(NegativeShiftCount(...))
# 内部条件: 正の分母で safe_div/safe_mod が Err。
# 現行静的期待値: 0 を用いた正常結果。通常 Surtr 入力での再現ではない。
```

- 既存テスト: `lib/tests/int.srt:420-433` が shift の成功・負数拒否を固定。

### 確定した扱い

分類: **把握済みの仕様として維持する**。`BitWidth` は Enum であり、バリアントごとの入力範囲から到達不能なケースを含む。正規の入力範囲で正常に処理されるなら、この内部分岐を修正対象にしない。

上の成功例と負 shift の拒否を維持する。「公開入力で除算 Error が起き、それを 0 にしている」とは扱わない。必要なら、分母が検証済み幅または `2^n` となることを内部説明に補足する。

## SD-06 ANSI escape の生成失敗が空文字になる

- 性質: 定数入力に対する内部成功フォールバック。確度: 静的読解。
- 実装: `lib/styled_doc.srt:232-236` の `String::from_codepoints([27], StringEncoding::Utf8)` が失敗すると空文字。
- ガード: 27 は Unicode scalar として有効。正常な builtin 契約では Error にならず、公開引数からの失敗再現はない。
- 正本: `lib/styled_doc.srt:137-147` の ANSI rendering、`String::from_codepoints` の文書・宣言 (`lib/types/string.srt:435-447`) が対応する。失敗時に escape を消す契約はない。

```surtr
String::from_codepoints([27], StringEncoding::Utf8)
# 正常期待値: Ok(ESC を1文字含む String)
# 内部条件: この定数変換が Err。
# 現行静的期待値: _esc() が ""、ANSI sequence の prefix が欠ける。
```

### 修正方針と確認済みの前提

分類: **メタ文字対応を前提に表現の簡素化を検討する**。文字列のエスケープ対応は改修済みで、確認時点の `docs/site/strings.md:26-44` は `\u{HEX}` を Unicode スカラー値1文字として定義している。定数 ESC は文字列リテラルで表せる。

```surtr
# 今後の置き換え案。実装済みという意味ではない。
def _esc() -> String { "\u{1b}" }
# 期待値: U+001B を1文字含む String。
# "\\u{1b}"（バックスラッシュから始まる文字列）とは区別する。
```

この案なら codepoint 変換と `Err => ""` 自体が不要になる。メタ文字の decode・表示は担当クレートの既存契約に従う。ANSI の色・reset・通常テキストの出力を維持し、無効な文字列 escape を成功へ変える経路は設けない。リテラルへの置換そのものは今回の指示で確定した実装ではなく、実装時の簡素化案として記録する。

## SD-07 `List::find_map` の Error 破棄は「最初の成功を探す」公開契約

- 性質: 文書化された通常 SRT の振る舞い。compiler の特権処理ではない。確度: 実測。
- 実装／正本: `lib/types/list.srt:436-448`。`Return the first successful mapped value` と明記。全失敗時の結果は `NoneError` であり、各失敗の具体 Error は保持しない。

```surtr
List::find_map([1, 2], {|n: Int|
  if(n == 1, Err(ZeroDivisionError), Ok(n))
})
# 現行実測・文書に基づく期待値: Ok(2)
```

- 既存境界: `lib/tests/` に find_map 専用ケースは検索上見つからなかった。今回は上記を HEAD バイナリで実測。

### 確定した修正方針

分類: **ソースコードと文書を修正する**。callback の戻り値を Option に変更する。最初の `Option::Some(value)` を `Ok(value)` として返し、`Option::None` は次の要素へ進む。空 List または全要素が None の場合は `Err(NoneError)` を返す。

```surtr
def find_map(values: List<$A>, f: ($A -> Option<$B>)) -> Result<$B, NoneError>

List::find_map([1, 2], {|n: Int|
  if(n == 1, Option::None, Option::Some(n))
})
# 修正後の期待値: Ok(2)

List::find_map([1, 2], {|n: Int| Option::None})
# 修正後の期待値: Err(NoneError)。具体型を確定する注釈は必要に応じて付ける。
```

ここで検索の終端（Terminal）は最初の Some であり、その後の callback は実行しない。古い `Result<$B>` を返す callback は型エラーにし、暗黙の Result→Option 変換による互換経路は残さない。変換が必要な利用者は SD-08 の明示変換を使う。

`lib/types/list.srt` の宣言・実装・`@doc` と、関連する利用例・文書を更新する。最初の Some、途中の None、空 List、全 None、成功後の打ち切り、旧 callback の拒否を検証する。冒頭の ZeroDivisionError の実測例は旧契約の記録であり、新シグネチャでは受理しない。

### 実施記録（2026-10-04）

callback を `Option` 戻り値へ変更し、`Some` で終了、`None` で次要素へ進むようにした。`@doc` と利用者ガイドの例も更新した。旧 Result callback への互換経路はない。

旧実装で Option callback が型エラーになる Red と、旧 Result callback が受理されることを確認した。変更後の `rtk proxy cargo run -- test --quiet lib/tests/monads/list.srt` は exit 0。最初の Some での打ち切り、途中の None、全 None、空入力を検証する。旧 callback の拒否は `tests/fixtures/script/fail/typecheck/list_find_map_result_callback.srt` に置き、明示的な `Result<Int>` 戻り値の関数を渡して型不一致を固定した。`rtk cargo nextest run -p rune --test integration run_srt` は9件成功（exit 0）。

## SD-08 `Result` → `Option` の Error 破棄は明示変換

- 性質: 文書化された通常 SRT の振る舞い。確度: 実測＋既存テストの期待値。
- 実装／正本: `lib/types/option.srt:226-237` は **dropping the concrete error value**。`docs/site/standard-library.md:394` は `Err(_)` を None に畳む明示変換と明記する。

```surtr
source: Result<Int> = Err(ZeroDivisionError)
to::<Option<Int>>(source)
# 現行実測・文書に基づく期待値: Option::None
```

- 既存テスト: `lib/tests/option.srt:51-54` は `Err(NoneError)` → `Option::None`。

### 確定した扱い

分類: **把握済みの明示変換として維持する**。利用者の確認により、任意の `Err` を `Option::None` にする現行挙動を維持する。`Err(NoneError)` のみに変換を制限しない。

```surtr
source: Result<Int> = Err(ZeroDivisionError)
to::<Option<Int>>(source)
# 維持する期待値: Option::None

none_value: Option<Int> = Option::None
to::<Result<Int>>(none_value)
# 維持する期待値: Err(NoneError)
```

`Err(NoneError)` と `Option::None` は標準の「値がない」表現として対応する。Result→Option は利用者が選ぶ明示的な情報破棄なので、任意の Error を None にする。`Ok(value)` と `Option::Some(value)` の対応も維持する。型適合のための暗黙変換は追加しない。必要なら標準の不在表現と明示変換の説明を補足する。

## SD-09 runtime 値の表示が未知 tag／欠損 payload を許す

- 性質: 内部破損・表示 fallback。正常ソースからの可達性は未確認。確度: 静的読解。
- 実装: `crates/sindr/src/runtime.rs:509-529`。TypeRegistry に entry がない tagged 値は、reserved tag 0/1 なら Result 表示、他は `Tagged(tag, fields)`。tag 0/1 の fields が空なら `unwrap_or_default()` で補う。
- 正本: `docs/dev/EldrVM_spec.md:181-190` が Tagged / Tag 表現を定義し、`306-315` は invalid tag を即時 RuntimeError の対象とする。表示関数自体もこの拒否契約に含めるかは明記されていない。reserved Result 自体は標準 runtime 表現であり、それを表示することは fallback 違反ではない。

```rust
// 内部 Value を直接組み立てる条件。通常 Surtr プログラムではない。
(Value::Tagged { tag: 0, fields: vec![] }).to_display_string(&TypeRegistry::new())
// 現行静的期待値: "Ok()"
(Value::Tagged { tag: 9999, fields: vec![] }).to_display_string(&TypeRegistry::new())
// 現行静的期待値: "Tagged(9999, [])"
```

- 既存テスト: `crates/sindr/src/runtime.rs:907-921` 付近は正しい payload の Ok/Err 表示。欠損 payload を正常結果へ見せる境界は未固定。

### 確定した修正方針

分類: **ソースコードと文書を修正する**。未知 tag と必要 payload の欠損は、バージョン不整合やコンパイラの不具合による異常状態として RuntimeError にする。`Tagged(...)`、`Ok()`、空文字へ救済しない。

追加の確認対象は `crates/eldr/src/builtin.rs:3473-3558`（方針追記時点）。Sindr の `to_display_string` だけでなく、Eldr の `render_tagged_value` にも同様の分岐がある。既知の struct/record を `zip` で表示する経路も、欠けたフィールドが黙って消えないよう確認する。

```rust
// 修正後に検証する内部状態と期待値。
Value::Tagged { tag: 0, fields: vec![] }     // RuntimeError: 必要 payload の欠損
Value::Tagged { tag: 9999, fields: vec![] }  // RuntimeError: 未知 tag
// 正しい reserved Result tag 0/1 と payload は従来どおり表示する。
```

- 表示・inspect の失敗を VM の RuntimeError として呼出し側へ伝える。言語の `Err` 値や Rust の panic で代用しない。
- List、tuple、map、既知 tag のフィールドなどに異常値が入る場合も、再帰表示でエラーを保持する。
- 正規の reserved Result 表現は維持する。未知 tag に合わせて registry を推測補完しない。
- 表示 API の戻り型変更と呼出し側の伝播方法は実装時に整理する。スキーマ・VM バージョンの引き上げや旧形式の互換経路は追加しない。

未知 tag、必要 payload の欠損、入れ子の異常値、正常 Result と正常ユーザー型の表示を検証する。

## SD-10 任意メタデータと editor tolerant parse は許可された補助経路

- `crates/sindr/src/ir.rs:1486-1488` は欠損 `Docs` / `SigT` を空にする。`docs/dev/EldrVM_spec.md:406-416` が両者を任意チャンクと明記する。`SigT` のない bytecode も受理する仕様。存在する payload の破損は `deserialize_optional` (`ir.rs:1634-1643`) が `DecodeFailed` を返す。実行に必要な `Code` / `CalT` / `Proc` 等は `deserialize_required` (`ir.rs:1470-1489`) であり、この任意処理を実行情報の欠損救済と混同しない。
- `crates/surtr-analysis/src/service.rs:185-265` は editor 用の部分 AST を得ても、active document の strict parse 失敗を ParseError として保持する。`docs/dev/Surtr_LSP_spec.md:439-448` は editor-only tolerant API を認め、compile/run/REPL の strict path に流すことを禁止する。

```surtr
def incomplete(
# editor の outline / syntax context に部分情報が残ることは許可。
# compile/run でこの断片を有効な定義として評価することは不可。
```

- 現行期待値: 静的読解では editor の部分情報と Parse diagnostic が併存。コンパイル成功への fallback ではない。project 他ファイルの解析は別に存在するため、全 project を止める仕様とは断定しない。

### LSP 改修ドラフトへの切り分け

分類: **現行の許可範囲を確認した上で、改修範囲をドラフトで整理する**。成果物の配置、任意チャンク、tolerant parse、ProjectRunner の補助経路は別々に判断する。

詳細は [LSP のエディタ解析・成果物境界の改修ドラフト](lsp_editor_artifact_boundary_draft.md) に移した。`target/` 内への配置はユーザーの成果物との衝突回避には使えるが、それだけで解析経路や metadata の契約は変わらない。正常診断ではソースを解析し、エディタの部分 AST はコンパイル成功に転用しない。任意 `Docs` / `SigT` の欠損と、存在する payload の破損は区別する。

SD-10 の観測記録は残すが、LSP の未確定仕様をこの項目で確定させない。

## SD-11 `>>` の call 式拒否という説明が現行挙動と一致しない

- 性質: 文書と実装の不一致。確度: 実測＋既存テストの期待値。
- 正本: `docs/site/function-operators.md:172` は「compose なので、`trim() >> render()` のような call 式は不許可」とする。
- 実装: `crates/scar/src/checker/expr.rs:4235-4252` は構文が call かどうかではなく、型検査後の型が `Ty::Func` かを検査する。call の戻り値が callable なら許可される。

```surtr
def make_inc() -> (Int -> Int) { {|x: Int| x + 1} }
def make_double() -> (Int -> Int) { {|x: Int| x * 2} }
composed = make_inc() >> make_double()
composed(3)
# 現行実測: 8
```

- 既存テスト: `crates/scar/tests/typecheck_surface.rs:7651-7664` の `compose_accepts_calls_returning_function_values` は上記形を受理。`compose_rejects_non_function_call_results_after_typechecking_call:7666-7674` は Int 返り値の call を拒否する。

### 確定した文書修正方針

分類: **把握済みの式評価・合成仕様を維持し、文書を修正する**。用語は次の5種類を使い分ける。

| 用語 | 意味・例 |
|---|---|
| 関数呼出し | 必要な引数をすべて満たした呼出し。例: `make_add(1)` |
| クロージャリテラル | `{|ARGS| EXPRS}` 形式の式 |
| キャプチャ | `&identity` などで関数値を作る式 |
| 関数値変数 | クロージャまたはキャプチャを格納している変数 |
| 引数待ち受け呼出し | パイプ演算子系列の RHS にあり、パイプ注入で引数が満たされる呼出し。例: `2 |> add(1)` の `add(1)` |

関数合成では引数待ち受け呼出しを使えない。合成演算子 `>>` / `>*` / `>=>` は引数を注入せず、各演算子の型契約を満たす関数値を受け取る。完全に引数を満たした関数呼出しも、評価後に適合する関数値を返すなら入力として使える。通常の呼出しの引数に関数呼出しを含む式がある場合も、その式を評価した値を渡す。

```surtr
def add(x: Int, y: Int) -> Int { x + y }
def double(x: Int) -> Int { x * 2 }
def make_add(n: Int) -> (Int -> Int) { {|x: Int| x + n} }

pipeline = make_add(1) >> &double
pipeline(2)
# 維持する期待値: 6。make_add(1) の戻り値を合成する。

2 |> add(1)
# 維持する期待値: 3。パイプ注入で add の残りの引数が満たされる。

add(1) >> &double
# 拒否する期待値: 引数不足。合成による引数注入はない。
```

修正時は `docs/site/`、`docs/dev/`、`lib/` の `@doc` を横断して用語を合わせる。優先して確認する記述は `docs/site/function-operators.md:172` の「call 式は不許可」、`docs/site/callables.md:18,59` の「実行結果ではなく」、`docs/site/language-reference.md:408` と `docs/site/language-guide.md:745` の括弧に関する説明。

通常の呼出しで引数の値を先に求めることと、パイプ両辺の一律な評価順は別の契約である。今回の文書修正でパイプ全体の新しい評価順は約束しない。Lazy の専用規則と、パイプ RHS の括弧が注入を抑止する規則も、通常呼出しや合成の説明に混ぜない。

## 検証記録

元の調査 worktree で `cargo build -p rune --bin surtr` 成功（exit 0）。この HEAD の `target/debug/surtr` へ REPL stdin を渡し SD-03/07/08/11 を実測した。REPL exit 0 は各式成功の証明ではないため、個別出力・diagnostic を読んで判断した。SD-08 は初回の不正な式内型注釈を除外し、binding 注釈で再測定している。

既存テストは `target/debug/surtr test --quiet styled_doc` を実行し、exit 0。quiet のため件数は出力されていない。他の掲載テストは文書・期待値の読み取りのみ。SD-04/05/06/09 の内部破損条件は未実測。

今回の移動・方針追記は文書のみ。修正後の例は受入条件であり、修正済みの実測結果ではない。ビルド・テスト・全体 CI は再実行していない。他の調査記録は変更していない。
