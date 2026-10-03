# PR: 有限列 Generator と無限列 InfiniteGenerator の再設計

## 1. 状態と目的

- 状態: 未実装の仕様提案。製品コード、標準定義、実行可能テストは変更しない。
- 作成日: 2026-10-03。
- 調査基準: `de3a8b40eca908ac2050d334a4b79613cc83e8df`。
- level: 4。公開型、生成・終端の契約、評価タイミング、opaque runtime value と VM 継続に影響する。
- 入力: 添付「Surtr Generator 再設計ドラフト」。有限列はこのドラフトを基にする。無限列は旧 `generator_spec.md` の遅延生成・変換案と現行実装を基に再設計する。

有限列は `Generator<$Item>`、無限列は `InfiniteGenerator<$Item>` とする。生成 protocol の step / next は、どちらも外側の `Result` を返さない。有限列の正常終端は `Option::None`、無限列の生成成功は通常の pair で表す。

有限列の責務を生成と List 化に限定し、無限列では取得前の遅延変換を提供する。両方とも元の値を変更せず、進行後の値を呼び出し側が明示的に採用する。

本書の API 名、無限用の変換集合、`scan` の規則は再設計案である。例と signature は実装後の目標であり、現行 REPL で動作する例とは扱わない。

対象外は専用構文、暗黙の型変換、Monad / Functor 等の新しい Trait 実装、dynamic Trait、共有 mutable cursor、生成失敗を Result で伝える別 producer protocol。任意の callback の終了性や純粋性を証明する型機能も追加しない。

## 2. 現行との差分

[現行標準定義](../lib/types/generator.srt) と [標準テスト](../lib/tests/generator.srt) を確認した。

| 項目 | 現行 | 本提案 |
|---|---|---|
| 公開型 | `Generator<State, Item>` | 有限・無限とも Item だけを公開 |
| 内部表現 | private bridge で作る `(idx, List)` | state と具体化済み callable を保持する opaque value |
| `unfold` | `(state, idx)` の step を先に全件実行 | state だけの step を要求時に実行 |
| 終端 | step の任意の `Err(_)` を終了として吸収 | 有限は None、無限は終端 protocol なし |
| `next` | `Result<(item, next_gen), NoneError>` | 有限は Option、無限は pair |
| `map` / `with_index` | 残り List 全体を先に変換 | 有限から削除、無限では遅延変換 |
| `iterate` | count 付き有限列を先に全件構築 | 引数は seed / count / step のまま、無限列ベースで直接 List を返す |
| `take` | 生成済み List の先頭を取得 | 要求件数までを生成し、List と rest を返す |

現行の next も元の値を変更しない。不変な進行状態は維持する契約であり、今回初めて導入する機能ではない。現行に動作する無限列の実装はなく、旧案の遅延 adapter も未実装である。

`range_step` / `range_char` / `range_char_step` は、有限列の特定パターンを引数から構築する API として残す。引数、入力検証、Error の種類を維持し、公開型だけを `Generator<Item>` に移行する。これらの外側 Result は構築前の入力検証であり、生成 step / next の Result protocol とは別である。range literal の入力検証も維持する。

[Scar の range literal lowering](../crates/scar/src/checker/expr.rs) は、整数の場合に Generator::range と to_list、文字の場合に range_char の Result を SafeBind した後で to_list を呼ぶ。helper 名と生成・検証・List 化の接続を保ち、新しい一引数の Generator 型へ移行する。

## 3. 共通契約

### 3.1 型と state

```surtr
@builtin type Generator<$Item>
@builtin type InfiniteGenerator<$Item>
```

内部 state の型 `$State` は unfold の引数で型検査する。step が受ける state と返す次 state は同じ型でなければならない。生成値 `$Item` と state は別型でよい。state を公開型へ露出させず、existential 型の構文を追加しない。

二つの型は別の nominal identity を持つ。期待型によって有限・無限を選ぶ overload や暗黙変換は作らない。`Generator::to_list` は InfiniteGenerator を受け付けない。

### 3.2 不変な進行状態

next(g0) は g0 を変更しない。得られた g1 を次の呼び出しへ渡したときだけ、進んだ状態を使う。別 binding、構造体 field、List、Facet に保持した g0 も変化しない。process の共有状態とは別の通常の値として扱う。

