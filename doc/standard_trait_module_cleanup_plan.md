# 標準 trait / module API 整理方針

最終更新: 2026-05-04

## 実施結果サマリ

この整理方針に対して、2026-05-04 時点で次を反映済み。

- `List::sort_by(values, cmp)` を追加済み
- `List::sort(values: List<Int>)` を追加済み
- `Option::to_result` は追加せず、`from(value, Result)` を正本として docs / spec / site を整合済み
- trait dispatch failure の implemented list は `TypeError.hint` を正本として統一済み
- site docs では trait を capability / operator dispatch / compatibility に分類済み
- REPL は OnceRead universe 前提のまま整理しつつ、既存 universe に対する `import` は可能という実装に合わせて docs を補正済み

一方で、`Compare` bound による `List::sort` / `List::max` / `List::min` の汎化までは今回入れていない。残件は末尾へ退避する。

## 目的

`where` clause を採用しない前提で、標準 trait と標準 module API のレベル感を揃える。

Surtr はシンプルな trait system を維持する。

- trait は method 契約のみを持つ
- 制約は `<$T: Trait>` または parameter position の `impl Trait` に限定する
- return position の `impl Trait` は禁止する
- `where` clause / multi-trait bounds / associated types / associated consts は採用しない
- 呼び出し時点では concrete data type が確定している前提で compile-time dispatch する

この方針により、標準 API は次のどれかに分類する。

1. 制約なしの concrete module helper
2. 振る舞いを関数引数として受け取る `*_by` helper
3. 単一 trait bound を要求する generic helper

## 対象外

### `Result`

`Result` module は今回の整理対象外とする。

理由:

- Surtr の signature 表現は制約が強いため、`Result` は user-friendly API を優先する
- `lift` / `with` / `flat_lift` / `flat_with` などは、多少 API 面が厚くても利用者向け convenience として扱う
- `Result` は `=?`、error chain、recover 系 special form と密接であり、trait 整理と同時に触ると影響範囲が広い

`Functor` / `Chainable` の `Result` 実装は operator lowering 契約として維持するが、今回の標準 API 整理では変更しない。

### REPL 中の incremental definition update

REPL 中に定義系を直接評価して trait impl universe を増分更新する設計は、今回の整理対象外とする。

方針:

- REPL は起動時に標準 module と指定 preload script を OnceRead する
- REPL 中の `include` は禁止する
- REPL 中の `defstruct` / `defenum` / `deftrait` / `impl` / `defmod` は禁止または staged error とする
- REPL 中の `import` は、起動時に読み込まれた固定 universe に対する既存 symbol 導入としてのみ扱う
- trait impl 候補一覧は、REPL 起動時に確定した compile universe に閉じる

理由:

- dynamic include / definition update を許すと、declaration index、trait impl index、Scar session、Forge session、VM function table の整合を毎回更新する必要がある
- error message が動的 table 参照になると、診断発生時点と表示時点で候補一覧がずれる
- `run` / `check` / `REPL OnceRead` を同じ「compile unit 確定後に診断を生成する」モデルで扱える

## 標準 trait の整理方針

### Capability trait

利用者が型に実装し、標準 API の単一 bound として使う trait。

維持する。

```surtr
deftrait Show {
  def to_string(self: Self) -> String
}

deftrait Eq {
  def eq(self: Self, rhs: Self) -> Boolean
}

deftrait Compare {
  def compare(self: Self, rhs: Self) -> Ordering
}

deftrait From<$To> {
  def from(self: Self, to: TypeRef<$To>) -> $To
}

deftrait TryFrom<$To> {
  def try_from(self: Self, to: TypeRef<$To>) -> Result<$To, Error>
}
```

`Json` など外部 domain の decode もこの層に置く。

```surtr
deftrait Decode<$To> {
  def decode(self: Self, to: TypeRef<$To>) -> Result<$To, DecodeError>
}
```

`Decode<User> for Json` のように concrete impl を置き、`where Json: Decode<$T>` のような制約構文は導入しない。

### Operator dispatch trait

演算子 lowering の正本として維持する trait。

- `Add` / `Sub` / `Mul`
- `Eq` / `Neq`
- `Lt` / `Lte` / `Gt` / `Gte`
- `Concat`
- `PipeApply`
- `Compose`
- `Functor`
- `Chainable`
- `Composable`
- `LiftComposable`
- `KleisliComposable`

