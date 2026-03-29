# Surtr

<img src="./vscode-surtr-icons/icons/surtr.png" alt="Surtr Icon" width="96" />


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

## Docs

- [Phase 1 Flow](./doc/phase1-flow.md)
- [Open Issues](./doc/open-issues.md)
- [Ideas (Draft)](./doc/Idea7-10.md)
- [Requirements (V8, Japanese)](./doc/要件定義v8.md)

## Status

Current implementation target is Phase 1.

Implemented core includes:

- Primitive types: `Int`, `Float`, `String`, `Boolean`, `Unit`
- `List` literals and empty list typing
- `defstruct` / `defrecord` / `deferror`
- `if` and `match` (Boolean / Result subset)
- Builtins: `print`, `to_string`, `eprint`
