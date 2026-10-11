# Pattern wildcard design

## Goal

Names beginning with `_` in a pattern discard their matched value and never
create a binding. An as-pattern must introduce a usable binding name.

## Rules

- `_` and every pattern identifier beginning with `_` are wildcard patterns.
- Wildcard patterns do not enter resolver scope, type environments, generated
  slots, REPL bindings, or binding-result presentation.
- The right side of `pattern @ name` must be a binding identifier: it must not
  begin with `_`.
- `pattern @ _` and `pattern @ _name` are parse errors. The diagnostic must
  say that an as-pattern needs a binding identifier and provide a concrete
  replacement such as `pattern @ value`.

## Architecture

Classify underscore-prefixed identifiers at the parser boundary, so every later
phase receives `AstPattern::Wildcard`. Validate the separate as-pattern alias
grammar in the parser before it constructs `AstPattern::As`; this prevents a
no-op as-pattern from reaching resolution. Resolver, Scar, Forge, and Xldr
continue to treat their existing wildcard variants as non-binding.

## Tests

- Parser tests cover `_` and `_ignored` as wildcard patterns, plus helpful
  parse failures for `pattern @ _` and `pattern @ _ignored`.
- Resolver and REPL tests establish that wildcard patterns do not leave a name
  available for later lookup, including tuple destructuring.
- Existing named as-pattern coverage remains to prove valid aliases still bind.
