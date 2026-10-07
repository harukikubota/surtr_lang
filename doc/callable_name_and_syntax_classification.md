# 関数名・予約構文・標準特殊処理の変更仕様

作成日: 2026-10-04。状態: ユーザ指定により分類と一般中置の制限を確定。製品コードへの反映は未実施。

## 目的と方針

TC-01 の `curry` 問題を起点に、名前の予約と標準宣言の特殊処理を分離する。関数名の綴りだけで通常関数の本体・戻り値・副作用を置き換えない。

分類番号は従来の議論を引き継ぎ、①と③を採用する。②の対象9名は①へ移し、②は採用しない。名前の使用位置と引数の文法は別に記録する。

| 分類 | 宣言・シャドーイング | 呼出し構文 |
|---|---|---|
| ① 予約構文・予約名 | 利用者による同名宣言・束縛を禁止。標準 trait method の実装は維持 | 対象ごとの文法・標準宣言に従う |
| ③ 通常の名前 | 通常の宣言・scope・import 規則に従う | 呼出し先の宣言 identity に従う |

③でも、選択された標準宣言にだけ Lazy・ErrorKind・特殊な型検査が必要な場合がある。「通常の名前」は「すべて eager」「任意の関数値として取得可能」「特殊型を利用者の署名に書ける」を意味しない。
同一 scope の重複宣言や import 衝突まで許可する案ではない。シャドーイングは通常の scope 規則が許す範囲に限る。

## 分類一覧

| 対象 | 現行 | 確定した変更後の規則 |
|---|---|---|
| `match`、`cond`、`bulk_update`、`do` | 専用 token / AST / 引数文法 | ①を維持 |
| 宣言・修飾・構文接続語 | 専用 token | ①を維持。下記の有限集合を使う |
| `on`、`lt`、`lte`、`gt`、`gte`、`eq`、`neq`、`and`、`or` | 関数宣言可能(標準のみ)・変数・引数・Pattern 束縛・フィールド名は不可 | ①へ変更。同名の独立した関数定義も禁止し、標準宣言・標準 trait method に固定 |
| `curry`、`uncurry` | 通常名。`curry` に綴り判定の問題あり | ③。標準 `Function::curry` のみ identity に基づく特殊検査 |
| `fmap`、`ap`、`bind`、`choice`、`pipe`、`compare`、関数合成 API | 通常関数名 | ③。対応する記号演算子の文法とは分離 |
| `if`、`if_then` | 通常名、標準宣言は Lazy 特殊処理 | ③を維持。`if` は字句キーワードではない |
| `require`、`ensure`、`map_err`、`cause` | 通常名、標準宣言は Lazy / Error 制約 | ③を維持 |
| `recover`、`Error::kind` / `same_kind` / `message` / `format`、`eprint` 等 | Error の型・受け渡し制約を持つ標準 API | 関数名は③。Error の公開制限は維持 |
| `recover_kind`、`assert_err_kind`、`assert_cause_chain` | 通常名。解決後に標準 ErrorKind API と識別 | ③を維持。ErrorKind の一般値化はしない |
| `if_let`、`if_let_then`、`is_match`、`apply_pattern` | ①相当の予約 consumer。第2引数を Pattern として解析 | ①を維持。同名の通常関数・変数・user member は禁止 |
| `print`、`to_string`、List / String / 数値 / I/O 等のその他の公開関数 | 通常の名前解決と宣言・builtin metadata | 原則③。`@builtin` だけを理由に名前を予約しない |

### ①の追加対象と予約範囲

lexer の宣言・接続語は次の集合である（`crates/spire/src/lexer.rs`）。

- 宣言: `def`、`defp`、`defmod`、`deftrait`、`defstruct`、`defrecord`、`defenum`、`deferror`、`defextractor`、`defagent`、`defgenserver`、`defsupervisor`、`defdynamic_supervisor`。
- その他: `namespace`、`import`、`include`、`impl`、`for`、`when`、`private`、`public`、`readonly`、`const`、`type` / `Type`、`where`、`supervisor_init`。

