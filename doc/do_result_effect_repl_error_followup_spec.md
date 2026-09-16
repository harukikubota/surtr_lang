# REPLのdo Result-effect Error生成フォローアップ

状態: 再現確認済み・原因調査／実装修正は未完了（2026-09-16）。
対象確認commit: `9bc3e459`。旧N11計画の残タスクではなく、文書例の検証で発見した実装不整合。
現行契約の正本: [do intrinsic](../docs/dev/Do_intrinsic_spec.md)。

## 再現と期待値

`cargo run --quiet -- repl --quiet --no-local-config`で以下を各一行として入力する。

```surtr
mismatch: Result<Int> = do::<Result> { 2 <- Ok(1); Ok(3) }
inspect(mismatch)
mismatch_t: OptionT<Result, Int> = do::<OptionT<Result, _>> { 2 <- OptionT::some::<Result>(1); OptionT::some::<Result>(3) }
inspect(OptionT::run(mismatch_t))
```

両方の期待値は`Err(PatternMismatch("Pattern did not match."))`の表示文字列。
実際は前者が`Err(Empty List.("not implemented"))`、後者が
`Err(division by zero("Empty List."))`となる。
新規REPLの最初のdo入力でも再現し、空の専用`SURTR_STDLIB_CACHE_DIR`でも再現した。
既存のdisk cacheだけに起因する問題とは扱わない。

## 確認済み範囲

- `cargo run --quiet -- test --quiet do`と`cargo run --quiet -- test --quiet monad_transformers`は成功。
- REPLのResult/Option通常sequence、明示lift、Optionのpartial `<-`は期待どおり。
- ResultおよびOptionT<Result>のSafeBindで`Option::Some(value) =? Option<Int>::None`は
  `PatternMismatch`とmessageを正しく保持する。
- OptionT<Result>の`guard(False)`は`Ok(Option::None)`で、Result-effect選択の対象ではない。

## 次の作業

1. Forgeの`ResultEffectFailure`から`emit_result_effect_error_value`／`emit_error_value`へ至る経路を追う。
2. `ForgeSession::from_bytecode`で再構成するError constructorのfunction IDと、
   REPL chunkのfunction／constant／Error template relocationの整合を調査する。
   現時点で原因は未確定であり、relocation不足と断定しない。
3. 根本原因を修正し、旧経路やError情報を置き換えるfallbackを追加しない。
4. fresh REPLと複数chunkの双方でcanonical ResultとOptionT<Result>を検証し、
   Error kind/messageを直接assertする回帰ケースをREPL境界へ追加する。
5. SafeBind、通常script、Alternativeのempty、guardの境界を維持し、
   公開文書の既知問題注記を修正確認後に削除する。

今回の文書クリーンアップではcompiler/runtimeや実行テストの変更は行っていない。
