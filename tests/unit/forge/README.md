# forge unit scope

Implemented unit tests:
- `crates/forge/src/lib.rs`

Covered focus:
- TypedNode -> opcode sequence (`GetField`)
- function table `fun_idx == index` invariant
- function-call opcode generation under the current calling convention
- user type tag order (`Ok=0`, `Err=1`, user-defined from `2`)
