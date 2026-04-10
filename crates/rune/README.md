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
surtr --version
surtr run <file.srt|file.eldr>
surtr repl [--quiet] [--banner] [--version]
surtr build <file.srt> [output.eldr]
surtr dump <file.eldr> [--format json]
```

## Responsibilities

- Read source file input
- Execute the full pipeline (`parse -> resolve -> typecheck -> codegen -> execute`) for `.srt` input
- Build `.eldr` bytecode files
- Run compiled `.eldr` bytecode files
- Dispatch `surtr repl` to `xldr`
- Dump `.eldr` metadata/bytecode as JSON (`jq` friendly)
- Convert phase errors into human-readable diagnostics
- Exit with non-zero status on failure

## Non-responsibilities

Rune does not implement parser/resolver/typechecker/vm logic itself.
Rune does not own REPL session internals (`xldr` owns REPL state/loop/commands).

## Usage

```bash
cat > main.srt <<'EOF'
print("hello, Surtr")
EOF

cargo run -p rune -- run main.srt
cargo run -p rune -- repl
cargo run -p rune -- build main.srt main.eldr
cargo run -p rune -- run main.eldr
cargo run -p rune -- dump main.eldr --format json | jq .
```
