# Diagnostics

**Diagnostics** is the shared error rendering crate of Surtr.

## Role

Diagnostics converts phase errors and runtime errors into human-readable and machine-readable reports.

## Responsibilities

- Render parser, resolver, typechecker, runtime, and REPL diagnostics
- Maintain source registry and source-id aware location mapping
- Produce ariadne text output and serializable diagnostic reports
- Hold shared heuristics for labels, notes, and help text

## Non-responsibilities

Diagnostics does not decide compiler semantics. Phase crates construct their own error values and pass rendering context here.
