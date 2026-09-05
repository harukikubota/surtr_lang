# List `Packed` 表現・`flat_map` builtin 化 実装計画

## 1. 状態と正本関係

本書は、現行 Surtr コンパイラへ List の内部表現追加と `List::flat_map` の
runtime 最適化を導入するための実装入力である。

本書は現時点では draft とし、次を正本として優先する。

- 言語全体の意味論: [`要件定義v9.md`](./要件定義v9.md)
- VM と execution context: [`../docs/dev/EldrVM_spec.md`](../docs/dev/EldrVM_spec.md)
- TypeConstructor / Monad dispatch: [`../docs/dev/Trait_system_spec.md`](../docs/dev/Trait_system_spec.md)
- `do` intrinsic: [`do_intrinsic_spec.md`](./do_intrinsic_spec.md)
- テスト配置: [`../docs/dev/テスト方針.md`](../docs/dev/テスト方針.md)
- user-facing diagnostics: [`../docs/dev/diagnostics.md`](../docs/dev/diagnostics.md)

実装着手時は、`要件定義v9.md` の「VM 上で cons cell 表現を前提とする」という
暫定記述を、本書の外部契約に合わせて先に更新する。本書と更新前の正本が衝突する間は、
本書だけを根拠に実装を開始しない。

## 2. 結論

初期改修では、次の構成を採用する。

```text
surface
  List<$A>
      |
runtime-only representation
      +-- Empty
      +-- Cons
      `-- Packed

List::flat_map
      |
      `-- CallBuiltin(list_flat_map)
              |
              `-- ListBuilder<Vec<Value>>
                      |
                      `-- finish -> Packed ListHandle

Monad::bind for List
      `-- List::flat_map
