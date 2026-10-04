# Constructor capability の値の由来を保持する修正

既存仕様の修正として、型の具象化によって値に与えられた能力制約が強まらないようにする。入力ファイルなしで実装するという利用者の指定に従う。level3 とし、既存のワークツリーで実施する。

## 受入条件

- generic な引数間の同一型変数関係を使って、実際の入力値の由来を未注釈 callback の仮引数へ渡す。callback の本文はその由来を設定してから一度検査する。
- 通常関数、Trait method、`do` の生成 callback で同じ規則を使う。引数の記述順や carrier 名に依存しない。
- generic 関数の capture は、元の宣言が要求した constructor capability を関数値の利用時も検査する。通常呼び出し、`Function::apply`、`fmap`、`ap` を含む。
- `List<$F<Int>>` のように宣言された要求が入力の内側にある場合も、構造に沿って射影する。直接呼び出しと capture の同じ誤受理を回帰で固定し、共通処理で修正する。
- Extractor の成功値も、元の宣言の入力と出力の型変数関係に従って由来を射影する。`match`、`=?`、`apply_pattern` に共通で適用する。
- `Functor` のみを保持する値に `Monad` を要求する利用は拒否する。fresh な nominal 値や十分な能力を保持する値は受理する。
- 独立して固定 nominal 型を宣言した関数や、明示した callback parameter の契約は変更しない。`do` の束縛パターンの型注釈は値の由来を強めない。

## 実施順序

1. Astra が対象経路と現行仕様の境界を調査する（完了）。
2. Scar の登録済み surface テストと carrier テストに成功・拒否境界を追加し、誤受理による Red を確認する（完了）。
3. callable 引数の共通検査で由来を伝播し、capture の要求を保持する。Extractor の成功値の射影を補う。旧経路や再検査 fallback は追加しない。
4. 正本文書へ既存規則の適用範囲を記載し、対象 Scar テスト、標準テスト全件、workspace CI profile を実行する。最終差分を Astra にレビューさせる。

検証コマンドは `rtk cargo nextest run -p scar --test typecheck_surface --test type_constructor_carriers`、`cargo run -- test --quiet --all`、`rtk cargo nextest run --profile ci --workspace --test-threads 4` とする。完了した変更は利用者のコミット依頼に従ってコミットする。main への統合は対象外。
