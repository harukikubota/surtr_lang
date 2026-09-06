use sindr::ir::Bytecode;

pub(crate) fn bytecode_interactive_vm(bytecode: Bytecode) -> eldr::InteractiveVm {
    eldr::InteractiveVm::from_bytecode(bytecode)
}
