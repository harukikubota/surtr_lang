# Type Constructor Signature Unification — Task 4 後フォールバック是正範囲

## 目的

[`type_constructor_signature_unification_implementation_plan.md`](type_constructor_signature_unification_implementation_plan.md) の Task 1--4 完了時点で残っている暫定経路を整理し、本来の型エラー、ambiguity、metadata 不整合が成功扱いまたは別診断へ変換されないようにする。

この文書は既存計画を置き換えない。Task 5 以降へ進む前後に必要な是正範囲と、各修正を既存 Task のどこへ統合するかを固定する。

## 進捗（2026-09-06）

実装済み:

- [x] witness のない省略 ReturnTypeArgument を Scar の program boundary で `AmbiguousReturnTypeArgument` にする。
- [x] expected return で解決された呼び出しと、外側の generic result へ転送された呼び出しは受理する。
- [x] callable result に残る RTA は、後続の callable application が witness を与えられるため boundary ambiguity から除外する。
- [x] ambiguity に closed `TypeDiagnosticReason`、structured data、専用 headline を付ける。
- [x] Scar 単体テストと Rune compile-error fixture で上記を固定する。
- [x] canonical callable role list の長さと ReturnTypeArgument ordinal を構築前に検証し、`zip` の切り詰めを禁止する。
- [x] 登録済み user function / builtin の canonical signature 欠落を structured error にし、legacy positional checker への降格を禁止する。
- [x] witness に Trait identity がない場合の slot 数・候補数による carrier 既定化を削除する。
- [x] Enum constructor application の positional slot fallback を削除する。
- [x] constructor Trait の short-name lookup を unique-only にし、登録順依存の先頭一致を禁止する。
- [x] builtin の qualified owner/name を明示 allowlist に限定し、任意 owner の surface variant 生成を禁止する。
- [x] malformed builtin signature を 0 引数 callable に変換せず、metadata 検証で fail-fast にする。
- [x] contextual constructor candidate の全失敗を structured data に保持し、一般経路への降格を禁止する。
- [x] candidate probe の型環境・substitution・obligation・capability・witness・warning を候補ごとに rollback する。

継続中:

- [ ] source-level callable instantiation boundary へ判定を移し、program boundary の走査を safety net に限定する。
- [ ] explicit / `_` / omitted ReturnTypeArgument を同じ call constraint で解く。
- [ ] solver が確定した substitution を specialization へ直接渡し、`retain_generic_fallback` を削除する。
- [ ] constructor carrier、Trait identity、canonical callable、builtin metadata、candidate failure の各 fallback を以下の範囲で除去する。

現段階の boundary check は、Task 5 solver 完成前にも ambiguity を成功扱いにしないための fail-closed な検査である。compiler-generated process helper は source-level ReturnTypeArgument の対象外であり、それぞれの lowering 経路で解決されるため除外している。

## 確認済みの問題

### P1: return-only ReturnTypeArgument の ambiguity が成功扱いになる

対象:

- `crates/scar/src/checker/specialize.rs::specialize_program`
- `crates/scar/src/checker/specialize.rs::infer_specialization_mapping`
- Scar から Forge へ渡す直前の未解決型検査

現状:

- `retain_generic_fallback` が、ReturnTypeArgument の型変数が value parameter に現れず、定義本体に pending Trait call がなければ汎用定義を残す。
- specialization mapping は value argument と value parameter の組だけから復元し、ReturnTypeArgument と expected return を入力にしない。
- Forge 境界では pending Trait dispatch を検査するが、未解決 ReturnTypeArgument、`Ty::Var`、constructor witness は検査しない。

再現:

```surtr
def make::<$A>() -> Result<$A> { todo() }
make()
```

現状の `rune check` は `errors: []` を返す。期待結果は `AmbiguousReturnTypeArgument` である。

修正:

- [ ] `retain_generic_fallback` を削除する。
- [ ] Task 5 の call constraint へ、明示・`_`・省略 ReturnTypeArgument、value argument、expected return を同時に投入する。
- [ ] 省略 ReturnTypeArgument を全項目 `_` と同じ inference hole として扱う。
- [ ] callable instantiation boundary で残った ReturnTypeArgument の `Deferred` を `AmbiguousReturnTypeArgument` に変換する。
- [x] program boundary で残った source-level ReturnTypeArgument を `AmbiguousReturnTypeArgument` に変換する。
- [ ] Forge 境界で未解決 ReturnTypeArgument、constructor witness、実行可能位置の `Ty::Var` を拒否する。
- [ ] specialization は solver が確定した substitution のみを受け取り、value argument から mapping を再構築しない。

