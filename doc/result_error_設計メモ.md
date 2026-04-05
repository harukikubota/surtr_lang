# Surtr Result / Error 設計整理メモ

## 目的

本メモは、Surtr における `Result` / `Error` 設計について、言語思想・型システム・構文・型検査・ランタイム責務の観点から体系的に整理したものである。  
とくに以下を確定対象とする。

- `Result<T>` と `Result<T, E>` の位置づけ
- `Error` と具象エラー型の役割分担
- `deferror` の責務
- コード上の簡潔さと契約情報の可視性の両立
- 型検査器 `scar` がどこまで責任を持つか

---

## 1. 基本思想

### 1.1 失敗は `Result` で扱う

Surtr における失敗の扱いは、`Error` の分解や列挙ではなく、**`Result` を通じた成功 / 失敗の計算表現**として扱う。

つまり、Either 志向とは `Error` に対するものではなく、**`Result<T, E>` に対するアプローチスタンス**である。

- `Ok(T)` は成功
- `Err(E)` は失敗
- 呼び出し側はこれを伝播・停止・変換する
- `Error` の内部構造を言語機能で操作することは中心ではない

### 1.2 設計目標

この設計が目指すものは次の通り。

- コード上の失敗処理をシンプルに保つ
- 処理系に複雑なエラー階層や enum 的分解機構を持ち込まない
- 失敗契約はシグネチャに表現できるようにする
- ランタイム診断とコード上の制御構文を分離する

## 2. `Result<T, E>` の位置づけ

### 2.1 内部モデル

型理論上、Surtr の失敗型は内部的には次の形を持つ。

```surtr
Result<T, E>
```

ここで `E` は具象エラー型を表す。  
ただし、この `E` はコード上で広く操作されるものではなく、主に以下のために存在する。

- 関数契約の明示
- ドキュメント性の向上
- 型検査器による整合性確認
- 将来の補助表示への利用

### 2.2 コード上の主表記

コード上では、失敗計算は原則として次の表記で扱う。

```surtr
Result<T>
```

つまり、コード上ではエラー側を抽象化し、利用者が普段触る表記は `Result<T>` を基本とする。  
この方針により、ユーザコードが具象エラーの enum 的構造へ引き寄せられることを防ぐ。

### 2.3 シグネチャでのみ `E` を許可する

具象エラー型 `E` は、関数シグネチャに限って記述可能とする。

例:

```surtr
def parse_int(text: String) -> Result<Int, ParseError>
```

ここでの `E` は、利用側が関数契約を読むための情報である。

## 3. `Error` と具象エラー型の関係

### 3.1 `Error` は抽象型

`Error` はコード上で扱う抽象的な失敗値の型である。

- `Error` は抽象型である
- `Error` 自体は直接構築できない
- `Error` は言語上の分解対象ではない
- `Error` はランタイム診断や失敗表現の共通基盤である

### 3.2 具象エラー型は `deferror` で定義する

具体的なエラー値は `deferror` により定義する。

例:

```surtr
deferror ParseError { "Failed to parse integer." }
deferror NoneError { "None Value." }
```

これにより、`ParseError` や `NoneError` は具象エラー型として存在できる。

### 3.3 具象エラー型は平坦である

具象エラー型同士には継承や階層を持たせない。

- `ParseError <: Error` のような概念は表現上の抽象関係としてのみ扱う
- 具象エラー間に親子関係は持たない
- enum の variant tree 的な構造は持たない
- エラーの構造化はランタイム側の責務とし、型システムには持ち込まない

## 4. `deferror` の責務

### 4.1 `deferror` はアトミックなエラー生成子

`deferror` は、アトミックなエラー値を生成するためのコンストラクタ定義である。

その責務は以下に限定する。

- 具象エラー型を定義する
- エラー値を生成する
- 文字列表現や診断表示用の基本情報を与える

### 4.2 `deferror` に持たせないもの

`deferror` に以下の責務は持たせない。

- 他のエラー型の内包
- エラーの和型的構造
- エラー合成の知識
- cause 構造の静的型表現
- エラーの分類木

つまり、`deferror` 自体に「エラー処理の知識」は持たせない。

### 4.3 cause はランタイム責務

cause によるエラー連鎖や構造化はありうるが、それはランタイム管理の責務とする。

- コード上で `E1`, `E2` の構造を追わせない
- 型システムや構文に持ち込まない
- ランタイムで診断情報として保持する

## 5. `Err(term)` のルール

### 5.1 許可される値

