# Facet API Inference Follow-ups

## This Change Scope

- Facet API first arguments are treated as `Facet<K, S, A, T, B>` expectation points.
- The shared completion path should suggest:
  - path-constructable type roots: `defstruct`, `defrecord`, `defenum`, plus structural builtin roots such as `Tuple`, `List`, `HashMap`
  - visible bindings whose type is `Facet<_, _, _, _, _>`
- Primitive roots are not path-constructable candidates:
  - `String`
  - `Int`
  - `Float`
  - `Boolean`
  - `Function`
- `Result` is not a Facet variant root. A `Result<S>` source is transparent to path checking and keeps the API execution context as `Result`.

Covered Facet APIs:

- `Facet::view`
- `Facet::preview`
- `Facet::put`
- `Facet::set`
- `Facet::over`
- `Facet::over_result`
- `Facet::case_set`
- `Facet::case_over`
- `Facet::chain`

## Currently Inferable

- `Facet::set(User.name, user, "bob")`
- `Facet::set(User.name, ret, "bob")` where `ret: Result<User>`
- `Facet::view(User.name, ret)` where `ret: Result<User>`, returning `Result<String>`
- `Facet::set(~user.name, "bob")`
- `Facet::set(~ret.name, "bob")`
- Nested structural paths such as `User.profile.name` and `~ret.profile.name`
- Result-backed focus fields, such as `Facet::over(User.score, user, {|score| Ok(score + 1)})` where `score: Result<Int>`

## Still Not Inferable By Design

- Standalone `_.name` as a Facet path. It remains a unary callable-context form.
- Passing, returning, capturing, or storing Facet values across runtime boundaries.
- `Result.Ok` / `Result.Err` as Facet variant selectors.

## Follow-ups

- Done: primitive-root diagnostics now make `Facet::view(String.len, "abc")` report a Facet/path-root error instead of a generic undefined-variable-style error.
- Consider making `PathConstructable` an explicit Scar/query capability instead of deriving completion candidates from declaration signatures.
- Done: shared analysis completion ranks ordinary call arguments with trait constraints such as `Self: Compare`; `compare(` uses visible `impl Compare for ...` signatures instead of a builtin type whitelist.
- Done: `Function::on` inference was verified to rely on same-expression evidence from the expected function type and key function, not on enumerating all impl target types.
