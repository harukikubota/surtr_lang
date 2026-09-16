# Extractor 返り値契約の更改案（未採用）

## 1. 状態と入力

- 状態: 未採用・保留。本文は現行仕様ではない。
- 入力: Extractor の失敗時に定義側が Error と message を返し、SafeBind がその Error を保持できるようにする。
- level: 4。Extractor の型規則、pattern の評価規則、SafeBind の failure target、Scar / Forge 間の契約を変更する。
- 現行実装・正本: `Option<T>` の `Some` / `None` 契約。`None` は SafeBind で共通
  `PatternMismatch` Error になり、Result effect では保持、Alternative route では破棄する。
  実装依頼時は本案ではなく `doc/要件定義v9.md`、`docs/dev/diagnostics.md`、
  `docs/site/extractors.md` を使う。本案を再開するには新たな仕様決定を必要とする。
- 過去実装の参照点: `3990b2f9c4727d9bc925048835faee92cf9d8517^`。削除直前の型制約、Extractor body 制約、completion、typed pattern、lowering を復元時の比較対象にする。

本案は実装済みのdo / MonadT契約およびGenerator再設計から独立して保留する。再開する場合は、実装時点のconsumerと現行正本を再監査する。

## 2. 変更理由

過去の `MatchResult(Success, NoMatch, Err)` は、Extractor 自身に `NoMatch` と `Err` の区別を要求した。この三値では、不正な pattern や分解失敗をどちらへ分類するかが Extractor ごとに揺れ、`match` が `Err` を次の arm へ送るべきか評価を中断すべきかも一意に決められなかった。

Extractor の責務は「分解に成功したか」と「失敗時にどの Error を返すか」に限定する。失敗した Error を観測するか破棄するかは pattern consumer の責務とする。これにより Extractor は二状態だけを返し、`match` と SafeBind は同じ結果に対してそれぞれの制御フローを明示的に選べる。

## 3. 採用する `MatchResult`

Extractor は compiler-managed な `MatchResult` を返す。状態は次の二つだけである。

- `MatchResult::OK(payload)`: 分解成功。`payload` を Extractor pattern の子 pattern へ渡す。
- `MatchResult::Err(error)`: 分解失敗。`error` は Extractor 定義側が構築した Error である。

`NoMatch` は持たない。Extractor の不成功はすべて `Err(error)` とし、consumer が Error を保持するか破棄するかを決める。

`MatchResult` の正本定義は `lib/types/special_types.srt` に置く。過去実装の compiler-special な型・constructor 制約を再利用し、variant 集合だけを `OK / Err` の二値へ変更する。仕様上の canonical surface は次とする。

```surtr
@builtin
defenum MatchResult<$Value> {
  OK($Value),
  Err(Error),
}
```

Extractor signature では `MatchResult<$Value, Error>` を canonical 表示とする。過去実装と同じく `MatchResult<$Value>` も入力時の短縮表記として受理し、両者を同じ型として扱う。Error 側は abstract `Error` に固定し、compiler 内部では成功 payload 型だけを型引数として保持してよい。表示・文書・標準定義は二引数形へ統一する。任意の第二型引数は受理せず、`MatchResult<$Value, Int>` 等は拒否する。

```surtr
deferror InvalidDecimal(input: String) {
  "invalid decimal"
}

defmod Decimal {
  defextractor decimal(self: String) -> MatchResult<Int, Error> {
    # 成功時: MatchResult::OK(value)
    # 失敗時: MatchResult::Err(InvalidDecimal(self))
  }
}
```

上記は未実装 surface の仕様例であり、現時点の REPL で動作する例ではない。

### 3.1 `Result` との境界

一般の `Result<T, E>` を Extractor の戻り値として流用しない。

- `Result` はユーザが通常値として構築・保持・分解・受け渡しできる。
- `MatchResult` は Extractor 定義と compiler-known pattern consumer の間だけに存在する。
- `Result` / `Option` と `MatchResult` の暗黙変換は行わない。
- variant 名や runtime tag が似ていることを根拠に互換 fallback を設けない。

`defextractor` と `@builtin defextractor` の戻り型は `MatchResult<$Value, Error>` に限定する。旧 `Option<T>`、一般の `Result<T, E>`、その他の型は Extractor の戻り型として拒否する。

### 3.2 compiler-managed の意味

`MatchResult` は一般のユーザ値ではない。

- 型名は `defextractor` / `@builtin defextractor` の戻り型位置と Extractor 本文内の型位置でのみ書ける。
- `MatchResult::OK(...)` / `MatchResult::Err(...)` は Extractor body 内でのみ構築できる。
- Extractor body の `if` / `match` 等の各経路も、最終的に同じ `MatchResult` expected typeへ一致させる。
- 通常の変数、引数、field、collection 要素、通常関数や closure の戻り値として保持できない。
- `MatchResult` 自体を通常の pattern で分解できず、利用者が `Err` payload を束縛する経路を追加しない。
- capture、Trait 実装、operator dispatch、`From` / `TryFrom` の対象にしない。
- REPL completion、一般の型候補、通常の API documentation の値型候補として提示しない。

