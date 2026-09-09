# 標準 MonadT 実装型の仕様とインターフェース意図

## 1. 状態・対象・前提

- 配置: `/doc`。標準 Transformer の未実装仕様。
- 基準 commit: `f1986a27d84728e1e7a88de457a1d6b55cfe5ab8`。
- 前提: `monadt_language_extension_spec.md` の最小言語機能とインターフェース確定ゲート。
- 対象: `OptionT`、`EitherT`、`ReaderT`、`StateT`。
- 対象外: `IdentityT`、Writer/WriterT、ListT、ResultT、ContT、Transformerそのものを受ける抽象API。

通常 `Identity` / `Reader` / `State` の追加は `monad_instances_spec.md` の別タスク。ここでは再実装・再設計しない。

根拠は本会話と「Surtr Monad Transformer 検討メモ」§2–10、「MonadT / Alternative / do 構文 検討メモ」§2–6。APIの候補一覧を、型関係・責務・RTAが分かる形へ具体化する。会話で未確定だったhelperの細部は§11に分け、共通Traitの契約へ無断で追加しない。

本書のSurtr例は実装後の目標であり、本書作成時のcompilerで実行済みのコードではない。

## 2. 共通の意図

Transformerは、通常の値を保持する名義型である。MonadTはその型とbase Monadの接続capabilityであり、特殊なruntime値カテゴリではない。

ユーザは以下のどのスタイルでも扱える。

```text
通常の関数: T<M,A> -> T<M,B>
Functor/Monad演算子: 同じ通常型の値を合成
Facet: 公開された通常のfieldを選択・再構築
do: bindを用いて同じcarrierの値を合成
```

どのスタイルも出力を通常のfield・引数・戻り値・collectionへ渡せる。do外の利用を二次的な経路にしない。

## 3. 型定義

```surtr
defstruct OptionT<$M: Monad, $A> {
  inner: $M<Option<$A>>
}

defstruct EitherT<$L, $M: Monad, $A> {
  inner: $M<Either<$L, $A>>
}

defstruct ReaderT<$R, $M: Monad, $A> {
  run_reader: ($R -> $M<$A>)
}

defstruct StateT<$S, $M: Monad, $A> {
  run_state: ($S -> $M<($A, $S)>)
}
```

fieldはpublic。標準のTrait implがrepresentationを直接読む方針とし、private可視性を拡張しない。

型ごとに必須の inherent `new` を定義する。struct literalはowner内に置き、Trait implからの再構築には `Type::new` を使う。public fieldだから外部struct literalを許す、という規則にはしない。

| 型 | mapped | captured |
|---|---|---|
| `OptionT<M,A>` | A | M |
| `EitherT<L,M,A>` | A | L, M |
| `ReaderT<R,M,A>` | A | R, M |
| `StateT<S,M,A>` | A | S, M |

`M`を変える操作は同じcarrierのfmap/bindではなく、出力型を変える通常のhelper操作とする。

## 4. 共通Trait

全対象に Functor / Applicative / Monad / `MonadT<M>` を実装する。baseには初期仕様としてMonadを要求し、型ごとにFunctorだけの弱い部分実装まで分割しない。

```surtr
deftrait MonadT<$M: Monad>
where
  Self: Monad
{
  def lift::<Self>(value: $M<$A>) -> Self<$A>
}
```

共通methodは `lift` だけ。`run`や`map_t`をMonadT共通methodに追加しない。型によってrepresentationと追加の実行引数が違うためである。

`pure` / `return` / `fmap` / `ap` / `bind` は既存のTrait namespaceまたは既存helperを使う。具象型のinherent namespaceへ同名methodを自動注入しない。

Functor・Monadのimplは既存のslot mappingとmethod contractで検証する。field名、型引数の順番、標準型名でcompilerが意味論を選ばない。

## 5. OptionT

### 5.1 意味

```text
representation: M<Option<A>>
new / run: wrapperの構築とrepresentationの取り出し
pure(a): M::pure(Some(a)) を包む
lift(ma): M::fmap(ma, Some) を包む
```

bindはbaseのbindでOptionを観測し、Someならmapperへpayloadを渡し、NoneならbaseのpureでNoneを返す。base側の失敗・分岐はbaseのbindに従う。

