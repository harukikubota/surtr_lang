# Surtr REPL 実装タスク指示（短縮版）

## タスク
REPL の Results ペインを `ratatui` の `List` ベースで実装すること。

## 方針
- 実行結果は **1回の評価単位ごとに 1 `ListItem`** とする
- `Table` は使わない
- source/result/error を別項目に分割しない
- CLI 履歴を強化した見た目にする

## 表示形式
各項目は次のような複数行表示にすること。

```text
----------------------------------------
xldr(1)> num = add(1,2)
num: Int = 10
----------------------------------------
```

エラーも同様に 1 項目へまとめる。

```text
----------------------------------------
xldr(2)> add(1, "a")
TypeError: expected Int, found String
----------------------------------------
```

## 必須要件
1. Results ペインは `List` で描画する
2. 1回の入力と結果を 1 項目にまとめる
3. 各項目に区切り線を入れる
4. 先頭行は `xldr(idx)> source` 形式にする
5. 元入力 `source` を必ず保持する
6. `:v idx` は表示全体ではなく `source` を入力欄へ戻す
7. `:j idx` は実行単位 idx の項目選択に接続できる形にする

## データ構造
少なくとも以下のような構造にすること。

```rust
pub enum ResultEntryKind {
    EvalSuccess,
    EvalError,
    CommandOutput,
    Info,
}

pub struct ResultEntry {
    pub idx: usize,
    pub source: String,
    pub rendered_lines: Vec<String>,
    pub kind: ResultEntryKind,
}
```

## 実装指示
- `draw_results` は `app.results` を走査して `ListItem` を生成する
- `ResultEntry -> ListItem` の変換関数を作る
- `source` は後で `:v idx` から再利用できるよう必ず保持する
- `selected_result: Option<usize>` を持てるようにし、将来 `ListState` に接続しやすい形にする
- 最初は色や装飾を増やしすぎず、テキストだけで読めることを優先する

## 禁止事項
- 結果1行ごとに別 `ListItem` に分割しない
- source と result を別ペイン扱いしない
- `Table` ベースへ戻さない
- `:v idx` で区切り線や結果文字列までコピーしない

## 実装順
1. `ResultEntry` の導入または整理
2. `source` 保持
3. `ResultEntry -> ListItem` 変換関数
4. `draw_results` を `List` ベースへ変更
5. `:v idx` を `source` 参照へ修正
6. `:j idx` 用に選択状態を整理

## 完了条件
- Results ペインが `List` で動作する
- 実行単位ごとにまとまって表示される
- `source` を再利用できる
- 将来的な選択・ジャンプ拡張が可能な構造になっている