`Err(term)` の `term` は、具象エラー値のみを受け取る。  
つまり、`term` は `deferror` により定義されたエラーコンストラクタの結果でなければならない。

例:

```surtr
Err(ParseError)
Err(NoneError)
```

### 5.2 禁止される値

抽象 `Error` を直接 `Err` に入れることは禁止する。

```surtr
Err(Error)   # コンパイルエラー
```

これにより、抽象型の直接構築を防ぎ、`Error` をあくまで共通抽象として保つ。

## 6. `match` における扱い

### 6.1 `match` は `Result` を見る

Surtr の `match` は `Result` に対しては次を扱える。

```surtr
Ok(x)
Err(e)
```

ここで重要なのは、`match` が見ているのは `Result` の成功/失敗構造であって、`Error` の具象型構造ではないという点である。

### 6.2 具象エラー分解は提供しない

以下のような具象エラー分解は提供しない。

```surtr
match err {
  ParseError => ...
  NoneError => ...
}
```

あるいは

```surtr
match result {
  Err(ParseError) => ...
}
```

のような具体パターン分岐も提供しない。

### 6.3 理由

これは、静的に具象エラーの候補が列挙できることと、その差異に基づいてコード上の分岐権限を与えることは別問題であるためである。

Surtr では

- 具象エラー型は契約ラベル
- `Result` は成功/失敗の計算表現
- `Error` は抽象失敗値

という役割分担を守る。

## 7. `E` を書ける場所

### 7.1 許可する場所

`Result<T, E>` を構文上許可するのは、原則として関数シグネチャの戻り値位置のみとする。

対象:

- 通常関数の戻り値
- trait / 抽象インターフェースの戻り値
- builtin / 外部宣言の戻り値

例:

```surtr
def parse_int(text: String) -> Result<Int, ParseError>
```

### 7.2 不許可とする場所

次の場所では `Result<T, E>` を許可しない。

変数注釈:

```surtr
value: Result<Int, ParseError> = parse_int(text)   # 不許可
value: Result<Int> = parse_int(text)               # 許可
```

関数引数:

```surtr
def handle(value: Result<Int, ParseError>) -> String   # 不許可
```

クロージャ注釈 / 関数型内部:

```surtr
handler: (String -> Result<Int, ParseError>)   # 不許可
```

struct / record field:

```surtr
defstruct Response {
  value: Result<Int, ParseError>   # 不許可
}
```

ただし、抽象側の `Result<T>` は許可する。

```surtr
defstruct Response {
  value: Result<Int>   # 許可
}
```

type alias / NewType:

`Result<T, E>` を構造の一部として型エイリアスや NewType に閉じ込めることも許可しない。

理由:

- 具象エラー型がコード上の型検査対象へ広がる
- 戻り値専用でしか意味を持たず、言語機能として弱い
- ルールが部分的になり、理解コストが上がる

`deferror` field:

`deferror` 自身のフィールドに `Result<T, E>` を持たせることも許可しない。

理由:

- `deferror` にエラー構造の知識を持たせないため
- cause 構造はランタイム責務に留めるため

## 8. trait / 抽象インターフェースとの整合

trait や抽象インターフェースは最終的に関数宣言へ落ちる前提である。  
そのため、戻り値シグネチャに関するルールは通常関数と同じでなければならない。

もし trait では `Result<T, E>` を書けるが、実装関数では書けない、あるいはその逆、というような差異があると、

- シグネチャの意味が揺らぐ
- 抽象インターフェースから外れる
- 利用者に混乱を与える

よって、シグネチャなら具象 `E` を書けるというルールで統一する。

## 9. scar の責務

### 9.1 scar が保証すること

型検査器 scar は、少なくとも次を保証する。

1. `E` の存在チェック  
   `Result<T, E>` に現れる `E` が登録済み型であることを確認する。
2. `E` が `deferror` 起源であること  
   `E` は `deferror` により定義された型でなければならない。  
   `defstruct` / `defrecord` / 基本型などは `E` に使えない。
3. `Err(term)` の妥当性  
   `Err(term)` の `term` が具象エラー値であることを確認する。  
   `Err(ParseError)` は許可  
   `Err(Error)` は拒否
4. 非許可文脈での `Result<T, E>` を拒否  
   関数戻り値以外で `Result<T, E>` が書かれた場合、型エラーとする。

### 9.2 scar が持ちすぎない責務

scar に次の責務は持たせない。

- 具象エラーの完全列挙証明
- 全分岐の返却集合の完全解析
- 具象エラー階層の推論
- concrete error matching の整合証明
- cause 構造の型追跡

