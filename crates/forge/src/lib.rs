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
    use sindr::builtin::builtin_meta_by_name;
    use spire::ast::Ast;

    const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../lib/bootstrap.srt");
    const KERNEL_PRELUDE_SOURCE: &str = include_str!("../../../lib/kernel.srt");
    const NUMERIC_MODULE_SOURCE: &str = include_str!("../../../lib/trait/numeric.srt");
    const SHOW_MODULE_SOURCE: &str = include_str!("../../../lib/trait/show.srt");
    const EQ_MODULE_SOURCE: &str = include_str!("../../../lib/trait/eq.srt");
    const COMPARE_MODULE_SOURCE: &str = include_str!("../../../lib/trait/compare.srt");
    const ORD_MODULE_SOURCE: &str = include_str!("../../../lib/trait/ord.srt");
    const CONCAT_MODULE_SOURCE: &str = include_str!("../../../lib/trait/concat.srt");
    const FROM_MODULE_SOURCE: &str = include_str!("../../../lib/trait/from.srt");
    const TRY_FROM_MODULE_SOURCE: &str = include_str!("../../../lib/trait/try_from.srt");
    const INT_MODULE_SOURCE: &str = include_str!("../../../lib/int.srt");
    const STRING_MODULE_SOURCE: &str = include_str!("../../../lib/string.srt");
    const BOOLEAN_MODULE_SOURCE: &str = include_str!("../../../lib/boolean.srt");
    const ORDERING_MODULE_SOURCE: &str = include_str!("../../../lib/ordering.srt");
    const ERROR_MODULE_SOURCE: &str = include_str!("../../../lib/error.srt");
    const LIST_MODULE_SOURCE: &str = include_str!("../../../lib/list.srt");
    const RESULT_MODULE_SOURCE: &str = include_str!("../../../lib/result.srt");
    const LENS_MODULE_SOURCE: &str = include_str!("../../../lib/lens.srt");
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
        .unwrap_or_else(|err| panic!("std module {fallback_module_path} should parse: {err:?}"));

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
                        auto_import: attrs.auto_import,
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
                auto_import: false,
            });
        }

        if !shared_global_defs.is_empty() {
            let mut global_ast = shared_imports;
            global_ast.extend(shared_global_defs);
            lowered.push(sigil::StagedModuleAst {
                module_path: String::new(),
                ast: global_ast,
                module_doc: None,
                auto_import: false,
            });
        }

        lowered
    }

    fn std_module_stages() -> Vec<Vec<sigil::StagedModuleAst>> {
        vec![
            parse_std_module_stage(BUILTIN_PRELUDE_SOURCE, "Bootstrap"),
            [
                ("Kernel", KERNEL_PRELUDE_SOURCE),
                ("Numeric", NUMERIC_MODULE_SOURCE),
                ("Show", SHOW_MODULE_SOURCE),
                ("Eq", EQ_MODULE_SOURCE),
                ("Ordering", ORDERING_MODULE_SOURCE),
                ("Compare", COMPARE_MODULE_SOURCE),
                ("Ord", ORD_MODULE_SOURCE),
                ("Concat", CONCAT_MODULE_SOURCE),
                ("From", FROM_MODULE_SOURCE),
                ("TryFrom", TRY_FROM_MODULE_SOURCE),
                ("Int", INT_MODULE_SOURCE),
                ("String", STRING_MODULE_SOURCE),
                ("Boolean", BOOLEAN_MODULE_SOURCE),
                ("Error", ERROR_MODULE_SOURCE),
                ("List", LIST_MODULE_SOURCE),
                ("Result", RESULT_MODULE_SOURCE),
                ("Lens", LENS_MODULE_SOURCE),
                ("Float", FLOAT_MODULE_SOURCE),
            ]
            .into_iter()
            .flat_map(|(name, source)| parse_std_module_stage(source, name))
            .collect(),
        ]
    }

    fn typed_with_builtin_prelude(source: &str) -> Vec<scar::typed::TypedNode> {
        let module_stages = std_module_stages();
        let user_ast = spire::parse_with_context(source, spire::ParserContext::project(0))
            .expect("source should parse");
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
    fn function_calls_compile_under_current_opcode_set() {
        let bytecode = codegen_source(
            r#"def add(x: Int, y: Int) -> Int { x + y }
print(to_string(add(1, 2)))"#,
        );

        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::Call { arity: 2, .. })));
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

impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }
}

