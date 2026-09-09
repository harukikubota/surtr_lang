# 標準 Monad インスタンス追加仕様 — Identity / Reader / State

## 1. 状態・対象

- 分類: `/doc` に置く、仕様決定・実装待ちの実装入力。
- 基準 commit: `f1986a27d84728e1e7a88de457a1d6b55cfe5ab8`。
- 追加対象: `Identity<A>`、`Reader<R, A>`、`State<S, A>`。
- 独立タスク: MonadT の言語機能・標準 Transformer 追加と別に実装・検証・完了判定する。
- 前提: 既存の通常 generic nominal type、関数型 field、Trait method dispatch、ReturnTypeArgument を使う。

この3型の追加を理由に、nominal constructor parameter、parameterized TypeCtorTrait、部分適用型構文、runtime dictionary を先行導入しない。既存機能の不備が見つかった場合は最小の基盤修正として分離し、MonadT 機能を巻き込まない。

根拠は本会話の標準追加決定、入力メモ「Surtr Monad / do / State / Generator 検討メモ」§3–9・§13–16、「Surtr Monad Transformer 検討メモ」§5・§6・§8、および既存 `lib/traits/operator/{functor,applicative,monad}.srt` である。

以下の helper 名・引数順は、会話中の候補を実装可能な一つの仕様案へ具体化したもの。取り込み時に異なる命名を採用する場合は、宣言・例・テストを同時に直す。本文中の Surtr 例は仕様例であり、このファイル作成時にコンパイル実行した結果ではない。

## 2. 共通の設計意図

Monad は Surtr の runtime 値モデルではなく、通常のデータ型に対して提供する合成 framework である。

3型とも通常の値として引数・戻り値・field・property・List・tuple に保持できる。Monad API、pipeline、do のどれを使うかは利用側が選び、結果は同じ通常型の値になる。

Reader と State が保持するのは通常の関数値である。標準 helper は process を生成・検索・更新せず、registry、Supervisor、設定 process を暗黙に利用しない。

## 3. 型と carrier

| 型 | representation | mapped slot | captured argument |
|---|---|---|---|
| `Identity<A>` | `A` | `A` | なし |
| `Reader<R, A>` | `R -> A` | `A` | `R` |
| `State<S, A>` | `S -> (A, S)` | `A` | `S` |

標準型ではこの順で型引数を並べる。ただし compiler は「最後だから mapped」と判定せず、既存の slot mapping を検証する。ユーザ定義型に同じ引数順を強制しない。

`Reader<R, _>` と `State<S, _>` は carrier 説明用表記であり、本書で新しい call-site 部分適用構文を追加しない。

## 4. 通常の構築・可視性

標準型の field は public とし、直接 representation を読む Trait impl と Facet を利用可能にする。新しい private 可視性機構は追加しない。

struct literal は型所有者の inherent method 内で使い、Trait impl からは `Type::new` を通して再構築する。public field であることと、外部から struct literal が許可されることは別である。

```surtr
defstruct Identity<$A> {
  value: $A
}

impl Identity {
  def new(value: $A) -> Identity<$A> {
    Identity { value }
  }

  def run(self: Identity<$A>) -> $A {
    self.value
  }
}
```

```surtr
defstruct Reader<$R, $A> {
  run_reader: ($R -> $A)
}

impl Reader {
  def new(run_reader: ($R -> $A)) -> Reader<$R, $A> {
    Reader { run_reader }
  }
}
```

```surtr
defstruct State<$S, $A> {
  run_state: ($S -> ($A, $S))
}

impl State {
  def new(run_state: ($S -> ($A, $S))) -> State<$S, $A> {
    State { run_state }
  }
}
```

型所有者に必須の `new`、通常の関数アリティ、struct literal の位置制限を省略しない。上の定義に後述の helper と Trait impl を追加する。

## 5. Trait 契約

3型すべてに `Functor`、`Applicative`、`Monad` を実装する。

```surtr
deftrait Functor
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}
```

既存の `Applicative::pure`、`Applicative::ap`、`Monad::return`、`Monad::bind` の契約を変えない。`pure` と `return` は同じ値構築意味論に揃える。

標準として `Alternative` は付けない。Identity、通常 Reader、通常 State には任意の payload 型を返せる標準 empty を設けない。従って `guard`、partial `<-`、non-Result do の SafeBind を使うために、これらへ都合のよい empty を捏造しない。

`Show` / `Eq` / `Compare` / `Default` 等の追加は本件の必須範囲ではない。関数値を持つ Reader / State の等価性を runtime の closure identity で定義しない。

## 6. Identity

### 6.1 API

```surtr
# impl Identity 内
def new(value: $A) -> Identity<$A>
def run(self: Identity<$A>) -> $A
```

全型入力が value parameter から得られるため、上記 helper に ReturnTypeArgument は不要。

