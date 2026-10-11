# Lazy 引数を持つ標準関数の .srt 実装調査

調査日: 2026-10-11。以下は修正前の調査記録であり、調査時点では製品コード・実行可能なテストを変更していない。実装と検証の結果は末尾に追記する。
現行ソース・正本文書・既存テストを読み、3 サブエージェントで宣言、コンパイラ経路、契約の境界を確認した。以下の実装例は未実装・未実行の案である。
対象分類と ensure の説明は、調査後のユーザー指定に合わせて更新した。この更新では追加調査・実装・テストを行っていない。

## 結論と対象

`Lazy<T>` を呼び出し位置の正規化契約として残し、実装本体には通常の `(-> T)` を渡す。今回の `.srt` 化対象は 6 関数とする。ただし `@builtin` を削除するだけでは成立しない。標準の通常関数での Lazy 宣言許可と、通常 call への正規化済み callable の受け渡しが必要になる。

現行で VM builtin を呼んでいても、`map_err` は Err ペイロードの写し替えだけなので `.srt` 化対象に含める。`cause` はそのままにする。`and` / `or` は部分的最適化の余地を残すため builtin 宣言と専用経路を維持する。PatternConsumer も維持し、Pattern をオブジェクトとして受け渡す言語仕様拡張は行わない。利用者による新しい Lazy 宣言も対象外とする。

| 分類 | 宣言 | 現行の宣言位置 | 理由 |
|---|---|---|---|
| `.srt` 化候補 | `Kernel::if` | `lib/kernel.srt:96` | Boolean の照合と選択 branch の呼び出し |
| `.srt` 化候補 | `Kernel::if_then` | `lib/kernel.srt:47` | Boolean の照合、True のときだけ branch を呼ぶ |
| 維持 | `Kernel::and` | `lib/kernel.srt:251` | 部分的最適化の余地を残すため builtin のままにする |
| 維持 | `Kernel::or` | `lib/kernel.srt:274` | 同上 |
| `.srt` 化候補 | `Kernel::require` | `lib/kernel.srt:305` | Boolean の照合と Ok/Err 構築 |
| `.srt` 化候補 | `Kernel::ensure` | `lib/kernel.srt:333` | predicate の呼び出し、Boolean の照合と Ok/Err 構築 |
| `.srt` 化候補 | `Result::or_else` | `lib/types/result.srt:190` | Result の照合、Err のときだけ fallback を呼ぶ |
| `.srt` 化候補 | `Result::map_err` | `lib/types/result.srt:115` | Err ペイロードを写し替えるだけなので通常の照合と Err 構築で表す |
| 維持 | `Result::cause` | `lib/types/result.srt:142` | Err 分岐で VM builtin `cause` を呼ぶ |
| 維持 | `Kernel::if_let` | `lib/kernel.srt:145` | Pattern の照合と呼び出し位置の成功スコープを扱う |
| 維持 | `Kernel::if_let_then` | `lib/kernel.srt:174` | 同上 |
| 変更不要 | `MonadFail::fail_if` | `lib/traits/monad_fail.srt:73` | 既に `@builtin` なしで `.srt` 本体を持つ |

Lazy 引数を宣言する関数は計 12 件。Kernel に 8 件、Result に 3 件、MonadFail に 1 件ある。`Result::recover_kind` の handler と `Result::then` は通常の関数型であり、Lazy 宣言の移行対象ではない。

対象 6 件のうち `map_err` 以外の 5 件は runtime builtin registry に登録されていない。現行 Forge の分岐処理で閉じる（`crates/forge/src/codegen.rs:12856` 以降、`:13017` 以降）。維持する `and` / `or` は Sigil が `Resolved::If` へ展開する（`crates/sigil/src/resolver/special_forms.rs:152` 以降）。

