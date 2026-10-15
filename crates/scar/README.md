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

## Internal layout

`checker/mod.rs` declares the internal modules and preserves the public checker
API through explicit re-exports. The modules share the existing `Checker`;
splitting the implementation does not introduce another checking path.

| Module | Responsibility |
|---|---|
| `session` / `session/function_indices` | Public typecheck entry points, checkpoints, transactional commit, and function-index reconciliation |
| `state` | Checker construction, persistent-state conversion, and child-state transfer |
| `bootstrap` / `profile` | Builtin environment initialization and typecheck profiling |
| `program` / `warnings` / `process` | Program pass order, warning collection, and process declaration contracts |
| `expr` | Expression dispatch and expected-type propagation; child modules own calls, arguments, operators, closures, literals, control flow, Lazy, SafeBind, and process calls |
| `expr/facet` | Facet input preparation, path construction, and source rebuilding/update checks |
| `type_syntax` / `inference` | Type syntax and contextual restrictions; substitution, compatibility, and bounds |
| `resolve_typed` / `type_display` | Typed IR finalization and diagnostic type rendering |
| `predeclare` / `definitions` | Declaration registration and body checking, separated by declaration kind |
| `expr/constructors` | Value constructors and Error construction |
| `specialize` | Callable instantiation and its rewrite, substitution, and pattern passes |
| `audit` | Completion checks for pending dispatch and unresolved executable types |

`signatures`, `trait_selection`, `relations`, and `provenance` retain their existing
contracts. State ownership and pass order are unchanged. File separation alone is
not evidence of faster Surtr compilation.

## Usage

```rust
use scar::typecheck;

let typed = typecheck(resolved)?;
```