```text
run(bind(x, f)) = bind_M(run(x), option ->
  Some(a): run(f(a))
  None:    pure_M(None)
)
```

### 5.2 最小API

以下は inherent method signature。各 `$M` / `$N` は `where ...: Monad` を要求する。

```surtr
def new(inner: $M<Option<$A>>) -> OptionT<$M, $A>
def run(self: OptionT<$M, $A>) -> $M<Option<$A>>

def some::<$M>(value: $A) -> OptionT<$M, $A>
def none::<$M, $A>() -> OptionT<$M, $A>
def from_option::<$M>(value: Option<$A>) -> OptionT<$M, $A>
```

`new`と`run`はcontainer層を増減する通常のwrapper APIであり、base計算を余計に実行しない。`some`/`none`/`from_option`はbaseのpureを使う。

### 5.3 追加helper候補

```surtr
def map_inner(
  self: OptionT<$M, $A>,
  mapper: (Option<$A> -> Option<$B>)
) -> OptionT<$M, $B>

def map_t(
  self: OptionT<$M, $A>,
  mapper: ($M<Option<$A>> -> $N<Option<$B>>)
) -> OptionT<$N, $B>
```

`map_inner`はOption全体、`fmap`はSome payload、`map_t`はbaseを含むrepresentation全体を変換する。どれを選ぶかで作用範囲を明示する。mapperの内部型から `$B` / `$N` を導入するため、これらをRTAへ重複指定しない。

## 6. EitherT

### 6.1 意味

```text
representation: M<Either<L,A>>
pure(a): M::pure(Right(a)) を包む
lift(ma): M::fmap(ma, Right) を包む
```

Rightを通常のmapped payloadとし、Leftは保持してmapperを呼ばない。Leftの型Lはcarrier内で固定。

`L`は通常のデータ型であり、SurtrのError運搬制限を迂回するためのslotではない。`Either<Error,A>`を本仕様の一般例として許可しない。

### 6.2 最小API

```surtr
def new(inner: $M<Either<$L, $A>>) -> EitherT<$L, $M, $A>
def run(self: EitherT<$L, $M, $A>) -> $M<Either<$L, $A>>

def left::<$M, $A>(value: $L) -> EitherT<$L, $M, $A>
def right::<$L, $M>(value: $A) -> EitherT<$L, $M, $A>
def from_either::<$M>(value: Either<$L, $A>) -> EitherT<$L, $M, $A>
```

各RTAは値引数から得られない入力だけを、上記の順で宣言する。call-siteのRTAを部分的な個数で指定しない。部分推論には既存の `_` を使う。

`map_left`、`map_inner`、`map_t`は既存Either APIを全面複製せず、必要な変換面だけを提供する候補。Lを変える操作では結果のcarrierが変わるため、普通の型変換として出力型を検査する。

## 7. ReaderT

### 7.1 意味

```text
representation: R -> M<A>
run(x,r): xが保持する関数をrで呼ぶ
pure(a): rを使わずpure_M(a)を返す関数
lift(ma): rを使わずmaを返す関数
bind(x,f): r -> bind_M(run(x,r), a -> run(f(a),r))
```

同じ環境rを前後の計算へ渡す。Readerと同様、Process wrapperではない。環境の取り出しは関数入力の受渡しであり、registry検索をしない。

### 7.2 最小API

```surtr
def new(run_reader: ($R -> $M<$A>)) -> ReaderT<$R, $M, $A>
def run(self: ReaderT<$R, $M, $A>, environment: $R) -> $M<$A>
def ask::<$R, $M>() -> ReaderT<$R, $M, $R>
def asks::<$M>(mapper: ($R -> $A)) -> ReaderT<$R, $M, $A>
def local(
  self: ReaderT<$R, $M, $A>,
  mapper: ($R -> $R)
) -> ReaderT<$R, $M, $A>
```

`asks`はplain mapperの結果をbaseのpureへ渡す。すでに `R -> M<A>` がある場合はnewを使い、暗黙flattenや関数戻り値の推測によるoverloadを作らない。

`local`の環境型変更は初期範囲外。元値・外部processの状態を更新しない。

## 8. StateT

### 8.1 意味

```text
representation: S -> M<(A,S)>
run(x,s0): xが保持する関数をs0で呼ぶ
pure(a): s -> pure_M((a,s))
lift(ma): s -> fmap_M(ma, a -> (a,s))
bind(x,f): s0 -> bind_M(run(x,s0), (a,s1) -> run(f(a),s1))
```

