pub mod bytecode;
pub mod codegen;
pub mod error;
pub mod opcode;
pub mod registry;

pub use codegen::{
    codegen, codegen_typed_program, compose_bytecode_with_chunk, BindingInfo, ChunkMeta,
    ForgeCheckpoint, ForgeSession, ReplCallableDisplay, ReplCallableKind, ReplFacetInfo,
    ReplFacetSegmentInfo, ReplTypeKind, TypeDefDisplay,
};

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::{codegen, codegen_typed_program};
    use crate::bytecode::Constant;
    use crate::opcode::Opcode;
    use crate::registry::TypeKind;
    use scar::typed::{
        ComposeFlavor, TypedClosureParam, TypedFunParam, TypedInner, TypedMatchArm,
        TypedMatchPattern, TypedNode, TypedPattern,
    };
    use scar::types::Ty;
    use sigil::resolved::ResolvedId;
    use sindr::builtin::builtin_id_by_name;
    use sindr::ir::{CallableTemplateComposeFlavor, CallableTemplateKind};
    use sindr::primitives::int;
    use spire::ast::{Ast, BinOp, Lit, Span, Visibility};

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
    const ENCODE_MODULE_SOURCE: &str = include_str!("../../../lib/traits/encode.srt");
    const DECODE_MODULE_SOURCE: &str = include_str!("../../../lib/traits/decode.srt");
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
    const LENS_MODULE_SOURCE: &str = include_str!("../../../lib/facet.srt");
    const FLOAT_MODULE_SOURCE: &str = include_str!("../../../lib/types/float.srt");
    const JSON_MODULE_SOURCE: &str = include_str!("../../../lib/types/json.srt");
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
                        process_spec: None,
                    });
                }
                Ast::Defagent(_, module_path, body, process_spec, attrs)
                | Ast::Defgenserver(_, module_path, body, process_spec, attrs)
                | Ast::Defsupervisor(_, module_path, body, process_spec, attrs)
                | Ast::DefdynamicSupervisor(_, module_path, body, process_spec, attrs) => {
                    let mut module_ast = shared_imports.clone();
                    module_ast.extend(body);
                    lowered.push(sigil::StagedModuleAst {
                        module_path,
                        doc_module_path: None,
                        ast: module_ast,
                        module_doc: attrs.doc,
                        auto_import: attrs.auto_import,
                        process_spec: Some(process_spec),
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
                        process_spec: None,
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
                ("Encode", ENCODE_MODULE_SOURCE),
                ("Decode", DECODE_MODULE_SOURCE),
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
                ("Facet", LENS_MODULE_SOURCE),
                ("Float", FLOAT_MODULE_SOURCE),
                ("Json", JSON_MODULE_SOURCE),
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
        let mut boot_ast = Vec::new();
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
                        process_spec: None,
                    })
                }
                Ast::Defagent(_, module_path, body, process_spec, attrs)
                | Ast::Defgenserver(_, module_path, body, process_spec, attrs)
                | Ast::Defsupervisor(_, module_path, body, process_spec, attrs)
                | Ast::DefdynamicSupervisor(_, module_path, body, process_spec, attrs) => {
                    let mut module_ast = shared_imports.clone();
                    module_ast.extend(body);
                    Some(sigil::StagedModuleAst {
                        module_path,
                        doc_module_path: None,
                        ast: module_ast,
                        module_doc: attrs.doc,
                        auto_import: attrs.auto_import,
                        process_spec: Some(process_spec),
                    })
                }
                other @ Ast::SupervisorInit(_, _) => {
                    boot_ast.push(other);
                    None
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
            boot_ast,
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

    fn fun_param(name: &str, unique_id: u32, ty: Ty) -> TypedFunParam {
        TypedFunParam {
            id: resolved_id(name, unique_id),
            ty,
        }
    }

    fn closure_param(name: &str, unique_id: u32, ty: Ty) -> TypedClosureParam {
        TypedClosureParam {
            id: resolved_id(name, unique_id),
            ty,
        }
    }

    fn local_var(name: &str, unique_id: u32, ty: Ty) -> TypedNode {
        TypedNode {
            ty,
            span: test_span(),
            node: TypedInner::Var(resolved_id(name, unique_id)),
        }
    }

    fn user_func_var(
        name: &str,
        unique_id: u32,
        fun_idx: u32,
        params: Vec<Ty>,
        ret: Ty,
    ) -> TypedNode {
        TypedNode {
            ty: Ty::UserFunc {
                fun_idx,
                type_params: Vec::new(),
                params,
                ret: Box::new(ret),
            },
            span: test_span(),
            node: TypedInner::Var(resolved_id(name, unique_id)),
        }
    }

    fn builtin_func_var(name: &str, unique_id: u32, params: Vec<Ty>, ret: Ty) -> TypedNode {
        TypedNode {
            ty: Ty::BuiltinFunc {
                name: name.to_string(),
                params,
                ret: Box::new(ret),
            },
            span: test_span(),
            node: TypedInner::Var(resolved_id(name, unique_id)),
        }
    }

    fn builtin_app(name: &str, args: Vec<TypedNode>, ret: Ty) -> TypedNode {
        let params = args.iter().map(|arg| arg.ty.clone()).collect::<Vec<_>>();
        TypedNode {
            ty: ret.clone(),
            span: test_span(),
            node: TypedInner::App(Box::new(builtin_func_var(name, 9_000, params, ret)), args),
        }
    }

    fn user_capture(
        name: &str,
        unique_id: u32,
        fun_idx: u32,
        params: Vec<Ty>,
        ret: Ty,
    ) -> TypedNode {
        TypedNode {
            ty: Ty::Func(params.clone(), Box::new(ret.clone())),
            span: test_span(),
            node: TypedInner::Capture(
                Box::new(user_func_var(name, unique_id, fun_idx, params, ret)),
                Vec::new(),
            ),
        }
    }

    fn builtin_capture(name: &str, unique_id: u32, params: Vec<Ty>, ret: Ty) -> TypedNode {
        TypedNode {
            ty: Ty::Func(params.clone(), Box::new(ret.clone())),
            span: test_span(),
            node: TypedInner::Capture(
                Box::new(builtin_func_var(name, unique_id, params, ret)),
                Vec::new(),
            ),
        }
    }

    fn identity_def(name: &str, fun_idx: u32, fun_uid: u32, param_uid: u32, ty: Ty) -> TypedNode {
        let param = fun_param("value", param_uid, ty.clone());
        TypedNode {
            ty: Ty::Unit,
            span: test_span(),
            node: TypedInner::Def(
                fun_idx,
                resolved_id(name, fun_uid),
                Vec::new(),
                vec![param.clone()],
                ty.clone(),
                Box::new(local_var("value", param.id.unique_id, ty)),
                Visibility::Public,
            ),
        }
    }

    fn binary_first_def(name: &str, fun_idx: u32, fun_uid: u32) -> TypedNode {
        let left = fun_param("left", fun_uid + 1, Ty::Int);
        let right = fun_param("right", fun_uid + 2, Ty::Int);
        TypedNode {
            ty: Ty::Unit,
            span: test_span(),
            node: TypedInner::Def(
                fun_idx,
                resolved_id(name, fun_uid),
                Vec::new(),
                vec![left.clone(), right],
                Ty::Int,
                Box::new(local_var("left", left.id.unique_id, Ty::Int)),
                Visibility::Public,
            ),
        }
    }

    fn top_level_opcodes(bytecode: &sindr::ir::Bytecode) -> Vec<&Opcode> {
        bytecode
            .opcodes
            .iter()
            .take_while(|opcode| !matches!(opcode, Opcode::Halt))
            .collect()
    }

    fn function_body_opcodes<'a>(
        bytecode: &'a sindr::ir::Bytecode,
        fun_idx: u32,
    ) -> Vec<&'a Opcode> {
        let entry_pc = bytecode.functions[fun_idx as usize].entry_pc as usize;
        let end_pc = bytecode
            .functions
            .iter()
            .filter_map(|entry| {
                let pc = entry.entry_pc as usize;
                (pc > entry_pc).then_some(pc)
            })
            .min()
            .unwrap_or(bytecode.opcodes.len());
        bytecode.opcodes[entry_pc..end_pc].iter().collect()
    }

    fn int_lit(value: i64) -> TypedNode {
        TypedNode {
            ty: Ty::Int,
            span: test_span(),
            node: TypedInner::Lit(Lit::Int(int(value))),
        }
    }

    fn str_lit(value: &str) -> TypedNode {
        TypedNode {
            ty: Ty::Str,
            span: test_span(),
            node: TypedInner::Lit(Lit::Str(value.to_string())),
        }
    }

    fn list_lit_int(values: impl IntoIterator<Item = i64>) -> TypedNode {
        TypedNode {
            ty: Ty::List(Box::new(Ty::Int)),
            span: test_span(),
            node: TypedInner::ListLiteral(values.into_iter().map(int_lit).collect()),
        }
    }

    fn assert_no_call_builtin(bytecode: &sindr::ir::Bytecode, builtin_name: &str) {
        let builtin_id = builtin_id_by_name(builtin_name)
            .unwrap_or_else(|| panic!("{builtin_name} builtin metadata must exist"));
        assert!(!bytecode.opcodes.iter().any(|op| {
            matches!(
                op,
                Opcode::CallBuiltin {
                    builtin_id: id,
                    ..
                } if *id == builtin_id
            )
        }));
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
        let bytecode = codegen_typed(vec![
            TypedNode {
                ty: Ty::Unit,
                span: test_span(),
                node: TypedInner::Def(
                    0,
                    resolved_id("add", 1),
                    Vec::new(),
                    vec![fun_param("x", 2, Ty::Int), fun_param("y", 3, Ty::Int)],
                    Ty::Int,
                    Box::new(TypedNode {
                        ty: Ty::Int,
                        span: test_span(),
                        node: TypedInner::BinOp(
                            BinOp::Add,
                            Box::new(local_var("x", 2, Ty::Int)),
                            Box::new(local_var("y", 3, Ty::Int)),
                        ),
                    }),
                    Visibility::Public,
                ),
            },
            TypedNode {
                ty: Ty::Int,
                span: test_span(),
                node: TypedInner::App(
                    Box::new(user_func_var("add", 4, 0, vec![Ty::Int, Ty::Int], Ty::Int)),
                    vec![int_lit(1), int_lit(2)],
                ),
            },
        ]);

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
    fn zero_capture_closure_literal_omits_capture_closure_zero() {
        let x = closure_param("x", 10, Ty::Int);
        let bytecode = codegen_typed(vec![TypedNode {
            ty: Ty::Func(vec![Ty::Int], Box::new(Ty::Int)),
            span: test_span(),
            node: TypedInner::Closure(
                vec![x.clone()],
                Vec::new(),
                Box::new(TypedNode {
                    ty: Ty::Int,
                    span: test_span(),
                    node: TypedInner::BinOp(
                        BinOp::Add,
                        Box::new(local_var("x", x.id.unique_id, Ty::Int)),
                        Box::new(int_lit(1)),
                    ),
                }),
            ),
        }]);

        assert!(!bytecode
            .opcodes
            .windows(2)
            .any(|ops| matches!(ops, [Opcode::LoadFunctionRef(_), Opcode::CaptureClosure(0)])));
    }

    #[test]
    fn capturing_closure_literal_still_emits_capture_closure() {
        let base = resolved_id("base", 20);
        let x = closure_param("x", 21, Ty::Int);
        let bytecode = codegen_typed(vec![
            TypedNode {
                ty: Ty::Unit,
                span: test_span(),
                node: TypedInner::Bind(
                    TypedPattern::Var(Ty::Int, base.clone()),
                    Box::new(int_lit(10)),
                ),
            },
            TypedNode {
                ty: Ty::Func(vec![Ty::Int], Box::new(Ty::Int)),
                span: test_span(),
                node: TypedInner::Closure(
                    vec![x.clone()],
                    vec![base.clone()],
                    Box::new(TypedNode {
                        ty: Ty::Int,
                        span: test_span(),
                        node: TypedInner::BinOp(
                            BinOp::Add,
                            Box::new(local_var("x", x.id.unique_id, Ty::Int)),
                            Box::new(local_var("base", base.unique_id, Ty::Int)),
                        ),
                    }),
                ),
            },
        ]);

        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::CaptureClosure(count) if *count > 0)));
    }

    #[test]
    fn tail_closure_call_lowers_to_fused_tail_opcode() {
        let f = fun_param("f", 30, Ty::Func(vec![Ty::Int], Box::new(Ty::Int)));
        let value = fun_param("value", 31, Ty::Int);
        let bytecode = codegen_typed(vec![TypedNode {
            ty: Ty::Unit,
            span: test_span(),
            node: TypedInner::Def(
                0,
                resolved_id("apply_tail", 32),
                Vec::new(),
                vec![f.clone(), value.clone()],
                Ty::Int,
                Box::new(TypedNode {
                    ty: Ty::Int,
                    span: test_span(),
                    node: TypedInner::App(
                        Box::new(local_var("f", f.id.unique_id, f.ty.clone())),
                        vec![local_var("value", value.id.unique_id, Ty::Int)],
                    ),
                }),
                Visibility::Public,
            ),
        }]);
        let apply_tail = bytecode
            .functions
            .iter()
            .find(|entry| {
                entry
                    .qualified_name
                    .as_deref()
                    .is_some_and(|name| name.ends_with("apply_tail"))
            })
            .expect("apply_tail function should be present");
        let body = function_body_opcodes(&bytecode, apply_tail.fun_idx);

        assert!(body
            .iter()
            .any(|opcode| matches!(opcode, Opcode::TailCallClosure { arity: 1, .. })));
        assert!(!body
            .windows(2)
            .any(|ops| { matches!(ops, [Opcode::CallClosure { .. }, Opcode::Return]) }));
    }

    #[test]
    fn pipe_direct_user_capture_lowers_to_call_without_callclosure() {
        let bytecode = codegen_typed(vec![
            identity_def("id_int", 0, 10, 11, Ty::Int),
            TypedNode {
                ty: Ty::Int,
                span: test_span(),
                node: TypedInner::Pipe(
                    Box::new(int_lit(7)),
                    Box::new(user_capture("id_int", 12, 0, vec![Ty::Int], Ty::Int)),
                ),
            },
        ]);
        let top_level = top_level_opcodes(&bytecode);

        assert!(top_level.iter().any(|op| matches!(
            op,
            Opcode::Call {
                fun_idx: 0,
                arity: 1,
                ..
            }
        )));
        assert!(!top_level
            .iter()
            .any(|op| matches!(op, Opcode::CallClosure { .. })));
    }

    #[test]
    fn pipe_direct_builtin_capture_lowers_to_callbuiltin_without_callclosure() {
        let builtin_id = builtin_id_by_name("to_string").expect("to_string builtin exists");
        let bytecode = codegen_typed(vec![TypedNode {
            ty: Ty::Str,
            span: test_span(),
            node: TypedInner::Pipe(
                Box::new(int_lit(7)),
                Box::new(builtin_capture("to_string", 12, vec![Ty::Int], Ty::Str)),
            ),
        }]);
        let top_level = top_level_opcodes(&bytecode);

        assert!(top_level.iter().any(|op| {
            matches!(
                op,
                Opcode::CallBuiltin {
                    builtin_id: id,
                    arity: 1,
                    ..
                } if *id == builtin_id
            )
        }));
        assert!(!top_level
            .iter()
            .any(|op| matches!(op, Opcode::CallClosure { .. })));
    }

    #[test]
    fn pipe_direct_injected_user_call_lowers_without_partial_wrapper() {
        let bytecode = codegen_typed(vec![
            binary_first_def("first_int", 0, 20),
            TypedNode {
                ty: Ty::Int,
                span: test_span(),
                node: TypedInner::Pipe(
                    Box::new(int_lit(7)),
                    Box::new(TypedNode {
                        ty: Ty::Func(vec![Ty::Int], Box::new(Ty::Int)),
                        span: test_span(),
                        node: TypedInner::InjectCall(
                            Box::new(user_func_var(
                                "first_int",
                                23,
                                0,
                                vec![Ty::Int, Ty::Int],
                                Ty::Int,
                            )),
                            vec![int_lit(9)],
                        ),
                    }),
                ),
            },
        ]);
        let top_level = top_level_opcodes(&bytecode);

        assert!(top_level.iter().any(|op| matches!(
            op,
            Opcode::Call {
                fun_idx: 0,
                arity: 2,
                ..
            }
        )));
        assert!(!bytecode
            .functions
            .iter()
            .any(|entry| entry.flags.partial_apply_wrapper));
        assert!(!top_level
            .iter()
            .any(|op| matches!(op, Opcode::CallClosure { .. } | Opcode::CaptureClosure(_))));
    }

    #[test]
    fn direct_compose_uses_callable_template_without_generated_wrapper() {
        let bytecode = codegen_typed(vec![
            identity_def("left_int", 0, 30, 31, Ty::Int),
            identity_def("right_int", 1, 32, 33, Ty::Int),
            TypedNode {
                ty: Ty::Func(vec![Ty::Int], Box::new(Ty::Int)),
                span: test_span(),
                node: TypedInner::Compose(
                    ComposeFlavor::Plain,
                    Box::new(user_capture("left_int", 34, 0, vec![Ty::Int], Ty::Int)),
                    Box::new(user_capture("right_int", 35, 1, vec![Ty::Int], Ty::Int)),
                ),
            },
        ]);
        let top_level = top_level_opcodes(&bytecode);

        assert!(bytecode.callable_templates.iter().any(|template| matches!(
            template.kind,
            CallableTemplateKind::ComposeDirect {
                flavor: CallableTemplateComposeFlavor::Plain,
            }
        )));
        assert!(!bytecode
            .functions
            .iter()
            .any(|entry| entry.flags.generated && !entry.flags.closure && entry.arity == 1));
        assert!(top_level
            .iter()
            .any(|op| matches!(op, Opcode::LoadCallableTemplateRef(_))));
        assert!(top_level
            .iter()
            .any(|op| matches!(op, Opcode::CaptureClosure(2))));
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
            bytecode.type_registry.entries().len(),
            baseline.type_registry.entries().len() + 2
        );

        let user = &bytecode.type_registry.entries()[baseline.type_registry.entries().len()];
        assert!(user.tag >= 2);
        assert_eq!(user.name, "Global::User");
        assert_eq!(user.kind, TypeKind::Struct);

        let point = &bytecode.type_registry.entries()[baseline.type_registry.entries().len() + 1];
        assert_eq!(point.tag, user.tag + 1);
        assert_eq!(point.name, "Global::Point");
        assert_eq!(point.kind, TypeKind::Record);
    }

    #[test]
    fn direct_int_bitwise_builtin_calls_lower_to_specialized_opcodes() {
        let bytecode = codegen_typed(vec![
            builtin_app("bit_not", vec![int_lit(6)], Ty::Int),
            builtin_app("bit_and", vec![int_lit(6), int_lit(3)], Ty::Int),
            builtin_app("bit_or", vec![int_lit(6), int_lit(3)], Ty::Int),
            builtin_app("bit_xor", vec![int_lit(6), int_lit(3)], Ty::Int),
        ]);

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

        for name in ["bit_not", "bit_and", "bit_or", "bit_xor"] {
            assert_no_call_builtin(&bytecode, name);
        }
    }

    #[test]
    fn direct_string_len_builtin_call_lowers_to_specialized_opcode() {
        let bytecode = codegen_typed(vec![builtin_app(
            "string_len",
            vec![str_lit("あb")],
            Ty::Int,
        )]);

        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::StringLen)));

        assert_no_call_builtin(&bytecode, "string_len");
    }

    #[test]
    fn direct_list_len_builtin_call_lowers_to_specialized_opcode() {
        let bytecode = codegen_typed(vec![builtin_app(
            "len",
            vec![list_lit_int([1, 2, 3])],
            Ty::Int,
        )]);

        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::ListLen)));

        assert_no_call_builtin(&bytecode, "len");
    }

    #[test]
    fn direct_safe_mod_builtin_call_lowers_to_specialized_opcode() {
        let bytecode = codegen_typed(vec![builtin_app(
            "safe_mod",
            vec![int_lit(7), int_lit(3)],
            Ty::Int,
        )]);

        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::SafeModInt)));

        assert_no_call_builtin(&bytecode, "safe_mod");
    }

    #[test]
    fn direct_string_predicate_builtin_calls_lower_to_specialized_opcodes() {
        let bytecode = codegen_typed(vec![
            builtin_app(
                "string_contains",
                vec![str_lit("surtr"), str_lit("urt")],
                Ty::Bool,
            ),
            builtin_app(
                "string_starts_with",
                vec![str_lit("surtr"), str_lit("sur")],
                Ty::Bool,
            ),
            builtin_app(
                "string_ends_with",
                vec![str_lit("surtr"), str_lit("tr")],
                Ty::Bool,
            ),
        ]);

        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::StringContains)));
        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::StringStartsWith)));
        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::StringEndsWith)));

        for name in ["string_contains", "string_starts_with", "string_ends_with"] {
            assert_no_call_builtin(&bytecode, name);
        }
    }

    #[test]
    fn direct_string_split_and_replace_stay_as_builtin_calls() {
        let bytecode = codegen_typed(vec![
            builtin_app(
                "string_split",
                vec![str_lit("a,b"), str_lit(",")],
                Ty::List(Box::new(Ty::Str)),
            ),
            builtin_app(
                "string_replace",
                vec![str_lit("banana"), str_lit("na"), str_lit("NA")],
                Ty::Str,
            ),
        ]);

        for (name, arity) in [("string_split", 2), ("string_replace", 3)] {
            let builtin_id = builtin_id_by_name(name)
                .unwrap_or_else(|| panic!("{name} builtin metadata must exist"));
            assert!(bytecode.opcodes.iter().any(|op| {
                matches!(
                    op,
                    Opcode::CallBuiltin {
                        builtin_id: id,
                        arity: actual_arity,
                        ..
                    } if *id == builtin_id && *actual_arity == arity
                )
            }));
        }
    }

    #[test]
    fn int_bit_index_helpers_stay_as_builtin_calls() {
        let bytecode = codegen_typed(vec![
            builtin_app("test_bit", vec![int_lit(5), int_lit(0)], Ty::Bool),
            builtin_app("set_bit", vec![int_lit(0), int_lit(1)], Ty::Int),
            builtin_app("clear_bit", vec![int_lit(7), int_lit(1)], Ty::Int),
            builtin_app("toggle_bit", vec![int_lit(5), int_lit(0)], Ty::Int),
        ]);

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
    fn facet_set_and_over_are_lowered_without_runtime_builtin_calls() {
        let bytecode = codegen_source(
            r#"defrecord User(name: String)
user = User("alice")
user2 = Facet::set(User.name, user, "bob")
user3 = Facet::over(User.name, user2, {|name| Ok(name ++ "!")})"#,
        );

        let facet_set_id = builtin_id_by_name("set").expect("set builtin metadata must exist");
        let facet_over_id = builtin_id_by_name("over").expect("over builtin metadata must exist");

        assert!(!bytecode.opcodes.iter().any(|op| {
            matches!(
                op,
                Opcode::CallBuiltin {
                    builtin_id,
                    ..
                } if *builtin_id == facet_set_id || *builtin_id == facet_over_id
            )
        }));
        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::CallClosure { arity: 1, .. })));
    }

    #[test]
    fn facet_bindings_are_erased_and_only_viewed_values_are_captured() {
        let bytecode = codegen_source(
            r#"defrecord User(name: String)
facet = User.name
name = Facet::view(facet, User("alice"))
getter = {|| name}
result = getter()"#,
        );

        let facet_view_id = builtin_id_by_name("view").expect("view builtin metadata must exist");
        let facet_compose_id =
            builtin_id_by_name("compose").expect("compose builtin metadata must exist");

        assert!(!bytecode.opcodes.iter().any(|op| {
            matches!(
                op,
                Opcode::CallBuiltin {
                    builtin_id,
                    ..
                } if *builtin_id == facet_view_id || *builtin_id == facet_compose_id
            )
        }));
        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::CallClosure { .. })));
    }

    #[test]
    fn facet_variant_mismatch_detail_includes_segment_context() {
        let bytecode = codegen_source(
            r#"defenum Expr {
  Add(Int, Int),
  Halt,
}
expr = Expr::Halt
Facet::view(Expr.Add, expr)"#,
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

        assert!(function_names.contains(&"Global::List::max"));
        assert!(function_names.contains(&"Global::List::min"));
        assert!(function_names.contains(&"Global::List::sort"));
    }

    #[test]
    fn codegen_typed_program_embeds_runtime_process_specs() {
        let typed = typed_module_program_with_builtin_prelude(
            r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
    handlers {
      out: OutHandler = StdOut
    }
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
        assert_eq!(bytecode.runtime_process_specs.entries.len(), 2);
        let spec = bytecode
            .runtime_process_specs
            .entries
            .iter()
            .find(|entry| entry.type_name == "Global::Counter")
            .expect("Counter runtime process spec");
        assert_eq!(spec.type_name, "Global::Counter");
        assert_eq!(spec.state.state_type.name, "Int");
        assert_eq!(spec.init.policy, sindr::ir::RuntimeInitPolicy::Eager);
        assert_eq!(spec.handlers.len(), 3);
        assert_eq!(spec.dependencies.handlers.len(), 1);
        assert_eq!(spec.dependencies.handlers[0].slot, "out");
        assert_eq!(spec.dependencies.handlers[0].capability, "OutHandler");
        assert_eq!(spec.dependencies.handlers[0].default_target.name, "StdOut");
    }

    #[test]
    fn codegen_typed_program_embeds_runtime_boot_plan() {
        let typed = typed_module_program_with_builtin_prelude(
            r#"defagent Logger {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
    handlers {
      out: OutHandler = StdOut
    }
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, label: String) -> Result<String> { Ok(label) }
}

supervisor_init {
  Logger {
    timeout: 5s
    handlers {
      out: FileOutHandler(path: "./logs/app.log")
    }
  }

  DynamicSupervisor {
    max_restarts: 10
    allow_adopt: True
  }
}"#,
        );

        let bytecode = codegen_typed_program(typed).expect("codegen should succeed");
        assert_eq!(bytecode.runtime_boot_plan.singletons.len(), 1);
        let entry = &bytecode.runtime_boot_plan.singletons[0];
        assert_eq!(entry.process_name, "Global::Logger");
        assert_eq!(entry.init_timeout_ms, 5_000);
        assert_eq!(bytecode.runtime_boot_plan.handler_overrides.len(), 1);
        let handler = &bytecode.runtime_boot_plan.handler_overrides[0];
        assert_eq!(handler.target_process, "Global::Logger");
        assert_eq!(handler.slot, "out");
        assert_eq!(handler.handler_target.name, "FileOutHandler");
        assert_eq!(handler.handler_target.named_args[0].name, "path");
        assert_eq!(handler.handler_target.named_args[0].value, "./logs/app.log");
        assert_eq!(bytecode.runtime_boot_plan.supervisor_overrides.len(), 1);
        let supervisor = &bytecode.runtime_boot_plan.supervisor_overrides[0];
        assert_eq!(supervisor.process_name, "DynamicSupervisor");
        assert_eq!(supervisor.policy.max_restarts, 10);
        assert!(supervisor.policy.allow_adopt);
    }

    #[test]
    fn codegen_rejects_boot_plan_handler_override_for_unknown_slot() {
        let typed = typed_module_program_with_builtin_prelude(
            r#"defagent Logger {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
    handlers {
      out: OutHandler = StdOut
    }
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, label: String) -> Result<String> { Ok(label) }
}

