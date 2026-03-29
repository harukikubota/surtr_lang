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
surtr run <file.srt>
```

Current implementation supports `run` only.

## Responsibilities

- Read source file input
- Execute the full pipeline (`parse -> resolve -> typecheck -> codegen -> execute`)
- Convert phase errors into human-readable diagnostics
- Exit with non-zero status on failure

## Non-responsibilities

Rune does not implement parser/resolver/typechecker/vm logic itself.

## Usage

```bash
cargo run -p rune -- run lib/hello.srt
```