同じ値の再利用は、同じ保存済み state から計算を始めることを保証する。同じ item を返す保証は callback が純粋で決定的な場合に限る。IO や時刻参照の結果を固定せず、外部作用を巻き戻さない。

callback の実行回数・タイミングを変える暗黙の memoization は行わない。一回の呼び出しが VM の予算切れや待機で中断した場合は、その途中から再開し、完了済み callback を呼び直さない。元の値に対する別の next は別の評価である。

### 3.3 Result を生成 protocol にしない

| 境界 | 有限 | 無限 |
|---|---|---|
| producer step | `$State -> Option<($Item, $State)>` | `$State -> ($Item, $State)` |
| `unfold` | `Generator<$Item>` | `InfiniteGenerator<$Item>` |
| `next` | `Option<($Item, Generator<$Item>)>` | `($Item, InfiniteGenerator<$Item>)` |
| take による取得 | `(List<$Item>, Generator<$Item>)` | `(List<$Item>, InfiniteGenerator<$Item>)` |

生成 protocol として Err(NoneError) を受け付けない。callback が VM の実行失敗を起こした場合は、既存の失敗経路で伝え、None、空 List、途中までの成功 List に変換しない。不正な callback 戻り値や runtime state も RuntimeError とする。VM 内部の Rust `Result<_, RuntimeError>` はこの規則によって廃止しない。

`$Item` が `Result<A>` や `Option<A>` であることは禁止しない。これは item の通常のデータであり、生成 protocol ではない。

```text
有限: Some((Err(error), next_gen)) は一件の正常生成
無限: (Err(error), next_gen) は一件の正常生成
```

item を自動 flatten せず、item の Err / None を終端として扱わない。取得した値の検証で Result を返す操作は利用側に置く。range_* の構築前検証と take_exact の取得後検証は、生成 protocol に Result を追加する理由にはしない。

### 3.4 index

両方とも共通の idx API と step の追加 index 引数を持たない。index が生成規則に必要なら state に含める。無限用の with_index は出力へ番号を付ける合成関数とし、opaque value に共通 cursor 番号を追加する理由にしない。

## 4. 有限列 Generator

### 4.1 公開 API

```surtr
impl Generator {
  @builtin def unfold(
    seed: $State,
    step: ($State -> Option<($Item, $State)>),
  ) -> Generator<$Item>

  @builtin def next(
    gen: Generator<$Item>,
  ) -> Option<($Item, Generator<$Item>)>

  @builtin def take(
    gen: Generator<$Item>, count: Int,
  ) -> (List<$Item>, Generator<$Item>)

  def take_exact(
    gen: Generator<$Item>, count: Int,
  ) -> Result<(List<$Item>, Generator<$Item>)>

  @builtin def take_while(
    gen: Generator<$Item>, predicate: ($Item -> Boolean),
  ) -> (List<$Item>, Generator<$Item>)

  def take_until(
    gen: Generator<$Item>, predicate: ($Item -> Boolean),
  ) -> (List<$Item>, Generator<$Item>)

  @builtin def to_list(gen: Generator<$Item>) -> List<$Item>

  @builtin def range(start: Int, stop: Int) -> Generator<Int>

  def range_step(start: Int, stop: Int, step: Int) -> Result<Generator<Int>, Error>

  def range_char(start: String, stop: String) -> Result<Generator<String>, Error>

  def range_char_step(
    start: String, stop: String, step: Int,
  ) -> Result<Generator<String>, Error>
}
```

unfold は seed と関数値を保持し、構築時に step 本体を呼ばない。next はこの producer の step を一回実行する。Some((item, next_state)) なら item と次 state を保持した Generator を返し、None なら正常終端とする。range のような専用 producer は同じ公開契約を直接実行してよい。

有限用 unfold には、有限回で None を返す step を渡すことを利用条件とする。任意の関数の終了性を型検査では証明しない。常に Some を返す関数も signature だけでは拒否できず、その場合の to_list は完了を保証しない。型を分けることで無限用の全件取得を静的に拒否できるが、有限用 callback の終了性まで保証できるとは説明しない。一定回数で勝手に終了させる fallback も作らない。

### 4.2 take と to_list

