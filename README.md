# Surtr

<img src="./icons/surtr.png" alt="Surtr Icon" width="96" />


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
cat > main.srt <<'EOF'
print("hello, Surtr")
EOF

cargo run -p rune -- run main.srt
```

```bash
cargo run -p rune -- build main.srt main.eldr
cargo run -p rune -- run main.eldr
cargo run -p rune -- dump main.eldr --format json | jq .
```

## Testing

Use `cargo-nextest` as the default test runner for this workspace.

```bash
cargo nextest run
```

For coverage runs, install the coverage toolchain once:

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov
```

Then use the repository aliases:

```bash
cargo cov
cargo cov-html
cargo cov-json
```

- `cargo cov`: runs workspace tests through `cargo llvm-cov nextest --workspace`
- `cargo cov-html`: writes an HTML report to `target/llvm-cov/html`
- `cargo cov-json`: writes a machine-readable summary to `target/coverage-summary.json`

For a clean baseline run that includes compilation:

```bash
cargo clean
/usr/bin/time -p cargo nextest run
```

`cargo test` remains available when you specifically want the standard Cargo runner.

## Docs

- Public user docs in `docs/site/`
  - [Docs index](./docs/site/README.md)
  - [Standard modules](./docs/site/standard-modules.md)
  - [Definitions and usage](./docs/site/definitions-and-usage.md)
  - [Type annotations](./docs/site/type-annotations.md)
  - [Trait impls](./docs/site/trait-impls.md)
  - [Lens](./docs/site/lens.md)
  - [Kernel](./docs/site/kernel.md)
  - [Agents](./docs/site/agents.md)
  - [Pattern matching](./docs/site/pattern-matching.md)
  - [Extractors](./docs/site/extractors.md)
  - [Language features](./docs/site/language-features.md)
- Developer docs in `docs/dev/`
  - [Docs index](./docs/dev/README.md)
  - [VM spec entry](./docs/dev/EldrVM_spec.md)
  - [REPL spec entry](./docs/dev/Xldr_spec.md)
  - [Observability spec entry](./docs/dev/Rune_observability.md)
  - [Test policy entry](./docs/dev/テスト方針.md)
- Canonical specs and internal design notes in `doc/`
  - [Requirements (V9, Japanese)](./doc/要件定義v9.md)
  - [Open issues](./doc/open-issues.md)
  - [Float memo](./doc/float.md)
- Internal docs index
  - [Internal docs guide](./docs/internal/README.md)
- Standard-library docs live in `lib/*.srt` via `@@doc`
- Implementation contracts live in Rust doc comments under `crates/**`
- Install guide
  - [INSTALL.md](./INSTALL.md)
- Working ledger: [AGENTS.md](./AGENTS.md)

## Status

Current work is focused on stabilizing the V9 baseline and cleanup items from the recent review pass.

Implemented core includes:

- Primitive types: `Int`, `Float`, `String`, `Boolean`, `Unit`
- Generic type annotations: `List<T>`, `Result<T>`, user-defined `Name<T, ...>`
- `List` literals and empty list typing
- `defstruct` / `defrecord` / `deferror`
- `if` and `match` (binding / wildcard / literal / list / `Ok(...)` / `Err(...)`)
- Builtins: `print`, `to_string`, `eprint`

Implementation notes:

- `Int` uses unbounded `BigInt` semantics across the pipeline
- runtime-internal tags stay fixed-width and separate from user-visible `Int`
- `type` is a reserved keyword; std modules can declare builtin surfaces with `@@builtin def ...` and `@@builtin type ...`
- std modules are split into `Bootstrap`, `Kernel`, and type-oriented modules (`Int`, `String`, `Boolean`, `Error`, `List`, `Result`, `Float`); cross-cutting builtins live under `defmod Kernel` in `kernel.srt`, and each builtin type head is declared at the top level of its corresponding `lib/*.srt`
- closure parameter annotations are optional and match-arm LHS follows the same pattern grammar as safe-bind
- `Float` remains implemented, but its precise contract is tracked separately in [doc/float.md](./doc/float.md)
