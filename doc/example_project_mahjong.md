# Example Project: 麻雀点数計算 / シャンテン数計算

## 1. 目的

本ドキュメントは、Surtr の example project として実装する
「麻雀点数計算 / シャンテン数計算」の外部契約を整理するための正本です。

この example project は次を目的とします。

- Surtr で非自明なドメインロジックを記述する実例を作る
- 文字列入力、集約、探索、分解、CLI I/O を組み合わせた実装例を示す
- 言語機能・標準モジュール・エラーハンドリングの実運用イメージを示す

本 project は library-first で構成し、CLI はその薄い入口として実装します。

---

## 2. 対象範囲

### 2.1 入力

- 手牌文字列
  - 例: `"123m456789p567s11w"`
- 13 枚入力
  - シャンテン数を返す
- 14 枚入力
  - 和了形として解釈できる場合は点数計算を返す
  - 和了できない場合はエラーを返す

### 2.2 初期スコープ

初期実装では次を含めます。

- 手牌文字列のパース
- 手牌検証
- 13 枚時のシャンテン数
  - 通常手
  - 七対子
  - 国士無双
- 14 枚時の和了判定
  - 通常手
  - 七対子
  - 国士無双
- 点数計算
  - 役判定
  - 符計算
  - 親子 / ツモロン差分

### 2.3 初期スコープ外

初期段階では次は optional とします。

- 赤ドラ
- 裏ドラ
- 途中流局系
- ローカル役
- ルール差分を広く吸収する設定 DSL

---

## 3. 入力表現

### 3.1 手牌文字列

入力は suit suffix 方式で表します。

- `m`: 萬子
- `p`: 筒子
- `s`: 索子
- `w`: 字牌

例:

- `"123m456789p567s11w"`
- `"19m19p19s1234567w"`

### 3.2 字牌の番号

初期実装では字牌を次で固定します。

- `1w`: 東
- `2w`: 南
- `3w`: 西
- `4w`: 北
- `5w`: 白
- `6w`: 發
- `7w`: 中

### 3.3 入力検証

入力検証では少なくとも次を行います。

- 不正文字の検出
- suit のない数字列の検出
- 字牌の範囲外 digit の検出
- 同一牌 4 枚超えの検出
- 総枚数が 13 または 14 であることの検証

---

## 4. ドメインモデル

### 4.1 基本型

```surtr
defenum Suit {
  Man,
  Pin,
  Sou,
}

defenum Honor {
  East,
  South,
  West,
  North,
  White,
  Green,
  Red,
}

defenum Tile {
  Suited(Suit, Int),
  Honor(Honor),
}
```

### 4.2 手牌

```surtr
defrecord Hand13(tiles: List<Tile>)
defrecord Hand14(tiles: List<Tile>)
```

実装内部では、探索と集約を安定させるために 34 種カウント表を使います。

```surtr
defrecord TileCounts(entries: HashMap<Int>)
```

ここで `HashMap<Int>` は
「牌種 ID -> 枚数」を表す内部表現です。

### 4.3 面子・和了形

```surtr
defenum Meld {
  Shuntsu(Tile, Tile, Tile),
  Kotsu(Tile),
  Kantsu(Tile),
  Toitsu(Tile),
}

defenum AgariShape {
  Standard(List<Meld>),
  Chiitoitsu(List<Tile>),
  Kokushi(List<Tile>),
}
```

### 4.4 コンテキスト

```surtr
defenum WinType {
  Tsumo,
  Ron,
}

defenum Wind {
  East,
  South,
  West,
  North,
}

defrecord WinContext(
  win_type: WinType,
  seat_wind: Wind,
  round_wind: Wind,
  is_dealer: Boolean,
  riichi: Boolean,
  ippatsu: Boolean,
  haitei: Boolean,
  rinshan: Boolean,
  chankan: Boolean,
)
```

### 4.5 結果

```surtr
defrecord ShantenResult(
  normal: Int,
  chiitoi: Int,
  kokushi: Int,
  best: Int,
)

defrecord FuBreakdown(
  base_fu: Int,
  total_fu: Int,
)

defrecord ScoreBreakdown(
  han: Int,
  fu: Int,
  total: Int,
  yaku: List<String>,
)
```

---

## 5. 関数 API

### 5.1 入口 API