### P1: constructor carrier を slot 数・候補数から逆決定する

対象:

- `crates/scar/src/checker/types.rs::constructor_application_slots_for_witness`
- `crates/scar/src/checker/types.rs::apply_constructor_application`
- `crates/scar/src/checker/types.rs::resolve_ty` の `Ty::SelfApp` 分岐
- `crates/scar/src/checker/specialize.rs::match_specialization_ty`

現状:

- witness に Trait identity がない場合、同じ slot 数を持つ Trait を走査し、結果が一意ならその slot を採用する。
- Enum の constructor slot mapping が不明な場合、型引数数が一致すれば全位置を positional slot とみなす。
- constructor application を適用できない場合、エラーまたは `Deferred` にせず未解決 `Ty::SelfApp` を返す。
- specialization 中に slot を取得できない場合、mapping 追加を行わず処理を継続する。

修正:

- [x] witness に canonical Trait family identity がない状態では carrier を選ばない。
- [x] slot 数、impl 数、候補の一意性を carrier 決定根拠にしない。
- [x] Enum の positional slot fallback を削除し、検証済み `constructor_slot_positions` を必須にする。
- [ ] slot mapping 不足を `Rejected`、未確定 witness を `Deferred` として区別する。
- [ ] `Option<Vec<Ty>>` で失敗理由を捨てず、Task 7 の selection state または同等の明示 enum を返す。
- [ ] `resolve_ty` が適用不能な executable `SelfApp` を黙って残さない。
- [ ] boundary で `MissingTypeConstructorCapability`、`TypeConstructorFamilyMismatch`、または ambiguity に確定する。

### P1: constructor Trait identity が short/display name の先頭一致で決まる

対象:

- `crates/scar/src/checker/types.rs::constructor_trait_key_for_ast_ty`
- `crates/scar/src/checker/types.rs::resolve_signature_like_ast_ty_in_context`
- `crates/scar/src/checker/predeclare.rs::trait_key_by_short_name`
- `crates/scar/src/checker/signatures.rs::remember_direct_constructor_input`

現状:

- `self.traits` を走査し、surface name が一致した最初の Trait を採用する。
- 同じ short name を持つ Trait が複数見える場合の ambiguity を検出しない。
- `HashMap` の走査順によって direct TypeCtorTrait normalization の対象 family が変わり得る。

修正:

- [ ] Sigil が解決した canonical Trait identity を Scar の signature occurrence まで保持する。
- [ ] direct TypeCtorTrait syntax を surface name ではなく resolved identity から正規化する。暫定的に surface lookup は unique-only とした。
- [ ] short-name lookup が必要な compiler-owned Trait は、起動時に canonical identity を一度解決して保持する。暫定的に複数候補時は `None` とし、先頭候補を採用しない。
- [ ] 同一可視範囲に複数候補がある場合は最初の候補を採用せず compile error にする。
- [ ] carrier family key、capability view、where constraint が同じ canonical Trait identity を参照することをテストする。

## canonical callable contract の防御的修正

### P2: role list が `zip` で黙って切り詰められる

対象:

- `crates/scar/src/checker/signatures.rs::canonical_callable_signature`

現状:

- source ReturnTypeArgument と解決済み型、source ValueParameter と解決済み型をそれぞれ `zip` している。
- 要素数が異なる場合、末尾の role、ordinal、name、mode、origin が診断なしで消える。

修正:

- [x] `canonical_callable_signature` を fallible にし、各 role list の長さを構築前に厳密比較する。
- [x] 不一致は structured `CallableSignatureMetadataMismatch` として発生元 span と role detail を保持する。
- [x] ReturnTypeArgument の ordinal が欠落・重複していないことを検証する。ValueParameter ordinal は canonical list の構築位置から生成する。
- [x] call-site と specialization mapping の arity を `zip` 前に厳密比較し、partial mapping を拒否する。

### P2: canonical signature 欠落時に旧 callable checker へ降格する

対象:

- `crates/scar/src/checker/expr.rs` の `Ty::BuiltinFunc` application
- `crates/scar/src/checker/expr.rs` の `Ty::UserFunc` application
- `crates/scar/src/checker/predeclare.rs::predeclare_functions`