user: User = User("alice", 30)
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

impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }
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

    #[test]
    fn direct_int_bitwise_builtin_calls_lower_to_specialized_opcodes() {
        let bytecode = codegen_source(
            r#"negated = Int::bit_not(6)
left = Int::bit_and(6, 3)
mid = Int::bit_or(6, 3)
right = Int::bit_xor(6, 3)"#,
        );

        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::BitNotInt)));
        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::BitAndInt)));
        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::BitOrInt)));
        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::BitXorInt)));

        let bit_not_id = builtin_meta_by_name("bit_not")
            .expect("bit_not builtin metadata must exist")
            .builtin_id;
        let bit_and_id = builtin_meta_by_name("bit_and")
            .expect("bit_and builtin metadata must exist")
            .builtin_id;
        let bit_or_id = builtin_meta_by_name("bit_or")
            .expect("bit_or builtin metadata must exist")
            .builtin_id;
        let bit_xor_id = builtin_meta_by_name("bit_xor")
            .expect("bit_xor builtin metadata must exist")
            .builtin_id;

        assert!(!bytecode.opcodes.iter().any(|op| {
            matches!(
                op,
                Opcode::CallBuiltin {
                    builtin_id,
                    ..
                } if *builtin_id == bit_not_id
                    || *builtin_id == bit_and_id
                    || *builtin_id == bit_or_id
                    || *builtin_id == bit_xor_id
            )
        }));
    }

    #[test]
    fn int_bit_index_helpers_stay_as_builtin_calls() {
        let bytecode = codegen_source(
            r#"tested = Int::test_bit(5, 0)
setted = Int::set_bit(0, 1)
cleared = Int::clear_bit(7, 1)
toggled = Int::toggle_bit(5, 0)"#,
        );

        for name in ["test_bit", "set_bit", "clear_bit", "toggle_bit"] {
            let builtin_id = builtin_meta_by_name(name)
                .unwrap_or_else(|| panic!("{name} builtin metadata must exist"))
                .builtin_id;
            assert!(bytecode.opcodes.iter().any(|op| {
                matches!(
                    op,
                    Opcode::CallBuiltin {
                        builtin_id: id,
                        arity: 2,
                        ..
                    } if *id == builtin_id
                )
            }));
        }
    }

    #[test]
    fn numeric_trait_calls_lower_to_existing_targets() {
        let bytecode = codegen_source(
            r#"sum = 1 + 2
quot = Numeric::safe_div(8, 2)
largest = Numeric::max(1.5, 2.5)"#,
        );

        let safe_div_id = builtin_meta_by_name("safe_div")
            .expect("safe_div builtin metadata must exist")
            .builtin_id;

        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::AddInt)));
        assert!(bytecode.opcodes.iter().any(|op| {
            matches!(
                op,
                Opcode::CallBuiltin {
                    builtin_id,
                    arity: 2,
                    ..
                } if *builtin_id == safe_div_id
            )
        }));
        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::Call { arity: 2, .. })));
    }

    #[test]
    fn lens_set_and_over_are_lowered_without_runtime_builtin_calls() {
        let bytecode = codegen_source(
            r#"defrecord User(name: String)
user = User("alice")
user2 = Lens::set(User.name, user, "bob")
user3 = Lens::over(User.name, user2, {|name| Ok(name ++ "!")})"#,
        );

        let lens_set_id = builtin_meta_by_name("set")
            .expect("set builtin metadata must exist")
            .builtin_id;
        let lens_over_id = builtin_meta_by_name("over")
            .expect("over builtin metadata must exist")
            .builtin_id;

        assert!(!bytecode.opcodes.iter().any(|op| {
            matches!(
                op,
                Opcode::CallBuiltin {
                    builtin_id,
                    ..
                } if *builtin_id == lens_set_id || *builtin_id == lens_over_id
            )
        }));
        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::CallClosure { arity: 1, .. })));
    }

    #[test]
    fn bounded_numeric_generic_helpers_emit_specialized_functions() {
        let bytecode = codegen_source(
            r#"def double<$N: Numeric>(x: $N) -> $N { x + x }
a = double(21)
b = double(1.5)"#,
        );

        let double_entries = bytecode
            .functions
            .iter()
            .filter(|entry| entry.qualified_name.as_deref() == Some("double"))
            .count();

        assert_eq!(double_entries, 2);
        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::AddInt)));
        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::AddFloat)));
    }
}