```

List の型、順序、pattern matching、永続性は変更しない。`Packed` と `ListBuilder` は
runtime-only implementation detail とし、user code、型検査、`.eldr` の Value 種別へ
新しい surface type を追加しない。

`do` は本改修の実装対象に含めない。将来 `do` が導入されたときも、初期実装は
正本どおり resolved `Monad::bind` へ generic lowering し、List 専用 opcode、専用 frame、
nested-builder fusion は追加しない。

## 3. 現行実装の基準線

現行実装は次の状態にある。

- `sindr::runtime::ListHandle` は `head: Option<Rc<ListNode>>` と `len: usize` を持つ
  persistent cons list である。
- `ListHandle::cons`、`head_value`、`tail_handle`、`iter` が List runtime API の中心である。
- `ListEmpty` / `ListNil` / `ListCons` / `ListIsEmpty` / `ListHead` / `ListTail` /
  `ListFromItems` opcode が存在する。
- `ListFromItems` と Rust 側で List を返す builtin は、多くの場合
  `ListHandle::from_items(Vec<Value>)` を利用する。
- `List::flat_map` は `lib/types/list.srt` に通常の Surtr 関数として定義されている。
- 現行 `flat_map` は `reduce + _append_reverse + reverse` であり、単一呼び出しの
  漸近計算量はすでに `O(N + M)` である。ここで `N` は入力要素数、`M` は mapper が
  返した全 List の合計要素数である。
- `impl Monad for List` の `bind` はすでに `List::flat_map(self, mapper)` を呼んでいる。
- callable composition の runtime には `CallableTemplateComposeFlavor::ListBind` があり、
  `Vec<Value>` に flatten してから `ListHandle::from_items` する類似処理が存在する。
- REPL は別の List interpreter を持たず、compile した chunk を `InteractiveVm` / Eldr で
  実行する。
- `do` surface はまだ実装されておらず、`do_intrinsic_spec.md` の開始条件を満たした後に
  generic lowering として追加する計画である。

したがって `flat_map` builtin 化の主目的は、計算量クラスの改善ではない。主目的は、

- intermediate cons node allocation の削減
- 連続 buffer を使った局所性の改善
- Rust runtime 内にすでにある List flatten 処理との共通化
- 将来の generic `do -> Monad::bind -> List::flat_map` 経路の一定化

である。

## 4. 目的

- List を先頭追加・先頭分解中心の persistent sequence として維持する。
- List の生成 origin によらず `cons` / `uncons` / `head` / `tail` を `O(1)` にする。
- Rust 側で一括生成される List を要素ごとの `Rc<ListNode>` に変換しない。
- `List::flat_map` の結果構築を一つの内部 Builder に集約する。
- `Monad::bind for List` の source-level 定義を維持する。
- batch 実行と REPL で同じ bytecode、builtin metadata、Eldr runtime 実装を使う。
- representation の違いを equality、display、iteration、pattern matching から観測不能にする。

## 5. 非目標

初期改修では次を行わない。

- `append` を `O(1)` にする Concat tree / rope の導入
- lazy `reverse`
- List 専用 `do` opcode / runtime frame
- nested `flat_map`、`map`、`filter` pipeline の fusion
- `map` / `filter` / `reverse` / `append` / `concat` の一括 builtin 化
- user-visible `ListBuilder` 型
- runtime Trait dictionary または dynamic Monad dispatch
- process 間送信時の強制 deep copy
- Packed buffer の chunk 化や retained prefix の自動 compaction

## 6. List の外部契約

user-visible type は常に `List<$A>` とする。

```surtr
[]
[1, 2, 3]
[head, ..tail]
List::cons(head, tail)
```

次を外部不変条件とする。

1. List は immutable persistent sequence である。
2. `List::cons(value, list)` は logical front に一要素を追加する。
3. MatchBlock の `[head, ..tail]` と `Kernel::uncons` は、physical representation ではなく
   logical first と logical rest を返す。
4. `append(left, right)` は `left` の全要素の後に `right` の全要素を置く。
5. `reverse(values)` は logical order を反転する。
6. display、inspection、iteration、equality は logical order を基準にする。
7. Empty / Cons / Packed の区別は user code、診断、stack trace、REPL 表示へ露出しない。

最低限、次の代数則を満たす。

```text
append([], xs) == xs
append(xs, []) == xs
append(append(xs, ys), zs) == append(xs, append(ys, zs))
reverse(reverse(xs)) == xs
map(identity, xs) == xs
flat_map(xs, return) == xs
```

ここで `==` は physical node equality ではなく、logical-order elementwise equality を表す。

## 7. Runtime representation

### 7.1 概念型

初期実装は次の概念モデルに合わせる。

```rust
pub struct ListHandle {
    repr: ListRepr,
    len: usize,
}

enum ListRepr {
    Empty,
    Cons(Rc<ListConsNode>),
    Packed(PackedList),
}

struct ListConsNode {
    value: Value,
    tail: ListHandle,
}