```surtr
def parse_hand(input: String) -> Result<List<Tile>>

def parse_hand13(input: String) -> Result<Hand13>
def parse_hand14(input: String) -> Result<Hand14>

def shanten(hand: Hand13) -> Result<ShantenResult>
def score(hand: Hand14, ctx: WinContext) -> Result<ScoreBreakdown>
```

### 5.2 補助 API

```surtr
def to_counts(tiles: List<Tile>) -> TileCounts

def shanten_normal(counts: TileCounts) -> Int
def shanten_chiitoi(counts: TileCounts) -> Int
def shanten_kokushi(counts: TileCounts) -> Int

def agari_shapes(hand: Hand14) -> List<AgariShape>

def agari_standard(hand: Hand14) -> List<AgariShape>
def agari_chiitoi(hand: Hand14) -> List<AgariShape>
def agari_kokushi(hand: Hand14) -> List<AgariShape>

def detect_yaku(shape: AgariShape, ctx: WinContext) -> List<String>
def calc_fu(shape: AgariShape, ctx: WinContext) -> FuBreakdown
def calc_score(shape: AgariShape, ctx: WinContext) -> ScoreBreakdown

def best_score(results: List<ScoreBreakdown>) -> Result<ScoreBreakdown>
```

### 5.3 API 方針

- 文字列から直接点数計算する shortcut は持ってよい
- ただし内部は `parse -> domain -> compute` の 3 層に分離する
- 探索結果が複数あるものは `List` で返す
- 失敗は `Result` で返す
- `NoMatch` ではなく利用者向け concrete error を返す

---

## 6. CLI

### 6.1 コマンド方針

CLI は library API の薄い wrapper とします。

想定コマンド:

```text
surtr-mahjong shanten <hand>
surtr-mahjong score <hand> [options]
```

### 6.2 `shanten`

```text
surtr-mahjong shanten "123m456789p567s"
```

出力例:

```text
normal: 1
chiitoi: 3
kokushi: 10
best: 1
```

### 6.3 `score`

```text
surtr-mahjong score "123m456789p567s11w" --tsumo --seat east --round east
```

出力例:

```text
han: 3
fu: 40
total: 5200
yaku:
- 門前清自摸和
- 役牌
```

### 6.4 CLI options

最低限の option は次を持ちます。

- `--tsumo`
- `--ron`
- `--seat east|south|west|north`
- `--round east|south|west|north`
- `--dealer`
- `--riichi`
- `--ippatsu`
- `--haitei`
- `--rinshan`
- `--chankan`

### 6.5 終了コード

- `0`: 正常終了
- `1`: 入力エラー
- `2`: 和了不能 / 計算不能
- `3`: 実装外ルールまたは内部失敗

---

## 7. エラー方針

利用者向けには concrete error を返します。

```surtr
deferror InvalidHandString(detail: String) { detail }
deferror InvalidTileCount(detail: String) { detail }
deferror InvalidHandSize(detail: String) { detail }
deferror NotAgari(detail: String) { detail }
deferror UnsupportedRule(detail: String) { detail }
```

方針:

- parse 失敗は `InvalidHandString`
- 枚数・重複違反は `InvalidTileCount` / `InvalidHandSize`
- 14 枚だが和了不能なら `NotAgari`
- まだ未対応の分岐は `UnsupportedRule`

---

## 8. 実装レイヤ

### 8.1 `domain`

- `Tile`
- `Suit`
- `Honor`
- `Meld`
- `AgariShape`
- `WinContext`
- `ScoreBreakdown`

### 8.2 `parser`

- 手牌文字列の分解
- digit block + suit suffix の読み取り
- `List<Tile>` 生成

### 8.3 `core`

- `TileCounts`
- シャンテン計算
- 和了形列挙

### 8.4 `scoring`

- 役判定
- 符計算
- 最終点数化

### 8.5 `cli`

- 引数パース
- context 構築
- 出力整形

---

## 9. 実装順

1. `Tile` / `Suit` / `Honor`
2. 手牌文字列パーサ
3. 入力検証
4. `TileCounts`
5. 13 枚シャンテン
6. 14 枚和了判定
7. 通常手分解
8. 七対子 / 国士無双
9. 役判定
10. 符計算
11. 点数化
12. CLI

---

## 10. 設計方針

- Extractor / MatchBlock は局所分解に使う
- 主たる探索は `List` ベースで行う
- API は `Result` を優先し、失敗を明示する
- CLI は薄く保ち、計算ロジックは library に閉じ込める
- example project として、Surtr の標準機能だけで読める構成を優先する

以上を、この example project の初期設計方針とする。
