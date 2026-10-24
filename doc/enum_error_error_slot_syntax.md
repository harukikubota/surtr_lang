# Result の Error 位置の表記

ユーザーが 2026-10-13 に確認した変更仕様。先行する EnumError 仕様と訂正文のうち、Error 位置の明示 `_` を許可する記述をこの規則で置き換える。

`Result<T>` は省略した Error 位置を内部で共通 Error へ導出する短縮形である。内部の導出規則をソースに `_` として書くことはできない。`Result<T, _>` と `ResultT<M, T, _>` は、変数、field、引数、戻り値、関数型、入れ子、call-site ReturnTypeArgument のすべてで拒否する。Error 位置には Error、EnumError、具体化時に検査する型変数を指定する。Result の成功型や通常の型引数にある `_` の規則は変えない。ResultT は三引数を維持し、共通 Error も明示する。

共通 Error の標準署名は `Result<T>` と書く。宣言コマンドは記述された metadata を保持し、補完・実型表示は共通 Error を省略して EnumError を明示する。MatchResult の実型表示は `MatchResult<T>` に揃える。

型構文の受理規則を変更するため level4 とする。旧受理経路を削除し、入力した `_` の span を使って診断する。

1. 拒否テストで各型位置と RTA を固定し、短縮 Result と明示 Error / EnumError の成功を残す。
2. Scar の通常注釈・signature・RTA 解決を揃える。標準 ResultT、fixture、補完、文書を更新する。
3. 対象テスト後、`rtk cargo nextest run --profile ci --workspace` と `rtk proxy cargo run -- test --quiet --all` を直列実行する。
4. Astra に新規問題と最終差分を確認してもらい、未対応指摘と失敗がなくなった時点で完了とする。

コミット・マージは実施しない。

## 実装・検証結果

通常注釈、signature、RTA、Error marker の明示 `_` 補完を削除した。標準 ResultT と文書を明示 Error に揃え、補完と宣言コマンドの表記を分けた。既存の typed Result constructor は成功型一引数のままとし、その arity 拒否を変更していない。

- `rtk cargo nextest run --profile ci --workspace`: 2,573 passed、71 binaries。
- `rtk proxy cargo run -- test --quiet --all`: 終了コード 0。
- 型注釈文書の二行のサンプルを実行し、Result と ResultT の内側 Result がともに `Ok(10)` になることを確認。
- Astra の最終レビュー: 未対応指摘なし。
- `git diff --check`: 成功。

変更はこの worktree に未コミットで保持している。