struct PackedList {
    items: Rc<Vec<Value>>,
    offset: usize,
}
```

正確な Rust 型名と `Rc<Vec<Value>>` / `Rc<[Value]>` の選択は実装に合わせてよい。
ただし Builder が所有していた要素を clone せず、所有権移動で immutable backing storage に
変換できることを優先する。

### 7.2 不変条件

- `len == 0` と `ListRepr::Empty` は同値である。
- Empty 以外では `len > 0` である。
- `Cons` の `len` は `tail.len + 1` である。
- `Packed` では `offset < items.len()` かつ `len == items.len() - offset` である。
- 空の Packed value は作らず Empty に正規化する。
- `Cons` の tail は Empty / Cons / Packed のいずれでもよい。
- Packed backing storage は ListHandle 経由で mutation しない。
- `ListHandle` clone は logical value の clone であり、内部の immutable storage を共有してよい。
- `ListHandle` / `Value` の `PartialEq` は representation を比較せず、長さと logical iterator を
  比較する。

`usize` length は本案では許容する。Concat DAG を作らず、logical element count が実際に保持する
Value 数を指数的に超えないためである。すべての `len + 1` と buffer length 変換は checked API を
使い、wraparound は許さない。

### 7.3 Empty

空 List の canonical representation とする。空の Cons / Packed は許可しない。

### 7.4 Cons

`cons(value, tail)` は、tail の representation を変換せず新しい Cons node を一つ作る。

```text
cons(a, Packed([b, c, d], 0))
=> Cons(a, Packed([b, c, d], 0))
```

`cons`、`head`、`tail` は worst-case `O(1)` とする。

### 7.5 Packed

Packed は List literal、Rust builtin、Builder などが複数要素を一括生成するときに使う。

```text
Packed([a, b, c, d], offset = 0)
```

Packed の tail は backing storage をコピーせず offset を一つ進める。

```text
head = a
tail = Packed([a, b, c, d], offset = 1)
```

このため Packed の `head` / `tail` / `uncons` は worst-case `O(1)` である。

初期実装では suffix が backing buffer 全体を保持することを許容する。たとえば大きな Packed List の
最後の一要素だけを保持しても、元 buffer は解放されない。この retained-prefix 特性は意味論ではなく
memory trade-off であり、実測上問題になった場合に bounded chunk または compaction policy を別途導入する。

### 7.6 Iterator

`ListHandle::iter()` は Cons / Packed の混在を logical order で走査する。

- Cons では value を返して tail へ進む。
- Packed では `offset..items.len()` を順に返す。
- Cons の tail が Packed なら、そのまま Packed 走査へ切り替える。

iterator 初期化、各 `next` は worst-case `O(1)`、全走査は `O(n)` とする。

## 8. 操作別計算量

| Operation | 初期契約 |
|---|---:|
| empty / `len` | `O(1)` |
| `cons` | worst-case `O(1)` |
| `uncons` / `head` / `tail` | worst-case `O(1)` |
| full logical traversal | `O(n)` |
| `List::append(left, right)` | `O(length(left))` time / space |
| `List::concat(parts)` | `O(number_of_parts + total_elements)` |
| `List::reverse(values)` | `O(n)` |
| `List::map` / `filter` | `O(n)` |
| `List::flat_map(values, mapper)` | `O(N + M + mapper_cost)` |
| display / logical equality | `O(n)`、element operation の cost を除く |

`append` は left side の logical elements を新しい spine へコピーし、right side は共有してよい。
これは現行の Elixir 的な List 契約を維持する。`List::append` と二項 `++` を混同しない。
現時点では List に `Concat` trait / `++` surface は定義されていない。

`List::concat` の surface は二項関数ではなく、`List<List<$A>>` を受ける一項関数である。

## 9. ListBuilder

### 9.1 所有権

`ListBuilder` は Eldr 内部だけの Rust 型とする。

```rust
struct ListBuilder {
    items: Vec<Value>,
}
```

`Value` variant、Sindr builtin type、Surtr source type、bytecode constant には追加しない。
したがって次は構造上発生しない。

- user binding への保存
- closure capture
- process state / message payload 化
- REPL result 化
- `.eldr` serialization

### 9.2 操作

```text
push(value)
extend_list(list)
finish() -> ListHandle
```

- `push` は amortized `O(1)`。
- `extend_list` は logical iterator を使い `O(length(list))`。
- Packed の未消費範囲には slice clone の fast path を使用してよい。
- `finish` は空なら Empty、それ以外なら Builder の buffer 所有権を移した Packed を返す。
- `finish` 後の Builder 再利用は Rust ownership により禁止する。

`Value` は一般に `Copy` ではないため、Packed fast path も要素 clone 自体を省略できるとは限らない。
本仕様が保証するのは logical order と計算量であり、memcpy の使用ではない。

## 10. `List::flat_map` builtin

### 10.1 surface

公開 surface は引き続き次とする。

```surtr
@builtin def flat_map(values: List<$A>, mapper: ($A -> List<$B>)) -> List<$B>
```

internal runtime name は `list_flat_map` とし、qualified surface identity
`List::flat_map` から metadata 経由で解決する。

### 10.2 実行意味論

```text
builder = ListBuilder::new()

for value in logical_order(values):
    mapped = invoke mapper(value) exactly once
    require mapped is List
    builder.extend_list(mapped)

