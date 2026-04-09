pub mod bytecode;
pub mod codegen;
pub mod error;
pub mod opcode;
pub mod registry;

pub use codegen::{
    codegen, BindingInfo, ChunkMeta, ForgeCheckpoint, ForgeSession, ReplTypeKind, TypeDefDisplay,
};

#[cfg(test)]
mod tests {
    use super::codegen;
    use crate::opcode::Opcode;
    use crate::registry::TypeKind;
    use spire::ast::Ast;

    const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../lib/bootstrap.srt");
    const KERNEL_PRELUDE_SOURCE: &str = include_str!("../../../lib/kernel.srt");
    const INT_MODULE_SOURCE: &str = include_str!("../../../lib/int.srt");
    const STRING_MODULE_SOURCE: &str = include_str!("../../../lib/string.srt");
    const BOOLEAN_MODULE_SOURCE: &str = include_str!("../../../lib/boolean.srt");
    const ERROR_MODULE_SOURCE: &str = include_str!("../../../lib/error.srt");
    const LIST_MODULE_SOURCE: &str = include_str!("../../../lib/list.srt");
    const RESULT_MODULE_SOURCE: &str = include_str!("../../../lib/result.srt");
    const FLOAT_MODULE_SOURCE: &str = include_str!("../../../lib/float.srt");

    fn strip_test_annotations(source: &str) -> String {
        source
            .lines()
            .filter(|line| !line.trim_start().starts_with("@@test"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn parse_std_module_stage(
        source: &str,
        fallback_module_path: &str,
    ) -> Vec<sigil::StagedModuleAst> {
        let ast = spire::parse_with_context(
            &strip_test_annotations(source),
            spire::ParserContext::module(0, None).with_rules(spire::SourceRules::std_module()),
        )
        .expect("std module should parse");

        let shared_imports = ast
            .iter()
            .filter_map(|stmt| match stmt {
                Ast::Import(_, _, _) => Some(stmt.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut lowered = Vec::new();
        let mut shared_global_defs = Vec::new();
        let mut shared_result_ctor_contracts = Vec::new();

        for stmt in ast {
            match stmt {
                Ast::Defmod(_, module_path, body, attrs) => {
                    let mut module_ast = shared_imports.clone();
                    module_ast.extend(body);
                    lowered.push(sigil::StagedModuleAst {
                        module_path,
                        ast: module_ast,
                        module_doc: attrs.doc,
                    });
                }
                Ast::Import(_, _, _) => {}
                Ast::ResultCtorDecl(_, _, _, _, _) => shared_result_ctor_contracts.push(stmt),
                other => shared_global_defs.push(other),
            }
        }

        // Keep std-file organization from changing the user-visible global
        // builtin surface in unit tests, while still letting `Result::Ok` /
        // `Result::Err` attach to the `Result` module where the checker expects
        // their canonical contract.
        if !shared_result_ctor_contracts.is_empty() && lowered.len() == 1 {
            let insert_at = lowered[0]
                .ast
                .iter()
                .take_while(|stmt| matches!(stmt, Ast::Import(_, _, _)))
                .count();
            lowered[0]
                .ast
                .splice(insert_at..insert_at, shared_result_ctor_contracts);
        } else if !shared_result_ctor_contracts.is_empty() {
            let mut global_ast = shared_imports.clone();
            global_ast.extend(shared_result_ctor_contracts);
            lowered.push(sigil::StagedModuleAst {
                module_path: fallback_module_path.to_string(),
                ast: global_ast,
                module_doc: None,
            });
        }

        if !shared_global_defs.is_empty() {
            let mut global_ast = shared_imports;
            global_ast.extend(shared_global_defs);
            lowered.push(sigil::StagedModuleAst {
                module_path: String::new(),
                ast: global_ast,
                module_doc: None,
            });
        }

        lowered
    }

    fn std_module_stages() -> Vec<Vec<sigil::StagedModuleAst>> {
        vec![
            parse_std_module_stage(BUILTIN_PRELUDE_SOURCE, "Bootstrap"),
            [
                ("Kernel", KERNEL_PRELUDE_SOURCE),
                ("Int", INT_MODULE_SOURCE),
                ("String", STRING_MODULE_SOURCE),
                ("Boolean", BOOLEAN_MODULE_SOURCE),
                ("Error", ERROR_MODULE_SOURCE),
                ("List", LIST_MODULE_SOURCE),
                ("Result", RESULT_MODULE_SOURCE),
                ("Float", FLOAT_MODULE_SOURCE),
            ]
            .into_iter()
            .flat_map(|(name, source)| parse_std_module_stage(source, name))
            .collect(),
        ]
    }

    fn typed_with_builtin_prelude(source: &str) -> Vec<scar::typed::TypedNode> {
        let module_stages = std_module_stages();
        let user_ast = spire::parse(source).expect("source should parse");
        let declaration_index = sigil::precollect_declaration_index(&module_stages)
            .expect("std modules should precollect");
        let resolved =
            sigil::resolve_staged_program(&module_stages, user_ast, &declaration_index, None)
                .expect("source should resolve");
        scar::typecheck(resolved).expect("source should typecheck")
    }

    fn codegen_source(source: &str) -> sindr::ir::Bytecode {
        let typed = typed_with_builtin_prelude(source);
        codegen(typed).expect("codegen should succeed")
    }

    #[test]
    fn does_not_emit_deprecated_frame_opcodes() {
        let bytecode = codegen_source(
            r#"x = 1
if(True, print("ok"), print("ng"))
print(to_string(x + 2))"#,
        );

        assert!(!bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::MakeFrame(_) | Opcode::PopFrame)));
    }

    #[test]
    fn function_table_preserves_fun_idx_index_invariant() {
        let baseline = codegen_source(r#"print("baseline")"#);
        let bytecode = codegen_source(
            r#"def add(x: Int, y: Int) -> Int { x + y }
deferror Oops {
  "oops"
}
print(to_string(add(1, 2)))"#,
        );

        assert_eq!(bytecode.functions.len(), baseline.functions.len() + 2);
        for (idx, entry) in bytecode.functions.iter().enumerate() {
            assert_eq!(entry.fun_idx as usize, idx);
        }
    }

    #[test]
    fn field_access_emits_getfield_with_resolved_index() {
        let bytecode = codegen_source(
            r#"defstruct User {
  name: String,
  age: Int,
}

user: User = User { name: "alice", age: 30 }
print(to_string(user.age))"#,
        );

        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::GetField { field_index: 1 })));
    }

    #[test]
    fn type_registry_reserves_result_tags_and_starts_user_tags_from_two() {
        let baseline = codegen_source(r#"print("baseline")"#);
        let bytecode = codegen_source(
            r#"defstruct User {
  name: String,
  age: Int,
}

defrecord Point(x: Float, y: Float)
print("ok")"#,
        );

        assert_eq!(
            bytecode.type_registry.entries.len(),
            baseline.type_registry.entries.len() + 2
        );

        let user = &bytecode.type_registry.entries[baseline.type_registry.entries.len()];
        assert!(user.tag >= 2);
        assert_eq!(user.name, "User");
        assert_eq!(user.kind, TypeKind::Struct);

        let point = &bytecode.type_registry.entries[baseline.type_registry.entries.len() + 1];
        assert_eq!(point.tag, user.tag + 1);
        assert_eq!(point.name, "Point");
        assert_eq!(point.kind, TypeKind::Record);
    }
}
