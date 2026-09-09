# Generator 改修仕様 — 遅延生成と persistent な進行状態

## 1. 状態・根拠・変更範囲

- 分類: `/doc` に置く、仕様決定・実装待ちのGenerator改修入力。
- 基準 commit: `f1986a27d84728e1e7a88de457a1d6b55cfe5ab8`。
- 根拠: 本会話の不変な進行状態に関する決定、入力メモ「Surtr Monad / do / State / Generator 検討メモ」§11–13、基準commitの `lib/types/generator.srt`。
- 今回はGenerator自体へのMonad実装を追加しない。

基準の実装は既に `next` が `(item, next_generator)` を返す。mutable cursorを導入してから戻す改修ではない。

主な変更は、公開型の `Generator<State,Item>` を `Generator<Item>` へ変更し、生成状態を隠し、unfold/map等で列全体を先に構築している経路を遅延生成へ置き換えること。不変な旧値再利用は維持する。

## 2. 確定する値意味論

Generatorはruntimeが内部representationを管理してよいが、利用者から観測できるcursorをその場で変更してはならない。

```text
g0 --next--> Ok((a, g1))
g1 --next--> Ok((b, g2))
```

g0は変わらない。g1を次の呼び出しへ渡せば進んだ状態を使い、g1を捨てれば呼び出し元に残るg0は呼び出し前の状態のまま。

```surtr
g0 = Generator::range(1, 3)
(a, g1) =? Generator::next(g0)
(b, g2) =? Generator::next(g1)
(again, _) =? Generator::next(g0)
# rangeのような決定的producerでは a=1, b=2, again=1。
```

これは内部で巻き戻しを実行する機能ではない。古い値を変えないため、古い状態から再開できる。

`a = g0` / `b = g0` は共有mutable cursorを作らない。GCや構造共有は可能だが、片方のnextが他方を進めることはない。

## 3. Process・Stateとの区別

| 対象 | 次状態をどこが採用するか |
|---|---|
| Process callback | callbackの返り値をruntimeがprocess状態として採用 |
| `State<S,A>` | runがpairを返し、呼び出し側が必要な次状態を保持 |
| `Generator<A>` | nextが次Generator値を返し、呼び出し側が保持 |

Generatorはprocess、Agent、registry登録された共有状態ではない。Generatorの内部producer stateは、State Monadの公開parameter `S` とも別である。

## 4. 公開型と内部状態

```surtr
@builtin type Generator<$A>
```

公開する型parameterはitem型Aだけ。内部producer stateのSはunfoldの通常引数型として型検査されるが、Generatorの公開型からは隠す。

概念上の内部情報:

```text
concrete producer state
concrete step callable
必要なら進行index・adapterの残り情報
```

これは説明用の内部モデルであり、source言語にexistential型やdyn Traitを導入する構文ではない。

stepとstateの組は構築時に型検査し、具体化済みのcallableと対応するstateだけをruntime representationへ渡す。runtimeはimpl候補を探さない。実装上の型消去・格納方式はruntime内部の責任であり、言語レベルのdynamic Trait APIへ露出させない。

## 5. next

正常生成と終端だけを扱う最小契約:

```surtr
def next(gen: Generator<$A>) -> Result<($A, Generator<$A>), NoneError>
```

成功はitemと次Generator、終端はErr(NoneError)。nextは元Generatorを変更しない。

filterなどのadapterでは一つのitemを返すまで内部stepを複数回呼ぶ場合がある。「nextは必ず内部stepを一回だけ呼ぶ」という一般契約にはしない。

実エラーの扱いは§11で移行判定する。終端と任意のcallbackエラーを無条件に同一視する規則は、本会話からは確定していない。

## 6. unfoldと評価タイミング

入力メモの候補はstateのみを受けるstep、基準実装は `(state, idx)` を受けるstepである。最小改修案では既存callbackアリティを維持する。

```surtr
def unfold(
  state: $S,
  step: ($S, Int -> Result<($A, $S), NoneError>)
) -> Generator<$A>
```

このsignatureは正常生成/終端だけを扱う初期案。ResultのError補助表記の扱いは、既存の型規則を拡張せず確認する。

unfoldは通常引数式を評価してstateと関数値を取得するが、その場でstepを繰り返し実行してListを構築しない。stepの本体はnext等の要求で評価する。

