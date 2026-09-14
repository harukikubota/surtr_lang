# 標準 MonadT 実装型の仕様とインターフェース意図

## 1. 状態・対象・前提

- 配置: `/doc`。共通 `MonadT` Traitと言語機能はN04、標準 Transformer 4型はN05で実装済み。
- 基準 commit: `f1986a27d84728e1e7a88de457a1d6b55cfe5ab8`。
- 前提: `monadt_language_extension_spec.md` の実装済み言語機能と確定インターフェース。
- 対象: `OptionT`、`EitherT`、`ReaderT`、`StateT`。
- 対象外: `IdentityT`、Writer/WriterT、ListT、ResultT、ContT、Transformerそのものを受ける抽象API。

通常 `Identity` / `Reader` / `State` は実装済みであり、利用者向け契約は
`../docs/site/{identity,reader,state}.md` に置く。ここでは再実装・再設計しない。

根拠は本会話と「Surtr Monad Transformer 検討メモ」§2–10、「MonadT / Alternative / do 構文 検討メモ」§2–6。
N05で採用したAPIと非採用境界を、型関係・責務・RTAが分かる形へ固定する。interface判断の結果は§11にまとめ、
共通Traitの契約へ型固有helperを追加しない。

§4の共通`MonadT` Traitと、OptionT / EitherT / ReaderT / StateTの各impl・helperは実装済みである。
標準sourceの正本は次のファイルであり、仕様本文から過去の配置を参照しない。

- 共通Trait: `../lib/traits/monad_t.srt`
- `OptionT`: `../lib/types/monad_transformer/option_t.srt`
- `EitherT`: `../lib/types/monad_transformer/either_t.srt`
- `ReaderT`: `../lib/types/monad_transformer/reader_t.srt`
- `StateT`: `../lib/types/monad_transformer/state_t.srt`

利用者向け説明は`../docs/site/monad-transformers.md`に置く。

### 1.1 移管状況

| 節・受け入れ条件 | 状態 | 実装正本・検証根拠・残作業 |
|---|---|---|
| §§1–4 | 実装済み・移管済み | `../lib/traits/monad_t.srt`、`../docs/site/monad-transformers.md`、`../docs/dev/Trait_system_spec.md` |
| §§5–8（各意味・最小API・採用helper） | 実装済み・移管済み | `../lib/types/monad_transformer/{option_t,either_t,reader_t,state_t}.srt`、`../lib/tests/monad_transformers.srt` |
| §§9–9.2 | 実装済み・移管済み | 本follow-upで標準sourceの`@doc`と`../docs/site/monad-transformers.md`へmultiplicity、eager評価、同一初期状態、rollback非保証を明記 |
| §10.1 | 実装済み・移管済み | 本follow-upで`../crates/xldr/tests/repl_core.rs`の同一owner成功とcross-nominal拒否を対にして固定 |
| §§10.2–10.3 | 実装済み・移管済み | `do` / SafeBind接続を実装。4 TransformerのMonad carrier利用を`../lib/tests/do.srt`でpipelineと実行比較 |
| §10.4 | 実装済み・移管済み | 標準sourceの通常関数・closure実装と本書§9、利用者文書の評価順説明 |
| §11 MT-SI01–04 | 実装済み・移管済み | N05で採用interfaceを固定。`map_inner` / `map_t`は非採用境界 |
| §12 MT-S01–13、MT-S15–18 | 実装済み・移管済み | `../lib/tests/monad_transformers.srt`、script pass/fail fixtures、Scar / Xldrテスト。対応表は実装計画§10 |
| §12 MT-S14 | 実装済み・移管済み | OptionT / EitherT / ReaderT / StateTの`do`とpipelineの観測結果を`../lib/tests/do.srt`で比較 |
| §13 | 実装済み・移管済み | 本follow-upで標準source、利用者文書、実装計画、実テストへ対応を固定。入力文書は今回は削除しない |

状態名は本follow-up仕様の分類に従う。着手時に「実装済み・追加移管あり」だった箇所は、
補強完了後の現在形として「実装済み・移管済み」へ更新し、今回追加した文書・テスト対応は根拠欄に残す。

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
defstruct OptionT<$M, $A>
where
  $M: Monad
{
  inner: $M<Option<$A>>
}

