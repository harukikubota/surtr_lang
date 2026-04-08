# Surtr

<img src="./vscode-surtr-icons/vsicons-custom-icons/surtr.png" alt="Surtr Icon" width="96" />


Surtr is a statically typed functional language compiler implemented in Rust.

Pipeline:

```
Spire -> Sigil -> Scar -> Forge -> Eldr
                                  ^
                                Rune (CLI entry)
```

## Crates

| Crate | Role |
|---|---|
| `spire` | Parser (`&str -> Vec<Ast>`) |
| `sigil` | Name resolver (`Vec<Ast> -> Vec<Resolved>`) |
| `scar` | Type checker (`Vec<Resolved> -> Vec<TypedNode>`) |
| `forge` | Codegen (`Vec<TypedNode> -> Bytecode`) |
| `eldr` | VM (`Bytecode -> execution`) |
| `rune` | CLI entrypoint (`surtr run`, `surtr build`, `surtr dump`) |

## Quick Start

```bash
cargo run -p rune -- run lib/hello.srt
```

```bash
cargo run -p rune -- build lib/hello.srt lib/hello.eldr
cargo run -p rune -- run lib/hello.eldr
cargo run -p rune -- dump lib/hello.eldr --format json | jq .
```

## Testing

Use `cargo-nextest` as the default test runner for this workspace.

```bash
cargo nextest run
```

For a clean baseline run that includes compilation:

```bash
cargo clean
/usr/bin/time -p cargo nextest run
```

`cargo test` remains available when you specifically want the standard Cargo runner.

## Docs

- [Public Docs Index](./doc/site/README.md)
- [Language Guide](./doc/site/language-guide.md)
- [Language Reference](./doc/site/language-reference.md)
- [Compiler Design Guide](./doc/site/compiler-design.md)
- [Crate Reference](./doc/site/crate-reference.md)
- [Open Issues](./doc/open-issues.md)
- [Requirements (V9, Japanese)](./doc/要件定義v9.md)
- [Float Policy (Japanese)](./doc/float.md)
- [Improvement Workflow (Japanese)](./作業フロー.md)

## Status

Current work is focused on stabilizing the V9 baseline and cleanup items from the recent review pass.

Implemented core includes:

- Primitive types: `Int`, `Float`, `String`, `Boolean`, `Unit`
- `List` literals and empty list typing
- `defstruct` / `defrecord` / `deferror`
- `if` and `match` (Boolean / Result subset)
- Builtins: `print`, `to_string`, `eprint`

Implementation notes:

- `Int` is being migrated to unbounded `BigInt` semantics across the pipeline
- runtime-internal tags stay fixed-width and separate from user-visible `Int`
- `Float` remains implemented, but its precise contract is tracked separately in [doc/float.md](./doc/float.md)
