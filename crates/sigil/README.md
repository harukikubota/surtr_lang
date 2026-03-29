# Sigil

**Sigil** is the name resolver crate of Surtr.

## Role

Sigil takes `Ast` from Spire and resolves identifiers into unambiguous bindings (`Resolved`).

## Position in the pipeline

```
Spire -> Sigil -> Scar -> Forge -> Eldr
          ^
          here
```

## Responsibilities

- Assign a stable `unique_id` to each binding
- Resolve references across lexical scopes
- Handle shadowing safely
- Report `ResolveError` for undefined names
- Convert `if` application form to a dedicated `Resolved::If` node

## Non-responsibilities

Sigil does not check types and does not generate bytecode.

## Usage

```rust
use sigil::resolve;

let resolved = resolve(ast)?;
```