`map_err` / `cause` は現行では Forge の `emit_result_error_transform` が Err の場合だけ `CallBuiltin` を生成する（`crates/forge/src/codegen.rs:12973` 以降）。正本 registry は `crates/sindr/src/builtin.rs:845` 以降、実行本体は `crates/eldr/src/builtin.rs:2300` 以降。`map_err` は Ok 入力をそのまま返し、Err 入力では新しい Error に写し替える。旧 Error とその cause chain は引き継がず、新しい Error 自体が持つ情報を保持する。この処理を `.srt` へ移す。`cause` は Error 内部の cause chain を操作するため維持する。

## 既存の通常メソッド経路

`MonadFail::fail_if` は次の本体を持つ。

```surtr
def fail_if(condition: Boolean, err: Lazy<Error>, then: Lazy<Self>) -> Self {
  if(condition, MonadFail::fail(err()), then())
}
```

標準 Trait の宣言にある Lazy は、型検査時に通常の `Ty::Func([], T)` へ消去する（`crates/scar/src/checker/types.rs:1848` 以降）。呼び出し側の `check_trait_lazy_argument` は eager 境界を一時 binding へ取り出し、通常の Closure を生成して本体へ渡す（`crates/scar/src/checker/expr.rs:6994` 以降）。本体は必要な branch だけ `err()` / `then()` を呼ぶ。

`crates/scar/tests/lazy_trait_contract.rs:7` 以降に標準 default method、利用者 Trait の拒否、標準 Trait override の継承、capture の拒否境界がある。実行 fixture は `tests/fixtures/script/pass/stdmod/monad_fail_lazy_override.srt`。今回これらは読んだが再実行していない。

この経路は通常関数化の出発点にできる。ただし Trait 専用の宣言情報と単一引数の正規化に依存しており、`if` の両 branch の正規化をそのまま代替するものではない。

## .srt 本体の案

宣言の表面には Lazy を残し、本体の仮引数型だけ `(-> T)` として扱う。以下は各既存 owner 内に置く案で、現状のコンパイラでは通常関数の Lazy 宣言を拒否する。

```surtr
def if(flag: Boolean, then_branch: Lazy<$A>, else_branch: Lazy<$A>) -> $A {
  match flag { True => then_branch(), False => else_branch() }
}

def if_then(flag: Boolean, then_branch: Lazy<Unit>) -> Unit {
  match flag { True => then_branch(), False => () }
}

def require(flag: Boolean, err: Lazy<Error>) -> Result<Unit> {
  match flag { True => Ok(()), False => Err(err()) }
}

def ensure(value: $A, pred: ($A -> Boolean), err: Lazy<Error>) -> Result<$A> {
  match pred(value) { True => Ok(value), False => Err(err()) }
}

def or_else(value: Result<$T>, next: Lazy<Result<$T>>) -> Result<$T> {
  match value { Ok(_) => value, Err(_) => next() }
}

def map_err(result: Result<$T>, err: Lazy<Error>) -> Result<$T> {
  match result { Ok(_) => result, Err(_) => Err(err()) }
}
```

分岐の実行を `.srt` 本体へ移し、呼び出し側コンパイラの責務を入力の型・scope・評価順と callable の構築に絞る。新しい builtin、opcode、VM/スキーマのバージョン変更は不要な見込みである。関数呼び出しと Closure の生成が増える可能性はあり、実装時には命令列と必要に応じて性能を確認する。

## 属性削除だけでは移行できない理由と変更点

### 1. 標準の通常関数で Lazy 引数を宣言できない

通常 `def` は仮引数型を `TypeSyntaxContext::General` で解決する（`crates/scar/src/checker/definitions.rs:1412` 以降）。通常関数の predeclare と本体検査の両方で、標準宣言に限定して Lazy を許可し、実装署名を `(-> T)` へ変換する必要がある。Trait の既存処理と共通化できる範囲を使う。

表面の Lazy 引数位置は消去せず宣言契約として保持し、通常 call の型検査時に参照する。通常の関数値型だけでは、利用者が明示した `(-> T)` と標準の `Lazy<T>` を区別できない。generic の実体化や imported declaration でも契約を失わないようにする。