`def[a-z]*` は説明上の総称に留める。現行 lexer は列挙した名前を認識し、`def` で始まる任意の識別子を予約してはいない。prefix 全体の予約は不要な使用禁止を増やすため提案しない。

次は関数名の3分類だけでは表せないため、予約対象の位置も併記する。

| 対象 | 予約するもの |
|---|---|
| `True`、`False`、`Ok`、`Err` | 標準 Enum variant の予約 alias。①側に置くが、すべてを特殊関数とは呼ばない |
| `Int`、`Result`、`Error`、`ErrorKind`、`Lazy`、`Hole`、`MatchResult`、`ExtractorClosure`、特殊ブロックの型等 | 型・owner 名の予約。完全な集合と使用可否は `sindr::names` の型 metadata を正本とする |
| `Self`、`Type`、`new` | 型や owner/member の文脈に依存する規則。小文字関数名の全域予約と混同しない |
| `_`、`_1`〜`_16`、`&1`〜`&16`、`^` | wildcard、projection、pipe slot、capture、pin の文脈規則。通常名として解放しない |
| `dbg!`、`hash!` | `!` を含む専用構文。`dbg` / `hash` という通常名の禁止とは分離 |
| `pid`、`spawn`、`adopt`、`status`、`workers` | process owner 内の compiler-managed member の予約。全モジュールの同名関数を禁止しない |
| `set`、`over`、`over_result`、`case_set`、`case_over` 等 | `bulk_update` 内の操作文法。通常の同名関数とは別 |
| `@builtin`、`@doc` 等の属性、compiler 内部の `__...` 名 | 属性位置・内部生成宣言の規則。公開関数の shadowing 許可から内部権限を導かない |

## on グループ: ①に確定

`on`、`eq`、`neq`、`lt`、`lte`、`gt`、`gte`、`and`、`or` を予約名とする。

- 利用者の変数・引数・Pattern 束縛・フィールド・独立した関数・独自 member の同名定義を禁止する。別モジュールや修飾名を使った定義にも広げない。
- 標準宣言と、標準 Eq / Compare trait の契約を実装する同名 method は許可する。trait implementation は既存の標準 method の実装として扱い、名前の一致だけで例外にしない。
- 標準関数の prefix call、修飾 call、許可された capture は維持する。比較は型に応じた trait dispatch を維持する。
- 標準関数を別名の関数値変数へ保存できる場合も、その変数の中置呼出しは禁止する。通常の prefix call は維持する。
- 同名の変数を無視して標準宣言を選ぶ経路は設けない。禁止された宣言・束縛はその位置で拒否する。

| 予約名 | 標準 target | 中置の構文・評価 |
|---|---|---|
| `on` | `Function::on` | 現行の低い優先順位を維持。修飾中置 `Function::on` も維持 |
| `eq` / `neq` | 標準 `Eq` trait の対応 method | 現行の比較優先順位と trait dispatch を維持 |
| `lt` / `lte` / `gt` / `gte` | 標準 `Compare` trait の対応 method | 同上 |
| `and` / `or` | `Kernel::and` / `Kernel::or` | 現行の論理優先順位と標準の短絡評価を維持 |

「裸の中置」は ``left `name` right`` を指す。空白だけで区切った `left name right` ではない。記号演算子と標準 Lazy capture の既存制約も維持する。

現行は同名関数の宣言を許可し、`on` だけが裸の中置で標準固定となっている。変更後は9名の独立した同名定義を禁止する。従来の②として変数名を解放する案は採用しない。

## 一般の名前付き中置の制限（決定済み）

現行実装は通常名の中置を通常の `Ast::App` へ変換し、関数値変数も受理する。これを次の規則へ変更する。

1. ローカルから外側へ通常の名前解決を行い、最初に見つかった参照を確定する。修飾名は明示した owner の解決規則に従う。
2. その参照が関数宣言なら、中置呼出しの対象として署名・引数を検査する。標準 builtin 関数宣言や trait method も宣言として扱う。
3. 変数・関数引数などの値への参照なら、関数型であっても中置呼出しを拒否する。非関数値も拒否する。
4. 拒否した後で外側の同名関数・元の関数・標準関数を探し直さない。関数宣言だけを絞り込んで検索することもしない。