- take(gen, count) は最大 count 件を生成順に取得し、`(values, rest)` を返す。
- count <= 0 なら `([], gen)` を返し、producer を評価しない。
- count 件に達したら、次の step や終端確認を実行しない。rest は最後に取得した値の次の位置。
- count 件より前に None が返れば、取得済み List と終端の rest を正常な結果として返す。
- to_list は None まで残りを全件生成し、List だけを返す。空列は空 List。
- どちらも元 gen を変更しない。rest を使うか、元の gen を再利用するかは呼び出し元が選ぶ。

unfold が m 件で終わる場合、count == m の take は step を m 回だけ呼び、終端確認はしない。この時点の rest に残りの値がないかどうかは未確認である。count > m の take と to_list は None を得るために m + 1 回呼ぶ。

終端を観測した take 系は、新しい終了済みの Generator を rest として返す。その next は callback を呼ばず None を返す。元の Generator を終了済みに変更することはなく、元の値への別の next は同じ state から step を再評価する。これは元 producer の評価結果を暗黙にキャッシュする規則ではない。

正常に完了した to_list は終端を確認している。一方、take 系による List 化は件数や条件で停止できるため、List を取得できたことだけでは rest の終端を保証しない。必要なら呼び出し元が rest の next で確認する。

構築時の事前 materialize は廃止する。to_list と take 系は生成順に runtime 内の buffer へ追加し、完了時にその所有権を ListHandle へ移す。非空の結果は直接 Packed、空は Empty とする。PureSurtr の再帰的 cons / reverse による中間 List や、完成後の要素コピーは行わない。容量と連続実行量の制御は第 7 節を参照する。

### 4.3 take_exact

正の count 件を必ず取得できたかを検証する標準合成 API として採用する。

1. count <= 0 は生成前に引数エラーとし、producer を評価しない。
2. count > 0 は take で `(values, rest)` を取得する。
3. List::len(values) == count なら `Ok((values, rest))` を返す。
4. 必要数に足りなければ、取得後の検証で引数エラーとは別の不足エラーを返す。

二つのエラーは別の canonical deferror とし、不正 count は requested、不足は requested / actual を保持する。具体的な名前と診断文は実装時に既存の Error 定義規約へ合わせる。生成 step の失敗を Result へ包み直す API ではない。

不足時の Err は成功 pair を含まないため、rest を返さない。元 gen は変更されておらず、呼び出し元が保持できる。失敗時にも取得済み List と rest を必要とする利用コードは take の結果を検証する。count 件揃った成功でも、その rest が終端かどうかは保証しない。

### 4.4 take_while / take_until

どちらも条件による打ち切り、または有限 producer の正常終端まで取得し、`(values, rest)` だけを返す。終了理由を示す flag / enum は追加しない。戻り値から条件停止と正常終端のどちらで終了したかを直接区別することはできない。

| API | List に追加して継続する条件 | 追加前に打ち切る条件 |
|---|---|---|
| take_while | predicate が True | False |
| take_until | predicate が False | True |

一件を生成したら、List へ追加する前に predicate を一回評価する。終了条件を満たした時点で打ち切り、その item は List に含めない。「境界値を含む」は条件が反転する直前の item まで含む意味とする。

条件停止した rest は、終了条件を満たした item の生成前の Generator とする。その item を読み飛ばさず、次の評価に残す。item を生成するために一回評価済みであっても、rest の再利用時には同じ state から再評価する。時刻や IO を観測する producer では同じ値になる保証はない。

正常終端なら終了済みの rest を返す。predicate は存在する item にだけ適用し、None には適用しない。終了条件が最初の item で成立する場合は空 List と元の gen を返す。

```surtr
g = Generator::range(1, 4)
(xs, rest) = Generator::take_while(g, {|n| n < 3})
# xs = [1, 2]。rest の先頭は 3。
(ys, rest2) = Generator::take_until(g, {|n| n >= 3})
# ys = [1, 2]。rest2 の先頭も 3。
```

take_while は predicate と List builder を扱う runtime materialize API とする。take_until は predicate の否定を take_while に渡す標準合成関数とし、取得順序と rest の規則を共有する。

### 4.5 range_*

有限列の特定パターンを引数から構築する API として、range / range_step / range_char / range_char_step をそのまま残す。入力の事前検証後、生成 step は Result を返さない。