許可は標準ソースの宣言 identity に基づける。利用者の独立した関数・Trait、戻り値、local annotation、field、container での Lazy は引き続き拒否する。標準 Trait の override 継承は維持する。同名・同 owner の綴りを救済条件にしない。

### 2. Sigil が .srt 本体を呼ばない専用経路へ展開する

`classify_canonical_special_form_callee` は選択された宣言 identity の QName から分類し、`@builtin` の有無を見ない（`crates/sigil/src/resolver/expr.rs:208` 以降、`:3238` 付近）。属性を外して本体を書いても、現行の呼び出しは `Resolved::If` / `Require` / `Ensure` / `OrElse` 等へ展開される。

対象 6 件は宣言契約を持つ通常 call へ接続し、対応する旧 special-form call 展開を置換する。canonical identity による選択、同名の利用者関数が通常評価に従う境界、裸 capture の拒否と診断の由来は維持する。`and` / `or`、`cause` と Pattern consumer の経路は残す。

現在 `resolve_lazy_input` は grouped Lazy 入力に限って `reject_eager_capture_parameters` を呼ぶ（`crates/sigil/src/resolver/special_forms.rs:49` 以降）。通常 App の引数解決は単に `resolve_node` を呼ぶため（`crates/sigil/src/resolver/expr.rs:3249` 以降）、専用 call 展開を削除するとこの検査も失う。通常 call の引数解決でも、選択した標準宣言の Lazy 位置を参照し、固定 eager 式のスコープ検査と既存の名前解決診断の優先順を維持する。

`&&` / `||` には別の producer がある。Spire は局所の `and` / `or` へ解決先が変わらないよう記号名を保つ（`crates/spire/src/parser/expr.rs:347` 以降）。Sigil は canonical 分類より前にこれらを `resolve_logic_call` へ渡す（`crates/sigil/src/resolver/expr.rs:3225` 以降）。derive の `fold_and` も `&&` の App を生成する（`crates/sigil/src/resolver/derive.rs:293` 以降）。`and` / `or` を維持する方針に合わせ、これらの経路も維持する。Lazy の共通処理を整理する場合も、短絡・既存の eager 順序・局所 binding による解決先変更の禁止を保つ。

### 3. branch を consume する処理と callable を渡す処理を整理する

現行の `normalize_lazy_pair` と `normalize_lazy_single` は、選択後に shell を一回 consume した式を返す（`crates/scar/src/checker/expr.rs:14303` 以降）。通常 call には consume 前の `(-> R)` が必要になる。

既存 Trait の処理を共有できる形へ整理し、callable を渡す処理と、残る intrinsic で branch を実行する処理の両方を同じ正規化判断につなぐ。消去後の仮引数へ Lazy 規則を再適用しない。戻り値が callable でも本体の `branch()` は一回の呼び出しにとどめる。

特に `if` は両 branch を一緒に正規化する。各引数を期待型から独立して thunk 化するだけでは、浅い側だけ最大一段 wrap する規則、既知 branch からの placeholder 要求型、期待型が既知 depth を変えない規則を失う。

### 4. 現行の評価順を通常 call の前に保持する

通常 UserFunc の call は引数を順に評価する（`crates/forge/src/codegen.rs:12618` 以降）。現行 intrinsic は以下の順である。これはソースからの確認で、今回副作用 probe は実行していない。

| 対象 | 現行の実行順 |
|---|---|
| `if` / `if_then` | then、else の eager 境界を引数順に事前評価 → 条件式 → 選択 branch |
| `and` / `or` | Sigil が If に展開するため右辺 eager 境界 → 左辺 → 選択 branch |
| `require` | error の eager 境界 → 条件式 → 必要な場合だけ error branch |
| `ensure` | value → error の eager 境界 → predicate の取得と呼び出し → 必要な場合だけ error branch |
| `Result::or_else` | 左辺 Result → fallback の eager 境界 → Err の場合だけ fallback |
| `Result::map_err` | 左辺 Result → error の eager 境界 → Err の場合だけ error branch |