状態型Sを固定し、返された状態値を引き継ぐ。baseの失敗でpairが得られなければ、次の状態を生成して返したことにはしない。

Applicativeのapもmapper側、value側の順に状態を引き継ぐ。baseのapをpointwiseに使って両方へ同じ初期状態を渡す実装にはしない。

### 8.2 最小API

```surtr
def new(run_state: ($S -> $M<($A, $S)>)) -> StateT<$S, $M, $A>
def run(self: StateT<$S, $M, $A>, initial: $S) -> $M<($A, $S)>
def eval(self: StateT<$S, $M, $A>, initial: $S) -> $M<$A>
def exec(self: StateT<$S, $M, $A>, initial: $S) -> $M<$S>
def get::<$S, $M>() -> StateT<$S, $M, $S>
def gets::<$M>(mapper: ($S -> $A)) -> StateT<$S, $M, $A>
def put::<$M>(state: $S) -> StateT<$S, $M, Unit>
def modify::<$M>(mapper: ($S -> $S)) -> StateT<$S, $M, Unit>
```

eval/execは一度のrun結果をbaseのfmapで射影する。`modify`はplainなS->S、baseを返すstate関数はnewで明示する。新たなeffectful helperの追加は別にAPI判断する。

## 9. Alternative

| 型 | 標準の提供条件・方針 |
|---|---|
| OptionT | `M: Monad`。内側のOptionの不在をemptyにする |
| EitherT | 初回は提供しない。デフォルトLeftやErrorを生成しない |
| ReaderT | baseが `Monad + Alternative` を満たす場合に提供 |
| StateT | baseが `Monad + Alternative` を満たす場合に提供 |

一般的な「MonadTならAlternativeもある」という導出規則を作らない。

### 9.1 OptionT

```text
empty_OptionT = new(pure_M(None))
```

`M::empty()`へ自動切り替えない。例:

```text
M=Result: OptionT自身のemptyのrepresentationはOk(None)。Err(e)とは別。
M=List:   OptionT自身のemptyのrepresentationは[None]。[]とは別。
```

chooseは左のbase計算からSomeならそれを保持し、Noneなら右のrepresentationへ進む。baseがListなら各分岐についてこの規則を適用し、base由来の分岐を勝手に集約しない。

### 9.2 ReaderT / StateT

```text
run(empty_ReaderT, r) = empty_M
run(choose_ReaderT(l,r), env) = choose_M(run(l,env), run(r,env))

run(empty_StateT, s) = empty_M
run(choose_StateT(l,r), s0) = choose_M(run(l,s0), run(r,s0))
```

既存の `Alternative::choose` はeager。左右の引数式を評価しない保証や外部作用の取り消しを付けない。StateTの両branchに同じ初期状態を渡すことと、外部作用をrollbackすることは別である。

通常State/Reader/IdentityにはAlternativeを付けない。その結果 `StateT<S,Result,A>` も、標準ResultにAlternativeがない初期構成ではAlternativeを得ない。

## 10. 言語機能への接続

### 10.1 Facet単体で公開representationを扱う

```surtr
x: OptionT<Result, Int> = OptionT::new(Ok(Option::Some(1)))
raw: Result<Option<Int>> = Facet::view(OptionT.inner, x)
y = Facet::put(OptionT.inner, x, Ok(Option::Some(2)))
```

これはMonadを使わない通常のstruct操作である。xは変わらない。実際のpath eligibilityや型変更は既存Facet規則で検査する。

```surtr
# 部分定義例。Modelには通常規則どおりnewを別途定義する。
defstruct Model {
  current: OptionT<Result, Int>
}
```

Model.currentのfocusはOptionT全体。その値をpipelineまたはdoで作り、Facetへ渡せる。公開fieldを辿る場合も、明示した構造pathだけを使う。

### 10.2 do

doはMonadTを知らず、選ばれたcarrierのMonadを使う。MonadT実装の有無だけでbaseの値を自動liftしない。

```surtr
x: OptionT<Result, Int> = MonadT::lift(Ok(1))
y: OptionT<Result, Int> = do {
  a <- x
  pure(a + 1)
}
```

