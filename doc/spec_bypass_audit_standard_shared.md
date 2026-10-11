# 標準定義・共有表現の暗黙処理とフォールバック調査・修正方針

項目番号は元の調査との対応のため維持する。SD-06の簡素化案とSD-10のLSP詳細は、未確定の検討事項を含む。

## SD-06 ANSI escape の生成失敗が空文字になる

- 性質: 定数入力に対する内部成功フォールバック。確度: 静的読解。
- 実装: `lib/styled_doc.srt:232-236` の `String::from_codepoints([27], StringEncoding::Utf8)` が失敗すると空文字。
- ガード: 27 は Unicode scalar として有効。正常な builtin 契約では Error にならず、公開引数からの失敗再現はない。
- 正本: `lib/styled_doc.srt:137-147` の ANSI rendering、`String::from_codepoints` の文書・宣言 (`lib/types/string.srt:435-447`) が対応する。失敗時に escape を消す契約はない。

```surtr
String::from_codepoints([27], StringEncoding::Utf8)
# 正常期待値: Ok(ESC を1文字含む String)
# 内部条件: この定数変換が Err。
# 現行静的期待値: _esc() が ""、ANSI sequence の prefix が欠ける。
```

### 修正方針と確認済みの前提

分類: **メタ文字対応を前提に表現の簡素化を検討する**。文字列のエスケープ対応は改修済みで、確認時点の `docs/site/strings.md:26-44` は `\u{HEX}` を Unicode スカラー値1文字として定義している。定数 ESC は文字列リテラルで表せる。

```surtr
# 今後の置き換え案。実装済みという意味ではない。
def _esc() -> String { "\u{1b}" }
# 期待値: U+001B を1文字含む String。
# "\\u{1b}"（バックスラッシュから始まる文字列）とは区別する。
```

この案なら codepoint 変換と `Err => ""` 自体が不要になる。メタ文字の decode・表示は担当クレートの既存契約に従う。ANSI の色・reset・通常テキストの出力を維持し、無効な文字列 escape を成功へ変える経路は設けない。リテラルへの置換そのものは今回の指示で確定した実装ではなく、実装時の簡素化案として記録する。

## SD-10 任意メタデータと editor tolerant parse は許可された補助経路

成果物の配置、任意チャンク、tolerant parse、ProjectRunnerの補助経路は別々に判断する。未確定の改修範囲は[LSPのエディタ解析・成果物境界の改修ドラフト](lsp_editor_artifact_boundary_draft.md)で扱う。

`target/`内への配置はユーザーの成果物との衝突回避には使えるが、それだけで解析経路やmetadataの契約は変わらない。正常診断ではソースを解析し、エディタの部分ASTはコンパイル成功に転用しない。任意`Docs` / `SigT`の欠損と、存在するpayloadの破損は区別する。これらの既存契約を維持し、LSPの未確定仕様をこの項目で確定させない。
