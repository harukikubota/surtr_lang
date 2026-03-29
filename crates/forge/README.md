# Forge

**Forge** is the code generation crate of Surtr.

## Role

Forge lowers typed nodes from Scar into VM bytecode (`Bytecode`).

## Position in the pipeline

```
Spire -> Sigil -> Scar -> Forge -> Eldr
                          ^
                          here
```

## Responsibilities

- Emit opcodes from typed AST nodes
- Build constants and locals layout
- Emit `CallBuiltin(u16, u8)` for builtin calls
- Build `TypeRegistry` metadata consumed by Eldr
- Report `CodegenError` when code generation fails

## Non-responsibilities

Forge does not parse, resolve names, check types, or execute programs.

## Usage

```rust
use forge::codegen;

let bytecode = codegen(typed)?;
```