方針:

- 通常の標準 module API の設計では、これらをなるべく前面に出さない
- operator と helper alias の dispatch 契約として文書化する
- user-defined 型で演算子を使いたい場合だけ実装対象にする

Open / closed 方針:

- `|>` / `>>` に対応する `PipeApply` / plain function composition は closed 寄りに扱う
- それ以外の operator trait は user impl 可能な open trait として扱う
- open trait のエラー診断では、標準 impl と user impl の両方を候補一覧に含める

### `Numeric`

`Numeric` は算術演算子全体の親 trait ではない。

位置づけ:

- `+` / `-` / `*` は `Add` / `Sub` / `Mul`
- `Numeric` は `safe_div` / `abs` / `min` / `max` の helper capability
- `List::sum` など数値 helper の bound として使う

### `Compare` / `Ord`

`Compare` を新規 API の正本にする。

- `Compare::compare(self, rhs) -> Ordering` は sort / min / max などの三値比較 API に使う
- `<` / `<=` / `>` / `>=` は `Lt` / `Lte` / `Gt` / `Gte` の operator dispatch trait に任せる
- `Ord` は互換用 grouped Boolean helper とし、新規標準 API の bound には使わない

作業方針:

- docs では `Ord` を advanced / compatibility 扱いへ下げる
- `List::sort` / `List::max` / `List::min` などの将来 API は `Compare` bound にする
- `Ord` の削除は別判断とし、今回の整理では deprecated 候補として記録するだけにする

## 標準 module API の整理方針

### 基本ルール

標準 module 関数は、必要な能力だけを関数単位で要求する。

`defmod` / `impl Type` 自体に generic parameter や bound を持たせない。

```surtr
impl List {
  def map(values: List<$A>, f: ($A -> $B)) -> List<$B>
  def sort<$A: Compare>(values: List<$A>) -> List<$A>
}
```

### `List`

優先整理対象。

`List` は関数単位 bound の基準例にする。

#### 制約なし

- `cons`
- `first`
- `last`
- `len`
- `append`
- `flat_map`
- `concat`
- `reverse`
- `at`
- `reduce`
- `reduce_while`
- `map`
- `filter`
- `find`
- `find_map`
- `any`
- `all`
- `count`
- `take`
- `drop`
- `take_while`
- `drop_while`
- `span`
- `partition`
- `zip`

#### 関数引数で振る舞いを受け取る

- `max_by(values, cmp)`
- `min_by(values, cmp)`
- `sort_by(values, cmp)` を追加済み

```surtr
def sort_by(values: List<$A>, cmp: ($A, $A -> Ordering)) -> List<$A>
```

#### trait bound を要求する

- `group_count<$A: Eq>`
- `dedup<$A: Eq>`
- `max<$A: Compare>`
- `min<$A: Compare>`
- `sort<$A: Compare>` は将来候補にする
- `sum<$A: Numeric>` は検討対象

```surtr
def group_count<$A: Eq>(values: List<$A>) -> List<($A, Int)>
def dedup<$A: Eq>(values: List<$A>) -> List<$A>
def max<$A: Compare>(values: List<$A>) -> Result<$A, NoneError>
def min<$A: Compare>(values: List<$A>) -> Result<$A, NoneError>
def sort<$A: Compare>(values: List<$A>) -> List<$A>
```

短期判断:

- 今回は `sort_by` を追加し、`sort` は `List<Int>` 固定で導入した
- 今回は `sum` / `max` / `min` を `List<Int>` 固定のまま維持した
- trait 整理を進める次段階で `Compare` / `Numeric` generic 化を検討する

### `HashMap`

整理対象だが、trait bound は基本的に不要。

理由:

- key は `String` 固定
- value `$V` の能力を要求しない操作が中心
- deterministic order は key order で決める

維持する API:

- `empty`
- `from_entries`
- `len`
- `contains_key`
- `get`
- `insert`
- `remove`
- `keys`
- `values`
- `entries`
- `map_values`

方針:

- `$V` に `Eq` / `Compare` / `Show` を要求しない
- value の能力が必要な API を追加する場合だけ関数単位 bound を置く

### `Option`

整理対象。

基本 helper は小さく保つ。

- `is_some`
- `is_none`
- `wrap`