つまり、scar は契約の妥当性と局所整合性を担当し、それ以上の高度解析は行わない。

## 10. 表示ルール

### 10.1 ソースコード上の主表記

コード上では、`Result<T>` を主表記とする。  
これにより、利用者は日常的なコードにおいて具象エラー型を追わずに済む。

### 10.2 シグネチャでの表示

関数シグネチャに限り、`Result<T, E>` を書ける。

例:

```surtr
def parse_int(text: String) -> Result<Int, ParseError>
```

これは契約表示であり、式レベルの操作権限を意味しない。

### 10.3 補助表示

REPL や LSP では、必要に応じて補助情報として具象エラー契約を表示できる。  
ただし、その詳細仕様は今後の検討課題とする。

現時点の方針は以下。

- コード上の主表記は `Result<T>`
- シグネチャでは `Result<T, E>`
- REPL / LSP では必要に応じて補助的に契約情報を示せる余地を残す

## 11. 言語思想との整合

この設計は、Surtr の次の思想と整合する。

### 11.1 型で契約を見せる

具象エラー型 `E` は、関数が何を失敗として返しうるかを明示するために使う。

### 11.2 コード上の制御は単純にする

コード上では `Result<T>` を扱い、`Ok` / `Err` による伝播・停止・回復に集中させる。

### 11.3 ランタイムへ責務を押し込む

詳細なエラー構造や cause 連鎖はランタイムが保持し、コード上の型機構や分岐構文には広げない。

### 11.4 enum 的世界観へ寄せない

具象エラー型をコード上の分岐材料にしないことで、Rust 的な enum variant 主導のエラー設計とは異なる位置を保つ。

## 12. 例

### 12.1 契約を明示する関数

```surtr
def parse_int(text: String) -> Result<Int, ParseError> {
  ...
}
```

### 12.2 呼び出し側は抽象的に扱う

```surtr
value: Result<Int> = parse_int("123")
```

### 12.3 `=?` による伝播

```surtr
def load_age(text: String) -> Result<Int, ParseError> {
  age =? parse_int(text)
  Ok(age)
}
```

ここで利用者が意識するのは `Result` の成功 / 失敗であり、`ParseError` の分解ではない。

### 12.4 禁止例

変数で具象 `E`:

```surtr
value: Result<Int, ParseError> = parse_int("123")   # 禁止
```

field で具象 `E`:

```surtr
defstruct Response {
  value: Result<Int, ParseError>   # 禁止
}
```

抽象 `Error` の直接注入:

```surtr
Err(Error)   # 禁止
```

## 13. 最終整理

Surtr における Result / Error 設計は、次の一文で要約できる。

> Surtr では失敗は Result を通じて扱い、具象エラー型は関数契約のためにのみ可視化する。  
> コード上の制御は常に抽象化された失敗として扱い、詳細なエラー構造はランタイムに委ねる。

この設計により、

- コードはシンプル
- シグネチャは説明的
- 型検査器は過剰に肥大化しない
- ランタイムは豊富な診断情報を持てる

というバランスを取る。

## 14. 仕様ルール（簡約版）

本章は、実装・レビュー・将来の仕様拡張時に参照しやすいよう、前章までの内容を規則形式で再整理したものである。

### 14.1 `Result`

1. Surtr の失敗計算は内部的に `Result<T, E>` を持つ。
2. コード上の主表記は `Result<T>` とする。
3. `Result<T, E>` は関数シグネチャの戻り値位置に限って記述できる。
4. `E` は `deferror` により定義された具象エラー型に限る。
5. `E` は関数契約・文書化・型検査補助のための情報であり、通常の式レベル制御に露出しない。

---

### 14.2 `Error`

1. `Error` は抽象型である。
2. `Error` はコード上の失敗値の共通抽象である。
3. `Error` 自体は直接構築できない。
4. `Error` は具象エラー型の分解対象ではない。
5. `Error` の詳細構造はランタイムが管理する。

---

### 14.3 `deferror`

1. `deferror` は具象エラー型を定義する。
2. `deferror` はアトミックなエラー値生成子である。
3. `deferror` はエラー階層を持たない。
4. `deferror` は他のエラー型を型システム上で内包しない。
5. `deferror` は cause などのエラー連鎖構造を型として表現しない。

---

### 14.4 `Err`

