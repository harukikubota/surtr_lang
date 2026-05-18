# Scar

**Scar** is the type checker crate of Surtr.

## Role

Scar takes `Resolved` nodes from Sigil, validates type constraints, and produces typed nodes (`TypedNode`).

## Position in the pipeline

```
Spire -> Sigil -> Scar -> Forge -> Eldr
                  ^
                  here
```

## Responsibilities

- Infer expression types
- Validate type annotations
- Enforce operator/function argument typing rules
- Resolve field access names into field indexes for codegen
- Typecheck staged module programs and stdlib/user boundaries
- Preserve process, facet, and runtime-facing metadata for Forge
- Report `TypeError` with spans and hints

## Non-responsibilities

Scar does not resolve names and does not emit opcodes.

## Usage

```rust
use scar::typecheck;

let typed = typecheck(resolved)?;
```