- range は両端を含む昇順の Int 列、step は 1。start > stop は空列。
- range_step は step > 0 なら昇順、step < 0 なら降順。方向と端点が逆なら空列、step == 0 は InvalidRangeStep。
- range_char / range_char_step は単一 ASCII 文字の String endpoint を受け付ける。単一文字・ASCII の検証を維持し、不正入力は InvalidCharRange。range_char_step の step == 0 は InvalidRangeStep。
- Int は BigInt のまま。固定幅整数の上限を利用者向け終端として持ち込まない。
- 正常な入力を検証する際にも列全体を先に作らない。文字への変換に失敗しない範囲を構築前に確定する。

```surtr
Generator::to_list(Generator::range(1, 3)) # [1, 2, 3]
(xs, rest) = Generator::take(Generator::range(1, 3), 5)
# xs = [1, 2, 3]。rest は終端。
Generator::to_list(Generator::range(3, 1)) # []
stepped =? Generator::range_step(7, 1, -2)
Generator::to_list(stepped) # [7, 5, 3, 1]
```

runtime 内部の専用 Range producer は許容する。利用者には常に `Generator<Item>` として見える。

### 4.6 変換の配置

有限用には map / filter / with_index / scan / map_accum を置かない。取得した List は List の関数や通常関数で処理する。take 系は rest を返すため、List の変換へ渡す前に pair を分解する。

```surtr
(xs, rest) = Generator::take(Generator::range(1, 4), 3)
List::map(xs, {|n| n * 2}) # [2, 4, 6]
# rest の先頭は 4。
```

## 5. 無限列 InfiniteGenerator

### 5.1 生成と取得 API

```surtr
impl InfiniteGenerator {
  @builtin def unfold(
    seed: $State,
    step: ($State -> ($Item, $State)),
  ) -> InfiniteGenerator<$Item>

  @builtin def next(
    gen: InfiniteGenerator<$Item>,
  ) -> ($Item, InfiniteGenerator<$Item>)

  @builtin def take(
    gen: InfiniteGenerator<$Item>, count: Int,
  ) -> (List<$Item>, InfiniteGenerator<$Item>)

  def take_exact(
    gen: InfiniteGenerator<$Item>, count: Int,
  ) -> Result<(List<$Item>, InfiniteGenerator<$Item>)>

  @builtin def take_while(
    gen: InfiniteGenerator<$Item>, predicate: ($Item -> Boolean),
  ) -> (List<$Item>, InfiniteGenerator<$Item>)

  def take_until(
    gen: InfiniteGenerator<$Item>, predicate: ($Item -> Boolean),
  ) -> (List<$Item>, InfiniteGenerator<$Item>)

  def iterate(
    seed: $Item, count: Int, step: ($Item -> $Item),
  ) -> List<$Item>
}
```

無限用 unfold は seed と step を保持し、構築時に step 本体を呼ばない。base producer の next は step を一回実行し、必ず item と次 state の pair を要求する。Option / Result で終端や生成失敗を返す callback は型不一致として拒否する。

take は count > 0 なら count 件を返すまで評価し、`(values, rest)` を返す。正常な短い List という結果はない。count 件を取得できれば停止し、先読みしない。count <= 0 は callback を一度も呼ばず `([], gen)` を返す。callback が戻らない場合や filter が値を見つけられない場合まで、取得の完了を保証するものではない。

無限用 take_exact も第 4 節と同じく count <= 0 を生成前の引数エラーとし、take の List を取得後に検証する。正常に戻る take は count 件を返すので、不足エラーは通常の無限列の公開契約では発生しない。

take_while / take_until は第 4 節の先判定・反転前までを含む境界・rest の規則を共有する。無限 producer 自体の正常終端はないため、正常に返る場合は条件による停止である。take_while は predicate が True のまま、take_until は False のままなら停止しない。これらは取得操作であり、無限 producer に終端を導入する adapter ではない。

無限用には to_list と stop を持つ range を設けない。take 系の List 化も rest を無限用 Generator のまま返し、有限用 Generator に変換しない。両方の生成型を変換する API は追加しない。

### 5.2 iterate

iterate は無限列ベースの件数指定取得として扱う。既存の `(seed, count, step)` の引数順・型を維持し、直接 `List<Item>` を返す。公開 owner は InfiniteGenerator とし、count を持つ有限 Generator の構築経路や、二引数で Generator を返す iterate は残さない。

