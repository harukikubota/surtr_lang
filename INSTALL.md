# Install

## Surtr

```bash
cargo build
cargo run -p rune -- repl
cargo run -p rune -- run path/to/file.srt
```

`cargo` と Rust toolchain が入っていれば、まずはこれで Surtr 本体を動かせます。

## Install as a binary command

`surtr` をシェルから直接呼べるようにするには、workspace ルートで次を実行します。

```bash
cargo install --path crates/rune --root ~/.local
```

`~/.local/bin` が `PATH` に入っていれば、以後は次のように実行できます。

```bash
surtr repl
surtr run path/to/file.srt
surtr build path/to/file.srt output.eldr
```

## VSCode extensions

VS Code 向けの補助拡張は `vscode-extensions/` にあります。

- `vscode-extensions/surtr-language-support`
- `vscode-extensions/surtr-tools`
- `vscode-extensions/surtr-file-icons`
- `vscode-extensions/eldr-viewer`

各拡張の導入手順は、それぞれの `README.md` / `INSTALL.md` を参照してください。

## Recommended reading

- 利用者向け: `docs/site/README.md`
- 開発者向け: `docs/dev/README.md`
- 内部設計: `docs/internal/README.md`
