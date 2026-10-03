# Error の生成位置と Pattern 不一致の表示

## 入力仕様と対象

チャットで合意した調査結果を実装入力とする。Error を生成した行で十分に追跡できるため、
呼出し境界ごとの位置変更規則は追加しない。REPL も script と同じ生成位置の契約を使う。

Error の生成位置は parser / resolver / checker / codegen / VM / renderer を通る契約であり、
変更 level は 4 とする。伝播する既存 Error の kind、message、cause、Result-effect 選択、式の評価順を保つ。
構文 Pattern の失敗は正本の診断を使う。固定長 list の長さ不一致は、入れ子でも root と同じ
`IndexOutOfBounds` とし、失敗した list 自体の位置を指す。

## 振る舞い

- 明示的な Error は構築式の位置を主キャプションにする。
- 構文 Pattern の不一致は実際に失敗した子だけを表示する。構造自体の失敗はその構造 Pattern を表示する。
- 関数、named Extractor、ExtractorClosure、`apply_pattern` は同じ規則を使う。
- carrier への格納と SafeBind の伝播は元 Error の kind / message / location / cause を保持する。
- partial `<-` は Result-effect context だけ元 Error を保持する。非 Result-effect の Option / List 等は現行どおり Error を破棄し Alternative の empty を返す。
- 新しい Error で wrap したら、新しい Error の構築位置を表示する。元 Error は cause として元の位置を保つ。
- 呼出し経路は stack trace に残す。主キャプションを stack trace の先頭で上書きしない。
- REPL の入力単位ごとに元ソースを保持し、後続入力で呼び出した関数・Extractor 内の Error も生成元の入力内の行・列を表示する。複数行と Unicode scalar value 単位の座標を維持する。
- `eprint` も生成位置をそのまま表示し、REPL 専用の行加算や列の置換を行わない。

```surtr
1 =? Err(NoneError)    # RHS の Error 構築位置
1 =? Int::parse("2")  # LHS の 1
```

## 実装手順と受入条件

1. CLI script と VM / compiler の直接テストを先に追加し、生成位置が呼出し位置や RHS に変わる失敗を確認する。
2. Pattern の各ノードの元 span をフェーズ間で保持し、最初に失敗した子の Error 生成へ渡す。親位置で代用する旧経路を除去する。
3. VM と表示側で生成位置を保持し、Result-effect partial `<-` は既存 Extractor Error を返す経路へ接続する。Result-effect のない経路は現行どおり維持する。
4. 正本の diagnostics / Pattern / do / EldrVM / 利用者向け error handling を同期する。
5. 対象テスト、`rtk cargo nextest run --profile ci --workspace`、`cargo run -- test --quiet --all` を実行し、最終差分の別エージェントレビューを行う。

literal / pin / list / constructor / as-pattern の失敗位置、Extractor 成功後の子の不一致、
定義内 Error と構文不一致、伝播と wrap、Result と `OptionT<Result>` の元 Error・cause 保持、
非 Result-effect Option / List の既存挙動を受入条件とする。
Human と VM dump が同じ生成位置を示し、Unicode scalar value 単位の span を維持することを確認する。
旧経路、Error 名や message による位置推測、位置欠落を曖昧に隠すフォールバックは設けない。

## REPL 対応の実装計画

変更 level は 4。言語の評価規則を変えず、入力ソースと実行時位置情報のフェーズ間契約を script に揃える。

1. Xldr core と Rune REPL CLI に、直接入力、複数行・Unicode、別入力で定義した Error 返却関数の又呼び、Extractor の又呼びと cause 保持の回帰ケースを追加し、旧経路での失敗を確認する。
2. 入力単位の AST 全体へ source ID を付けた span を渡し、VM に生成元ソースを保持させる。上書きされる現在入力への依存と `eprint` の REPL 専用補正を除去する。
3. diagnostics / Xldr の正本を同期し、static diagnostics、dbg、保存・再読込、preload への影響を確認する。
4. 対象テスト、`rtk cargo nextest run --profile ci --workspace`、`cargo run -- test --quiet --all` を実行する。独立コンテキストの Astra に新規問題と最終差分を相談し、指摘を解決する。