根拠は `emit_if`（`:12862` 以降）、tail の If（`:12683` 以降）、`emit_require`（`:12916` 以降）、`emit_ensure`（`:12949` 以降）、`emit_result_or_else`（`:13022` 以降）、`emit_result_error_transform`（`:12973` 以降）。`docs/site/lazy-evaluation.md:25` では各 special form の既存順序に従うとしている。

現行契約を維持する移行案では、call 前の通常 Block / Bind に評価を取り出し、確定した評価済みの値・callable を通常 App へ渡す。各入力を一回だけ評価する。単純な左から右の通常 call への置換は、eager branch と副作用を持つ条件式の順を変えるため採用しない。

全対象を通常の引数順へ統一する案はコンパイラを簡素化できるが、別の評価規則変更である。今回は採用せず、実装時の回帰テストで既存順序を固定する。

### 5. ensure の predicate の型契約と今回残す構文制約

`ensure` は predicate の型が一致していても call expression を拒否する（`crates/scar/src/checker/expr.rs:14720` 以降、`lib/kernel.srt:322` 以降）。`tests/fixtures/script/fail/typecheck/ensure_predicate_call_expr.srt` の `is_even()` は `(Int -> Boolean)` を返すが、現行では拒否される。

ユーザー指定では、predicate の意図は引数待ち受けの拒否であり、シグネチャが `($A -> Boolean)` に一致する関数値・キャプチャなどは渡せる。関数値やキャプチャ自体を拒否する規則ではない。シグネチャによる型検査へ統一し、ensure 固有の call expression 拒否を削除することは可能という方向性が示された。ただし、今回の移行では現行の構文制約をそのまま残す。将来の削除を今回の受入条件には含めない。この制約は Lazy 宣言そのものとは別の契約である。

### 6. positional-only の呼び出し契約を保持する

現行の canonical Lazy call は `collect_fixed_positional_args` により named / spread / arity を検査する（`crates/sigil/src/resolver/special_forms.rs:67`、`:79`、`:93`、`:105`、`:146` 付近）。`tests/fixtures/script/fail/resolve/and_named_arg.srt` は `and(left: True, right: False)` を resolve 段階で拒否する。

通常 call の引数処理へ移すと、named 引数などの受理範囲が変わり得る。移行対象の標準宣言契約で既存の positional-only 制約と arity 診断を保持する。単なる署名型の消去と分岐本体の移行には含まれない変更点である。

### 7. 旧経路と契約テストの整理

`crates/scar/src/checker/definitions.rs:710` 以降の期待署名には `@builtin def ...` が固定されている。移行対象は通常の標準宣言契約として検証し、不要になる builtin 特殊宣言検証を削除する。runtime registry と Eldr の対応実装から `map_err` を削除し、`cause` は維持する。ID は定義順なので、両 registry の対応と参照を合わせて更新する。旧 ID の予約枠や互換経路を残さず、VM/スキーマのバージョンも上げない。

対象の専用 `Require` / `Ensure` / `OrElse` / `MapErr` 表現・walkers・Forge emitter は、全 producer を確認したうえで不要な経路を削除する。`map_err` / `cause` が共有していた error transform emitter は、`cause` に必要な処理だけ残す。`TypedInner::If` は維持する `and` / `or` と `cond` の型検査など内部の producer がある（`crates/scar/src/checker/relations.rs:457`）ため、Lazy 関数の移行だけを理由に一括削除しない。残る intrinsic の共通処理も必要性に応じて維持する。

`and` / `or` の予約名規則は属性から独立して維持する（`crates/sigil/src/resolver/declarations.rs:1263` 以降）。旧 IR shape を固定したテストは、通常 call と正規化後の callable を確認する内容へ更新する。旧入力の必要な拒否テストは残す。

## if_let 系を分ける理由

Pattern は通常の値ではない（`docs/dev/Pattern_spec.md:5`、`:39`）。binding を作る成功 branch は、呼び出し位置の Pattern-success-scope に直接書いた式である。binding の値を通常の Lazy callable として事前に生成できる契約ではない。

