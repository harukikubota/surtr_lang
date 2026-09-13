# Callable 表示の生成 origin と型 signature

## 目的と level

REPL の callable 表示を、束縛式か入れ子値か、Trait helper が合成した callable かに依存させず一貫させる。capture の生成 origin、元 callable の identity、生成位置で確定した callable 型を分けて保持し、runtime value の表示に使う。これは表示 metadata のフェーズ間契約を定める Level 4 の仕様であり、型受理・Trait dispatch・実行意味論は変更しない。

## 現状

- `&Add::add` は Scar の Trait helper 経路で synthetic `Resolved::Closure` になり、Forge が Closure として metadata を作る。REPL の binding 表示でも `Monoid(1, &Add::add)` の field 表示でも Closure になる。
- `&id` は Scar で capture-site の `Ty::Func` を持つが、Forge は関数参照のみを出す。Eldr が共有関数宣言の generic signature を読むため、`f: (Int -> Int) = &id` でも表示 signature は未具体化のままである。
- `BindingInfo.callable_display` による top-level 表示補正は、`Ok(&id)`、field、List、HashMap、tuple、戻り値などの runtime 値には届かない。
- Forge の `__cap_` 引数名推測は Trait helper の synthetic Closure を見分けられず、空引数列では `all(...)` が真になるためゼロ引数 Closure を Capture と誤分類しうる。
- Eldr は Closure / Unknown callable の lexical capture から origin を再帰推測する。また partial-apply metadata を lexical capture 個数から作るため、placeholder の順序・重複と residual signature を正確に表せない。

## 受入契約

1. Callable metadata は **生成 origin**（Capture / Closure / Unknown）、**元 callable identity**（canonical module / name または動的 capture 元）、**現在の callable signature** を独立して表す。runtime renderer は callable 値自身の metadata を正本にする。
2. named function、builtin、Trait method の Capture は、その名前を直接 capture した値でも Trait helper の合成 wrapper を経た値でも Capture として表示する。`&Add::add` は `FnCapture(module: Add, name: add, sig: (Int, Int -> Int))` を表示し、`Monoid(1, &Add::add)` の `combine` field も同じ表示になる。Trait identity は実装 identity を区別できる形で保持し、method name のみで別実装を同一視しない。Facet capture の表示 identity は OI-036 の対象として未確定のため、この実装範囲から除く。
3. `sig` は capture 作成位置で型解決と specialization を終えた callable 型である。`f: (Int -> Int) = &id` は `(Int -> Int)`、`g: (String -> String) = &id` は `(String -> String)` を表示する。同じ generic function の共有宣言 metadata を変更してはならない。`Ok(&id)` など nested value でも同じ site-specific signature を表示する。
4. placeholder を含む partial capture は元の Capture identity を保ち、生成 callable の型から residual signature を表示する。residual の引数順序・型は生成 wrapper の型を正本とし、lexical capture 数を適用引数数として数えたり、signature 文字列を切り落として推定したりしない。
5. Capture を変数経由で再 capture すると Capture origin と元 identity を保つ。Closure を再 captureした callable は Closure のままとする。Capture を呼ぶ式を closure literal で包んだ値も Closure とする。
6. 明示的な closure literal は、引数がゼロの場合や body が named function / Capture を呼ぶ場合にも Closure と表示する。Capture と Closure の判定を parameter 名、body 形、最初の lexical capture の内容から推定しない。
7. 直接 binding、関数戻り値、構造体/record field、`Result`、List、HashMap、tuple の payload は、同じ callable metadata に従って同じ表示をする。`BindingInfo` は表示時に runtime value の origin を上書きしない。
8. 必須 metadata のない callable は汎用表示とする。user-facing 表示へ Function / Template / Builtin の内部 ID を出さない。compiler が生成する Capture metadata に identity や signature が欠ける状態を lexical capture から推測して補わない。
9. 表示 metadata の変更は型検査、Trait implementation 選択、call dispatch、評価順序、capture placeholder の受理条件を変更しない。現在の拒否条件と診断を維持する。
10. 明示 `origin_source` の連鎖に任意の段数上限を設けず、完全な metadata がある再 capture chain は最後まで origin を解決する。