1. `Err(term)` の `term` は具象エラー値でなければならない。
2. `Err(Error)` はコンパイルエラーとする。
3. `Err` は抽象 `Error` の直接注入手段ではない。
4. `Err` は失敗値の構築子であり、エラーの分解子ではない。

---

### 14.5 `match`

1. `match` は `Result` の成功 / 失敗構造を扱う。
2. `match result { Ok(x) => ..., Err(e) => ... }` は許可する。
3. `Err(e)` の `e` は抽象 `Error` としてのみ扱う。
4. 具象エラー型でのパターン分解は提供しない。
5. 具象エラー候補の静的列挙可能性は、分解権限を意味しない。

## 15. 許可 / 不許可一覧

### 15.1 許可する構文

関数戻り値:

```surtr
def parse_int(text: String) -> Result<Int, ParseError>
```

trait / 抽象インターフェース戻り値:

```surtr
trait Parser {
  def parse(text: String) -> Result<Int, ParseError>
}
```

builtin / 外部宣言戻り値:

```surtr
@@builtin def read_file(path: String) -> Result<String, IoError>
```

フィールド内の抽象 `Result<T>`:

```surtr
defstruct Response {
  value: Result<Int>
}
```

### 15.2 不許可とする構文

変数注釈での具象 `E`:

```surtr
value: Result<Int, ParseError> = parse_int(text)
```

関数引数での具象 `E`:

```surtr
def handle(value: Result<Int, ParseError>) -> Unit
```

関数型内部での具象 `E`:

```surtr
handler: (String -> Result<Int, ParseError>)
```

field での具象 `E`:

```surtr
defstruct Response {
  value: Result<Int, ParseError>
}
```

型エイリアス / NewType での具象 `E`:

```surtr
type ParseResult = Result<Int, ParseError>
```

`deferror` field 内での `Result<T, E>`:

```surtr
deferror Wrapped(inner: Result<Int, ParseError>) { ... }
```

抽象 `Error` の直接構築:

```surtr
Err(Error)
```

具象エラー分解:

```surtr
match result {
  Err(ParseError) => ...
}
```

## 16. 型検査方針

### 16.1 scar の受理規則

scar は以下の規則で `Result<T, E>` を受理する。

規則1: 文脈制限  
`Result<T, E>` は関数戻り値型としてのみ受理する。  
それ以外の文脈で現れた場合は型エラーとする。

規則2: `E` の存在  
`E` は型環境上に登録済みでなければならない。

規則3: `E` の起源  
`E` は `deferror` 起源でなければならない。  
`defstruct`, `defrecord`, 基本型, 任意のユーザ型は許可しない。

規則4: `Err(term)`  
term が具象エラー値であることを検査する。

規則5: `Error` の直接構築禁止  
抽象 `Error` を値として `Err` に渡すことを禁止する。

### 16.2 scar の責務外

以下は V9 では scar の責務外とする。

- 関数全体の具象エラー集合の完全証明
- 分岐ごとの到達可能エラー全列挙
- 再帰関数に対する fixpoint 解析
- 関数合成時の高度な error union 推論
- エラー階層 / 継承 / サブタイプ解析
- cause 連鎖の静的検査
- 具象エラー分解機能の整合性管理

### 16.3 将来的に行えてもよい補助

以下は型規則ではなく補助情報としては扱ってよい。

- 関数本体に明示的に現れた `Err(ParseError)` の収集
- `=?` で呼び出している既知関数の declared errors の収集
- REPL / LSP / ドキュメント生成への反映

ただし、これらは「契約補助」であり、「式レベルで具象エラーを扱える根拠」にはしない。

## 17. ランタイム責務

### 17.1 ランタイムが保持するもの

ランタイムは `Error` に関して、少なくとも以下の情報を保持しうる。

- kind
- message
- location
- cause
- 表示用メタデータ

これはコード上の型規則とは分離された責務である。

### 17.2 ランタイムがやること

- エラー値を診断可能な形で保持する
- トップレベル失敗時に適切な表示を行う
- RichError 相当の情報を一貫した形式で管理する
- cause 連鎖を表示・記録する

### 17.3 ランタイムがやらないこと

- コード上の具象エラー分岐の代行
- 型システムの代わりとなる静的保証
- 具象エラーの階層推論

ランタイムは「情報を持つ」が、「言語構文の複雑化を補うための制御機構」ではない。

## 18. 実装指針

### 18.1 AST / Typed 上の方針

実装上は次のどちらかを選ぶことになる。

案A: 関数戻り値専用の `E` を保持し、それ以外は `Result<T>` に正規化

