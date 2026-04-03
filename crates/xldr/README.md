# Xldr

**Xldr** is the interactive runtime crate of Surtr.

## Role

Xldr owns REPL session state and user-facing interactive execution behavior.

## Responsibilities

- Run the REPL loop
- Keep incremental session state across inputs
- Handle REPL commands like `:quit` and `:v`
- Drive incremental parse/resolve/typecheck/codegen/execute for interactive input
- Render the startup banner and version output for `surtr repl`

## Non-responsibilities

Xldr does not parse CLI command-line arguments (`surtr run/build/dump/repl` dispatch belongs to Rune).