`$S`と`$A`はstate・stepのvalue parameterから導入される。unfoldに余分なReturnTypeArgumentを追加しない。内部Sを隠したことを理由にruntime dictionaryを作らない。

stepのidxをsourceから廃止する場合は独立したAPI変更として記録する。一つのunfoldがcallbackアリティを推測して両方を受けるoverloadは作らない。

## 7. 変換API

### 7.1 map / filter

```surtr
def map(gen: Generator<$A>, mapper: ($A -> $B)) -> Generator<$B>
def filter(gen: Generator<$A>, predicate: ($A -> Boolean)) -> Generator<$A>
```

元Generatorを保持するadapterとして新しい値を返す。adapterの構築時に残りの列を全走査しない。mapper/predicateを呼ぶのはitemが要求された時点。

filterは一致するitemを探すために複数のsource stepを評価し得る。無限に不一致が続くproducerに対して、nextの終了を保証しない。

### 7.2 take / to_list

基準実装の `take` は `List<A>` を返す。以前の会話中にあった「takeがGeneratorを返す」という案と同じAPIではない。

最小改修案では既存の戻り値を維持する。

```surtr
def take(gen: Generator<$A>, count: Int) -> List<$A>
def to_list(gen: Generator<$A>) -> List<$A>
```

takeは最大count件を具体化するterminal operation。count<=0は空Listを返し、producerを進める必要がない。to_listは残り全てを具体化するため、有限で正常に終端するproducerが前提。

いずれも元genを更新しない。Listを返すAPIから進行後Generatorを暗黙に別のbindingへ保存しない。

遅延な件数制限adapterや、`(List<A>, rest)` を返す読み取りAPIは有用だが、名称・型は別途決める。同じtakeを戻り値期待型によって意味が変わるoverloadにしない。

### 7.3 scan / map_accum

入力メモの候補を採用案として整理する。

```surtr
def map_accum(
  gen: Generator<$A>,
  initial: $S,
  mapper: ($S, $A -> ($B, $S))
) -> Generator<$B>

def scan(
  gen: Generator<$A>,
  initial: $S,
  mapper: ($S, $A -> $S)
) -> Generator<$S>
```

accumulatorは各Generator値の次状態に含める。共有mutable accumulatorを作らない。scanがinitial自体を最初に出力するかどうかは§11で固定する。

## 8. 他機能との接続

### 8.1 SafeBind

```surtr
def read_two(gen: Generator<Int>) -> Result<((Int, Int), Generator<Int>)> {
  (a, g1) =? Generator::next(gen)
  (b, g2) =? Generator::next(g1)
  Ok(((a, b), g2))
}
```

二つ目で終了しても呼び出し元の元genは変わらない。外部作用が取り消されるという意味ではない。

### 8.2 pipeline

map/filterとtakeを普通の関数として接続する。map/filterの結果はGenerator、take/to_listの結果はListと明確にする。Generator固有の評価構文は増やさない。

### 8.3 Monad値をitemとして持つ

```text
Generator<A> を (A -> Result<B>) でmap
  -> Generator<Result<B>>

そのnextの成功
  -> Ok((Result<B>, next_generator))
```

内側itemがErrでも、外側nextの終端と同じ意味ではない。自動flattenや自動失敗伝播をしない。OptionT等の具象型をitemとして持つ場合も同様。

遅延な `Generator<Result<A>>` を `Result<Generator<A>>` に変換しただけで、将来の失敗を先に確認できたことにはならない。traverse_result等を追加する場合は、有限列の即時走査か、生成時の失敗なのかを別途明記する。今回は曖昧なbridge APIを必須追加にしない。

### 8.4 Facet

`Generator<A>` 全体を通常のfield/property型として保持し、Facetで読み取り・置換できる。cursorをその場で進めるFacet操作は作らない。

次Generatorを得てから、それをfieldの新しい値としてFacetへ渡す。古いenclosing構造体が保持しているGeneratorは変わらない。

### 8.5 REPL

```text
xldr> g0 = Generator::range(1, 3)
xldr> (a, g1) =? Generator::next(g0)
xldr> (b, g2) =? Generator::next(g1)
xldr> (again, _) =? Generator::next(g0)
```

g0/g1/g2はすべて具体的な `Generator<Int>`。以前のREPL入力で構築したstep/closureとstateの対応・寿命を保持し、新しい入力やcheckpoint操作で無効化しない。