内部では seed を先頭とし、以降は直前の item に step を適用する無限 producer を構築する。take で count 件を取得し、その List を返す。rest はこの API の返り値に含めず、内部で破棄する。iterate の List が完成しても、内部 producer の終端を示す意味はない。

count <= 0 は空 List を返し、利用者の step を呼ばない。m > 0 件の取得に必要な利用者 step 呼出しは m - 1 回。最初の item は seed なので step を呼ばず、最後の item の次を先に計算しない。callback 自体の終了性は利用条件とする。

```surtr
InfiniteGenerator::iterate(1, 5, {|x| x * 2}) # [1, 2, 4, 8, 16]
InfiniteGenerator::iterate(1, 0, {|x| x * 2}) # []
```

通常の unfold と「先頭かどうか」を含む state の合成で内部 producer を作る。公開の二引数 iterate を追加する必要はない。再開可能な無限列自体が必要なら unfold を使う。

### 5.3 遅延変換

旧案の adapter は無限用に移す。各変換は unfold と next で表せる標準合成関数とし、構築時に source を走査しない。

```surtr
impl InfiniteGenerator {
  def map(
    gen: InfiniteGenerator<$A>, mapper: ($A -> $B),
  ) -> InfiniteGenerator<$B>

  def filter(
    gen: InfiniteGenerator<$A>, predicate: ($A -> Boolean),
  ) -> InfiniteGenerator<$A>

  def with_index(gen: InfiniteGenerator<$A>) -> InfiniteGenerator<(Int, $A)>

  def map_accum(
    gen: InfiniteGenerator<$A>, initial: $State,
    mapper: ($State, $A -> ($B, $State)),
  ) -> InfiniteGenerator<$B>

  def scan(
    gen: InfiniteGenerator<$A>, initial: $State,
    mapper: ($State, $A -> $State),
  ) -> InfiniteGenerator<$State>
}
```

| API | 一件を要求した際の評価 | 出力と次状態 |
|---|---|---|
| map | source の next 一回、mapper 一回 | 変換値、進行後 source |
| filter | 一致するまで source の next と predicate | 一致値、その値の直後の source |
| with_index | source の next 一回 | (index, item)、進行後 source と index + 1 |
| map_accum | source の next 一回、mapper 一回 | mapper の値、進行後 source と次 accumulator |
| scan | 先頭は評価なし。その後は source の next 一回、mapper 一回 | 先頭 initial、その後の accumulator |

with_index は変換を構築した位置を 0 とする。進行後の変換値には次の番号を保持する。filter の後に付ければ一致値の番号、前に付ければ source の番号となる。削除する旧 idx の保存値を参照しない。

map_accum は initial 自体を出力しない。scan は initial を最初に出力する。例えば unfold(1, {|n| (n, n + 1)}) を加算 scan すると、initial が 0 の場合の先頭四件は [0, 1, 3, 6] となる。

filter の型は正常終端を持たないことを表すが、次の一致値の存在は証明しない。不一致が永久に続けば、その next や take は戻らない。一定の検索回数で空列・None・Err を返す救済規則を追加しない。検索中も VM の共通予算を使い、他の実行を妨げない。filter の探索対象を有限件に制限したい場合は、filter を適用する前の producer から先に件数指定で List 化し、`List::filter` を使う。List 化自体の終了は、その source の各 next が戻ることを前提とする。

```surtr
g = InfiniteGenerator::unfold(1, {|n| (n, n + 1)})
|> InfiniteGenerator::filter({|n| n % 2 == 0})
|> InfiniteGenerator::map({|n| n * 10})
(xs, rest) = InfiniteGenerator::take(g, 3)
# xs = [20, 40, 60]。rest は後続の変換値を生成する。
```

## 6. 成功・拒否境界

有限用の state と item が異なる例:

```surtr
g = Generator::unfold((1, 3), {|state|
  if(
    state._0 <= state._1,
    Option::Some((state._0, (state._0 + 1, state._1))),
    Option::None,
  )
})
(xs, rest) = Generator::take(g, 5)
# xs = [1, 2, 3]。rest は終端。
```

無限用の accumulator と item が異なる例:

```surtr
fib = InfiniteGenerator::unfold((0, 1), {|state|
  (state._0, (state._1, state._0 + state._1))
})
(xs, rest) = InfiniteGenerator::take(fib, 6)
# xs = [0, 1, 1, 2, 3, 5]。rest の先頭は 8。
```