### 6.2 意味

```text
run(new(a)) = a
fmap(new(a), f) = new(f(a))
pure(a) = new(a)
ap(new(f), new(a)) = new(f(a))
bind(new(a), f) = f(a)
```

`bind` の戻り値を再び Identity で包み、二重コンテナにしてはならない。

## 7. Reader

### 7.1 役割

Reader は `R -> A` を通常値として保持し、同じ入力 `R` を複数の計算へ渡す構造である。Process wrapper、設定取得器、依存性注入コンテナという固有機能は持たない。

### 7.2 API

```surtr
# impl Reader 内
def new(run_reader: ($R -> $A)) -> Reader<$R, $A>
def run(self: Reader<$R, $A>, environment: $R) -> $A
def ask::<$R>() -> Reader<$R, $R>
def asks(mapper: ($R -> $A)) -> Reader<$R, $A>
def local(self: Reader<$R, $A>, mapper: ($R -> $R)) -> Reader<$R, $A>
```

`asks` は単項関数から Reader を作る意図を示す helper で、意味は `new` と同じ。`local` は同じ `R` の入力値を変換する。入力型を別型へ変える一般化は初期範囲に含めない。

`ask` の `$R` は戻り値だけにあるため ReturnTypeArgument で導入する。それ以外は関数型 parameter の内部も含めて型入力を得られる。

### 7.3 意味

```text
run(new(f), r) = f(r)
run(ask(), r) = r
run(asks(f), r) = f(r)
run(local(x, f), r) = run(x, f(r))

run(fmap(x, f), r) = f(run(x, r))
run(pure(a), r) = a
run(ap(mf, ma), r) = let f = run(mf, r); a = run(ma, r) in f(a)
run(bind(ma, f), r) = run(f(run(ma, r)), r)
```

`bind` では後段へ同じ `r` を渡す。前段の payload を環境として渡さない。`local` は元の Reader 値や呼び出し元の環境値を更新しない。

### 7.4 pipeline と do

```surtr
reader: Reader<Int, Int> = Reader::asks({|n| n + 1})
pipeline: Reader<Int, Int> = reader |*> {|n| n * 2}

# do 実装後の同値な利用例。
sequenced: Reader<Int, Int> = do {
  n <- reader
  pure(n * 2)
}

Reader::run(pipeline, 10)  # 期待値 22
Reader::run(sequenced, 10) # 期待値 22
```

この例の do 部分は後段の統合テストであり、Reader 本体の追加タスクを do 完成待ちにしない。

## 8. State

### 8.1 役割

State は `S -> (A, S)` を通常値として保持する。可変オブジェクトや process handle ではない。各 `run` が次状態を返し、呼び出し元がそれを保持する。

`S` の型は一つの carrier 内で固定される。`A` の型は `fmap` / `bind` により変えられる。`State<Int, A> -> State<String, B>` を一つの通常 bind で繋ぐ Indexed State は別の抽象であり初期範囲外。

### 8.2 API

```surtr
# impl State 内
def new(run_state: ($S -> ($A, $S))) -> State<$S, $A>
def run(self: State<$S, $A>, initial: $S) -> ($A, $S)
def eval(self: State<$S, $A>, initial: $S) -> $A
def exec(self: State<$S, $A>, initial: $S) -> $S
def get::<$S>() -> State<$S, $S>
def gets(mapper: ($S -> $A)) -> State<$S, $A>
def put(state: $S) -> State<$S, Unit>
def modify(mapper: ($S -> $S)) -> State<$S, Unit>
```

`get` 以外は value parameter から必要な型入力を得る。`eval` / `exec` はそれぞれ一度の `run` の射影であり、内部で同じ action を複数回実行しない。

### 8.3 意味

```text
run(get(), s) = (s, s)
run(gets(f), s) = (f(s), s)
run(put(next), s) = ((), next)
run(modify(f), s) = ((), f(s))
run(pure(a), s) = (a, s)

run(fmap(ma, f), s0):
  (a, s1) = run(ma, s0)
  (f(a), s1)

run(bind(ma, f), s0):
  (a, s1) = run(ma, s0)
  run(f(a), s1)

run(ap(mf, ma), s0):
  (f, s1) = run(mf, s0)
  (a, s2) = run(ma, s1)
  (f(a), s2)
```

`ap` の状態は mapper 側から value 側へ引き継ぐ。同じ初期状態を両方へ独立に渡す Reader 風実装にしない。

### 8.4 pipeline と do の結果型を揃える

```surtr
pipeline: State<Int, Int> = State::get() |>= {|n|
  State::put(n + 1) |>= {|_| pure(n)}
}

# do 実装後の同値な利用例。
sequenced: State<Int, Int> = do {
  n <- State::get()
  State::put(n + 1)
  pure(n)
}

State::run(pipeline, 10)  # 期待値 (10, 11)
State::run(sequenced, 10) # 期待値 (10, 11)
```