- 実装は比較的単純
- コード上の表示と整合しやすい
- 契約情報は関数宣言ノードに紐づけて管理する

案B: 内部では広く `Result<T, E>` を保持し、表示段階で隠す

- LSP / doc / 補助情報へ流しやすい
- ただし「内部にはあるがコードでは見せない」設計になる

現時点の思想により近いのは B だが、言語規則としては A に見せる 形である。

### 18.2 TypeEnv / TypeKind の役割

型環境には少なくとも次が必要である。

- 型名の存在確認
- `deferror` 起源かどうかの識別
- 関数シグネチャの declared error 情報
- `Err(term)` の具象エラー判定

既存の `TypeKind::Error` はこの起点として利用できる。

### 18.3 エラーメッセージ方針

型検査エラーは、禁止した意図が見える形にする。

例: 変数注釈で `Result<T, E>`

```text
Result<T, E> is only allowed in function return signatures.
Use Result<T> in local code.
```

例: `E` が `deferror` 起源でない

```text
The error marker E in Result<T, E> must be a deferror-defined type.
```

例: `Err(Error)`

```text
Error is abstract and cannot be constructed directly.
Use a concrete deferror value in Err(...).
```

## 19. ドキュメント・表示方針

### 19.1 ソースコード

- 主表記: `Result<T>`
- 関数シグネチャのみ: `Result<T, E>`

### 19.2 REPL

- 主表示は `Result<T>`
- 必要に応じて declared errors を補助表示できる余地を残す

### 19.3 LSP

- 詳細仕様は今後の検討課題
- hover / signature help / completion detail で契約情報を補助表示できる設計余地はある

### 19.4 ドキュメント生成

- 関数宣言の `E` をそのまま契約表示として利用できる
- 「何が失敗しうるか」を自然言語で補足できる
- ただし、分解可能性を示唆しない文言にする

## 20. レビュー観点

今後 Result / Error 周辺の実装をレビューする際は、次を確認する。

### 20.1 ルール逸脱

- `Result<T, E>` が関数戻り値以外に出ていないか
- `Err(Error)` を許していないか
- `deferror` に不要な構造知識を持たせていないか

### 20.2 型検査逸脱

- `E` の存在チェックがあるか
- `E` の `deferror` 起源チェックがあるか
- 変数 / field / alias などの禁止文脈を弾けているか

### 20.3 言語思想逸脱

- 具象エラー分解へ寄っていないか
- enum 的な設計へ流れていないか
- ランタイム責務を型系へ持ち込んでいないか
- コード上の簡潔さが損なわれていないか

## 21. 非目標

本設計では、以下を意図的に扱わない。

- 具象エラーの exhaustive matching
- 具象エラーの union 型
- `Result<T, E1 | E2>` のような表現
- エラー階層や継承
- `impl Error` 的実装詳細の露出
- エラーのトレイトベース制御
- cause 構造の静的型表現
- `deferror` を使った複合エラー代数

これらは処理系とコードを重くするため、現在の Surtr の目的から外れる。

## 22. 一問一答形式の仕様固定

Q1. `Result<T, E>` はどこで書けるか？  
A. 関数シグネチャの戻り値でのみ書ける。

Q2. `E` には何を書けるか？  
A. `deferror` で定義された具象エラー型のみ書ける。

Q3. 変数注釈で `Result<T, E>` は書けるか？  
A. 書けない。コード上は `Result<T>` を使う。

Q4. field に `Result<T, E>` は持てるか？  
A. 持てない。`Result<T>` のみ許可する。

Q5. `Err(Error)` は許可されるか？  
A. 許可しない。`Error` は抽象型である。

Q6. `match` で具象エラー分解できるか？  
A. できない。`match` は `Ok` / `Err` の構造のみ扱う。

Q7. 具象エラー候補をコンパイラが列挙できるなら分解可能にすべきか？  
A. しない。列挙可能性は契約情報であり、分解権限ではない。

Q8. `deferror` は cause を型として持てるか？  
A. 持たない。cause はランタイム責務である。

## 23. 最終指針

Surtr における Result / Error は、次の原則で維持する。

- 失敗は `Result` で扱う
- 具象エラー型は契約にのみ露出する
- コード上は抽象 `Error` に留める
- 具象エラー分解は提供しない
- `deferror` はアトミックな生成子に留める
- ランタイムは詳細情報を持つが、コードは単純に保つ

これにより、Surtr は

- 契約が読める
- 実装が重くなりすぎない
- ユーザコードが複雑化しない
- ランタイム診断を豊かにできる
