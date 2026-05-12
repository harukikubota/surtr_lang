# forge unit scope

Implemented unit tests:
- `crates/forge/src/lib.rs`
- `crates/forge/src/codegen.rs`

Current structure:
- `lib.rs`: higher-level codegen regression tests that exercise typed input through the public `codegen*` entrypoints
- `codegen.rs`: lower-level codegen and chunk-composition contract tests for opcode rewriting, relocation, and failure paths

Covered focus:
- TypedNode -> opcode sequence and lowering regressions (`GetField`, call lowering, trait/helper specialization)
- function table invariant (`functions[idx].fun_idx == idx`)
- user type tag order (`Ok=0`, `Err=1`, user-defined from `2`)
- Result-related opcode fast paths (`MakeOk`, `MakeErr`, local tag fusion, compressed local/const ops)
- REPL chunk internals:
  - chunk-local constant / error-template / dbg-template index localization
  - `compose_bytecode_with_chunk` relocation and merge behavior
  - chunk/base mismatch and malformed artifact error paths
- process runtime metadata emission (`RuntimeProcessSpec`, `RuntimeBootPlan`) at the forge hot-test layer
