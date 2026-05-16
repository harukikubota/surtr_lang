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
surtr check <file.srt> [--format json]
surtr run <file.srt|file.eldr> [--entry <name>] [--vm-dump <path>] [--vm-dump-on error|always] [--vm-stats] [--vm-stats-json] [--trace-opcode] [--trace-call] [--trace-limit <n>] [--trace-filter <csv>] [--phase-times] [--error-context verbose] [-- <arg>...]
surtr test [--quiet|-q] <lib-relative-name|--all>
surtr repl [--quiet] [--banner] [--version] [--module <file.srt>] [--script <file.srt>]
surtr build <file.srt> [output.eldr]
surtr dump <file.eldr|entry.srt> [--format json|viewer-json] [--entry <name>] [--opcode-histogram] [--peephole-candidates]
surtr tui [file.eldr]
```

`dump --opcode-histogram` and `dump --peephole-candidates` are available only with
`--format json`; `viewer-json` is reserved for the UI viewer model.

## Responsibilities

- Read source file input
- Execute the full pipeline (`parse -> resolve -> typecheck -> codegen -> execute`) for `.srt` input
- Build `.eldr` bytecode files
- Run compiled `.eldr` bytecode files
- Dispatch `surtr repl` to `xldr`
- Dump `.eldr` metadata/bytecode or `.srt` entry-source bytecode as `json` / `viewer-json` (`jq` friendly)
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
cargo run -p rune -- repl --script main.srt
cargo run -p rune -- build main.srt main.eldr
cargo run -p rune -- run main.eldr
cargo run -p rune -- dump main.eldr --format json | jq .
cargo run -p rune -- dump main.srt --format viewer-json > viewer.json
cargo run -p rune -- test --all
```