宣言への import 等の別名参照は宣言 identity に従う。`f = &add` のような関数値の束縛は変数であり、元が関数宣言でも中置呼出しを許可しない。closure の束縛も同じである。

```surtr
combine = {|a: Int, b: Int| a + b}
value = 2 `combine` 3  # 変更後は拒否
value = combine(2, 3)  # 通常の関数値呼出しは許可
```

中置由来を AST と解決処理まで保持し、Sigil で選択した参照の種別を検査する。型が callable かどうかだけで判定しない。backtick prefix call と通常 call に、この中置限定の禁止を広げない。

## Pattern・Error・ErrorKind を分ける

### Error: ③でよい

Error を受けることは、引数を別の文法で読む理由にならない。標準 API が Error を受けられることと、同名の利用者関数を定義できることは別の契約である。
たとえば同名の通常関数が `Int` を受けるなら、その宣言 signature で検査する。標準 API の名前を使っただけで Error を parameter / return / field に公開できるようにはしない。

### ErrorKind: ③でよい。静的引数の制約を維持する

現行 Sigil は callee を解決し、`Result::recover_kind`、`Test::assert_err_kind`、`Test::assert_cause_chain` の標準宣言 identity から処理を選ぶ。parser がこれらの名前を一律に予約する仕組みではない。

標準 API の marker は具体的な `deferror` 名として解決する。`assert_cause_chain` の直接 List literal も静的な列として扱う。通常の同名関数は通常の引数式を受ける。runtime Error、文字列、動的 List、placeholder を標準 ErrorKind 引数へ流す fallback は設けない。

### Pattern: ①を維持する

`if_let`、`if_let_then`、`is_match`、`apply_pattern` は構文予約とする。現行 Spire の bare consumer 名および `Kernel::name` による Pattern 引数解析を維持し、同名の通常宣言・束縛を許可しない。
Regex の通常 API は `Regex::matches` とする。Pattern の一般値化、新しい Pattern 引数表記、解決結果による Expr / Pattern の再解析は導入しない。

### Lazy: 名前は③、標準宣言を解決した場合だけ特殊処理

`if`、`if_then`、`require`、`ensure`、`map_err`、`cause` 等の名前は③とする。名前解決で選択した標準宣言の identity に従って Lazy 引数を処理する。同名の通常関数・関数値変数なら、その署名と通常の引数評価に従う。
`and/or` は予約名として①に属し、標準宣言に対して Lazy 処理を適用する。`Lazy<T>` の利用者署名への公開禁止と、標準 Lazy capture の制約は維持する。

## TC-01 と同種の問題に適用する原則

現行 `crates/scar/src/checker/expr.rs` の `is_function_curry_callee` は `id.name == "curry"` 等で判定し、`check_function_curry` の callee 引数は未使用である。通常の署名処理より先に分岐する。行番号は今回の checkout では 3900 付近と 10824 付近で、調査入力の行番号から移動している。
`is_function_on_callee` にも同種の綴り判定がある。名前の予約だけに依存せず、標準宣言の identity に基づく検査へ揃える。

1. 特殊ブロック等の①の専用構文は専用 AST へ変換する。①の予約関数名は標準宣言に結び付ける。
2. parser は中置の優先順位と中置由来を保持する。Sigil は最初に解決した参照の種別を検査し、関数値変数なら拒否する。
3. 通常 call は Sigil が宣言 identity を確定する。builtin / 特殊検査はその宣言 metadata に結び付ける。
4. Scar は local 名・末尾の member 名・autoimport の有無から特殊処理を選ばない。標準 API が期待する arity、型、RTA は引き続き厳密に検査する。
5. 標準宣言への alias は同じ identity を保つ。通常 callable の変数は、その callable の署名と本体に従う。特殊宣言を値として扱えない経路では明示的に拒否する。
6. 解決失敗、型不一致、未対応 capture を同名の標準関数へ切り替えて救済しない。置換した綴り判定経路は削除する。