return Value::List(builder.finish())
```

次を保証する。

- mapper は入力の logical order で呼ぶ。
- 各入力要素について mapper を一度だけ呼ぶ。
- mapper が返した List も logical order で flatten する。
- empty result は何も emit しない。
- runtime failure が発生した時点で処理を中断し、未完成 Builder を破棄する。
- mapper の戻り値が List でない bytecode は RuntimeError とする。well-typed user source では到達しない。

### 10.3 builtin metadata

組込み関数追加は `crates/sindr/src/builtin.rs` の `BUILTIN_METAS` を起点にする。

- runtime name: `list_flat_map`
- arity: `2`
- signature: `(List<$A>, ($A -> List<$B>)) -> List<$B>`
- surface owner/name: `List::flat_map`
- parameter names: `values`, `mapper`

既存 builtin ID をずらさないため、新しい metadata と Eldr implementation entry は双方の table の
末尾へ同じ順序で追加する。

`flat_map` は user callable を呼び得る higher-order operation であり、callback が observable effect を
持ち得る。このため初期実装では専用 opcode を追加せず `CallBuiltin` を使用する。

### 10.4 Eldr helper の共通化

Eldr 内に一つの内部 helper を置き、少なくとも次の二経路から再利用する。

```text
builtin_list_flat_map
CallableTemplateComposeFlavor::ListBind
```

これにより `Monad::bind` と Kleisli composition の List flatten 順序、型ガード、Builder finalize を
別実装にしない。

### 10.5 scheduler 境界

現行 `BuiltinFn` は一回の同期呼び出しで完了し、callback には `VM::invoke_callable_sync` を利用できる。
したがって初期 `list_flat_map` も current heavy-builtin policy に従い、builtin loop 自体は quantum ごとの
preemptible continuation にしない。

これは現行 VM へ合わせた実装上の制約であり、将来の scheduler fairness 保証ではない。大規模 List と
process workload の benchmark で問題が確認された場合は、List 固有 do frame ではなく、まず汎用の
builtin continuation / resumable higher-order builtin として設計する。

## 11. `Monad::bind` と `do`

### 11.1 `Monad::bind`

現行 source-level 定義を維持する。

```surtr
impl Monad for List<$T> {
  def bind(self: List<$A>, mapper: ($A -> List<$B>)) -> List<$B> {
    List::flat_map(self, mapper)
  }
}
```

`bind` 自体を別 builtin にせず、concrete Trait dispatch 後もこの source definition を経由させる。
最適化と runtime type guard は `List::flat_map` に集約する。

### 11.2 `do`

`do` 実装時の初期経路は次とする。

```text
do::<List> { ... }
    -> resolved Monad::bind + continuation
    -> impl Monad for List
    -> List::flat_map
    -> CallBuiltin(list_flat_map)