| 拒否する入力 | 拒否理由 |
|---|---|
| `Generator<Int, Int>` | 旧二引数の公開型は廃止 |
| 有限 unfold の `(state, idx)` callback | callback は単一 state 引数 |
| 有限 step の `Ok((item, state))` / `Err(NoneError)` | 戻り値は `Option<(Item, State)>` |
| 無限 step の `Option::Some(...)` / `Option::None` | 戻り値は `(Item, State)` |
| 無限 step の外側 `Ok(...)` / `Err(...)` | 生成 protocol に Result を使わない |
| step の次 state が seed と別型 | state の型契約違反 |
| `Generator::to_list(infinite_gen)` | 異なる nominal 型 |
| `InfiniteGenerator::to_list(...)` | API が存在しない |
| 有限 `Generator::map(...)` / `idx(...)` / `with_index(...)` | API を削除 |
| `InfiniteGenerator::idx(...)` | 共通 index API は設けない |

iterate の count 引数の欠落と、take 系の pair を List として使う呼出しも通常の型検査・アリティ検査で拒否する。range_* の Result は維持し、生成 protocol の Result 拒否と混同しない。

通常の名前解決・型検査で拒否する。旧 signature の検出や callback のアリティ推測による互換分岐は置かない。診断の正確な文字列は実装時に既存診断規則に合わせる。

### 6.1 rest の再利用とエラー

rest は取得終了時の位置を表す通常の不変値であり、それを返したことだけでは終端・今後の無失敗を保証しない。条件停止した item は rest に残り、条件停止の rest や未終端の元値を再利用すると同じ state の評価を再実行する。終端を観測して作った Terminal rest の next は、callback を呼ばず None を返す。

再利用先で同じエラーが発生すること、条件停止した同じ位置から再び取得して空 List になることは、利用側の設計責任とする。処理系は再発を避けるために item を自動で読み飛ばさず、エラーを成功や終端へ変換しない。

VM 内の一回の取得を予算切れから再開する際の重複実行禁止と、利用者が rest や元値を使って新しく呼び出す再評価を区別する。外部作用の rollback は保証しない。

## 7. runtime とフェーズの責務

- Sindr: 二つの canonical 型、公開 arity、opaque runtime value と handle を定義する。BUILTIN_METAS を唯一の builtin 定義元とする。
- Sigil: 標準 owner / member の登録と通常の名前解決。型を import 対象にする変更は行わない。
- Scar: seed と具体的 callable の state / item 型を一致させる。既存の多相関数の具体化を使い、runtime Trait dictionary、専用 TypeCtorTrait 推論を追加しない。
- Forge: 通常の callable / CallBuiltin 経路で具体化済み producer を渡す。専用の評価構文・Opcode は追加しない。
- Eldr: 保存済み state と callable、必要なら有限 Range producer を扱う。元 handle を更新せず、次 handle を返す。
- Xldr: 別入力で保持した handle の state、closure capture、具体化済みコードの寿命と checkpoint を保つ。

概念上の内部表現は有限の `Unfold { state, step } | Range { current, stop, step } | Terminal` と、無限の `Unfold { state, step }`。無限用の合成関数は source handle や accumulator を通常の内部 state に含められる。有限の文字 range は検証済みの文字 state を保持する producer で扱う。Terminal は取得操作が実際に正常終端を観測した際の終了済み rest とする。二つの runtime kind は判別可能にし、誤った kind の入力を補正しない。

opaque 表示は `Generator(<opaque>)` / `InfiniteGenerator(<opaque>)` とし、表示のために step を実行しない。Tuple 分解や旧 `(idx, items)` 表示を残さない。新しい Eq / ordering / serialization Trait は提供しない。

[Eldr VM 仕様](../docs/dev/EldrVM_spec.md) の共通予算付き driver と builtin continuation を使う。producer callback、filter の検索、List への一件追加を中断・再開可能にする。closure、capture、部分適用、pipe、通常呼出しで同じ経路を使い、builtin 内の同期 callback 完走ループは作らない。

