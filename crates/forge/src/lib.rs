pub mod bytecode;
pub mod codegen;
pub mod error;
pub mod opcode;
pub mod registry;

pub use codegen::{
    codegen, codegen_typed_program, compose_bytecode_with_chunk, BindingInfo, ChunkMeta,
    ForgeCheckpoint, ForgeSession, ReplCallableDisplay, ReplCallableKind, ReplLensInfo,
    ReplLensSegmentInfo, ReplTypeKind, TypeDefDisplay,
};

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::{codegen, codegen_typed_program};
    use crate::bytecode::Constant;
    use crate::opcode::Opcode;
    use crate::registry::TypeKind;
    use scar::typed::{
        TypedFunParam, TypedInner, TypedMatchArm, TypedMatchPattern, TypedNode, TypedPattern,
    };
    use scar::types::Ty;
    use sigil::resolved::ResolvedId;
    use sindr::builtin::builtin_id_by_name;
    use sindr::primitives::int;
    use spire::ast::{Ast, Lit, Span, Visibility};

    const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../lib/bootstrap.srt");
    const SPECIAL_TYPES_SOURCE: &str = include_str!("../../../lib/types/special_types.srt");
    const KERNEL_PRELUDE_SOURCE: &str = include_str!("../../../lib/kernel.srt");
    const ADD_MODULE_SOURCE: &str = include_str!("../../../lib/traits/operator/add.srt");
    const SUB_MODULE_SOURCE: &str = include_str!("../../../lib/traits/operator/sub.srt");
    const MUL_MODULE_SOURCE: &str = include_str!("../../../lib/traits/operator/mul.srt");
    const NUMERIC_MODULE_SOURCE: &str = include_str!("../../../lib/traits/numeric.srt");
    const SHOW_MODULE_SOURCE: &str = include_str!("../../../lib/traits/show.srt");
    const EQ_MODULE_SOURCE: &str = include_str!("../../../lib/traits/operator/eq.srt");
    const NEQ_MODULE_SOURCE: &str = include_str!("../../../lib/traits/operator/neq.srt");
    const COMPARE_MODULE_SOURCE: &str = include_str!("../../../lib/traits/operator/compare.srt");
    const LT_MODULE_SOURCE: &str = include_str!("../../../lib/traits/operator/lt.srt");
    const LTE_MODULE_SOURCE: &str = include_str!("../../../lib/traits/operator/lte.srt");
    const GT_MODULE_SOURCE: &str = include_str!("../../../lib/traits/operator/gt.srt");
    const GTE_MODULE_SOURCE: &str = include_str!("../../../lib/traits/operator/gte.srt");
    const ORD_MODULE_SOURCE: &str = include_str!("../../../lib/traits/operator/ord.srt");
    const CONCAT_MODULE_SOURCE: &str = include_str!("../../../lib/traits/operator/concat.srt");
    const FROM_MODULE_SOURCE: &str = include_str!("../../../lib/traits/from.srt");
    const TRY_FROM_MODULE_SOURCE: &str = include_str!("../../../lib/traits/try_from.srt");
    const FUNCTOR_MODULE_SOURCE: &str = include_str!("../../../lib/traits/operator/functor.srt");
    const CHAINABLE_MODULE_SOURCE: &str =
        include_str!("../../../lib/traits/operator/chainable.srt");
    const PIPE_APPLY_MODULE_SOURCE: &str =
        include_str!("../../../lib/traits/operator/pipe_apply.srt");
    const COMPOSE_MODULE_SOURCE: &str = include_str!("../../../lib/traits/operator/compose.srt");
    const COMPOSABLE_MODULE_SOURCE: &str =
        include_str!("../../../lib/traits/operator/composable.srt");
    const LIFT_COMPOSABLE_MODULE_SOURCE: &str =
        include_str!("../../../lib/traits/operator/lift_composable.srt");
    const KLEISLI_COMPOSABLE_MODULE_SOURCE: &str =
        include_str!("../../../lib/traits/operator/kleisli_composable.srt");
    const INT_MODULE_SOURCE: &str = include_str!("../../../lib/types/int.srt");
    const STRING_MODULE_SOURCE: &str = include_str!("../../../lib/types/string.srt");
    const REGEX_MODULE_SOURCE: &str = include_str!("../../../lib/types/regex.srt");
    const BOOLEAN_MODULE_SOURCE: &str = include_str!("../../../lib/types/boolean.srt");
    const ORDERING_MODULE_SOURCE: &str = include_str!("../../../lib/types/ordering.srt");
    const ERROR_MODULE_SOURCE: &str = include_str!("../../../lib/types/error.srt");
    const LIST_MODULE_SOURCE: &str = include_str!("../../../lib/types/list.srt");
    const GENERATOR_MODULE_SOURCE: &str = include_str!("../../../lib/types/generator.srt");
    const HASH_MAP_MODULE_SOURCE: &str = include_str!("../../../lib/types/hash_map.srt");
    const RESULT_MODULE_SOURCE: &str = include_str!("../../../lib/types/result.srt");
    const DURATION_MODULE_SOURCE: &str = include_str!("../../../lib/types/duration.srt");
    const PROCESS_MODULE_SOURCE: &str = include_str!("../../../lib/process.srt");
    const OPTION_MODULE_SOURCE: &str = include_str!("../../../lib/types/option.srt");
    const LENS_MODULE_SOURCE: &str = include_str!("../../../lib/lens.srt");
    const FLOAT_MODULE_SOURCE: &str = include_str!("../../../lib/types/float.srt");
    const RANDOM_MODULE_SOURCE: &str = include_str!("../../../lib/Random.srt");
    const STYLED_DOC_MODULE_SOURCE: &str = include_str!("../../../lib/styled_doc.srt");
    const TEST_MODULE_SOURCE: &str = include_str!("../../../lib/test.srt");

    fn parse_std_module_stage(
        source: &str,
        _fallback_module_path: &str,
    ) -> Vec<sigil::StagedModuleAst> {
        let ast = spire::parse_with_context(
            source,
            spire::ParserContext::module(0, None).with_rules(spire::ParseRules::std_module()),
        )
        .unwrap_or_else(|err| panic!("std module {_fallback_module_path} should parse: {err:?}"));

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
                        doc_module_path: None,
                        ast: module_ast,
                        module_doc: attrs.doc,
                        auto_import: attrs.auto_import,
                        process_spec: attrs.process_spec,
                    });
                }
                Ast::ImplDef(span, target, methods, attrs) => {
                    let mut module_ast = shared_imports.clone();
                    module_ast.push(Ast::ImplDef(span, target.clone(), methods, attrs.clone()));
                    lowered.push(sigil::StagedModuleAst {
                        module_path: target,
                        doc_module_path: None,
                        ast: module_ast,
                        module_doc: attrs.doc,
                        auto_import: attrs.auto_import,
                        process_spec: attrs.process_spec,
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
                module_path: String::new(),
                doc_module_path: None,
                ast: global_ast,
                module_doc: None,
                auto_import: false,
                process_spec: None,
            });
        }

        if !shared_global_defs.is_empty() {
            let mut global_ast = shared_imports;
            global_ast.extend(shared_global_defs);
            lowered.push(sigil::StagedModuleAst {
                module_path: String::new(),
                doc_module_path: None,
                ast: global_ast,
                module_doc: None,
                auto_import: false,
                process_spec: None,
            });
        }

        lowered
    }

    fn std_module_stages() -> Vec<Vec<sigil::StagedModuleAst>> {
        vec![
            parse_std_module_stage(BUILTIN_PRELUDE_SOURCE, "Bootstrap"),
            [
                ("SpecialTypes", SPECIAL_TYPES_SOURCE),
                ("Kernel", KERNEL_PRELUDE_SOURCE),
                ("Add", ADD_MODULE_SOURCE),
                ("Sub", SUB_MODULE_SOURCE),
                ("Mul", MUL_MODULE_SOURCE),
                ("Eq", EQ_MODULE_SOURCE),
                ("Neq", NEQ_MODULE_SOURCE),
                ("Compare", COMPARE_MODULE_SOURCE),
                ("Lt", LT_MODULE_SOURCE),
                ("Lte", LTE_MODULE_SOURCE),
                ("Gt", GT_MODULE_SOURCE),
                ("Gte", GTE_MODULE_SOURCE),
                ("Concat", CONCAT_MODULE_SOURCE),
                ("Numeric", NUMERIC_MODULE_SOURCE),
                ("Show", SHOW_MODULE_SOURCE),
                ("Ordering", ORDERING_MODULE_SOURCE),
                ("Ord", ORD_MODULE_SOURCE),
                ("From", FROM_MODULE_SOURCE),
                ("TryFrom", TRY_FROM_MODULE_SOURCE),
                ("Functor", FUNCTOR_MODULE_SOURCE),
                ("Chainable", CHAINABLE_MODULE_SOURCE),
                ("PipeApply", PIPE_APPLY_MODULE_SOURCE),
                ("Compose", COMPOSE_MODULE_SOURCE),
                ("Composable", COMPOSABLE_MODULE_SOURCE),
                ("LiftComposable", LIFT_COMPOSABLE_MODULE_SOURCE),
                ("KleisliComposable", KLEISLI_COMPOSABLE_MODULE_SOURCE),
                ("Int", INT_MODULE_SOURCE),
                ("String", STRING_MODULE_SOURCE),
                ("Regex", REGEX_MODULE_SOURCE),
                ("Boolean", BOOLEAN_MODULE_SOURCE),
                ("Error", ERROR_MODULE_SOURCE),
                ("List", LIST_MODULE_SOURCE),
                ("Generator", GENERATOR_MODULE_SOURCE),
                ("HashMap", HASH_MAP_MODULE_SOURCE),
                ("Result", RESULT_MODULE_SOURCE),
                ("Duration", DURATION_MODULE_SOURCE),
                ("Process", PROCESS_MODULE_SOURCE),
                ("Option", OPTION_MODULE_SOURCE),
                ("Lens", LENS_MODULE_SOURCE),
                ("Float", FLOAT_MODULE_SOURCE),
                ("Random", RANDOM_MODULE_SOURCE),
                ("StyledDoc", STYLED_DOC_MODULE_SOURCE),
            ]
            .into_iter()
            .flat_map(|(name, source)| parse_std_module_stage(source, name))
            .collect(),
            [("Test", TEST_MODULE_SOURCE)]
                .into_iter()
                .flat_map(|(name, source)| parse_std_module_stage(source, name))
                .collect(),
        ]
    }

    fn cached_std_modules_and_declarations(
    ) -> &'static (Vec<Vec<sigil::StagedModuleAst>>, sigil::DeclarationIndex) {
        static CACHE: OnceLock<(Vec<Vec<sigil::StagedModuleAst>>, sigil::DeclarationIndex)> =
            OnceLock::new();

        CACHE.get_or_init(|| {
            let module_stages = std_module_stages();
            let declaration_index = sigil::precollect_declaration_index(&module_stages)
                .expect("std modules should precollect");
            (module_stages, declaration_index)
        })
    }

    fn typed_with_builtin_prelude(source: &str) -> Vec<scar::typed::TypedNode> {
        let (module_stages, declaration_index) = cached_std_modules_and_declarations();
        let user_ast = spire::parse_with_context(source, spire::ParserContext::project(0))
            .expect("source should parse");
        let resolved =
            sigil::resolve_staged_program(module_stages, user_ast, declaration_index, None)
                .expect("source should resolve");
        scar::typecheck(resolved).expect("source should typecheck")
    }

    fn typed_module_program_with_builtin_prelude(source: &str) -> scar::typed::TypedProgram {
        let (module_stages, _) = cached_std_modules_and_declarations();
        let ast = spire::parse_with_context(source, spire::ParserContext::project(0))
            .expect("source should parse");
        let shared_imports = ast
            .iter()
            .filter_map(|stmt| match stmt {
                Ast::Import(_, _, _) => Some(stmt.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let lowered = ast
            .into_iter()
            .filter_map(|stmt| match stmt {
                Ast::Defmod(_, module_path, body, attrs) => {
                    let mut module_ast = shared_imports.clone();
                    module_ast.extend(body);
                    Some(sigil::StagedModuleAst {
                        module_path,
                        doc_module_path: None,
                        ast: module_ast,
                        module_doc: attrs.doc,
                        auto_import: attrs.auto_import,
                        process_spec: attrs.process_spec,
                    })
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        let mut all_stages = module_stages.clone();
        all_stages.push(lowered);
        let declaration_index = sigil::precollect_declaration_index(&all_stages)
            .expect("module stages should precollect");
        let resolved = sigil::resolve_staged_program_from_state(
            &all_stages,
            Vec::new(),
            &declaration_index,
            None,
            0,
            sigil::ResolveResumeState::default(),
        )
        .expect("definition source should resolve");
        scar::typecheck_staged_program(resolved).expect("definition source should typecheck")
    }

    fn codegen_source(source: &str) -> sindr::ir::Bytecode {
        let typed = typed_with_builtin_prelude(source);
        codegen(typed).expect("codegen should succeed")
    }

    fn test_span() -> Span {
        Span { start: 0, end: 0 }
    }

    fn resolved_id(name: &str, unique_id: u32) -> ResolvedId {
        ResolvedId {
            name: name.to_string(),
            qualified_name: None,
            unique_id,
            compiler_generated: false,
            span: test_span(),
        }
    }

    fn int_lit(value: i64) -> TypedNode {
        TypedNode {
            ty: Ty::Int,
            span: test_span(),
            node: TypedInner::Lit(Lit::Int(int(value))),
        }
    }

    fn unit_lit() -> TypedNode {
        TypedNode {
            ty: Ty::Unit,
            span: test_span(),
            node: TypedInner::Lit(Lit::Unit),
        }
    }

    fn list_nil() -> TypedNode {
        TypedNode {
            ty: Ty::List(Box::new(Ty::Int)),
            span: test_span(),
            node: TypedInner::ListNil,
        }
    }

    fn list_cons_expr(depth: usize) -> TypedNode {
        let mut node = list_nil();
        for value in (0..depth).rev() {
            node = TypedNode {
                ty: Ty::List(Box::new(Ty::Int)),
                span: test_span(),
                node: TypedInner::ListCons(Box::new(int_lit(value as i64)), Box::new(node)),
            };
        }
        node
    }

    fn list_cons_bind_pattern(depth: usize) -> TypedPattern {
        let mut pat = TypedPattern::ListNil(Ty::List(Box::new(Ty::Int)));
        for _ in 0..depth {
            pat = TypedPattern::ListCons(
                Ty::List(Box::new(Ty::Int)),
                Box::new(TypedPattern::Wildcard(Ty::Int)),
                Box::new(pat),
            );
        }
        pat
    }

    fn list_cons_match_pattern(depth: usize) -> TypedMatchPattern {
        let mut pat = TypedMatchPattern::ListNil;
        for _ in 0..depth {
            pat = TypedMatchPattern::ListCons(Box::new(TypedMatchPattern::Wildcard), Box::new(pat));
        }
        pat
    }

    fn nested_tail_blocks(depth: usize, leaf: TypedNode) -> TypedNode {
        let mut node = leaf;
        for _ in 0..depth {
            node = TypedNode {
                ty: node.ty.clone(),
                span: test_span(),
                node: TypedInner::Block(vec![node]),
            };
        }
        node
    }

    fn codegen_typed(stmts: Vec<TypedNode>) -> sindr::ir::Bytecode {
        codegen(stmts).expect("typed codegen should succeed")
    }

    #[test]
    fn deep_list_cons_expression_codegen_uses_normal_test_stack() {
        let bytecode = codegen_typed(vec![list_cons_expr(512)]);
        let cons_count = bytecode
            .opcodes
            .iter()
            .filter(|op| matches!(op, Opcode::ListCons))
            .count();

        assert_eq!(cons_count, 512);
    }

    #[test]
    fn deep_list_cons_bind_pattern_codegen_uses_normal_test_stack() {
        let node = TypedNode {
            ty: Ty::Unit,
            span: test_span(),
            node: TypedInner::Bind(list_cons_bind_pattern(512), Box::new(list_cons_expr(512))),
        };
        let bytecode = codegen_typed(vec![node]);
        let list_head_count = bytecode
            .opcodes
            .iter()
            .filter(|op| matches!(op, Opcode::ListHead))
            .count();

        assert_eq!(list_head_count, 1024);
    }

    #[test]
    fn deep_list_cons_match_pattern_codegen_uses_normal_test_stack() {
        let node = TypedNode {
            ty: Ty::Unit,
            span: test_span(),
            node: TypedInner::Match(
                Box::new(list_cons_expr(512)),
                vec![TypedMatchArm {
                    pattern: list_cons_match_pattern(512),
                    guard: None,
                    body: unit_lit(),
                }],
            ),
        };
        let bytecode = codegen_typed(vec![node]);
        let list_head_count = bytecode
            .opcodes
            .iter()
            .filter(|op| matches!(op, Opcode::ListHead))
            .count();

        assert_eq!(list_head_count, 1024);
    }

    #[test]
    fn deep_tail_block_function_codegen_uses_normal_test_stack() {
        let body = nested_tail_blocks(512, int_lit(1));
        let node = TypedNode {
            ty: Ty::Unit,
            span: test_span(),
            node: TypedInner::Def(
                0,
                resolved_id("deep", 1),
                Vec::new(),
                Vec::<TypedFunParam>::new(),
                Ty::Int,
                Box::new(body),
                Visibility::Public,
            ),
        };
        let bytecode = codegen_typed(vec![node]);

        assert_eq!(bytecode.functions.len(), 1);
        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::Return)));
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

        let bit_not_id =
            builtin_id_by_name("bit_not").expect("bit_not builtin metadata must exist");
        let bit_and_id =
            builtin_id_by_name("bit_and").expect("bit_and builtin metadata must exist");
        let bit_or_id = builtin_id_by_name("bit_or").expect("bit_or builtin metadata must exist");
        let bit_xor_id =
            builtin_id_by_name("bit_xor").expect("bit_xor builtin metadata must exist");

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
            let builtin_id = builtin_id_by_name(name)
                .unwrap_or_else(|| panic!("{name} builtin metadata must exist"));
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

        let safe_div_id =
            builtin_id_by_name("safe_div").expect("safe_div builtin metadata must exist");

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

        let lens_set_id = builtin_id_by_name("set").expect("set builtin metadata must exist");
        let lens_over_id = builtin_id_by_name("over").expect("over builtin metadata must exist");

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
    fn lens_bindings_are_erased_and_only_viewed_values_are_captured() {
        let bytecode = codegen_source(
            r#"defrecord User(name: String)
lens = User.name
name = Lens::view(lens, User("alice"))
getter = {|| name}
result = getter()"#,
        );

        let lens_view_id = builtin_id_by_name("view").expect("view builtin metadata must exist");
        let lens_compose_id =
            builtin_id_by_name("compose").expect("compose builtin metadata must exist");

        assert!(!bytecode.opcodes.iter().any(|op| {
            matches!(
                op,
                Opcode::CallBuiltin {
                    builtin_id,
                    ..
                } if *builtin_id == lens_view_id || *builtin_id == lens_compose_id
            )
        }));
        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::CallClosure { .. })));
    }

    #[test]
    fn lens_variant_mismatch_detail_includes_segment_context() {
        let bytecode = codegen_source(
            r#"defenum Expr {
  Add(Int, Int),
  Halt,
}
expr = Expr::Halt
Lens::view(Expr.Add, expr)"#,
        );

        let has_segment_detail = bytecode.constants.iter().any(|constant| {
            matches!(
                constant,
                Constant::Str(message)
                    if message.contains("Variant mismatch at segment 1")
                        && message.contains(".Add")
            )
        });
        assert!(
            has_segment_detail,
            "expected variant mismatch detail with segment context in constants"
        );
    }

    #[test]
    fn bounded_add_generic_helpers_emit_specialized_functions() {
        let bytecode = codegen_source(
            r#"def double<$N: Add>(x: $N) -> $N { x + x }
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

    #[test]
    fn compare_bound_list_helpers_emit_specialized_functions() {
        let bytecode = codegen_source(
            r#"largest = List::max([1, 3, 2])
smallest = List::min([1.5, 3.25, 2.0])
sorted = List::sort([3.25, 1.5, 2.0, 1.5])"#,
        );

        let function_names = bytecode
            .functions
            .iter()
            .filter_map(|entry| entry.qualified_name.as_deref())
            .collect::<Vec<_>>();

        assert!(function_names.contains(&"List::max"));
        assert!(function_names.contains(&"List::min"));
        assert!(function_names.contains(&"List::sort"));
    }

    #[test]
    fn codegen_typed_program_embeds_runtime_process_specs() {
        let typed = typed_module_program_with_builtin_prelude(
            r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
  }

  @init
  def init() -> Result<Int> { Ok(41) }

  @get
  def get(state: Int, label: String) -> Result<String> {
    Ok(label ++ ":" ++ to_string(state + 1))
  }

  @set
  def set(state: Int, next: Int) -> Result<Int> {
    Ok(next)
  }
}"#,
        );

        let bytecode = codegen_typed_program(typed).expect("codegen should succeed");
        assert_eq!(bytecode.runtime_process_specs.entries.len(), 1);
        let spec = &bytecode.runtime_process_specs.entries[0];
        assert_eq!(spec.process_name, "Counter");
        assert_eq!(spec.module_path, "Counter");
        assert!(!spec.boot);
        assert!(spec.registry);
        assert_eq!(spec.set_fun_idx.is_some(), true);
    }
}