現状:

- `callable_signatures` に entry がなければ positional-only の旧引数検査へフォールバックする。
- builtin metadata がない場合でも `Ty::BuiltinFunc` binding は作られる。
- canonical route が検査する ReturnTypeArgument、named argument、parameter mode、provenance を迂回できる。

修正:

- [x] 登録済み `Ty::UserFunc` と non-intrinsic `Ty::BuiltinFunc` は canonical signature を必須にする。
- [x] registry entry がない場合は legacy checker へ降格せず internal consistency error にする。
- [x] unknown builtin metadata または未登録 owner は predeclare 時点でエラーにし、callable binding を作らない。
- [ ] positional-only checker は closure、first-class `Ty::Func` など canonical declaration identity を持たない値に限定する。

## builtin metadata の防御的修正

### P2: 未登録 owner の surface variant を生成する

対象:

- `crates/sindr/src/builtin.rs::builtin_surface_variant_for_decl`
- `crates/sindr/src/builtin.rs::surface_variant_named`
- `crates/sindr/src/builtin.rs::surface_signature`

現状:

- canonical owner/name variant が見つからなくても、任意 owner で `surface_variant_named` を生成する。
- metadata の signature parse に失敗すると、0 引数かつ signature 全文を戻り値型とする fallback signature を生成する。

修正:

- [x] owner/name は canonical surface variant または明示された qualified surface allowlist との完全一致を必須にする。
- [ ] owner-specific alias は暗黙生成せず `BUILTIN_METAS` の構造データとして宣言する。
- [x] signature parse の失敗を fallback せず、metadata 不整合を構築時および全 metadata 検証テストで失敗させる。
- [x] `@builtin` source declaration の owner identity 不一致を resolver 回帰テストで拒否する。

現段階では owner-specific alias を `builtin_runtime_name_for_qualified` の明示 allowlist に集約した。任意 owner の生成経路は閉じているが、追加・変更の起点を完全に `BUILTIN_METAS` だけに戻すため、Task 6 以降でこの allowlist を `BuiltinMeta` の構造フィールドへ移す。

## candidate probing と診断の保全

### P2: contextual candidate の具体的な失敗が `Ok(None)` に畳まれる

対象:

- `crates/scar/src/checker/expr.rs::try_check_context_bind_from_constructor`
- contextual map/bind 内の `check_node_with_expected(...).or_else(...)`

現状:

- candidate 内の callable shape mismatch、型不一致、dispatch 不成立を同じ失敗として rollback する。
- 全候補が失敗すると具体的な失敗理由を捨てて `Ok(None)` を返し、一般経路へフォールバックする。

修正:

- [ ] candidate-local `Rejected` と、入力不足による `Deferred` と、候補探索の対象外を区別する。
- [x] 全候補 reject 時は `CandidateSelectionData` に候補ごとの failure を保持し、一般経路へ降格しない。
- [x] expected-type retry では最初のエラーを無条件に捨てず、fallback も失敗した場合は expected-type 側の failure を保持する。
- [x] candidate probe が残した type environment、substitution、obligation、capability、witness、warning を rollback し、後続候補の成功テストで固定する。

### P2: 未移行診断が message heuristic に戻る

対象:

- `crates/diagnostics/src/typecheck.rs::type_error_spec`
- structured payload を持たない Scar `TypeError`

現状:

- legacy `message` / `hint` / source text を再解析して label、help、expected/got を推測する。
- semantic error が structured payload を持たない場合、元の failure category と origin relation を復元できない。

修正:

- [ ] 上記修正で追加する全エラーに closed `TypeDiagnosticReason` と対応する `DiagnosticData` を付ける。
- [ ] Rune / Xldr は structured payload がある場合、常に同じ structured renderer を使う。
- [ ] heuristic 撤去自体は Task 10 へ統合し、それ以前は新規 semantic error を legacy message-only 経路へ追加しない。

## 実装順序

1. P1 ambiguity の再現 fixture を追加し、`retain_generic_fallback` に依存する成功を失敗として固定する。
2. Task 5 の unified call solver と未解決 ReturnTypeArgument boundary を実装する。
3. constructor witness / slot mapping の fallback を明示的な `Solved` / `Deferred` / `Rejected` へ置き換える。
4. Trait short-name lookup を canonical identity へ置き換える。
5. canonical signature の長さ検証と registry 必須化を行う。
6. builtin surface metadata の owner・parse fallback を削除する。
7. candidate failure と structured diagnostic を Task 7--10 の正規経路へ統合する。

