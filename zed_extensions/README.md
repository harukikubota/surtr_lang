# Surtr for Zed

`.srt` ファイルの構文色分けを提供します。配色は Zed のテーマに従います。

## ローカルで使う

Node.js 20以上、npm、Gitを使います。テストにはCコンパイラも必要です
（macOSではXcode Command Line Tools）。リポジトリのルートから実行します。

```sh
cd zed_extensions
npm ci
npm test
npm run dev
```

Zed のコマンドパレットで `zed: install dev extension` を実行し、
このディレクトリ内の **`dist/surtr`** を選びます。
`.srt` ファイルを開き、言語が `Surtr` になっていることを確認してください。
色分けの確認には既存の `lib/kernel.srt` などを使えます。

文法のコンパイルに必要な wasi-sdk は Zed がダウンロードします。
公開用の拡張登録や LSP の設定は不要です。
文法やクエリを変更したら `npm test` と `npm run dev` を実行し、
Zed の拡張画面で開発用拡張を Rebuild してください。

## 対応範囲

- 宣言・制御キーワード、Boolean、数値（基数表記・`ms` を含む）、演算子
- 型名・型変数、関数の宣言・呼出し・capture、末尾 `?` 付きの関数
- フィールド、placeholder、ソース位置リフレクション
- 行コメントと行頭のアノテーション
- 両引用符の文字列、Unicode を含むエスケープ、三重引用符の raw 文字列
- `#{...}` の埋め込み式と、その内部の文字列・コメント・括弧
- 括弧・引用符・文字列補間の開始と終了の対応表示
- 括弧・引用符の自動補完（文字列本文・コメント・エスケープ内では抑制）
- ブロック・引数・リスト・文字列補間の自動インデント（既定はスペース2個）
- 関数・module・trait・型・impl・定数・型エイリアスのアウトライン
- `def`・`defmod`・`match`・`do`・`doc`のスニペット
- `Test`モジュールの`test`・`describe`・`it`・`xit`・`pend`、
  `assert_eq`・`assert_true`・`assert_false`のスニペット

`test`スニペットは`import Test;`を含むスイート全体を展開します。
他のテスト用スニペットには、そのファイルに`import Test;`が必要です。
ケース末尾のアサーションは`Result<()>`をそのまま返します。
複数のアサーションを並べるときは、途中のアサーションだけに`?`を付けます。
`xit`・`pend`には理由を記入してください。テストは`surtr test <file>`で実行します。

Tree-sitter文法は宣言の範囲、トークンと括弧の構造を扱います。
名前解決や型検査は行わず、構文の妥当性の判定は Spire に任せます。
大文字始まりの識別子を型名、呼出し位置などの識別子を関数名として分類します。
`re"..."` の本文は通常の文字列として着色します。
LSPによる補完、診断、定義移動には対応しません。

## 構成と検証

- `tree-sitter-surtr/`: 文法と文字列境界・アノテーション用の外部 scanner
- `surtr/`: 拡張メタデータと言語設定、`highlights.scm`
- `tests/`: Tree-sitter が実際に生成した構文木・capture の検証
- `scripts/prepare-dev.cjs`: 開発用拡張の準備

`npm run dev` は `dist/grammar-source` に破棄可能な Git リポジトリを作り、
生成した C 文法だけをスナップショットにします。
Zed はローカル文法にも取得先と Git revision を要求するため、このスナップショットを
`dist/surtr/extension.toml` の `file://` 参照に使います。
Surtr 本体の Git 履歴・index は変更しません。`dist/` は生成物として除外しています。
生成済み C 文法は `npm run generate` で再生成し、追跡しません。

検証は `npm test` で行います。色分けの境界条件に加え、`lib/`内の全`.srt`ファイルを
解析し、文字列や括弧の境界が崩れないことを確認します。
コンパイラや標準定義は変更しないため、
Cargo の全体テストはこの拡張の検証には使いません。

参考: [Zed の言語拡張仕様](https://zed.dev/docs/extensions/languages)、
[開発用拡張の導入手順](https://zed.dev/docs/extensions/developing-extensions)。