TC-01 の提案期待値は `value: Int = 999`。その後の `value(2)` は非 callable の呼出しとして拒否する。標準 `Function::curry(&add)` は従来の curry として検査する。利用者の `curry::<...>` は利用者宣言の RTA 契約で判定し、標準の curry 規則を流用しない。

## 受入条件と実装順序

level 4。名前使用・中置の呼出し先・評価規則・フェーズ境界に関わる。今回は文書のみで、実装や実行可能テストは変更しない。一般中置の変数拒否・fallback 禁止と、on グループの①への移行は決定済み。

1. on グループの①への移行、Pattern の①維持、Error / ErrorKind / Lazy 系の③と、一般中置の制限を正本へ反映する。
2. 標準 identity と名前使用位置の責務を Sindr / Spire / Sigil で整理する。runtime builtin の正本は引き続き `BUILTIN_METAS` とし、別の文字列台帳を各フェーズへ複製しない。
3. `curry` の通常関数誤認を回帰テストで固定し、`curry` / `on` の Scar の綴りによる分岐を除去する。`on` の同名利用者定義は宣言拒否テストへ移す。
4. on グループの予約名規則と、一般中置の参照種別検査を実装する。Pattern の現行構文は維持する。旧経路は残さず、既存の拒否テストを新しい境界へ更新する。schema / VM version は上げない。

| 検証対象 | 必須の境界 | 主な配置 |
|---|---|---|
| `curry` | 標準成功・不正 arity、同名 def、別 module、local callable、宣言 alias、callable 値、capture、pipe、RTA、期待型の有無 | Scar と script / module fixture |
| 一般の名前付き中置 | 関数宣言は受理、closure 変数・関数 capture の変数・関数引数・非関数値は拒否、外側に同名関数があっても fallback しない、通常 call は維持 | Spire / Sigil と fixture |
| 本体・effect | 利用者関数の宣言戻り値と本体を維持。観測可能な副作用を一回実行 | script fixture |
| on グループ | 9名の変数・引数・Pattern 束縛・フィールド・独立した関数・独自 member 定義拒否、標準 trait 実装の成功、標準中置・短絡・prefix・修飾・capture の維持 | Spire / Sigil と fixture |
| Lazy / ErrorKind | 標準だけに特殊規則。同名通常関数では通常評価。Error・ErrorKind 公開禁止を維持 | Sigil / Scar の既存テスト拡張 |
| Pattern | 予約名拒否の維持、束縛 scope、OR / projection、`Regex::matches` の通常式、alias の許可・拒否 | Spire / Sigil と fixture |
| REPL / editor | chunk をまたいだ通常関数の署名・identity と script の一致、tolerant parsing の整合 | Xldr / Spire |

標準の `lib/function.srt` の REPL サンプル、`function_module_helpers_work_end_to_end`、`applicative_apply_operator.srt` を維持する。`reserved_infix_precedence.srt`、`logical_operator_shadowing_short_circuit.srt`、`reserved_member_calls`、Pattern 予約拒否 fixture、ErrorKind fixture は変更する契約に合わせて確認する。

実装時は該当 crate / fixture を先に検証し、最終段階で `rtk cargo nextest run --profile ci --workspace` と `cargo run -- test --quiet --all` を実行する。別エージェントによる最終レビューも行う。文書のみの本ターンではコンパイラテストを実行しない。

変更する正本は `docs/site/callables.md`、`language-reference.md`、`function-operators.md`、`pattern-matching.md`、`error-handling.md`、`docs/dev/Pattern_spec.md`、`Lazy_spec.md`、`diagnostics.md`、関連する `lib/function.srt` / `kernel.srt` / `types/result.srt` / `types/error.srt` / `test.srt` の `@doc`。決定した項目の範囲だけ更新する。

## 今回の調査の限界

ソース・正本文書・既存テストの内容を読み合わせた。ユーザ提示の REPL 再現は今回再実行していない。全 builtin の実行経路を個別に動的監査したものではなく、分類の基準と既知の専用経路を整理した変更仕様である。