Task 5 の solver 導入前に `retain_generic_fallback` だけを削除すると、正当な expected-return inference まで失敗する可能性がある。そのため、手順 1 と 2 は同一変更単位で扱う。

## 必須回帰テスト

### ReturnTypeArgument

- [x] omitted RTA が value/expected return のどちらからも解けない場合は `AmbiguousReturnTypeArgument`
- [x] expected return だけで解ける return-only RTA は成功
- [ ] explicit RTA と expected return の衝突は両 origin を持つ mismatch
- [ ] RTA arity の不足・超過を partial `zip` せず拒否
- [ ] capture 内で未解決 RTA が残る場合は capture boundary で ambiguity

### Type constructor

- [ ] 同じ slot 数の TypeCtorTrait が一つだけ存在しても、unconstrained witness を既定化しない
- [ ] impl 宣言順を反転しても結果が同じ
- [x] Trait 登録順を反転しても short-name lookup が候補を選ばない
- [ ] slot mapping 不足を positional mapping で補わない
- [ ] captured argument が異なる carrier を同一 family として統合しない
- [ ] 同名 Trait が複数可視の場合は deterministic compile error
- [ ] capability view が弱い引数へ同一 carrier の強い capability を付与しない

### Callable / builtin

- [x] canonical role list の長さ不一致が internal consistency error
- [x] canonical signature registry 欠落時に legacy checker へ降格しない
- [x] 未登録 owner の `@builtin` declaration を拒否
- [x] malformed builtin metadata signature を 0 引数 callable として扱わない

owner fallback の inventory で `Function::curry`、`Kernel::print` を含む既存 alias を列挙し、qualified surface allowlist へ移した。`surface_variant_named` は allowlist に一致した場合だけ呼ばれ、未知 owner には使われない。最終的には owner / name / runtime target の対応表を `BUILTIN_METAS` の構造フィールドへ移す。

### Diagnostics

- [ ] ambiguity、carrier mismatch、missing capability が distinct `reason` を持つ
- [ ] primary / related span と完全な expected / actual type を JSON と Ariadne の両方で保持
- [x] candidate-local rejection を単独の最終エラーとして誤報せず、全候補 reject の根拠を structured data に保持

## 検証コマンド

```bash
cargo nextest run -p scar --test return_type_arguments
cargo nextest run -p scar
cargo nextest run -p diagnostics
cargo nextest run -p sindr
cargo nextest run -p xldr
cargo nextest run -p rune --test integration run_srt
cargo nextest run -p rune --test integration module_import_fixtures
cargo nextest run --workspace
```

### 2026-09-06 チェックポイント結果

- `cargo nextest run -p scar`: 188 passed
- `cargo nextest run -p diagnostics`: 73 passed
- `cargo nextest run -p sindr`: 86 passed
- `cargo nextest run -p xldr`: 201 passed、74 skipped
- `cargo nextest run -p rune --test integration run_srt`: 27 passed
- `cargo nextest run -p rune --test integration module_import_fixtures`: 10 passed
- `cargo nextest run --workspace`: 1778 passed、202 skipped
- 前回のチェックポイントで 15 秒 timeout した `language_features_bucket_0` と `language_features_bucket_5` も、最新の workspace 実行ではともに成功

## 今回の修正範囲外

- `do` intrinsic の lexer、parser、AST、resolver、type checker、lowering、stdlib surface
- Trait method role-list 全体の Task 6 実装。ただし canonical list の切り詰め防止は先に行う。
- Task 7 の selection engine 全置換。ただし slot 数・候補数による carrier 既定化は先に禁止する。
- Task 10 の全 heuristic 削除。ただし今回追加・変更するエラーは最初から structured payload を持たせる。
- runtime dictionary、runtime candidate lookup、do 専用 opcode

## 完了条件

- 再現例が `AmbiguousReturnTypeArgument` で失敗する。
- expected return で解ける同型の例は成功する。
- canonical callable declaration が legacy application route へ降格しない。
- carrier、slot、Trait identity が候補数・登録順・表示名から決まらない。
- executable typed tree に未解決 ReturnTypeArgument、constructor witness、pending Trait dispatch が残らない。
- focused tests と workspace tests が成功する。