`Option` は `SafeBind` 対象ではないため、`Result` へ明示変換する導線を docs で明確にする。

```surtr
from(value, Result)
```

`Functor` / `Chainable` / compose 系 trait impl は operator lowering 契約として維持する。

### `Int` / `Float` / `String` / `Boolean`

concrete type owner API と capability trait 実装を分けて説明する。

#### Concrete owner API

- `Int::parse_*`
- `Int` bit 操作
- `String::split` / `trim` / `codepoints`
- `Boolean::not` / `xor` / `eqv` / `implies`
- `Float::abs` / `min` / `max`

#### Capability trait impl

- `Show`
- `From`
- `TryFrom`
- `Eq`
- `Neq`
- `Compare`
- `Lt` / `Lte` / `Gt` / `Gte`
- `Numeric`

方針:

- `Int::abs` と `Numeric::abs` のような重複は許容する
- docs では `Int::abs` は concrete helper、`Numeric::abs` は generic bound 用 trait method として説明する
- `String::try_to_int` / `String::try_to_boolean` は convenience helper とし、conversion の正本は `try_from(value, Int)` / `try_from(value, Boolean)` に寄せる

### `Regex`

trait 化しない。

`Regex` / `RegexCaptures` / `RegexMatch` の owner API として維持する。

- `Regex::compile`
- `Regex::is_match`
- `Regex::captures`
- `Regex::find`
- `Regex::find_all`
- `Regex::split`
- `Regex::replace`
- `Regex::replace_all`
- `Regex::escape`
- `Regex::group_names`
- `RegexCaptures::*`
- `RegexMatch::*`

### `Random`

trait 化しない。

方針:

- `Random::int_until` / `Random::int_range` は ambient random API
- `Random::seed` / `Random::next_*` は explicit `RandomGenerator` API
- 同じ module 内に置くが、docs では implicit / explicit RNG を分けて説明する

### `IO` / `Process` / `Task`

trait 化しない。

理由:

- 副作用あり
- runtime policy と結びつく
- special lowering / hidden builtin が多い

方針:

- public API と `@@hidden __*` builtin を docs 上で分離する
- `Task::call` / `async` / `launch` / `cast` は `Result` 文脈に固定し、operator trait へ寄せない

### `StyledDoc`

tooling / presentation helper として扱う。

trait 整理とは分離する。

方針:

- builder DSL として維持する
- 将来 `Show for StyledDocDoc` を追加する場合は、`plain` と `to_ansi` のどちらを `to_string` の正本にするか先に決める

### `Test`

整理対象。

`assert_eq` は本来 `Eq` を要求する。

```surtr
def assert_eq<$A: Eq>(expected: $A, actual: $A) -> Result<()>
```

失敗表示は `inspect` fallback を使う。

理由:

- `Eq + Show` の multi-trait bound は採用しない
- test assertion は user-friendly であるべきだが、trait system を広げる理由にはしない

### `Config` / `Project`

project definition builder API として扱う。

trait 整理とは分離する。

## trait 系エラーメッセージ整理方針

trait 系診断も今回の整理対象に含める。

目的:

- open trait を増やしても、利用者が「何が足りないか」と「現在使える型」をすぐ見られるようにする
- `run` / `check` / REPL で同じ trait diagnostic policy を使う
- エラー表示経路は動的 table 参照ではなく、発生時点で自己完結した message / hint を持つ

### 基本方針

trait dispatch failure では、可能な限り implemented list を hint に出す。

```text
Add is implemented for: Duration, Float, Int, Vec2.
```

候補一覧は、診断発生時点の compile universe で可視な impl から作る。

- 標準 impl を含める
- user impl を含める
- hidden / internal-only 型は除外する
- generic impl は signature を保って表示する

例:

```text
From<Option<$T>> is implemented for: Result<$T>.
Functor<$A, $B, List<$B>> is implemented for: List<$A>.
```

### message と hint の分担

主原因は `message` に置く。

候補一覧と次の行動は `hint` に置く。

例:

```text
message:
  `+` requires Add, but Boolean does not implement Add

hint:
  Add is implemented for: Duration, Float, Int, Vec2.
  Add a `<$T: Add>` bound for generic values, or use one of the implemented types.
```

非演算子 trait の例:

```text
message:
  Decode<User>::decode requires Json to implement Decode<User>

hint:
  Decode<User> is implemented for: String, JsonValue.
```

### dynamic table 参照は採用しない

`TypeError` には発生時点で rendered summary を入れる。

```rust
TypeError {
    message,
    hint: Some("Add is implemented for: Duration, Float, Int, Vec2.".into()),
}
```

理由:

- `run` は compile unit が一度だけ確定するため、別 table を持つ必要がない
- REPL も OnceRead universe に閉じるため、候補一覧はセッション中不変でよい
- JSON diagnostic / log / deferred display で表示内容がずれない
- 成功パスでは候補一覧を生成せず、dispatch failure 時だけ summary を作ればよい

### 整理対象となる診断経路

Scar 側:

- explicit trait call
  - `Add::add(1, False)`
  - `Show::to_string(value)`
  - `From::from(value, TypeRef)`
  - `TryFrom::try_from(value, TypeRef)`
  - 将来の `Decode::decode(value, TypeRef)`
- arithmetic operator
  - `+`
  - `-`
  - `*`
- equality / comparison operator
  - `==`
  - `!=`
  - `<`
  - `<=`
  - `>`
  - `>=`
- concat operator
  - `++`
- container / flow operator
  - `|*>`
  - `|>=`
  - `>*`
  - `>=>`
- compose operator
  - `/`
  - `>>`

Diagnostics 側:

- `diagnostics::TypeErrorDiagnostic` は `hint` をそのまま help として扱う
- binary operator heuristic が hint を消さないことを確認する
- callable signature hint と implemented list が競合しないようにする
- JSON diagnostic の `hint` に implemented list が残ることを確認する

Rune / Xldr 側:

- `rune run`
- `rune check`
- `rune test` の compile error path
- `xldr` REPL の typecheck error path
- REPL 起動時 OnceRead preload の trait impl universe

### 表示ルール

候補一覧:

- 安定順で表示する
- まずは全件表示でよい
- 将来 impl が増えすぎた場合に備え、summary helper は一箇所に寄せる

表示名:

- concrete type は user-facing type name で表示する
- generic target は `List<$A>` / `Result<$T>` のように型引数を含める
- trait args つき trait は requested args を反映する

例:

```text
From<String> is implemented for: Boolean, Error, Float, Int, String, Unit.
TryFrom<Int> is implemented for: String.
Decode<User> is implemented for: Json.
```

bound 不足と concrete impl 不足は分ける。

- generic value に bound がない場合: `<$T: Add>` などの bound 追加を案内する
- concrete type が未実装の場合: implemented list から使える型を案内する

### coherence / overlap

open trait を増やす場合、診断とは別に impl overlap 方針も固定する必要がある。

短期方針:

- 現行の `(Trait instance, target type)` 一意制約を維持する
- generic impl と concrete impl が重なりうる場合は conservative に禁止する
- `From` / `TryFrom` の相互排他は維持する

例:

```surtr
impl Decode<$A> for Json { ... }
impl Decode<User> for Json { ... }
```

上記のように重なりうる impl は、特別な specialization ルールを導入しない限り禁止候補とする。

## 作業内容

### 1. 正本仕様の整理

対象:

- `doc/要件定義v9.md`
- `docs/site/language-reference.md`
- `docs/site/standard-library.md`
- `docs/site/trait-impls.md`

作業:

- `where` を採用しない方針を明記する
- 標準 trait を capability trait / operator dispatch trait / compatibility trait に分類する
- `Ord` を新規 API の bound に使わない方針を明記する
- `Result` module は user-friendly API として整理対象外にする
- `|>` / `>>` 以外の operator trait を open trait として扱う方針を明記する
- REPL は OnceRead universe に閉じ、REPL 中の `include` / 定義系増分更新は対象外にする

### 2. `List` API 方針の反映

対象:

- `lib/types/list.srt`
- `doc/要件定義v9.md`
- `docs/site/standard-library.md`
- `tests/spec/stdmod/list_helpers.srt`
- `tests/compile_errors/**`

作業候補:

- `group_count<$A: Eq>` / `dedup<$A: Eq>` へ bound を明示する
- `sort_by` を追加する
- `sort<$A: Compare>` を追加する
- `max` / `min` の `Compare` generic 化を検討する
- `sum` の `Numeric` generic 化を検討する

注意:

- 既存 `List<Int>` 固定 API を急に壊さない
- generic 化はテストを先に追加して段階導入する

### 3. `Option` 変換導線の整合

対象:

- `lib/types/option.srt`
- `doc/要件定義v9.md`
- `docs/site/language-reference.md`
- `tests/spec/stdmod/option_helpers.srt`

作業:

- `Option::to_result(value, err)` は追加しない
- `Option` が `SafeBind` 対象外であることと、`from(value, Result)` を使う導線を docs に揃える

### 4. primitive owner API と trait impl の説明整理

対象:

- `lib/types/int.srt`
- `lib/types/float.srt`
- `lib/types/string.srt`
- `lib/types/boolean.srt`
- `docs/site/standard-library.md`

作業:

- concrete helper と capability trait impl の役割を分けて文書化する
- `Numeric` は arithmetic parent ではなく helper capability と明記する
- `String::try_to_*` は convenience helper、`TryFrom` は conversion 正本と明記する

### 5. operator-only trait の露出整理

対象:

- `lib/traits/operator/*.srt`
- `docs/site/language-reference.md`
- `docs/site/function-operators.md`
- `docs/site/pipe-operators.md`

作業:

- `Functor` / `Chainable` / compose 系は operator lowering 契約として説明する
- 標準 module API の一般設計では前面に出さない
- `Compose` / `Composable` の名前の近さは将来整理候補として記録する

### 6. public API と hidden builtin の分離

対象:

- `lib/process.srt`
- `lib/test.srt`
- `lib/kernel.srt`
- `docs/site/standard-library.md`

作業:

- `@@hidden __*` builtin は public API 一覧から分離する
- user-facing helper と compiler/runtime internal surface の境界を明記する

### 7. trait 系エラーメッセージ改善

対象:

- `crates/scar/src/checker/predeclare.rs`
- `crates/scar/src/checker/expr.rs`
- `crates/scar/src/checker/definitions.rs`
- `crates/scar/src/checker/specialize.rs`
- `crates/diagnostics/src/typecheck.rs`
- `crates/diagnostics/src/heuristics/*.rs`
- `crates/rune/src/compile.rs`
- `crates/xldr/src/repl/logic/core.rs`
- `tests/compile_errors/**`
- `crates/scar/src/typecheck_surface_tests.rs`
- `crates/diagnostics/src/tests/typecheck.rs`
- `crates/xldr/tests/repl_core.rs`

作業:

- `trait_implementation_summary` を user impl / generic impl 表示に対応させる
- hidden / internal-only 型を implemented list から除外する
- explicit trait call failure に implemented list を出す
- arithmetic / equality / comparison / concat operator failure に implemented list を出す
- `Functor` / `Chainable` / compose 系 operator failure に implemented list を出す
- requested trait args つき summary を出す
- `message` と `hint` の分担を統一する
- `diagnostics` の binary operator heuristic が implemented list を落とさないようにする
- `rune run/check` と REPL で同じ hint が見えることをテストする
- JSON diagnostic の `hint` に implemented list が残ることをテストする

注意:

- 成功パスで候補一覧を構築しない
- エラー発生時に rendered summary を `TypeError.hint` に入れる
- dynamic table 参照は導入しない

## 完了条件

- `where` なしで標準 trait / module API の拡張方針を説明できる
- 新規標準 API が「制約なし」「`*_by`」「単一 trait bound」のどれかに分類される
- `Result` は整理対象外として明確に扱われる
- `List` / `Option` / primitive owner API の整理対象が作業単位に分解されている
- docs と `lib/*.srt` の `@@doc` が同じレベル感で説明される
- trait dispatch failure で標準 impl / user impl の implemented list を一貫して表示できる
- `run` / `check` / REPL / JSON diagnostic で trait hint が同じ方針で保持される
- REPL は OnceRead universe 前提で、trait impl index の動的更新を要求しない

## 残タスク

- `List::sum<$A: Numeric>` の汎化要否の判断
- `group_count<$A: Eq>` / `dedup<$A: Eq>` の明示 bound を current parser / checker 制約の範囲で surface に出すかの判断
- `Compose` / `Composable` の命名整理を本当に進めるかの別件判断

今回完了:

- `List::sort<$A: Compare>`、`List::max<$A: Compare>`、`List::min<$A: Compare>` の汎化