## 9. persistentであることと外部作用の区別

同じGeneratorからの再開は、同じ保存済みproducer stateから計算を開始することを保証する。

同じitemまで保証するのは、step・mapper・predicateが純粋で決定的な場合である。callbackが時刻・IO・process状態等を明示的に観測した場合まで同じ結果を保証したとは扱わない。

返されたGeneratorを捨てても、すでに行った外部作用を取り消す機能はない。新しいeffect追跡やtransactionをGeneratorのためだけに導入しない。

初期実装では、callback評価回数を変える隠れたmemoizationを仕様に含めない。最適化を行う場合も、評価タイミングや外部から観測できる振舞いを変えないことを別途検証する。

## 10. 旧実装からの変更表

| 項目 | 基準実装 | 改修方向 |
|---|---|---|
| 公開型 | Generator<State,Item> | Generator<Item> |
| next | itemと次Generatorを返す | 値意味論を維持し、公開型を変更 |
| unfold | stepを繰り返して有限Listを構築 | stateとstepを保持して要求時に生成 |
| map | 残りListを先に変換 | 遅延adapter |
| take | Listを返す | 最小案では維持。lazy adapterとは別API |
| step引数 | stateとidx | 最小案では維持。入力メモとの差分を明記 |
| 内部表現 | idxとitemsを利用するbridge | hidden producer state / step / adapter情報 |
| Trait | 今回のMonad追加範囲には含めない | Monad実装を同時追加しない |

旧 `gen_make/gen_idx/gen_items` は実装詳細として参照を監査し、不要になった経路を削除する。単に新旧両方の表現を永久に残すcompatibility fallbackにしない。

## 11. API確定ゲート

| ID | 未確定・差分 | 実装前に固定すること |
|---|---|---|
| G-I01 | unfoldのcallbackアリティ | 本書の最小案は(state,idx)。単一state案へ変えるなら移行を明記 |
| G-I02 | 終端以外のcallback Error | 終端専用に限定するか、一般Errorをnext/terminal APIまで保持するかを固定。任意ErrをNoneErrorへ黙って変換しない |
| G-I03 | idx/with_indexと新adapterのindex | source進行indexとadapter出力indexを混同せず、公開idxの意味を一覧化 |
| G-I04 | scanの最初の要素 | initialを出力するか、最初のstep後からかを固定 |
| G-I05 | 遅延件数制限・rest付き読み取り | 必須coreとは別。名称と戻り値を決めてから追加 |

G-I02で一般Errorを採用する場合、`next`のError補助表記、take/to_listの戻り値と失敗保持も同時に変更する。型だけ終端専用のまま実エラーを返す、またはterminal operationで実エラーを消す仕様にはしない。

## 12. 受け入れ条件

| ID | 検証 |
|---|---|
| G-01 | 同じg0から独立にnextでき、片方が他方を変更しない |
| G-02 | g1を採用した場合だけ呼び出し側が次状態を利用する |
| G-03 | 結果を捨ててもg0を同じ保存状態から再利用できる |
| G-04 | unfold時にstep本体が呼ばれない |
| G-05 | map/filter/scan/map_accumが事前に列全体を評価しない |
| G-06 | nextが返すstateとstepの型対応を構築時に検証する |
| G-07 | 有限終端と空列を扱い、無限列を無制限に先行生成しない |
| G-08 | takeのcount<=0がproducerを評価せず空Listを返す |
| G-09 | callback実エラーと終端の採用方針をG-I02どおりに検証する |
| G-10 | Generator<Result<A>>のitem Errを外側終端と取り違えない |
| G-11 | field/property/collection/Facetで新旧値を保持できる |
| G-12 | REPLの別入力で具象Generator値とclosure/stateを再利用できる |
| G-13 | runtime Trait dictionaryや共有mutable cursorを追加しない |
| G-14 | callbackの外部作用をrollbackすると説明・実装しない |
| G-15 | 旧公開型・旧eager生成経路の参照を移行する |

## 13. 実装・docs移管

builtin追加/変更の正本は既存のSindr metadataとruntime bridgeの規約に従う。Generator専用のTypeCtorTrait推論やdo内特例を追加しない。

実装後は `lib/types/generator.srt` の `@doc` と利用者向けAPI、必要なEldr/Xldrの実装契約へ移管する。基準実装の既存persistent behaviorを「新たに導入した」と記録しない。
