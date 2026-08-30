# Task 6 Review

## Verdict

**REQUEST CHANGES**

The registry is threaded through the semantic APIs and all repository call sites, real module/process owners now come from `OwnerRegistry`, owner-derived declaration identities are generally preserved, and Xldr's runtime `ReplTypeDisplayCategory` / `:type` output remain separate from compile-space `TypeIdentity`. Two semantic lifecycle defects still prevent Task 6 from being complete.

## Findings

### [P1] Trait signature metadata reintroduces `TypeConstructor` completion kinds

`symbol_semantic_infos_from_compile_metadata` merges registry/declaration infos with generic doc/signature infos by the full completion key, which includes `CompletionKind` (`crates/surtr-analysis/src/semantic.rs:552-573`). Registry-backed trait owners correctly use `CompletionKind::TypePath`, but every `TraitDef` signature is emitted as `DocKind::Type`, and `completion_kind_for_doc_kind` maps that to `CompletionKind::TypeConstructor` (`semantic.rs:3119-3136`). The two records therefore do not merge.

Consequences:

- an ordinary or promoted trait with its normal signature metadata produces both a `TypePath` and a `TypeConstructor` semantic symbol;
- LSP completion may expose duplicate labels with different kinds;
- Xldr's `remove_shadowed_type_path_symbols` drops the registry-backed `TypePath` when the metadata `TypeConstructor` has the same label, so the retained trait completion kind is exactly the kind Task 6 says must not be introduced;
- the added test avoids the bug by calling `from_declaration_index` without the always-present trait signature metadata.

The metadata projection needs to reuse the owner/declaration completion kind before keying/merging (or merge docs/signatures into a matching canonical owner independently of their generic `DocKind::Type` projection). Add coverage with a real `TraitDef` plus collected signatures for both `Trait` and promoted `TypeConstructor` identities.

### [P2] Active script owners are omitted from the analysis-service registry lifecycle

In project-backed script analysis, `visible_ast` is collected and passed to doc/signature collection and later to staged resolution, but `precollect_declarations` is called only for `module_stages` (`crates/surtr-analysis/src/service.rs:988-1023`). The semantic index then receives only that prefix registry (`service.rs:1094-1128`). `resolve_staged_program_with_state` internally merges owners from the user AST, but that merged registry is not returned to or reused by semantic analysis.

As a result, owners declared in the active `load_project` script—module/process owners, types, traits, signature aliases, and constants—cannot receive their registry-backed identity in the semantic index. Source-location symbols do not repair this: they carry no identity, and `TypeAlias` is not projected there at all. This is a lifecycle-specific hole in the promised complete owner identity set.

Build or merge a user-AST owner registry before constructing the semantic index, using the same canonical lowering/precollection path as staged resolution, and add an analysis-service test whose active project script declares at least a signature alias and a trait/module member.

## Checks completed

- Reviewed `d31e5a66..be915433` (5 changed files, 296 insertions / 56 deletions).
- Confirmed repository call sites of the changed semantic constructors/queries now pass an `OwnerRegistry`.
- Confirmed synthetic fallback/impl module paths are no longer turned into module owner symbols; owner symbols are emitted from registry entries.
- Confirmed module/process, supervisor, signature alias, const, ordinary/promoted trait, and module-member identities in the added focused test.
- Confirmed enum/trait members and inherent/trait-impl members derive their nominal owner through declaration names/module paths rather than registering owners.
- Confirmed no diff to Xldr's runtime display enum or `:type` golden strings; Xldr edits only thread compile-space semantic metadata/capabilities.
- `cargo nextest run -p surtr-analysis`: **130 passed**.
- `cargo nextest run -p xldr -E 'binary(repl_core)'`: **133 passed (2 leaky)**. The plan's literal `cargo nextest run -p xldr repl_core` matched zero tests and exited 4.
- `cargo nextest run -p surtr-lsp`: **17 passed**.
- `git diff --check d31e5a66..be915433`: **passed**.

