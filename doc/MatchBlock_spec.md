# MatchBlock 仕様書

> 目的: MatchBlock を Surtr における「左辺専用の分解 DSL」として定義し、
> `=` / `=?` / `match` がそれをどう利用するかの外部契約を整理する。
> 本書は将来拡張の正本候補であり、V9 の確定範囲に未導入の事項を含む。

最終更新日: 2026-04-11

---

## 1. 概要

MatchBlock は、Surtr の左辺位置でのみ使われる分解記法である。

- `name = expr` の左辺
- `name =? expr` の左辺
- `match expr { lhs => rhs, ... }` の各 arm 左辺

MatchBlock は通常の式評価とは異なる。
右辺の値を起点に、左辺の構造・literal・Extractor を再帰的にたどり、
束縛・非一致・エラーを決定する。

MatchBlock は次を目的とする。

- 構造分解
- 条件付き束縛
- Extractor による命名された分解 API
- `match` / `=` / `=?` 間で共有される一貫した分解意味論

MatchBlock は次を目的としない。

- 一般式の逆算
- 算術演算の逆向き解釈
- マクロによる evaluator 代替

---

## 2. 基本方針

### 2.1 左辺専用 DSL

MatchBlock は通常の expression language とは別の、左辺専用 DSL として扱う。

- expression と見た目が似る記法があっても、MatchBlock 内では分解器として解釈する
- expression で許可されるものが、そのまま MatchBlock で許可されるとは限らない
- MatchBlock の意味論は「右辺を左辺へ適用して束縛を得る」ことである

### 2.2 構造優先

Surtr の MatchBlock は、構造から値を取り出す用途を中心に設計する。

- constructor pattern
- list pattern
- literal pattern
- variable binding
- Extractor call

一方で、任意の式を逆向きに解釈する設計は採らない。

例:

- `User(name, age)` は許可候補
- `[head, ..tail]` は許可候補
- `SomeExtractor(x)` は許可候補
- `x + 1` は原則として MatchBlock では許可しない
- `x ++ "suffix"` のような演算子 matcher は原則として増やさない

理由:

- マクロ責務が過大化しやすい
- literal / var / value のどれを matcher とみなすかが不透明になる
- 型検査と evaluator の責務境界が崩れやすい

### 2.3 命名された分解 API を優先

複雑な意味は operator matcher ではなく Extractor へ寄せる。

- 分解の意味が名前で読める
- ドメインごとの失敗理由を持てる
- MatchBlock evaluator 自体はシンプルなまま保てる

Surtr では、builtin Extractor と user-defined Extractor を
宣言レベルでは同列に扱うことを重視する。

- user-defined Extractor は `defextractor` 宣言で表す
- builtin Extractor も surface では同じレベルの宣言として見せてよい
- compiler は builtin Extractor の lowering / 型規則 / 実行意味だけを特別扱いする
- これにより、シグネチャ記述と `@@doc` 付与の形式を user-defined と揃えられる

---

## 3. 用語

### 3.1 MatchBlock

左辺位置に現れる分解記法全体。

### 3.2 Matcher

MatchBlock 内で 1 つの値に対して分解を行う要素。

例:

- wildcard
- variable
- literal
- list pattern
- constructor pattern
- Extractor call

### 3.3 Structural Matcher

入力の構造を見るだけで、成功時に部分値を取り出せる matcher。

例:

- `[]`
- `[head, ..tail]`
- `User(name, age)`
- `Enum::Variant(x)`

### 3.4 Extractor

任意ロジックを持つ命名された分解 API。
入力を観察し、部分値の列・非一致・独自エラーを返せる。

### 3.5 Total Pattern

その型に対して失敗しないことが型検査で保証される MatchBlock。

例:

- `x`
- `_`
- 型が確定している値に対する単純束縛

### 3.6 Partial Matcher

入力次第で `NoMatch` を返しうる matcher。

例:

- `[]`
- `[head, ..tail]`
- `User(name, age)`
- `SomeExtractor(x)`

---

