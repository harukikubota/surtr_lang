# Sindr

**Sindr** is the shared compiler/runtime data crate of Surtr.

## Role

Sindr owns data shapes that must stay stable across compiler phases and the VM.

## Responsibilities

- Define bytecode IR structures, opcodes, constants, and `.eldr` encoding
- Define builtin metadata and runtime builtin identifiers
- Define runtime values, type registry entries, and shared display helpers
- Provide viewer-model data for `surtr dump --format viewer-json`

## Non-responsibilities

Sindr does not parse source, resolve names, typecheck, generate bytecode, or execute programs.