defstruct EitherT<$L, $M, $A>
where
  $M: Monad
{
  inner: $M<Either<$L, $A>>
}

defstruct ReaderT<$R, $M, $A>
where
  $M: Monad
{
  run_reader: ($R -> $M<$A>)
}

defstruct StateT<$S, $M, $A>
where
  $M: Monad
{
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
deftrait MonadT<$M>
where
  $M: Monad
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

### 5.3 非採用helper境界

次の`map_inner` / `map_t`はN05の標準APIには採用しない。作用範囲の違いを曖昧な同名操作へ
まとめず、必要な処理は`new` / `run` / `fmap`の合成で明示する。

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

仮に導入するなら`map_inner`はOption全体、`map_t`はbaseを含むrepresentation全体を変換する一方、
既存`fmap`はSome payloadだけを変換する。この差を暗黙に吸収するcompiler規則や候補APIは追加しない。

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

def map_left(
  self: EitherT<$L, $M, $A>,
  mapper: ($L -> $B)
) -> EitherT<$B, $M, $A>
def map_right(
  self: EitherT<$L, $M, $A>,
  mapper: ($A -> $B)
) -> EitherT<$L, $M, $B>
def bimap(
  self: EitherT<$L, $M, $A>,
  on_left: ($L -> $B),
  on_right: ($A -> $C)
) -> EitherT<$B, $M, $C>
```

各RTAは値引数から得られない入力だけを、上記の順で宣言する。call-siteのRTAを部分的な個数で指定しない。部分推論には既存の `_` を使う。

`map_left` / `map_right` / `bimap` は base の `$M` を `Functor::fmap` で一度開き、
内側の `Either` の同名 helper へ変換を委譲する。`map_left` は L、`map_right` は A、
`bimap` は両 slot を変更するが、いずれも base carrier M は保持する。Lを変える操作でも
普通の型変換として出力型を検査し、compiler に EitherT 固有の変換規則を追加しない。

`Applicative::ap(mapper, value)` は base Monad 上で `mapper.inner` を先に sequence する。
結果が `Left` なら `value.inner` は sequence せず、`Right(function)` のときだけ
`value.inner` を function で map する。これは受け取った inner computation の順序であり、
通常の関数引数式を遅延化する契約ではない。

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

期待型と既存のbare headによる指定を用いる。`do::<OptionT<Result,_>>` は、applied carrier専用のdo文法ではなく、
通常のTypeCtorTrait ReturnTypeArgumentとして扱う。captured baseとmapped payloadは他の型制約から解決する。

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

## 11. N05で確定したinterface判断

必須のtrait contract・4型・通常値としての性質に加え、次のAPI細部をN05で採用・非採用まで固定した。

| ID | 状態 | 項目 | 意図・境界 |
|---|---|---|---|
| MT-SI01 | 実装済み・移管済み | OptionT/EitherTのmapping helperの標準採用 | 操作面を明示し、元container APIの全面複製をしない |
| MT-SI02 | 実装済み・移管済み | ReaderT/StateTの`map_t`非採用 | run結果へpointwiseに適用するか、保持関数全体を置換するかを同名overloadにしない |
| MT-SI03 | 実装済み・移管済み | helper名・引数順・RTA順 | 本書の最小API案を基準とし、既存関数の引数導入規則から外れない |
| MT-SI04 | 実装済み・移管済み | captured baseとTransformer stackのsource表記 | 言語拡張仕様のMT-I01の確定規則に従う。nominal型注釈のbare headと、RTA内の完全・部分型applicationを区別する |

N05 で採用する interface は次で固定する。

- MT-SI01: EitherT は captured Left と mapped Right の作用面を明示する
  `map_left` / `map_right` / `bimap` を採用する。OptionT の追加 mapping helper と、
  base representation 自体を変える `map_inner` / `map_t` は採用しない。
- MT-SI02: ReaderT / StateT の `map_t` は採用しない。run 結果の pointwise 変換と
  保持関数全体の置換を同名操作へまとめない。
- MT-SI03: §§5.2、6.2、7.2、8.2 の helper 名・引数順・RTA 順をそのまま採用する。
- MT-SI04: nominal 型注釈では `OptionT<Result, Int>` 等の完全な carrier を書き、
  call-site RTA では N04 の完全・部分 application と `_` の規則だけを使う。標準型名や
  impl 数から captured base / payload を補完する経路は追加しない。

MT-SI02で非採用を決めた比較対象:

```text
ReaderTのrun結果変換: M<A> -> N<B>
StateTのrun結果変換: M<(A,S)> -> N<(B,S)>

保持関数全体の置換:
  (R -> M<A>) -> (R -> N<B>)
  (S -> M<(A,S)>) -> (S -> N<(B,S)>)
```

会話の「representationへ接続」という説明だけでは、この二つは同一ではない。最小のnew/run/liftで迂回可能なので、曖昧なmap_tを先に追加しない。構文や型推論の特例で両方を受けない。

## 12. 受け入れ条件

| ID | 状態 | 検証 |
|---|---|---|
| MT-S01 | 実装済み・移管済み | 各new/runのrepresentation round-trip |
| MT-S02 | 実装済み・移管済み | fmapがpayloadだけを変え、captured型を保つ |
| MT-S03 | 実装済み・移管済み | pure/return/bind/apの型・順序・失敗保持 |
| MT-S04 | 実装済み・移管済み | OptionTのNoneとbase失敗を区別する |
| MT-S05 | 実装済み・移管済み | EitherTのLeftとbase失敗を区別する |
| MT-S06 | 実装済み・移管済み | ReaderTが同じ環境を次の計算へ渡す |
| MT-S07 | 実装済み・移管済み | StateTがbase内の次状態を引き継ぐ |
| MT-S08 | 実装済み・移管済み | 本follow-upで、OptionTのemptyがpure_M(None)でありbase emptyではないことを利用者文書と`@doc`へ補強 |
| MT-S09 | 実装済み・移管済み | 本follow-upで、ReaderT/StateTのAlternativeがbase capabilityを要求することと評価境界を補強 |
| MT-S10 | 実装済み・移管済み | 標準Result由来のErrを勝手にNone/Leftへ変換しない |
| MT-S11 | 実装済み・移管済み | 本follow-upで、通常field・関数・collection・Facetで値を保持・更新する検証先を固定 |
| MT-S12 | 実装済み・移管済み | REPLでnew/lift/fmap/runを別入力として具象型で扱える |
| MT-S13 | 実装済み・移管済み | carrier指定が曖昧な呼び出しを候補数から補完しない |
| MT-S14 | 実装済み・移管済み | 4 Transformerについてdo結果をpipelineと比較する。検証先は`../lib/tests/do.srt` |
| MT-S15 | 実装済み・移管済み | 本follow-upで、型正しいユーザ定義base/Transformerにも同じ経路が使える例と検証先を補強 |
| MT-S16 | 実装済み・移管済み | 有限・純粋な観測テストでMonad則とliftのpure/bind保存を確認する |
| MT-S17 | 実装済み・移管済み | IdentityT・抽象Transformer引数API・runtime辞書を追加しない |
| MT-S18 | 実装済み・移管済み | EitherTのmap_left/map_right/bimapが対象slotだけを変え、base carrierとbase failureを保つ |

法則は標準実装のテスト契約であり、任意のユーザ実装の数学的正しさを型検査だけで証明しない。

## 13. docs移管

言語機能は `docs/dev/Trait_system_spec.md` 側、標準型とhelperは標準 `.srt` の `@doc` と利用者ガイド側へ分離して移管する。非採用helperを実装済みAPI一覧に載せない。

N05では4型と各Trait implの`@doc`を標準sourceへ置き、利用者向けAPI、base失敗との区別、
条件付き`Alternative`、明示`lift`の境界を`../docs/site/monad-transformers.md`へ移管した。
N11で確定したTransformerの`do`接続も同ページと`../lib/tests/do.srt`へ反映した。