この制約は過去実装にあった `in_extractor_body` 相当の明示的な文脈判定を使い、型名や message の文字列解析で推測しない。

### 3.3 payload と Error

- Extractor の入力はちょうど一つである。
- `OK(payload)` の payload は一値、または複数の子 pattern に対応する tuple である。
- 子 pattern の数と tuple arity は静的に一致させ、arity 検査後にだけ対応付ける。
- `Err(error)` には具象 `deferror` 値を入れる。自由な String や任意の値を Error として受理しない。
- Error の kind、message、location、cause は既存の Error / RichError 契約に従う。kind と message の起点は Extractor 定義側が選ぶ。
- compiler が一律の `PatternMismatch` を合成して Extractor 定義側の Error を上書きしない。
- 標準 builtin Extractor も同じ契約を持つ。たとえば空の list / string に対する `uncons` は、標準定義が選んだ具象 Error を返す。型名や入力型から Forge が Error を合成しない。

## 4. consumer ごとの評価規則

Extractor occurrence は到達したとき一回だけ評価する。成功検査と束縛のために再実行しない。

| consumer | `OK(payload)` | `Err(error)` |
|---|---|---|
| `match` arm | 子 pattern の照合へ進む | Error を破棄し、現在の arm を不一致として次の arm へ進む |
| `if_let` | 子 pattern も成功すれば then branch | Error を破棄し、else branch |
| `if_let_then` | 子 pattern も成功すれば branch を評価 | Error を破棄し、branch を評価せず `Unit` |
| `is_match` | 子 pattern も成功すれば `True` | Error を破棄して `False` |
| SafeBind `=?` | 子 pattern の照合へ進み、すべて成功すれば束縛して続行 | Error を保持し、現在の SafeBind failure target へ渡す |

`match`、`if_let`、`if_let_then`、`is_match` は Error の kind、message、location、cause を表示・伝播・記録しない。これらは pattern の成否だけを観測する consumer である。

SafeBind だけが `MatchResult::Err` の Error を観測する。通常関数内では enclosing `Result` の failure target、REPL top-level では既存の診断表示とセッション継続境界へ接続する。do 内の SafeBind は do 仕様が選んだ明示的 failure target を使うが、Extractor の評価規則を再実装しない。

`OK(payload)` の後で子 pattern が不一致になった場合は Extractor の `Err` ではない。literal、constructor、list shape 等の既存 pattern failure 規則を使う。nested Extractor が `Err` を返した場合、SafeBind では実際に失敗した nested Extractor の Error を保持し、非 SafeBind consumer では同じ Error を破棄する。

### 4.1 SafeBind の Error 保持

RHS が canonical `Result` の場合は、既存仕様どおり外側一段だけを射影する。RHS 自体の `Err(error)` と、射影後の pattern に含まれる Extractor の `MatchResult::Err(error)` は起点の異なる failure だが、同じ明示的 SafeBind failure target へ接続する。どちらも元の Error 値を保持し、message を再構成しない。

SafeBind の戻り先が Error を保持する文脈では Extractor の Error をそのまま渡す。Alternative の `empty` 等、呼び出し側仕様が failure detail を破棄すると明示した文脈では、その外側 policy に従う。この場合も Extractor の `Err` を別の MatchResult 状態へ変更しない。

## 5. 呼び出し可能位置

Extractor は引き続き pattern / MatchBlock 位置でのみ呼び出せる。通常の ExprBlock、RHS、通常関数の引数位置から Extractor 名を直接呼び出せない。これにより、Extractor の成功 payload からだけ変数束縛を導入する現在の scope 規則を維持する。

`if_let`、`if_let_then`、`is_match` は通常の関数呼び出しに見えるが、pattern 引数を持つ compiler-known consumer である。その pattern 内では Extractor を評価できる。この限定を、Extractor 自体を第一級関数として呼び出せる根拠にしない。

total pattern だけを許す通常 Bind `=` は Extractor pattern を引き続き拒否する。Extractor が実装上常に `OK` を返すかを解析して total とみなす特例は設けない。

## 6. 静的拒否と内部契約違反

次は compile error とする。

- Extractor の戻り型が `MatchResult<T, Error>` / `MatchResult<T>` ではなく、旧 `Option<T>`、一般の `Result<T, E>`、またはその他の型である。
- `MatchResult<T, Error>` を許可された Extractor 文脈以外の型位置で使う。
- `MatchResult::OK` / `MatchResult::Err` を Extractor body 外で構築する。
- `OK` payload 型、tuple arity、子 pattern 型、Extractor 入力型が一致しない。
- `Err` payload が Error でない。
- Extractor を通常関数として直接呼び出す、capture する、値として受け渡す。

型検査後に未知の variant / tag、不正な field 数、runtime representation の不一致が現れた場合は compiler / runtime の内部契約違反として即時 failure にする。`Err`、pattern 不一致、次の match arm、SafeBind の利用者 Error へ fallback しない。

## 7. 旧契約からの移行

