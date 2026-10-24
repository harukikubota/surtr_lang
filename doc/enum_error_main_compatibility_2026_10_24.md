# EnumError と現行 main の互換性調査

この文書は 2026-10-24 の移植前の調査記録である。以下の問題と未検証範囲は調査時点のもの。後続の実装と検証は [現行 main 追従の計画・結果](enum_error_main_port_plan.md)に記録する。

調査時点では、変更をそのまま取り込める状態ではなかった。機械的な競合に加え、f504 単体の宣言順序依存を再現し、main の名義型表現・MonadExtractor との接続に対応が必要と確認した。この調査では製品コードの修正や main への統合はしていない。

比較対象は以下で固定した。

- f504 の基点: `72154707399d6cdaf7190ab75007f2049ab010c6`。
- EnumError: f504 の未コミット差分。明示 Error 位置の `_` 禁止を含む。
- main: `a7d75ed064504079660cd58149a4f3dea41df14b`。基点から 51 コミット進んでいる。

## 確認した問題

1. **P1・実行再現済み: EnumError を field より後に宣言すると拒否する。**

次の入力を、f504 の現行ソースから起動した `cargo run -- check` で確認した。

```surtr
deferror Missing { "missing" }
defstruct Holder { value: Result<Int, Failures> }
defenumerror Failures { Missing }
impl Holder {
  def new(value: Result<Int, Failures>) -> Holder {
    Holder { value }
  }
}
```

終了コードは 1。field の `Result<Int, Failures>` を `Result Error position must be Error or EnumError` として拒否し、構造化 reason は `TypecheckInvariantViolation` になった。`defenumerror Failures` を `defstruct Holder` より前へ移すだけで、終了コード 0、`{"errors":[]}` となる。

原因は、[EnumError metadata の登録](/Users/haruca/.codex/worktrees/f504/surtr/crates/scar/src/checker/predeclare.rs:1062)が Pass 2 の Enum 宣言到達時である一方、先行 field の Error 位置検査がその metadata を必要とすること。型名だけは予約されているが、集合としての判定には members の登録も必要である。

main の同 stage 宣言順序独立の契約を保つには、owner 予約後、field / payload の解決前に、当該 stage の EnumError members・Other・宣言 span を準備する必要がある。Kind だけを見て検査を通す方法では、後段の所属検査や診断情報が欠落する。

2. **P1・型定義と仮想マージで確認: 新しい NominalType と API が一致しない。**

main の [NominalType](/Users/haruca/work/rust/surtr/crates/scar/src/types.rs:7) は `arguments` だけを保持し、`new` も一引数になっている。field は解決済み宣言の schema から取得する。

一方、EnumError 差分には、[error_slots.rs の nominal.fields](/Users/haruca/.codex/worktrees/f504/surtr/crates/scar/src/checker/error_slots.rs:192)、[error_transport.rs の fields 比較](/Users/haruca/.codex/worktrees/f504/surtr/crates/scar/src/checker/error_transport.rs:80)、[carriers.rs の二引数 new](/Users/haruca/.codex/worktrees/f504/surtr/crates/scar/src/checker/carriers.rs:22) が残っている。これらは仮想マージでも残り、現行 main の型定義ではコンパイルできない。

field の参照は main の [nominal_fields](/Users/haruca/work/rust/surtr/crates/scar/src/checker/inference.rs:1152) 等へ移す必要がある。旧 fields を型値へ戻すのではなく、宣言 schema を参照する現行設計を維持する。再帰する型の変換検査も、この表現で再検証する必要がある。

3. **P1・コード上の不整合、統合実行は未検証: MonadExtractor の能力判定と変換が接続されていない。**

main は SafeBind の [入力型に対する能力判定](/Users/haruca/work/rust/surtr/crates/scar/src/checker/expr/do_block.rs:1444)を行ってから、通常の `MonadExtractor::extract` を呼ぶ。f504 の標準実装境界は、選択された標準実装に対し EnumError carrier を共通 Error へ変換する。