[現行 ListBuilder](../crates/eldr/src/builtin/list_flat_map.rs) は flat_map 実装内の private 型である。Generator から参照できる共有内部部品へ整理して再利用する。Builder の変更は所有する VM continuation 内に閉じ、公開 Generator handle に mutable Builder を持たせない。完了時にだけ buffer を ListHandle へ移し、非空なら Packed を直接作る。buffer は移すだけで要素を再構築しない。失敗時は途中 List を成功結果として公開しない。通常の中断では所有権を移し、checkpoint では独立な進行状態を保存する。

to_list は残件数が分からない unfold にも対応するため、buffer を必要に応じて増やす。take の count が巨大でも全件分を先に確保せず、事前確保する容量には内部の閾値を設ける。この閾値は予約容量だけに適用し、要求件数や返す List を切り詰めない。小さい列を Cons、大きい列を Packed に切り替える閾値は設けない。

連続実行量は共通 VM 予算で制御し、producer callback、predicate、buffer への追加も予算に含める。Packed 化や容量の閾値によって総メモリ量が一定になるわけではなく、完成した List の全要素分の記憶領域が必要となる。

count は BigInt のまま残件を扱い、先に usize へ丸めたり最大値へ clamp したりしない。List の容量上限や内部カウンタの不正状態は既存 runtime の失敗規則で扱う。schema / VM version は上げない。

## 8. 移行と正本文書

旧 gen_make / gen_idx / gen_items と `(idx, List)` decoder、eager unfold、有限の変換 API を削除する。二つの公開型と step の新契約だけを実装する。廃止した呼出しは必要な拒否 fixture として残し、旧動作の成功を期待するテストは残さない。

以下は実装時に整合させる。この提案だけで実装済み仕様を書き換えない。

| 対象 | 変更内容 |
|---|---|
| `lib/types/generator.srt` と新しい無限用標準定義 | 型・生成契約・API・実行できる @doc 例 |
| `lib/tests/generator.srt` / `lib/tests/schema/generator_schema.srt` | Option 終端、単一 state、進行状態、無限用の成功例 |
| `tests/fixtures/script/*` | helpers の移行、旧 signature / 型 / API の拒否 |
| `tests/integration/repl.rs` / `tests/integration/language_features/runtime_observation.rs` | 型表示、closure 寿命、旧 idx 観測の置換 |
| `docs/site/range.md` | 有限 List 化後の変換、無限列との区別 |
| `docs/site/language-guide.md` / `language-reference.md` | range literal の入力検証と helper 名の説明を移行 |
| `docs/site/standard-modules.md` / `standard-library.md` | 新 owner と API の案内 |
| `docs/dev/EldrVM_spec.md` / `Xldr_spec.md` / `テスト方針.md` | runtime kind、継続、予算、REPL、検証配置 |
| `docs/dev/diagnostics.md` / `Rune_observability.md` | 必要な診断・観測契約への影響を確認 |
| `doc/v0.1_release_codebase_audit.md` | 任意 Err を終端へ吸収する問題の解消を実装後に確認 |

range literal と文字 range が参照する検証は range_* の構築前検証として維持する。正常な整数・文字 range の List 化と、不正な ASCII / 単一文字 endpoint の拒否を維持する。range_* の名前と引数を維持し、旧 bridge と eager 実装だけを置き換える。Generator の再設計を既存 do / MonadT の未完了事項に戻さない。

## 9. 受け入れ条件

