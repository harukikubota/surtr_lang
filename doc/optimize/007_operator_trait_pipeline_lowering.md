# Operator Trait Pipeline Lowering

## 目的

- `|*>` / `|>=` を標準 trait dispatch に寄せ、`Result` / `List` 固定の compiler 分岐を surface 意味論から外す
- `Result` の map / bind 意味論を標準 Surtr コードへ移し、正本を `lib/types/result.srt` に置く
- 演算子由来 callsite には最適化候補 metadata を残し、将来 optimizer が安全に現行 `ResultMap` / `ResultBind` 相当の bytecode へ戻せるようにする

## Public Surface

標準 trait として次を追加する。

```surtr
deftrait Functor
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}

deftrait Applicative
where
  Self: Functor
{
  def pure(value: $A) -> Self<$A>
  def apply(mapper: Self<($A -> $B)>, value: Self<$A>) -> Self<$B>
}

deftrait Monad
where
  Self: Applicative
{
  def bind(self: Self<$A>, mapper: ($A -> Self<$B>)) -> Self<$B>
}
```

演算子の外部契約は変えない。

- `lhs |*> rhs` は `Functor::fmap(lhs, rhs)` と同じ値を返す
- `lhs |>= rhs` は `Monad::bind(lhs, rhs)` と同じ値を返す
- 右辺が call 式の場合は、従来どおり左辺値を第一引数へ注入した unary callable として扱う
- `|*>` は plain function を要求し、contextual output を返す RHS には `|>=` を案内する
- `|>=` は contextual function を要求し、plain output を返す RHS には `|*>` を案内する

標準実装は `Result` / `List` に提供する。`Result` は compiler 直生成ではなく、次の形を Surtr コードの正本にする。

```surtr
impl Functor for Result<$T> {
  def fmap(self: Result<$A>, mapper: ($A -> $B)) -> Result<$B> {
    match self {
      Ok(value) => Ok(mapper(value)),
      _ => self,
    }
  }
}

impl Monad for Result<$T> {
  def bind(self: Result<$A>, mapper: ($A -> Result<$B>)) -> Result<$B> {
    match self {
      Ok(value) => mapper(value),
      _ => self,
    }
  }
}
```

`List` は既存 helper へ委譲する。

```surtr
impl Functor for List<$T> {
  def fmap(self: List<$A>, mapper: ($A -> $B)) -> List<$B> {
    List::map(self, mapper)
  }
}

impl Monad for List<$T> {
  def bind(self: List<$A>, mapper: ($A -> List<$B>)) -> List<$B> {
    List::flat_map(self, mapper)
  }
}
```

## 実装方針

- `lib/traits/operator/{functor,applicative,monad}.srt` を追加する
- 標準定義ソース load order は `List` / `Result` より前に `Functor` / `Applicative` / `Monad` を読む
- `lib/types/result.srt` に 3 trait の impl を追加し、`Result` map / apply / bind の意味論を Surtr source へ移す
- `lib/types/list.srt` に 3 trait の impl を追加し、既存 `List::map` / `List::flat_map` へ委譲する
- Scar の trait target 解決で `Ty::List(_) -> "List"` を許可し、generic builtin container を trait impl target として扱えるようにする
- `check_context_map` / `check_context_bind` は hardcoded `Result` / `List` 分岐ではなく、operator origin 付きの trait method call を構築する
- Forge の通常経路では `TypedInner::ResultMap` / `TypedInner::ResultBind` を emit せず、選択された Surtr impl への user function call として扱う

trait method call 解決では、選択された impl head から戻り値型を具体化する。

- `Functor for Result<$T>` を選んだ場合、`Self<$B>` を `Result<$B>` に具体化する
- `Monad for Result<$T>` を選んだ場合も、`Self<$B>` を `Result<$B>` に具体化する
- `List` でも同じく `List<$B>` を具体化する
- user-defined container でも impl head と receiver / RHS function type から戻り値を決める

## Optimization Metadata

v1 では最適化を実行しない。ただし operator 由来 callsite を後段で識別できるよう、typed IR に metadata を残す。

候補形は `TypedInner::TraitCall` への origin 追加を第一候補とする。

```rust
pub enum TraitCallOrigin {
    Explicit,
    Operator {
        op: OperatorTraitOp,
        lhs_ty: Ty,
        rhs_ty: Ty,
    },
}

pub enum OperatorTraitOp {
    PipeMap,
    PipeBind,
}
```

保存する情報は次に固定する。