`State::put` だけで終わる pipeline は `State<Int, Unit>` であり、この例と同じ結果型とは説明しない。

## 9. 関数・ReturnTypeArgument・評価タイミング

引数不足による暗黙カリー化、新しい関数 overload、Trait method の inherent namespace への追加を行わない。`pure` は既存の helper、または `Applicative::pure` として呼ぶ。`Reader::pure` や `State::pure` を自動生成しない。

関数本体の型変数は既存の導入元と bound を使う。式位置に新しい `$A` を書いて generic value を作ることはできない。後から `::<...>` で任意の通常型変数を指定する構文も追加しない。

Reader / State の構築は渡された関数値を保持する。関数本体を呼ぶのは `run` の契約に従う時点である。ただし constructor の通常引数式は通常どおり評価される。

純粋で決定的な callback について、同じ action と同じ入力から同じ結果を得る。callback が process messaging や IO を明示的に使ったとき、その外部作用の再現性や rollback を Reader / State が保証するとはしない。新しい effect system は導入しない。

## 10. Facet と通常データとしての利用

Identity の payload field、Reader / State の関数 field は、既存の可視性と mutating Facet operation の規則に従う。Facet が Monad trait を検出して自動 bind/run する経路は作らない。

```surtr
id0 = Identity::new(10)
id1 = Facet::put(Identity.value, id0, 20)
# id0.value は10、id1.value は20。
```

Reader / State 自体を他の構造体の property に保持する場合、その property 全体が通常の focus になる。型変更可能更新では、変更後の nominal type と全 field の整合性を通常どおり検査する。

## 11. REPL

型定義・Trait impl は通常の file-oriented 宣言として標準ライブラリに置く。REPL が1入力ずつ扱うのは、読み込み済み型の値構築・関数適用である。

```text
xldr> s: State<Int, Int> = State::get()
xldr> State::run(s, 3)
(3, 3)
xldr> State::run(s, 8)
(8, 8)
```

各入力を実行する前に `R` / `S` / `A` を具体化する。`s = State::get()` の状態型がその入力単位で確定しなければ ambiguity とし、次の REPL 入力による後付け確定を待たない。

generic callable scheme は compiler 側に保持してよいが、未確定の State data value や dyn Monad を REPL に保存しない。

## 12. Transformer との境界

`Reader<R,A>` と `ReaderT<R,Identity,A>`、`State<S,A>` と `StateT<S,Identity,A>` は別の nominal type とする。コンパイラ上の型同値、alias、暗黙変換を設けない。

IdentityT は対象外。標準 Transformer を追加しなくても、本書の3型はそれぞれ使用・検証できなければならない。

## 13. 実装担当・検証

標準型・helper・Trait impl は通常の `.srt` 定義で記述する。初期配置候補は `lib/types/{identity,reader,state}.srt`。既存の標準ソース読込み規約に登録するが、型名ごとの dispatch hardcode、専用 opcode、専用 runtime process は追加しない。

| ID | 受け入れ条件 |
|---|---|
| MI-01 | 3型に通常の `new` があり、外部コードは constructor 経由で作る |
| MI-02 | 全ての mapped slot を明示的に検証し、captured `R` / `S` が保たれる |
| MI-03 | Identity の fmap/ap/bind が定義の期待値を返す |
| MI-04 | Reader の bind が同じ環境を次段へ渡す |
| MI-05 | Reader::local が元 Reader と呼出元環境を変更しない |
| MI-06 | State の bind/ap が返された次状態を左から右へ引き継ぐ |
| MI-07 | State の eval/exec が action を各一度だけ run する |
| MI-08 | 異なる `R` / `S` の carrier を混ぜた bind を拒否する |
| MI-09 | 一般の payload 型変更に余分な Eq/Show 等を要求しない |
| MI-10 | `pure` と `return` の意味が一致する |
| MI-11 | helper・演算子・具象関数値への capture が通常の静的 dispatch を使う |
| MI-12 | property / List / tuple / Facet で型全体を保持・更新できる |
| MI-13 | REPL の具象化成功、ambiguity失敗、失敗後の継続を確認する |
| MI-14 | MonadT が未実装でも本タスクを検証・完了できる |
| MI-15 | do 完成後に同値な pipeline/do の観測結果と型を比較する |
| MI-16 | 有限の純粋なテスト関数で Functor・Applicative・Monad の法則を検証する |

関数を含む値の law test は、代表入力で `run` した観測結果を比較する。compiler が全ての関数について法則を証明したとは扱わない。

## 14. docs 移管

実装後は標準 `@doc`、`docs/site` の標準モジュール一覧・利用説明、必要な `docs/dev/Trait_system_spec.md` の例へ同期する。未実装の MonadT 仕様を同時に実装済み扱いにしない。