実装は旧 `Option<T>` 経路を置換し、互換経路として残さない。

1. `lib/types/special_types.srt` に compiler-special `MatchResult` の正本を復元し、`OK / Err` の二 variant だけを定義する。
2. 過去実装の Extractor 文脈制約、型引数検査、completion 非表示、constructor 利用制限を現在の型・metadata 基盤へ移植する。
3. `defextractor` / `@builtin defextractor` の signature 検査を `MatchResult` 限定へ変更する。
4. typed pattern contract に成功 payload 型、`OK / Err` の canonical tag、source origin、consumer の discard / preserve policy を保持する。
5. pattern lowering を success と failure の二分岐にし、SafeBind だけが Error payload を failure target へ渡す。
6. `Option::Some` / `Option::None` を読む Extractor 専用 lowering、no-match tag、未使用の三状態 `err_tag` 等の旧表現を削除する。
7. user-defined / builtin Extractor を `MatchResult::OK` / `MatchResult::Err` へ移行する。失敗 Error は各定義が明示し、compiler が Extractor 名や carrier 名から推測しない。
8. 旧 `Option<T>` Extractor は拒否テストとして残し、現行仕様に合う診断へ更新する。

一般の `Option` と `Result` の値、Trait 実装、変換 API は本変更の対象外である。

## 8. 成功・拒否境界

実装時は少なくとも次を固定する。

- user-defined Extractor の `OK` が単値と tuple payload を正しく束縛する。
- `match` の `Err` が次の arm へ進み、Error を表示も伝播もしない。
- `if_let` / `if_let_then` / `is_match` が `Err` をそれぞれ else / no-op / `False` として扱う。
- SafeBind が Extractor 定義側の Error kind / message / location / cause を保持して早期 return する。
- nested Extractor は到達順に一回ずつ評価し、最初に到達した失敗以降の子 pattern を評価しない。
- `OK` 後の子 pattern 不一致と Extractor 自身の `Err` を SafeBind で区別する。
- builtin `uncons` の成功と空入力 Error を list / string の両方で検証する。
- `Option<T>` / `Result<T, E>` を返す旧 Extractor、MatchResult の通常値利用、直接呼び出し、payload / arity / input mismatch を拒否する。
- 不正 tag / representation は内部 failure となり、consumer の discard policy へ入らない。
- match / pattern exhaustiveness は Extractor を partial pattern として扱い、定義本体の解析による totality 推測を行わない。

## 9. 変更対象と検証

責務ごとの主な変更対象は次のとおりである。

- `lib/types/special_types.srt`: `MatchResult` の canonical 宣言と `@doc`。
- Spire / Sigil: Extractor 戻り型位置、constructor の利用位置、直接呼び出し禁止、source origin。
- Scar: compiler-special type identity、payload / arity、consumer policy、SafeBind failure target。
- Forge: Extractor の一回評価、二分岐、match 系の discard、SafeBind の Error 保持、旧 Option lowering の削除。
- Eldr / Sindr / Xldr loader: canonical type / tag / metadata、stdlib 読み込み順、builtin 実装の fail-closed 対応。
- Rune / Xldr: SafeBind Error の表示・早期終了・REPL 継続、非 SafeBind consumer での非表示。

実装は TDD で進め、変更した最小責務から検証する。Scar の型・arity、Forge の単一評価と分岐、Rune の script / module fixture、Xldr の REPL 継続を確認する。level 4 の完了条件として、最終 revision で次を実行し、別エージェントによる独立レビューを行う。

```sh
rtk cargo nextest run --profile ci --workspace
cargo run -- test --quiet --all
```

正本移管時は少なくとも `doc/要件定義v9.md` の builtin type / Error / SafeBind / Extractor 節、`docs/site/extractors.md` と pattern consumer の各ページ、`docs/dev/diagnostics.md`、`docs/dev/テスト方針.md`、標準 Extractor の `@doc`、N06 / do の Extractor 前提を同じ revision で整合させる。実装前の現行説明だけを先に将来契約へ書き換えない。

## 10. 受け入れ条件

1. Extractor の結果状態は `OK(payload)` / `Err(error)` の二つだけで、MatchResult 固有の `NoMatch` variant / tag / lowering 経路は残らない。通常 pattern の不一致分岐は維持する。
2. `MatchResult` は Extractor 戻り値契約に閉じ、一般のユーザ値として構築・保持・分解・受け渡しできない。
3. 非 SafeBind consumer は `Err` を不一致として破棄し、SafeBind だけが同じ Error を failure target へ渡す。
4. Error の内容は Extractor 定義側が決め、compiler が自由形式 message、Extractor 名、carrier 名から再構成しない。
5. payload / arity / input の型境界、単一評価、nested pattern の短絡、内部契約違反の fail-closed 処理を検証する。
6. 旧 Option-returning Extractor と no-match lowering を削除し、暗黙変換や互換 fallback を残さない。
7. 正本文書、利用者向け文書、標準 `@doc`、テスト、実装が同じ契約を示す。
8. level 4 の全体検証と最終差分に対する独立レビューが完了する。