- 演算子種別: `PipeMap` または `PipeBind`
- operator span: エラー表示と source map に使う
- LHS typed node: optimizer が match scrutinee を復元するために使う
- RHS typed node: optimizer が `Ok` branch の callable apply を復元するために使う
- selected impl: `Result` / `List` / user-defined container のどの impl を選んだか
- resolved result type: trait head 具体化後の戻り値型

将来 optimizer は、次の条件をすべて満たす場合だけ direct lowering へ rewrite できる。

- callsite origin が `Operator`
- selected impl が標準 `Result` impl
- 標準 `Result` impl body が期待する canonical match shape と一致する
- `PipeMap` は `_ => self` と `Ok(value) => Ok(rhs(value))` の形である
- `PipeBind` は `_ => self` と `Ok(value) => rhs(value)` の形である
- rewrite 前後で observable error span と source map の primary span が変わらない

明示的な `Functor::fmap(...)` / `Monad::bind(...)` 呼び出しは `origin = Explicit` とし、v1 では最適化候補にしない。

## 診断互換

既存の user-facing error は可能な限り維持する。

- `|*>` の LHS に trait impl がない場合は、従来の `requires Result or List on the left` から `requires Functor implementation` へ移行してよいが、`Result` / `List` で使う operator であることを hint に残す
- `|>=` の LHS に trait impl がない場合は、`requires Monad implementation` とし、標準では `Result` / `List` / `Option` が実装済みであることを hint に出す
- `|*>` RHS が `Result` / `List` を返す場合は `Use |>=` を維持する
- `|>=` RHS が plain value を返す場合は `Use |*>` を維持する
- `Result` / `List` 混在は、標準 impl 選択失敗としてではなく「container context mismatch」として説明する

## テスト方針

最小検証コマンド:

```bash
cargo nextest run -p rune --test integration run_srt
cargo nextest run --workspace
```

追加する成功系:

- `Result` の `|*>` が標準 Surtr `Functor` impl 経由で既存出力と一致する
- `Result` の `|>=` が標準 Surtr `Monad` impl 経由で既存出力と一致する
- `List` の `|*>` / `|>=` が trait impl 経由で既存出力と一致する
- user-defined container が `Functor` / `Applicative` / `Monad` を実装すると `|*>` / `|>=` を使える
- operator RHS の call injection が trait 化後も `f(lhs, arg)` 相当になる

追加する失敗系:

- impl のない user-defined container は `|*>` / `|>=` を使えない
- `|*>` に contextual RHS を渡すと `|>=` を案内する
- `|>=` に plain RHS を渡すと `|*>` を案内する
- `Result` と `List` の混在は拒否する

追加する unit 観点:

- Scar typed IR が operator origin metadata を保持する
- operator origin metadata が LHS / RHS / selected impl / result type を持つ
- 明示的 `Functor::fmap(...)` は `origin = Explicit` になる
- Forge の通常 codegen が `ResultMap` / `ResultBind` 直生成を通らない

## 移行順序

1. 標準 trait source と loader / test helper の include order を追加する
2. `List` / `Result` に標準 impl を追加する
3. Scar の trait target 解決に `List` を追加する
4. trait impl head 由来の戻り値具体化を実装する
5. `|*>` / `|>=` を operator origin 付き trait call に下げる
6. Forge の direct `ResultMap` / `ResultBind` 通常経路を外す
7. 既存 pipeline tests と compile error fixtures を trait 化後の診断へ合わせる
8. optimizer rewrite は別タスクとして残す

## 完了条件

- `Result` map / chain の意味論が `lib/types/result.srt` の Surtr code で説明できる
- `|*>` / `|>=` は標準 trait impl を通って実行される
- 既存の `Result` / `List` pipeline usecase が同じ値を返す
- operator 由来 callsite が将来 optimizer 用 metadata を保持する
- direct `ResultMap` / `ResultBind` は v1 の通常 codegen では使われない

## 未実施にすること

- v1 では optimizer rewrite を実装しない
- v1 では明示的 `Functor::fmap(...)` / `Monad::bind(...)` を最適化しない
- `>*` / `>=>` の trait 化は今回の対象外とする
- `.eldr` に optimization metadata chunk を追加しない

## 次の改善候補

- `>*` / `>=>` を `Functor` / `Monad` metadata と同じ枠組みに寄せる
- canonical match shape fingerprint を Scar typed IR で持つか、Forge optimizer 入り口で計算するか決める
- `surtr dump --format json` で operator-origin callsite を確認できる viewer metadata を検討する
- `Result` direct lowering rewrite を実装する場合は、opcode 列 before / after を unit test で固定する
