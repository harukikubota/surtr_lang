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

`Span` uses Unicode scalar / character offsets in the source text. It is not a
byte offset and it is not an LSP UTF-16 position. LSP-facing crates convert
Spire spans at the analysis / protocol boundary.

## Usage

```rust
use spire::parse;

let ast = parse(source)?;
```

## Strict and tolerant parsing

`parse` and `parse_with_context` are the strict compiler pipeline entry points.
They reject invalid documents and must stay suitable for compile, run, and REPL
evaluation paths.

Editor tooling can call `parse_tolerant_with_context(source, context,
cursor_char_offset)`. It returns:

```rust
pub struct TolerantParseResult {
    pub ast: Vec<Ast>,
    pub diagnostics: Vec<ParseDiagnostic>,
    pub tokens: Vec<SyntaxToken>,
    pub outline: Vec<SyntaxOutlineItem>,
    pub cursor_context: CursorSyntaxContext,
}
```

The tolerant AST contains only declarations or statements that were parsed as
valid `Ast` nodes. Spire does not introduce `Ast::Error`; broken syntax is
reported through diagnostics and declaration heads that are useful to an editor
are exposed separately through `SyntaxOutlineItem`.

## Syntax tokens

`SyntaxToken` is a protocol-independent token surface for highlighting and
cursor-context features. Token spans follow the same character-offset contract as
`Ast` spans.

Comments and newlines are returned. Spaces and tabs are not tokenized, so
formatting trivia is represented by gaps between adjacent spans. `::` is exposed
as `SyntaxTokenKind::PathSep`, and source `>>` remains
`SyntaxTokenKind::Compose` for editor consumers even though the strict parser may
adapt it internally for type syntax.