## 4. 評価結果モデル

MatchBlock evaluator は概念的に 3 状態を返す。

- `Success`
- `NoMatch`
- `Err`

### 4.1 `Success`

分解に成功し、必要な束縛が得られた状態。

### 4.2 `NoMatch`

その matcher とは合わなかった状態。
これは通常分岐の一部であり、実行不能エラーではない。

典型例:

- `[]` を期待したが非空 list だった
- `User(name, age)` を期待したが別 constructor だった
- Extractor が「この値はこの分解形ではない」と判断した

### 4.3 `Err`

分解処理自体が異常とみなすべき失敗。
`NoMatch` と異なり、通常は次候補へ進まず、そのまま失敗として扱う。

典型例:

- Extractor が独自エラーを返した
- MatchBlock evaluator 自体の内部不整合

### 4.4 内部モデル

surface では special form として扱ってよいが、内部モデルは次の形を基本とする。

```surtr
defenum MatchState {
  Success(Seq),
  NoMatch,
}

MatchResult<$E> = Result<MatchState, $E>
```

ここで `Seq` は MatchBlock evaluator が束縛対象の値列を運ぶための特別扱い型である。

---

## 5. `Seq` と `MatchResult`

### 5.1 `Seq`

`Seq` は MatchBlock evaluator 向けの内部的な値列である。

- heterogenous な束縛候補列を運べる
- `Seq` の要素数と左辺の期待 arity は一致しなければならない
- `Seq` の各要素は再帰的に MatchBlock evaluator へ渡される

`Seq` は一般-purpose tuple を導入するための機能ではない。
分解結果の受け渡し専用とする。

### 5.2 `MatchResult`

`MatchResult` は MatchBlock / Extractor 向けの特別扱いインターフェースである。

- `Success(Seq)` を保持できる
- `NoMatch` を保持できる
- 必要なら独自 error を `Err` で返せる

### 5.3 未指定 error の扱い

Extractor が error 型を明示しない場合、compiler は既定の match error を与えてよい。

ただし `NoMatch` は error へ潰さず、独立状態として保持する。

---

## 6. MatchBlock の許可要素

### 6.1 許可する要素

最低限、以下を許可対象とする。

- variable binding
- wildcard
- literal
- list pattern
- constructor pattern
- alias / as-pattern
- Extractor call

### 6.1.1 head の解決

`Name(...)` 形式は surface 上では同形だが、MatchBlock では symbol kind により解決する。

- 型 constructor として解決できるなら constructor pattern
- Extractor として解決できるなら Extractor call
- builtin / user-defined の別は解決後の実装選択にのみ影響し、surface の宣言形は揃えてよい
- 同一スコープで曖昧になる定義は許可しない

これにより、MatchBlock evaluator は「構造分解」と「任意ロジック分解」を
同じ見た目で扱いつつ、内部責務を分離できる。

### 6.2 variable binding

`x`

- 常に成功する
- 入力値を `x` へ束縛する

### 6.3 wildcard

`_`

- 常に成功する
- 束縛は作らない

### 6.4 literal

- `Int`
- `String`
- `Boolean`

意味:

- 入力値と literal が一致すれば `Success`
- 一致しなければ `NoMatch`

### 6.5 list pattern

- `[]`
- `[head, ..tail]`

意味:

- `[]` は空 list にのみ `Success`
- `[head, ..tail]` は非空 list にのみ `Success`
- 成功時は `head` と `tail` を再帰的に処理する

### 6.6 constructor pattern

例:

- `User(name, age)`
- `Enum::Variant(x)`

意味:

- constructor/tag が一致すれば `Success`
- 一致しなければ `NoMatch`
- payload 各要素を再帰的に MatchBlock として処理する

### 6.7 alias / as-pattern

例:

- `inner @ whole`
- `inner @ whole: Ty`

意味:

- `inner` を先に評価する
- `inner` が `Success` なら、元の入力値を `whole` へ束縛する

### 6.8 Extractor call

例:

