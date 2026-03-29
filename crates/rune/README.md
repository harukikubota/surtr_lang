# Rune

**Rune** is the CLI entry crate of Surtr.

## Role

Rune wires all compiler phases together and provides the `surtr` command.

## Position in the pipeline

```
Spire -> Sigil -> Scar -> Forge -> Eldr
                                  ^
                              orchestrated by Rune
```

## Command

```bash
surtr run <file.srt|file.eldr>
surtr build <file.srt> [output.eldr]
surtr dump <file.eldr> [--format json]
```

## Responsibilities

- Read source file input
- Execute the full pipeline (`parse -> resolve -> typecheck -> codegen -> execute`) for `.srt` input
- Build `.eldr` bytecode files
- Run compiled `.eldr` bytecode files
- Dump `.eldr` metadata/bytecode as JSON (`jq` friendly)
- Convert phase errors into human-readable diagnostics
- Exit with non-zero status on failure

## Non-responsibilities

Rune does not implement parser/resolver/typechecker/vm logic itself.

## Usage

```bash
cargo run -p rune -- run lib/hello.srt
cargo run -p rune -- build lib/hello.srt lib/hello.eldr
cargo run -p rune -- run lib/hello.eldr
cargo run -p rune -- dump lib/hello.eldr --format json | jq .
```
