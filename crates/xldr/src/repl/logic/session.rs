use sindr::ir::Bytecode;
use sindr::runtime::TypeRegistry;

pub(crate) fn empty_interactive_vm(type_registry: TypeRegistry) -> eldr::InteractiveVm {
    eldr::InteractiveVm::new(type_registry)
}

pub(crate) fn bytecode_interactive_vm(bytecode: Bytecode) -> eldr::InteractiveVm {
    eldr::InteractiveVm::from_bytecode(bytecode)
}