## 確認例

```text
f: (Int, Int -> Int) = &Add::add
f: (Int, Int -> Int) = FnCapture(module: Add, name: add, sig: (Int, Int -> Int))

identity_int: (Int -> Int) = &id
identity_int: (Int -> Int) = FnCapture(module: Global::Function, name: id, sig: (Int -> Int))

identity_string: (String -> String) = &id
identity_string: (String -> String) = FnCapture(module: Global::Function, name: id, sig: (String -> String))

identity_result: Result<(Int -> Int)> = Ok(&id)
Ok(FnCapture(module: Global::Function, name: id, sig: (Int -> Int)))

zero_arg: (-> Int) = {|| Add::add(1, 2)}
zero_arg: (-> Int) = Closure(-> Int)
```

## 対象外と追加 inventory

- ``&`op` `` と ``&`(,)` `` は要件定義上 closure 相当へ lower する。現行の Closure 扱いを変えず、operator / tuple constructor を `FnCapture(module, name)` として表示する identity 規約は別仕様とする。
- Facet capture の表示 identity は OI-036 で未確定のため変更しない。Facet-specific wrapper の Capture / Closure 表示はこの実装の受入条件に含めない。
- generic Trait method の明示 return-type argument、imported helper、user impl が複数ある場合の canonical identity、異なる placeholder 順序・重複、複数段再 capture は受入テスト inventory に含める。

## 実装責務と検証

- Sigil / Scar: source capture と literal Closure の由来を明示した typed representation に残す。named Trait helper capture も由来を落とさない。Facet wrapper の表示 identity は OI-036 に従い保留する。
- Forge: callable value の生成箇所で canonical identity と型解決済み signature を metadata に設定する。共有関数宣言に site-specific signature を書き込まない。
- Eldr: metadata を値の作成・partial wrapper・再 capture で正確に運び、Closure / Unknown の origin を lexical capture から推測しない。inspect と `to_string` の再帰 renderer は同じ metadata 契約を使う。既存の process initializer 推論に必要な delegate function metadata は表示 origin と独立して保持する。
- Xldr: direct binding と nested runtime value の表示が一致し、binding metadata による origin 上書きがないことを確認する。
- 受入テストは named Trait capture / imported helper、generic `id` の Int と String の同時 capture、nested `Ok`、Monoid field、partial capture の residual signature、Capture / Closure 再 capture、Capture を包む Closure、ゼロ引数 Closure、unknown metadata の internal ID 非表示を含める。既存の型受理・拒否 fixture で dispatch の範囲が変化しないことも確認する。
- 実装時は関連 crate と REPL の境界を先に検証し、Level 4 の `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace`、`cargo run -- test --quiet --all`、最終差分の独立レビューを完了条件とする。

## 実装計画

1. Sigil / Scar で Capture origin を明示的に保持し、literal Closure と分離する。named Trait helper を含む capture / closure の成功境界を `rtk cargo nextest run -p sigil` と `rtk cargo nextest run -p scar` で検証する。
2. Forge が resolved callable type と canonical identity から value-local metadata を生成し、Eldr が function/builtin reference、partial wrapper、再 capture へその metadata を運ぶ。`rtk cargo nextest run -p forge` と `rtk cargo nextest run -p eldr` で検証する。
3. Xldr direct binding と nested-value renderer が同一 metadata を使うことを `rtk cargo nextest run -p xldr --test repl_core` で検証する。型受理・拒否 fixture で dispatch boundary も確認する。
4. 最終状態で `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace` と `cargo run -- test --quiet --all` を実行し、別 agent の最終差分レビューを受ける。指摘修正後は Level 4 検証を再実行する。