supervisor_init {
  Logger {
    handlers {
      missing: NullOutHandler
    }
  }
}"#,
        );

        let err = codegen_typed_program(typed).expect_err("unknown handler slot should fail");
        assert!(err
            .message
            .contains("handler slot is not declared by the target process"));
    }

    #[test]
    fn codegen_typed_program_embeds_genserver_runtime_handler_specs() {
        let typed = typed_module_program_with_builtin_prelude(
            r#"defgenserver Logger {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @call
  def info(state: Int, message: String) -> Result<CallResult<String, Int>> {
    Ok(CallResult::Reply(message, state))
  }

  @cast
  def reset(_state: Int, next: Int) -> Result<CastResult<Int>> { Ok(CastResult::Next(next)) }
}"#,
        );

        let bytecode = codegen_typed_program(typed).expect("codegen should succeed");
        assert_eq!(bytecode.runtime_process_specs.entries.len(), 2);
        let spec = bytecode
            .runtime_process_specs
            .entries
            .iter()
            .find(|entry| entry.type_name == "Global::Logger")
            .expect("Logger runtime process spec");
        assert_eq!(spec.kind, sindr::ir::RuntimeProcessKind::GenServer);
        assert_eq!(spec.handlers.len(), 3);
        assert_eq!(spec.handlers[0].handler_id, 0);
        assert_eq!(spec.handlers[0].name, "init");
        assert_eq!(spec.handlers[0].kind, sindr::ir::RuntimeHandlerKind::Init);
        assert_eq!(spec.handlers[0].fun_idx, spec.init.callable.fun_idx);
        assert_eq!(spec.handlers[1].handler_id, 1);
        assert_eq!(spec.handlers[1].name, "info");
        assert_eq!(spec.handlers[1].kind, sindr::ir::RuntimeHandlerKind::Call);
        assert_eq!(spec.handlers[2].handler_id, 2);
        assert_eq!(spec.handlers[2].name, "reset");
        assert_eq!(spec.handlers[2].kind, sindr::ir::RuntimeHandlerKind::Cast);
    }

    #[test]
    fn codegen_typed_program_emits_v2_process_spec_for_lazy_process_init() {
        let typed = typed_module_program_with_builtin_prelude(
            r#"defgenserver LazyCache {
  meta {
    instance: Singleton
    init_policy: Lazy
    state: Int
  }

  @init
  def init() -> Result<ProcessInit<Int>> {
    Ok(Ready(0))
  }

  @call
  def value(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Reply(state, state))
  }
}"#,
        );

        let bytecode = codegen_typed_program(typed).expect("codegen should succeed");
        assert_eq!(bytecode.runtime_process_specs.entries.len(), 2);
        let spec = bytecode
            .runtime_process_specs
            .entries
            .iter()
            .find(|entry| entry.type_name == "Global::LazyCache")
            .expect("LazyCache runtime process spec");
        assert_eq!(spec.type_name, "Global::LazyCache");
        assert_eq!(spec.state.state_type.name, "Int");
        assert_eq!(spec.init.policy, sindr::ir::RuntimeInitPolicy::Lazy);
        assert!(matches!(
            spec.init.result_shape,
            sindr::ir::RuntimeInitResultShape::LazyProcessInit { .. }
        ));
        assert_eq!(spec.dependencies.handlers.len(), 0);
        assert!(spec.lifecycle.owner.is_none());
        assert_eq!(
            spec.supervision.parent.as_deref(),
            Some("RuntimeSupervisor")
        );
    }
}
