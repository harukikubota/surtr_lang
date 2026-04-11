# Surtr Docs Layout

This directory is for supplementary and public-facing documentation.

## What lives where

- `../doc/`
  - Canonical Japanese specs and open issues.
  - Use these when changing language contracts, VM behavior, or test policy.
- `site/`
  - Public guides and reference material.
  - These pages explain Surtr as a language and compiler without requiring the reader to open Rust code.
- `../lib/*.srt`
  - Standard-library source files.
  - User-facing API notes live here as `@@doc` so the same text can flow into `.eldr` metadata.
- `../crates/**`
  - Rust implementation contracts and module-level notes in rustdoc comments.

## Reading order

- Start with `site/README.md` for the public docs index.
- Read `site/language-guide.md` before `site/language-reference.md` if you are new to the language.
- Read `site/standard-library.md` when you want the standard module layout, builtin type contract, and `@@doc` conventions.
- Use `../doc/*.md` only when you need the canonical design contract.

## Maintenance rules

- Review memos and temporary planning notes should not live here.
- If a standard module API changes, update both the matching `lib/*.srt` `@@doc` block and the relevant page in `site/`.
- If a public page contradicts `../doc/`, update the canonical doc first and then bring `site/` into sync.
