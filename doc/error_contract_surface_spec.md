# Result / MatchResult の Error 位置と表示

入力仕様: 2026-10-03 の会話。level4（型注釈の受理規則とフェーズ間の表示契約）。

- Result と MatchResult は型定義どおり payload の型引数1個を取る。変数、引数、field、関数型、入れ子の型に Error 位置を設けない。NoneError の例外も削除する。
- 関数定義の直接の戻り値に限り Result<T, E> を許可する。named defextractor の直接の戻り値に限り MatchResult<T, Error> を許可する。既存の Error marker 検査は維持する。
- 値、型エラー診断、補完候補とシグネチャヘルプ、runtime callable metadata は Result<T> / MatchResult<T> を表示する。REPL コマンドの定義シグネチャは、定義に明記された Error 位置のみを保持し、省略された Error を補わない。
- ExtractorClosure の値表示は共通の inspect 処理で ExtractorClosure<(A -> MatchResult<P>)> とする。束縛の LHS / RHS、単独評価、コンテナ内、inspect で同じ規則を使う。
- MatchResult の一般値位置での使用禁止、評価規則、Error marker の意味は変更しない。

## 実施順序と受入条件

1. Scar / REPL の回帰テストで変数の NoneError 例外と inspect の不統一を再現する。定義の戻り値、関数型の戻り値、入れ子の型を区別する成功・拒否例を追加する。
2. Scar の2つの解決経路から例外を削除し、関数型の内側へ定義戻り値の許可文脈を伝播させない。入れ子の ignored-input callable の許可文脈は保持する。診断と Forge の値用 metadata を正規表記へ揃える。
3. Sigil の定義シグネチャで Error の自動補完を削除し、impl の metadata owner を resolver と揃える。ExtractorClosure を返す呼び出しでは返却型に応じた callable signature を設定する。Eldr の共通 inspect を使って Xldr の束縛表示の特例を削除する。関連する期待値と docs/site / docs/dev / lib の @doc を整合させる。
4. 対象回帰テスト、rtk cargo nextest run --profile ci --workspace、cargo run -- test --quiet --all、format / diff チェックを行う。最終差分の別エージェントレビューを受け、指摘を解決する。

コミット・マージは行わない。既存の未コミット変更を維持して補完する。