- `User(name, age)`
- `uncons(head, tail)`
- `idx(2, value)`
- `Ok(value)`
- `Err(err)`

MatchBlock 文脈では、これらは通常 call ではなく matcher invocation として扱う。

意味:

- 入力値に対して Extractor を適用する
- 結果が `Success(Seq(...))` なら各要素を左辺 subpattern へ再帰適用する
- `NoMatch` ならその matcher は失敗
- `Err` ならそのまま error を返す

---

## 7. 再帰評価規則

### 7.1 基本規則

MatchBlock evaluator は外側から内側へ再帰的にたどる。

例:

```surtr
User(name, age)
```

では次の順で処理する。

1. 入力値が `User` constructor と一致するかを見る
2. 第1 payload を `name` へ適用する
3. 第2 payload を `age` へ適用する
4. 全部成功なら全体 `Success`

### 7.2 合成規則

子 matcher の結果は次の優先度で親へ伝播する。

1. いずれかが `Err` なら全体 `Err`
2. `Err` はなく、いずれかが `NoMatch` なら全体 `NoMatch`
3. 全部 `Success` なら全体 `Success`

### 7.3 束縛の統合

複数の子 matcher が作る束縛は全体で統合する。

- 同名束縛の扱いは別途固定する
- 非線形 pattern を許可する場合は同値性検査規則が必要
- V1 では同名再束縛を禁止してもよい

---

## 8. `=` / `=?` / `match` での扱い

### 8.1 `=`

`=` は total な MatchBlock 向けとする。

- `Success` なら束縛成功
- `NoMatch` が起こりうる MatchBlock は原則として不許可
- もしくは型検査で total 性が証明できる場合のみ許可
- `Err` は compile error または runtime failure とする

狙い:

- 通常束縛は「失敗しない」ことを保つ

### 8.2 `=?`

`=?` は失敗しうる MatchBlock を手続きレベルで扱うための構文である。

- `Success` なら束縛して続行
- `NoMatch` は error へ変換して伝播してよい
- `Err` はそのまま伝播する

`=?` は `Result` 伝播を重視する Surtr の方向性に合わせ、
「手続きの途中で分解失敗したら中断したい」用途を担う。

### 8.3 `match`

`match` は分岐用途であり、`NoMatch` を次 arm へ流せる。

- arm が `Success` ならその arm を採用
- arm が `NoMatch` なら次 arm を試す
- arm が `Err` なら `match` 全体を失敗させる

この規則により、`NoMatch` と `Err` を明確に分ける。

---

## 9. 網羅性と default arm

### 9.1 構造網羅

構造だけを見れば網羅的な pattern がある。

例:

- `List<T>` に対する `[]` と `[head, ..tail]`
- `defenum` に対する全 variant 列挙

### 9.2 意味網羅

ただし、構造的に網羅でも内部に partial matcher があると、
意味的には total と限らない。

例:

```surtr
match xs {
  [] => a,
  [User(name), ..tail] => b,
  _ => c,
}
```

`[User(name), ..tail]` は list としては非空を覆うが、
先頭要素に対する `User(...)` が `NoMatch` を返しうる。

### 9.3 default arm 要求

MatchBlock 内に partial matcher が含まれる場合、
その `match` が意味網羅を証明できないなら default arm を要求してよい。

このルールにより、

- 純構造 pattern のみなら従来どおりの使い勝手を維持する
- Extractor や partial matcher が混ざるときだけ fallback を明示させる

構造網羅だけで total と判定してよい例:

- `List<T>` に対する `[]` と `[head, ..tail]`
- `defenum` に対する全 variant 列挙

構造網羅だけでは total と判定できない例:

- 非空 arm の内部で Extractor を使う
- variant payload 内で partial matcher を使う
- literal / Extractor の組み合わせにより一部入力が `NoMatch` になりうる

### 9.4 default arm の責務

default arm は `NoMatch` の受け皿である。

- `NoMatch` のみを吸収する
- `Err` は吸収せず伝播する

---

## 10. Extractor 契約

