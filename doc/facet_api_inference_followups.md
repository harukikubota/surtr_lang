# Facet API Inference Follow-ups

## This Change Scope

- Facet API first arguments are treated as `Facet<S, A>` expectation points.
- The shared completion path should suggest:
  - path-constructable type roots: `defstruct`, `defrecord`, `defenum`, plus structural builtin roots such as `Tuple`, `List`, `HashMap`
  - visible bindings whose type is `Facet<_, _>`
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
- Result-backed focus fields, such as `Facet::set(User.score, user, 3)` where `score: Result<Int>`

## Still Not Inferable By Design

- Standalone `_.name` as a Facet path. It remains a unary callable-context form.
- Passing, returning, capturing, or storing Facet values across runtime boundaries.
- `Result.Ok` / `Result.Err` as Facet variant selectors.

## Follow-ups

- Improve primitive-root diagnostics so `Facet::view(String.len, "abc")` reports a Facet/path-root error instead of a generic undefined-variable-style error.
- Consider making `PathConstructable` an explicit Scar/query capability instead of deriving completion candidates from declaration signatures.
- Extend shared analysis completion beyond Facet APIs so ordinary call arguments can carry constraints such as `Self: Compare`.
- Next turn: implement trait-aware completion for `compare(` and `Function::on` inference without listing all impl target types.