この境界を能力判定に接続しないと、`Result<T, Closed>` の正当な読み出しを抽出対象として認識できず、拒否するおそれがある。反対に Trait 呼出しだけへ変換を追加すると、main の [bare Var を要求する引数検査](/Users/haruca/work/rust/surtr/crates/scar/src/checker/expr/do_block.rs:1262)に `ErrorCoerce(Var)` が渡るため、IR 契約が一致しない。

能力判定と実呼出しで同じ標準境界を使い、チェック済みの変換と元の RHS の一回評価を保つ必要がある。partial `<-` にも [TraitCall → Closure → Match を直接検査する処理](/Users/haruca/work/rust/surtr/crates/scar/src/checker/expr/do_block.rs:751)があり、返却値の ErrorCoerce を含めて移植する必要がある。

再帰的な文末 `?` は、到達した各層で抽出境界を適用する。Result の成功型内部にある別の E まで一括で消去してはいけない。利用者の専用 impl を標準実装の規則で上書きしないことも維持する。

なお、非抽出の `_ =? value` が Err を無視して続行するという懸念は採用しない。main は [total pattern かつ非抽出の場合](/Users/haruca/work/rust/surtr/crates/scar/src/checker/expr/do_block.rs:1564)に拒否する。

## 仮想マージの結果

通常の index・branch・作業ファイルを変えず、専用の一時 index に 308 パスの差分を取り込み、`git merge-tree --write-tree --merge-base=<基点> <main> <差分tree>` で確認した。コミットは作成していない。

結果は終了コード 1、競合は 19 パス。うち以下の四つは main で分割・削除済みのファイルへの modify/delete 競合である。

- `crates/scar/src/checker/definitions.rs`
- `crates/scar/src/checker/expr.rs`
- `crates/scar/src/checker/predeclare.rs`
- `crates/scar/src/checker/specialize.rs`

旧モジュールを復活させる競合解消ではなく、main の分割先へ機能を配置し直す必要がある。新しい `AssertErrorMatches`、Pattern の `Located`、Facet の `segment_focus_tys`、宣言 stage / session / audit の処理を保って、ErrorCoerce / ErrorOther の走査を追加する。

競合全体は diagnostics、Forge、Scar、Spire / Sigil のテスト、Xldr のテスト、開発文書に及ぶ。統合ツリーが未確定のため、統合後のビルドと全件テストは実施していない。以前の 2,573 件成功は旧基点での結果であり、今回の main との互換性の証拠にはしない。

## 自動マージで保持されたもの

次の内容は実際の仮想マージで両立しており、現在の競合・不具合としては数えない。

- Result の main 側 MonadExtractor / identity Convert と、f504 側の Err 再構築。
- ResultT テストの main 側 `fail_test` と、f504 側の三引数注釈。
- 型関係 rollback の main 側 constructor witness と、f504 側の Error obligation sets。
- Do 文書の main 側 MonadExtractor / 再帰 `?` と、追加した EnumError failure target 条件。

通常 block の非最終文 Unit 制約と、f504 の Unit binding → 最終 closure という callable adapter に直接の矛盾は見つかっていない。

## 取り込み前に必要な確認

main を土台に、名義型・分割モジュール・stage の契約へ移植し、宣言順序依存を修正する。その上で、MonadExtractor 境界を追加する。

受入ケースは、同 stage の宣言順の入替え、明示 extract、`expect_error`、SafeBind 一段分解、混在・入れ子の `?`、partial `<-`、Error 情報保持、一回評価、利用者 impl の優先、閉集合の失敗先拒否とする。ResultT の三引数化と `_` 禁止も維持する。

main の `monad_extractor`、`assert_error_matches`、非最終文 / 宣言 stage のテストに EnumError の対象テストを組み合わせ、標準 Result / ResultT / MonadExtractor、fixture と CLI 境界を確認する。その後、全体 CI と標準テスト全件を実行する。

新規問題は Astra と確認した。今回の成果は調査結果であり、修正・統合済みという意味ではない。