### 10.1 基本形

```surtr
defextractor Name(self) -> MatchResult<$E>
```

Extractor は次を返せる。

- `Success(Seq(...))`
- `NoMatch`
- `Err(e)`

### 10.2 責務

Extractor の責務は次に限定する。

- 入力値の検査
- 必要なら部分値の抽出
- `NoMatch` と `Err` の選択

Extractor は、抽出後の再帰束縛には関与しない。
抽出後の subpattern 適用は compiler 側の MatchBlock evaluator が担う。

### 10.3 `NoMatch` を返すべきケース

次のような「その形ではない」ケースは `NoMatch` が基本である。

- 空/非空の不一致
- constructor/tag の不一致
- 形は合うが、その Extractor の定義意図には合わない

### 10.4 `Err` を返すべきケース

次のような「通常分岐ではなく明示的失敗として伝えたい」ケースは `Err` を使ってよい。

- ドメイン固有の異常
- 呼び出し側へ見せたい具体的理由
- `NoMatch` では埋もれてほしくない失敗

### 10.5 標準 Extractor

標準 Extractor は最小限に留める。

- 言語組み込みとして必須なもののみ
- 複雑な分解は user-defined Extractor へ寄せる

理由:

- operator matcher を増やしすぎない
- 分解意味を名前で読めるようにする
- MatchBlock evaluator の責務を増やしすぎない

### 10.6 builtin Extractor 宣言

builtin Extractor は code-level 実装の有無とは別に、宣言レベルでは
user-defined Extractor と同列に置けることを重視する。

- builtin Extractor はシグネチャ記述と `@@doc` のために宣言できる
- compiler はその宣言を読みつつ、意味論は builtin matcher として特別扱いしてよい
- user-defined Extractor も同じ `defextractor` レベルで宣言できる

例:

- `Ok`
- `Err`
- `uncons`

これらは surface では Extractor 宣言として表せても、runtime 実装や lowering は
compiler-special のままでよい。

---

## 11. total / partial の判定

### 11.1 total とみなしてよいもの

少なくとも次は total とみなしやすい。

- `x`
- `_`
- 型検査で常に成立すると証明できる単純束縛

### 11.2 partial とみなすもの

少なくとも次は partial とみなす。

- literal matcher
- list matcher
- constructor matcher
- Extractor call

### 11.3 伝播規則

- total のみで構成される MatchBlock は default 不要
- 構造網羅だけで意味網羅まで証明できる場合も default 不要
- partial matcher を含み、意味網羅を証明できない MatchBlock は default 必要と判定してよい

---

## 12. 診断

MatchBlock 導入時は少なくとも次の診断を持つ。

- Extractor が見つからない
- Extractor の返り値が MatchBlock 契約に合わない
- `Seq` の arity が左辺 subpattern 数と一致しない
- `=` で partial MatchBlock を使っている
- `match` に partial matcher があるのに default arm がない
- `Err` を default arm で吸収しようとしている
- duplicate binding in MatchBlock

---

## 13. 実装境界

### 13.1 compiler の責務

- MatchBlock を専用 IR へ lower する
- 再帰評価規則を実装する
- `Success / NoMatch / Err` の合成を実装する
- `=` / `=?` / `match` ごとの差分を適用する

### 13.2 Extractor の責務

- 入力から `Success / NoMatch / Err` を返す
- 必要なら値列を `Seq` で返す

### 13.3 macro の責務

macro は MatchBlock evaluator の意味論そのものを担わない。

- token 変形
- sugar 展開

までに留める。

---

## 14. 今後の保留事項

- 既束縛変数の再出現を比較として扱うか
- pin 相当記法を導入するか
- `MatchResult` / `Seq` の surface syntax をどこまで見せるか
- `=` で許す total 判定をどこまで自動化するか
- `match` の exhaustiveness 診断と partial matcher 診断をどう統合するか
- REPL 表示で MatchBlock 失敗をどう見せるか

---

*Surtr — 既存の妥協を、型で焼き払う。*