| ID | 条件 | 最も直接的な検証先 |
|---|---|---|
| G-01 | 二つの型は Item のみを公開し、相互の暗黙変換がない | Scar / script fail |
| G-02 | 有限 step は Option、無限 step は pair。外側 Result と旧 callback を拒否 | Scar / script fail |
| G-03 | 構築時に step / mapper / predicate を呼ばない | Eldr callback 回数 |
| G-04 | 有限 None、空列、range の両端と逆転を正常に扱う | 標準テスト |
| G-05 | 両 take は count <= 0 で無評価、要求件数後の先読みなし。有限 to_list / 短い take の終端評価回数も第 4 節に一致 | Eldr callback 回数 |
| G-06 | take 系は List と、入力 gen と同じ生成型・Item 型の rest を返す。件数停止の次位置、条件停止の未取得 item、有限終端の Terminal を区別して保持 | 標準テスト / Eldr |
| G-07 | g0 の分岐再利用、g1 の採用、Facet field の保持で元値を変更しない | 標準テスト |
| G-08 | 異なる state / item 型を保持し、次 state 型の違いを拒否 | 標準テスト / Scar |
| G-09 | item の Err / None を終端にせず、VM 失敗を成功 List に変換しない | 標準テスト / Eldr |
| G-10 | 無限 map / filter / index / 累積の順序と state が第 5 節に一致 | 標準テスト |
| G-11 | iterate は seed / count / step で直接 List を返し、正の count に対し step は count - 1 回。非正 count と scan の先頭は利用者 callback を評価しない | Eldr callback 回数 / 標準テスト |
| G-12 | 不一致 filter を bounded quantum で進めると他 process も進む | Eldr / process fixture |
| G-13 | 待機・予算切れで完了済み callback を再実行せず、取消後も再開しない | Eldr continuation |
| G-14 | REPL の別入力・checkpoint 復元で callable / state / 途中 builder を保持 | Xldr / REPL integration |
| G-15 | opaque 表示が無評価。通常呼出し・capture・pipe 等で同じ生成契約 | runtime / script fixture |
| G-16 | 旧 bridge・二引数型・eager 経路が残らず、version を変更しない | 最終差分・検索・拒否 fixture |
| G-17 | take_exact の非正 count は無評価の引数エラー、有限の不足は取得後の別エラー。成功時にも終端確認しない | 標準テスト / Eldr |
| G-18 | take_while / until は先判定で反転前までを含む。条件停止・正常終端の理由を別途返さない | 標準テスト |
| G-19 | range_* の引数、昇降順、step == 0、単一 ASCII endpoint、range literal を維持 | 標準テスト / script fixture |
| G-20 | 非空 materialize は Packed、空は Empty。初期容量閾値が件数・結果を変えない | Eldr / Sindr |
| G-21 | rest / 元値の再利用で同じエラーが発生し得る。自動 skip / 成功変換をしない | 標準テスト / Eldr |

無限に不一致の filter は完走を待つテストにしない。有限の quantum を与え、未完了の状態と他の実行の進行を確認する。任意の有限 step の終了性を証明したとするテストも作らない。

G-12 / G-13 は map → filter → scan → take のような入れ子の合成も一例に含める。内側の next / callback の中断から外側の取得へ戻る継続を確認し、source の進行や accumulator 更新を重複させない。

## 10. 実装計画と検証

1. 本書の生成・取得・rest・入力検証の契約に正本文書を合わせる。通常の型形成・callback 拒否テストを先に追加する。
2. 二つの canonical 型と runtime value、builtin metadata と実装対応を導入する。型形成・kind 不一致・opaque 表示を検証する。
3. 有限 unfold / next / take / take_while / to_list / range と無限 unfold / next / take / take_while を実装する。G-03〜G-09 を Red / Green で固定し、共通 continuation と List builder を整備する。
4. 無限用の List 取得 iterate、両方の take_exact / take_until、range_* と遅延変換を標準合成で実装し、G-10〜G-13 と G-17〜G-21 を確認する。合成で不足する runtime primitive を場当たり的に追加しない。
5. 旧公開契約と bridge を削除し、標準テスト・fixture・REPL・正本文書を移行する。G-14〜G-16 と range literal など依存機能の移行を確認する。
6. 全体検証と別エージェントレビューを行い、仕様・最終差分・検証結果に残件がないことを確認する。

対象ごとの最小範囲から実行する。例は `rtk cargo nextest run -p scar`、`rtk cargo nextest run -p eldr`、`rtk cargo nextest run -p rune --test integration run_srt`、`rtk cargo nextest run -p xldr`。REPL / process の CLI 境界には `--profile cold` の対象テストを使う。

実装完了時は `rtk cargo nextest run --profile ci --workspace` と `cargo run -- test --quiet --all` を実行する。今回の文書作成ではコンパイラの全体テストや未実装 API の実行は不要。相対リンク、差分、生成契約の整合と仕様レビューで確認する。

## 11. 本提案で採用する範囲

iterate の引数維持と直接 List 化、range_* の維持、take_exact / take_while / take_until の追加、take 系の rest は本提案の採用範囲である。未確定の追加候補として扱わない。

open-issues.md は実装・検証のやり残しを追記するための台帳とし、この仕様整理のためには変更しない。実装後に残件があれば、その具体的な内容を追記する。