Sigil は Pattern と成功 branch をその scope で解決し、`Resolved::IfLet` へ展開する（`crates/sigil/src/resolver/pattern_consumers.rs:190` 以降）。`if_let` / `if_let_then` の `.srt` 化には Pattern の値表現と binding スコープの再設計が必要になり、今回の 0 引数 callable 化だけでは表せない。binding のない呼び出しだけを通常関数へ逃がす二重経路も設けない。

## 実装時の受入条件と検証計画

level4 とする。言語の評価規則は維持するが、標準通常関数の Lazy 宣言権限と、Sigil → Scar → Forge の呼び出し契約を変更するためである。

1. 対象 6 件の `@builtin` がなくなり、呼び出し先の `.srt` 本体が実行される。維持対象 5 件と既存 `MonadFail::fail_if` の契約は変わらない。`map_err` の runtime builtin 登録・実装と専用 call 経路は削除する。
2. 非選択 branch の本体は実行せず、選択 branch は一回だけ呼ぶ。返された callable を追加で呼ばない。両 branch の depth 差二段以上・基底型不一致は拒否する。
3. `&if(&1, &2, 0)` は第二引数に `(-> Int)` を要求する。`&N` と `(&N)` を同じ直接 placeholder とし、同番号の要求型衝突、未知 signature、裸 capture、固定 eager 式内の今回の placeholder を拒否する。
4. 副作用を持つ条件式・左辺と grouped eager branch を組み合わせ、上記の順序を通常位置・tail 位置・capture 呼び出しで確認する。eager 入力を capture の生成時へ移動しない。
5. ensure はシグネチャが一致する関数値・キャプチャを受け入れ、現行の predicate call expression 拒否は今回そのまま残す。positional-only と named / spread / arity の拒否、通常の同名利用者関数、`and` / `or` 予約名、pipe の Lazy 引数注入拒否を維持する。`and` / `or`、`&&` / `||` と derive の論理式は現行経路を維持し、局所の同名値へ解決先を変えない。
6. 標準の通常関数は Lazy を宣言でき、利用者の独立した関数・Trait・通常値型は拒否する。標準 Trait default / override の継承を維持する。
7. `map_err` の置換、`cause` の cause chain、Pattern binding 成功スコープ、`cond` の評価が退行しない。旧経路・綴りによる fallback を残さない。
8. 通常 call と生成 Closure への移行でも、新規 Error の source ID / span は元の構築式の位置を保持し、既存 Error の再格納では location・cause・stack trace を再生成しない（`docs/dev/Error_spec.md:145` 以降）。frame 追加の影響は今回未検証であり、既存 Error の保持と新規構築位置を実装時に検証する。

既存の直接的な検証場所は `crates/scar/tests/lazy_capture_revision.rs`、`lazy_capture_diagnostics.rs`、`lazy_trait_contract.rs`、`crates/scar/tests/typecheck_surface.rs`、Sigil の resolver tests、script の `lazy_capture_normalization` / `lazy_grouped_branch_eager` / `lazy_eager_capture_scope` / `monad_fail_lazy_override`。IR shape を更新し、未固定の副作用順序と標準通常関数の許可境界だけ必要な回帰テストを追加する。

実装順は、宣言契約・署名消去、call の入力正規化と評価順、6 本体の移行と旧経路・map_err の runtime builtin 削除、正本文書の追従とする。正本は `docs/dev/Lazy_spec.md`、`docs/site/lazy-evaluation.md`、`lib/kernel.srt` / `lib/types/result.srt` の `@doc`、`lib/types/special_types.srt` の宣言許可説明。Pattern 契約自体は変更しない。

局所検証は `rtk cargo nextest run -p scar` と関連 Sigil / Forge テスト、実行 fixture は `rtk cargo nextest run -p rune --test integration run_srt`。最終検証はビルド・テストを直列に実施し、`rtk cargo nextest run --profile ci --workspace` と `rtk proxy cargo run -- test --quiet --all` を各一回通す。最終差分は別エージェントがレビューする。

## 調査時点の検証範囲と残件