期待型と既存のbare headによる指定を用いる。`do::<OptionT<Result,_>>` の新文法は追加しない。

### 10.3 SafeBindとliftは別

canonical Result以外のdoでSafeBindを使う場合は、既存do仕様のAlternative empty方針に従う。

```text
OptionT<Result,_> のdo:
  x <- lift(result)  はbase ResultのErrを保持する。
  x =? result       はSafeBind失敗をOptionT自身のemptyへ変える。
```

後者はrun後にOk(None)であり、Err保存とは異なる。StateT<S,Result,_>ではAlternativeがないため、必要なResultは明示的なliftなど通常の接続で扱う。

### 10.4 評価タイミング

`lift(base_operation())` は通常の引数評価を遅延化しない。ReaderT/StateTのrun時に呼びたい処理は、その関数値のbodyへ明示的に置く。lazy special formをMonadTへ追加しない。

`>*` / `>=>` は既存のLiftComposable/KleisliComposable契約も確認し、MonadTだけで自動提供されたとは扱わない。

## 11. 未確定インターフェースと判断意図

必須のtrait contract・4型・通常値としての性質は固定する。次のAPI細部は、実装前に採用する表を一つに揃える。

| ID | 項目 | 意図・境界 |
|---|---|---|
| MT-SI01 | OptionT/EitherTのmap_inner・map_left・map_tの標準採用 | 操作面を明示し、元container APIの全面複製をしない |
| MT-SI02 | ReaderT/StateTのmap_tのmapper型 | run結果へpointwiseに適用するか、保持関数全体を置換するかを同名overloadにしない |
| MT-SI03 | helper名・引数順・RTA順 | 本書の最小API案を基準とし、既存関数の引数導入規則から外れない |
| MT-SI04 | captured baseとTransformer stackのsource表記 | 言語拡張仕様のMT-I01に従う。表示例を受理文法と取り違えない |

MT-SI02の判断材料:

```text
ReaderTのrun結果変換候補: M<A> -> N<B>
StateTのrun結果変換候補: M<(A,S)> -> N<(B,S)>

保持関数全体の置換候補:
  (R -> M<A>) -> (R -> N<B>)
  (S -> M<(A,S)>) -> (S -> N<(B,S)>)
```

会話の「representationへ接続」という説明だけでは、この二つは同一ではない。最小のnew/run/liftで迂回可能なので、曖昧なmap_tを先に追加しない。構文や型推論の特例で両方を受けない。

## 12. 受け入れ条件

| ID | 検証 |
|---|---|
| MT-S01 | 各new/runのrepresentation round-trip |
| MT-S02 | fmapがpayloadだけを変え、captured型を保つ |
| MT-S03 | pure/return/bind/apの型・順序・失敗保持 |
| MT-S04 | OptionTのNoneとbase失敗を区別する |
| MT-S05 | EitherTのLeftとbase失敗を区別する |
| MT-S06 | ReaderTが同じ環境を次の計算へ渡す |
| MT-S07 | StateTがbase内の次状態を引き継ぐ |
| MT-S08 | OptionTのemptyがpure_M(None)でありbase emptyではない |
| MT-S09 | ReaderT/StateTのAlternativeがbase capabilityを要求する |
| MT-S10 | 標準Result由来のErrを勝手にNone/Leftへ変換しない |
| MT-S11 | 通常field・関数・collection・Facetで値を保持・更新できる |
| MT-S12 | REPLでnew/lift/fmap/runを別入力として具象型で扱える |
| MT-S13 | carrier指定が曖昧な呼び出しを候補数から補完しない |
| MT-S14 | do導入後、pipelineと同じ型・観測結果を得る |
| MT-S15 | 型正しいユーザ定義base/Transformerにも同じ経路が使える |
| MT-S16 | 有限・純粋な観測テストでMonad則とliftのpure/bind保存を確認する |
| MT-S17 | IdentityT・抽象Transformer引数API・runtime辞書を追加しない |

法則は標準実装のテスト契約であり、任意のユーザ実装の数学的正しさを型検査だけで証明しない。

## 13. docs移管

言語機能は `docs/dev/Trait_system_spec.md` 側、標準型とhelperは標準 `.srt` の `@doc` と利用者ガイド側へ分離して移管する。未採用のhelper候補を実装済みAPI一覧に載せない。
