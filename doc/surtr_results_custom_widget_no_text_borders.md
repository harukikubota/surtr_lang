# Surtr REPL Results ペイン 実装指示（罫線文字禁止版）

## 方針

Results ペインは `ratatui` の **custom widget** として実装すること。  
`List` は使わず、Results 専用の state + widget を持つ構成にする。

---

## 最重要ルール

### 罫線文字でカードを描画しないこと

以下のような **文字列としての罫線描画は禁止** とする。

- `----`
- `│`, `┌`, `└`, `─` などを本文文字列に埋め込む方式
- テキストを整形して枠に見せる方式

### 必ず `ratatui::widgets::Block` を使うこと

- ペイン全体の枠
- 各 result entry の枠

の両方を、**文字列ではなく widget 描画として実装**すること。

---

## 目的

文字列ベースの罫線描画だと、以下の問題があるため禁止する。

- ターミナル幅で崩れやすい
- 折り返し時に不安定
- 色や border style の制御がしにくい
- 選択状態やフォーカス強調との整合が悪い

そのため、**フレームはすべて `Block` の border 描画に統一**すること。

---

## 実装構成

### state 側
```rust
pub struct ResultEntry {
    pub idx: usize,
    pub source: String,
    pub rendered_lines: Vec<String>,
    pub kind: ResultEntryKind,
}

pub struct ResultsPaneState {
    pub entries: Vec<ResultEntry>,
    pub selected_idx: Option<usize>,
    pub scroll_y: u16,
}
```

### widget 側
```rust
pub struct ResultsPaneWidget<'a> {
    pub state: &'a ResultsPaneState,
    pub focused: bool,
}
```

---

## 描画ルール

## 1. ペイン全体
Results ペイン全体は `Block` で囲うこと。

### 要件
- タイトルを表示する
- フォーカス中は **黄色の border**
- 非フォーカス時は通常色
- まず outer block を描画し、その inner area に entry 群を描画する

---

## 2. 各 result entry
各 entry も **個別の `Block`** で描画すること。

### entry の見た目
- 1 entry = 1 カード
- カードごとに矩形領域を持つ
- カード内に以下を描画する
  - `xldr(idx)> source`
  - result / error / command output の本文

### 重要
- entry 本文の文字列の中に罫線を入れない
- 枠線は必ず `Block` が描くこと

---

## 3. レイアウト
ResultsPaneWidget 内で可視 entry を縦方向に積むこと。

### 流れ
1. Results 全体 block を描画
2. inner area を取得
3. visible な entry を上から順に配置
4. 各 entry について必要高さを計算
5. entry 用 `Rect` を切る
6. その `Rect` に entry block を描画
7. block の inner area に本文を描画

---

## 4. 色指定
色指定は widget style で行うこと。

### ペイン
- focused: yellow border
- unfocused: default or gray

### entry
- selected entry: border or title color を変更してよい
- error entry: title or text に error 用 style を付けてよい
- command output / info は区別できる style を付けてよい

---

## idx の扱い

各 `ResultEntry` は `idx` を持つこと。  
この `idx` は REPL 上の実行単位 ID であり、コマンドから参照可能であること。

### 用途
- `:j idx`
- `:v idx`
- 将来の pin / inspect / copy

---

## コマンドとの接続ルール

### 重要
コマンドは widget 自体を参照しないこと。  
参照対象は **state 内の ResultEntry** とすること。

### 正しい形
- `:j 5` -> `selected_idx = Some(5)`
- `:v 5` -> `entries.iter().find(|e| e.idx == 5)` で検索し `source` を入力欄へ戻す

### 禁止
- widget インスタンスをデータ所有者として扱う
- widget へ直接コマンド解決を結びつける

---

## `:v idx` のルール

`source` のみを入力欄へ戻すこと。  
以下はコピー対象に含めないこと。

- 枠線
- result 本文
- error 本文
- title 表示

---

## `:j idx` のルール

`idx` に対応する entry を選択状態にすること。  
必要ならスクロール位置も調整し、対象 entry が見えるようにすること。

---

## 禁止事項

以下はすべて禁止。

1. `┌`, `└`, `│`, `─` などを文字列として埋め込んで枠を描くこと
2. `----` の連続で区切りを表現すること
3. 結果項目を `ListItem` に戻すこと
4. `Table` ベースへ戻すこと
5. widget をコマンド参照対象にすること
6. `Vec` の添字を `idx` と同一視すること

---

## 実装の期待値

以下の状態を目指すこと。

- Results ペイン全体が `Block` で描画される
- 各 result entry も個別 `Block` で描画される
- フォーカス中ペインは黄色枠で強調される
- entry ごとに `idx` が保持される
- `:j idx` と `:v idx` が state の `idx` を使って解決される
- 枠線はすべて widget と style により描画され、文字列罫線を使わない

---

## 完了条件

1. Results が custom widget になっている
2. ペイン全体の枠が `Block` で描画されている
3. 各 entry の枠も `Block` で描画されている
4. 文字列罫線を一切使っていない
5. フォーカス中ペインが黄色枠で強調される
6. `idx` を使って `:j` と `:v` が解決できる
