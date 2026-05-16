# Spire

**Spire** is the parser crate of Surtr.

-----

## Role

Spire reads Surtr source text and produces an abstract syntax tree (`Ast`). It is the first structure in the pipeline to touch raw source, and the only one that knows about characters, tokens, and syntax.

All subsequent crates operate on the `Ast` that Spire produces. Spire itself knows nothing about names, types, or execution.

## Position in the pipeline

```
Spire -> Sigil -> Scar -> Forge -> Eldr
  ^
  here
```

## Responsibilities

- Tokenizing source text into a stream of lexemes
- Parsing the token stream into an `Ast`
- Attaching `Span` information to every node for downstream error reporting
- Applying `ParserContext` policy for script, module, std-module, and REPL inputs
- Lowering namespace/module surface syntax into canonical AST names
- Rejecting syntactically invalid programs with a `ParseError`

## Non-responsibilities

Spire does not resolve names, check types, or evaluate anything. If a node is syntactically valid, Spire accepts it. Semantic errors are the responsibility of Sigil and Scar.

## Output

```rust
pub enum Ast { /* ... */ }
pub struct Span { pub start: usize, pub end: usize }
```

## Usage

```rust
use spire::parse;

let ast = parse(source)?;
```
