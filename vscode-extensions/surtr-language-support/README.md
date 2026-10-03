# Surtr Language Support

Surtr source (`.srt`) 向けの基本編集体験を提供します。

- language id: `surtr`
- TextMate syntax highlighting
- language configuration
- snippets
- file outline 用の簡易 document symbols

ハイライトは現行 lexer の予約語・記号に合わせ、型変数、
キャプチャと placeholder、Pattern、パイプと関数合成、関数リテラル、
数値の基数表記、`ms`、正規表現生成文字列を扱います。
両引用符と三重引用符の文字列補間では内部の式を着色します。
行頭の空白に続く `@[A-Za-z][A-Za-z_0-9]*` だけをアノテーションとして着色します。
式の途中や文字列補間内の `@` は分類から除外し、`@doc` の本文は通常の三重引用符文字列として扱います。
実際の色は VS Code のテーマに従います。TextMate による構文分類のため、名前解決や型検査は行いません。

## 開発

```bash
cd vscode-extensions
npm install
cd surtr-language-support
npm run build
npm test
```

テストは `vscode-textmate` と `vscode-oniguruma` を使い、VS Code と同じ
TextMate 文法のトークン化で scope と複数行の状態を検証します。

VS Code ではこのフォルダを開いて `F5` を実行すると Development Host で確認できます。