```

`do` checker は List 名、`flat_map` 名、builtin ID、Builder の存在を検査しない。
carrier inference、partial pattern、SafeBind、`Alternative::empty` は
`do_intrinsic_spec.md` の generic contract だけで解決する。

nested `do` も通常の関数結果境界を使う。inner computation が返すのは完成済み `ListHandle` であり、
inner Builder を outer computation と共有しない。この性質は専用の escape analysis ではなく、各
`list_flat_map` builtin call の終了時に必ず `finish` することで得る。

## 12. REPL と process runtime

### 12.1 REPL

REPL 専用 representation や未完成 Builder は導入しない。

`InteractiveVm` も batch VM と同じ `Value::List`、List opcode、builtin ID、Eldr implementation を使う。
REPL chunk の実行結果、`last_result`、checkpoint に現れる List は、常に完成済み `ListHandle` である。

### 12.2 process runtime

Surtr process 間の payload は value semantics を持つ。List は immutable なので、runtime は同一 VM 内で
`Rc` backing storage を structural share してよい。process 間の physical pointer 非共有や常時 deep copy は
user-visible 契約にしない。

保証するのは次である。

- receiver 側から sender 側の List を mutation できない。
- ListBuilder は `Value` ではないため process state / payload に入らない。
- representation sharing の有無は equality、display、evaluation order から観測できない。

将来、別 thread / 別 VM / serialization boundary を導入する場合は、その boundary で Send-safe storage、
serialization、deep copy のいずれを使うかを ProcessRuntime spec 側で定める。

## 13. 実装順序

### Phase 0: 正本と baseline

1. `doc/要件定義v9.md` の cons-cell 固定記述を、外部 List 契約と内部 representation 非公開へ更新する。
2. `docs/dev/テスト方針.md` の `ListHandle` invariant を Empty / Cons / Packed / mixed representation へ拡張する。
3. 現行 `flat_map` の順序、callback 回数、出力、allocation / elapsed time の baseline を記録する。

### Phase 1: Hybrid `ListHandle`

1. `ListHandle` の field 直接参照を accessor へ閉じ込める。
2. Empty / Cons / Packed representation を追加する。
3. `empty`、`cons`、`from_items`、`head_value`、`tail_handle`、`iter`、`len` を両 representation 対応にする。
4. `from_items(Vec<Value>)` は空以外を Packed として構築する。
5. logical-order `PartialEq` と display を固定する。
6. 既存 List opcode と全 builtin consumer を回帰テストする。

この phase では `lib/types/list.srt` の公開関数定義を変更しない。

### Phase 2: Builder と `flat_map` builtin

1. Eldr 内部に `ListBuilder` と shared flat-map helper を追加する。
2. `BUILTIN_METAS` と `BUILTIN_IMPLS` の末尾に `list_flat_map` を追加する。
3. surface-to-runtime name mapping と parameter metadata を追加する。
4. `lib/types/list.srt` の `flat_map` を `@builtin def` 宣言へ変更する。
5. `CallableTemplateComposeFlavor::ListBind` を shared helper 経由にする。
6. Forge が `List::flat_map` を `CallBuiltin` として emit することを固定する。

### Phase 3: 検証と判断

1. focused unit / integration test を実行する。
2. workspace test を実行する。
3. allocation 数、elapsed time、peak RSS を baseline と比較する。
4. retained Packed prefix と heavy-builtin scheduler latency を測定する。
5. `map` / `filter` / `reverse` の builtin 化は、結果を見て別タスクで判断する。

### Phase 4: `do`（別タスク）

`do_intrinsic_spec.md` の開始条件を満たした後、generic lowering を実装する。本書の List 改修を
`do` 実装の開始条件にはしない。また、`do` 実装を Packed 導入の開始条件にもしない。

## 14. 主な変更対象

| 層 | 主なファイル | 変更内容 |
|---|---|---|
| 正本 | `doc/要件定義v9.md` | cons-cell 固定から logical List 契約へ更新 |
| テスト正本 | `docs/dev/テスト方針.md` | hybrid representation invariant と性能観点 |
| runtime type | `crates/sindr/src/runtime.rs` | ListHandle / iterator / equality |
| builtin metadata | `crates/sindr/src/builtin.rs` | `list_flat_map` を table 末尾へ追加、surface mapping |
| runtime builtin | `crates/eldr/src/builtin.rs` | implementation entry と metadata order test |
| VM helper | `crates/eldr/src/vm.rs` | shared flat-map helper、ListBind template 再利用 |
| bytecode emission | `crates/forge/src/codegen.rs` | resolved builtin call と境界テスト |
| stdlib surface | `lib/types/list.srt` | `flat_map` body を `@builtin def` へ変更、`bind` は維持 |
| spec fixture | `lib/tests/list.srt`, `tests/fixtures/**` | semantics / law / runtime 経路 |

Spire / Sigil / Scar の AST、名前解決、型規則には新しい List representation を追加しない。
変更は既存 `@builtin` 宣言の構造検証と callable metadata の通常経路に限定する。

## 15. テスト契約

### 15.1 Sindr runtime unit

- Empty / Cons / Packed / Cons-over-Packed の `len`、head、tail、iteration
- Packed tail が buffer copy なしで offset を進めること
- `from_items([])` が Empty、非空が Packed になること
- Cons と Packed で同じ logical sequence を作ったとき `PartialEq` が true になること
- representation が異なっても nested List / Tuple / Tagged の equality が一致すること
- mixed representation の display が通常 literal 順になること
- checked length invariant が壊れた内部値を作らないこと

### 15.2 Eldr unit

- `ListEmpty` / `ListNil` / `ListCons` / `ListHead` / `ListTail` / `ListFromItems`
- `list_flat_map` の empty、singleton、0/1/複数要素 mapper result
- input と mapper result の Cons / Packed 全組合せ
- mapper の left-to-right invocation order と exactly-once
- mapper runtime failure 後に後続要素を評価しないこと
- mapper が非 List を返す malformed bytecode の RuntimeError
- `CallableTemplateComposeFlavor::ListBind` と direct `List::flat_map` の同値性
- builtin implementation table と `BUILTIN_METAS` の順序一致

### 15.3 Forge / Scar / Sigil

- `List::flat_map` declaration が metadata signature と一致すること
- direct call が新 opcode ではなく正しい builtin ID の `CallBuiltin` になること
- builtin ID 追加で既存 ID が変わらないこと
- `Monad::bind` の concrete List dispatch が既存 source definitionを指すこと

### 15.4 Rune / fixture

- List literal、cons、head-tail pattern、append、concat、reverse の既存結果
- map、filter、flat_map の順序
- Functor / Applicative / Monad / Alternative law の既存 fixture
- REPL と script で同じ List result / display になること
- large flat_map が正しい順序と要素数を返すこと

### 15.5 benchmark

performance benchmark は correctness suite と分離する。

- source baseline flat_map と builtin flat_map の elapsed time
- cons-node allocation 数または代表 proxy
- Packed result の peak RSS
- Packed suffix 保持時の retained memory
- large flat_map 実行中の process scheduler latency

## 16. 検証コマンド

実装時は focused test を先に実行し、最後に workspace 全体を実行する。

```bash
cargo nextest run -p sindr
cargo nextest run -p eldr
cargo nextest run -p forge
cargo nextest run -p scar
cargo nextest run -p rune --test integration run_srt
cargo nextest run -p rune --test integration module_import_fixtures
cargo nextest run --workspace
```

実在する test target 名が異なる場合は `cargo nextest list` で確認し、同じ責務を持つ最小 targetへ置き換える。

## 17. 受け入れ基準

1. user-visible type は `List<$A>` のままで、Packed / Builder は公開されない。
2. Empty / Cons / Packed / mixed representation の logical semantics が一致する。
3. `cons` / `uncons` / `head` / `tail` が representation によらず worst-case `O(1)` である。
4. full traversal、display、logical equality が `O(n)` である。
5. `append` は `O(length(left))`、single `flat_map` は `O(N + M + mapper_cost)` を保つ。
6. `flat_map` は callback と各出力要素の順序、回数、failure behaviorを現行 source definitionから変えない。
7. Builder 所有 buffer が完成時に通常の Packed `ListHandle` になる。
8. `Monad::bind for List` は source-level に `List::flat_map` を呼び続ける。
9. batch VM と InteractiveVm が同じ builtin implementation を使う。
10. process 間で mutable Builder は共有されず、immutable List storage の共有は観測不能である。
11. 新しい List-do opcode / runtime frameを追加しない。
12. 既存 builtin ID、`.eldr` opcode enum、公開 List API を破壊しない。
13. focused test と `cargo nextest run --workspace` が成功する。

## 18. 後続判断

初期改修後の測定で必要性が確認された場合だけ、次を別仕様として検討する。

- bounded Packed chunk と retained-prefix compaction
- resumable higher-order builtin continuation
- `map` / `filter` / `reverse` の shared Builder 化
- List pipeline fusion
- typed IR 上の nested `flat_map` fusion
- List 専用 `do` specialization

List 専用 `do` specialization を検討する場合でも、先に generic `do` と同じ値、evaluation order、
callback count、failure、source trace を differential test で固定する。また `do_intrinsic_spec.md` と
`EldrVM_spec.md` の正本を更新してから opcode / frame を追加する。
