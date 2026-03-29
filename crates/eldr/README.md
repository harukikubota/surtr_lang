# Eldr

**Eldr** is the virtual machine crate of Surtr.

## Role

Eldr executes `Bytecode` produced by Forge.

## Position in the pipeline

```
Spire -> Sigil -> Scar -> Forge -> Eldr
                                  ^
                                  here
```

## Responsibilities

- Execute opcodes on a stack-based VM
- Manage operand stack and local slots
- Dispatch builtins by `builtin_id`
- Use `TypeRegistry` for runtime display/metadata lookup
- Report `RuntimeError` for execution-time failures

## Non-responsibilities

Eldr does not parse syntax, resolve names, or infer/check types.

## Usage

```rust
use eldr::VM;

let mut vm = VM::new(bytecode);
vm.run()?;
```