ソース・正本文書・既存テストの照合とサブエージェントの独立調査を実施した。ビルド、テスト、副作用 probe、命令列・性能測定は未実行であり、提示した `.srt` 本体のコンパイル成功は確認していない。現行の通常関数では Lazy 宣言が許可されないため、実装完了とは扱わない。

未確定なのは、宣言契約を既存の callable 情報へ保持する具体的な型配置と、正規化を共有する内部 API の形である。現行評価順を維持する前提で実装時に選ぶ。次の一手は、標準通常関数の Lazy 宣言と一件の通常 call を受け入れる回帰テストを用意し、現行拒否を確認してから宣言・呼び出し経路を実装することである。

## 実装結果（2026-10-11）

対象 6 関数を通常の `.srt` 本体へ移行した。標準の通常関数は Lazy 引数を宣言でき、実装本体には通常の0引数 callable を渡す。利用者の独立した宣言での Lazy は引き続き拒否する。標準 Trait の default / override が継承する契約も維持した。

Sigil は選択された宣言と Lazy 引数位置を保持し、対象 6 関数の入力を `Resolved::LazyApp` にまとめる。これは入力正規化の中間表現であり、Scar が通常の Block / Bind / Closure / App へ変換する。呼び出し先は元の標準宣言の `.srt` 本体である。generic 実体化と import 後も宣言契約を保持し、通常の引数型・callable provenance の検査を通す。

旧 `Require` / `Ensure` / `MapErr` / `OrElse` の専用 IR、walkers、Forge emitter と、`map_err` の両 runtime registry・実装を削除した。`cause` の emitter は必要な処理だけに整理した。`and` / `or`、Pattern consumer、内部の `If` producer は維持した。旧 ID の予約枠、互換経路、VM / schema のバージョン変更は追加していない。

評価順を通常 binding で保持し、通常位置・tail 位置・capture 呼び出しの副作用 probe を追加した。capture の生成時には固定 eager 入力を評価しない。旧 IR shape のテストは、通常 App と正規化済み callable を検証する内容へ変更した。

通常 App の Facet / MatchResult の受け渡し制限は維持した。Extractor のテスト補助定義にあった `if` による MatchResult の構築は Boolean の `match` へ追従し、元の Extractor 診断の期待値は維持した。VM の末尾呼び出し回数のテストも通常 `if` と branch の呼び出しを含む値へ更新し、最大 frame depth と非末尾再帰での frame 増加を引き続き検証する。

Error 構築式を含む closure の直接呼び出し template 化では、構築位置の span が失われることを実行テストで確認した。Error constructor の canonical identity に基づいてこの最適化を適用せず、通常 closure の本体に元の call span を保持する。通常実行と bytecode roundtrip で構築位置を検証し、既存 Error の location・cause・stack trace・payload が再格納後も一致することを確認した。

正本文書と標準 API の `@doc` を追従した。Skill の `surtr-source-update` と `surtr-runtime-extension` には、Lazy 関数がビルトインを呼ぶ場合は `@builtin` 宣言とする規則を追加した。

### 最終検証

最終差分を固定し、以下を直列に実行した。

- `rtk cargo nextest run --profile ci --workspace`: 67 binaries、2,476 passed、失敗なし。
- `rtk proxy cargo run -- test --quiet --all`: 終了コード0。標準 `.srt` テスト全件成功。
- `cargo fmt --all -- --check` と差分の whitespace 検査: 成功。
- 更新した2つの Skill の `quick_validate.py`: ともに成功。

別エージェントが最終差分を独立レビューし、旧専用経路の削除、宣言権限、評価順、Error 構築位置、通常 App の受け渡し制限、回帰テストを確認した。追加の機能上の指摘はない。

全件検証の開始直前に元の worktree が archive cleanup で削除されたため、変更を含む保存 snapshot `57b030734116152d147fad9a25c77ad1266f263a` をこのチャットの作業用 checkout に復元した。別エージェントが消失前の最終変更との一致を確認し、上記の全件検証は復元先で実施した。
