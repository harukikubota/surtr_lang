pub mod checker;
pub mod env;
pub mod error;
pub mod typed;
pub mod types;

pub use checker::{
    typecheck, typecheck_with_context, ScarCheckpoint, ScarSession, TypecheckContext,
};

#[cfg(test)]
mod tests {
    use super::{typecheck, typecheck_with_context, TypecheckContext};
    use crate::typed::TypedInner;
    use crate::typed::TypedNode;
    use spire::ast::Ast;
    use spire::{EntryPoint, SetExitCodePolicy, SourceRules};

    const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../lib/bootstrap.srt");
    const KERNEL_PRELUDE_SOURCE: &str = include_str!("../../../lib/kernel.srt");

    fn parse_std_module_stage(source: &str) -> Vec<sigil::StagedModuleAst> {
        let ast = spire::parse_with_context(
            source,
            spire::ParserContext::module(0, None).with_rules(SourceRules::std_module()),
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
        let mut shared_defs = Vec::new();

        for stmt in ast {
            match stmt {
                Ast::Defmod(_, module_path, body) => {
                    let mut module_ast = shared_imports.clone();
                    module_ast.extend(body);
                    lowered.push(sigil::StagedModuleAst {
                        module_path,
                        ast: module_ast,
                    });
                }
                Ast::Import(_, _, _) => {}
                other => shared_defs.push(other),
            }
        }

        if !shared_defs.is_empty() {
            let mut global_ast = shared_imports;
            global_ast.extend(shared_defs);
            lowered.push(sigil::StagedModuleAst {
                module_path: String::new(),
                ast: global_ast,
            });
        }

        lowered
    }

    fn resolve_with_builtin_prelude(source: &str) -> Vec<sigil::resolved::Resolved> {
        let module_stages = vec![
            parse_std_module_stage(BUILTIN_PRELUDE_SOURCE),
            parse_std_module_stage(KERNEL_PRELUDE_SOURCE),
        ];
        let user_ast = spire::parse(source).expect("source should parse");
        let declaration_index = sigil::precollect_declaration_index(&module_stages)
            .expect("std modules should precollect");
        sigil::resolve_staged_program(&module_stages, user_ast, &declaration_index, None)
            .expect("source should resolve")
    }

    fn typecheck_with_builtin_prelude(source: &str) -> Vec<TypedNode> {
        let resolved = resolve_with_builtin_prelude(source);
        typecheck(resolved).expect("source should typecheck")
    }

    fn typecheck_with_rules(
        source: &str,
        source_rules: SourceRules,
    ) -> Result<Vec<TypedNode>, crate::error::TypeError> {
        let resolved = resolve_with_builtin_prelude(source);
        typecheck_with_context(resolved, TypecheckContext { source_rules })
    }

    #[test]
    fn field_access_is_resolved_to_numeric_index() {
        let resolved = resolve_with_builtin_prelude(
            r#"defstruct User {
  name: String,
  age: Int,
}

user: User = User { name: "alice", age: 30 }
age = user.age"#,
        );

        let typed = typecheck(resolved).expect("typecheck should succeed");
        let field_index = typed.iter().find_map(|node| {
            if let TypedInner::Bind(_, rhs) = &node.node {
                if let TypedInner::FieldAccess(_, idx) = &rhs.node {
                    return Some(*idx);
                }
            }
            None
        });

        assert_eq!(field_index, Some(1));
    }

    #[test]
    fn match_bool_requires_exhaustive_arms() {
        let resolved = resolve_with_builtin_prelude(
            r#"flag = True
print(match flag {
  True => "yes",
})"#,
        );

        let err = typecheck(resolved).expect_err("typecheck should fail");
        assert!(err.message.contains("Non-exhaustive match. Missing: False"));
    }

    #[test]
    fn safebind_rhs_must_be_result() {
        let resolved = resolve_with_builtin_prelude("num =? 10");
        let err = typecheck(resolved).expect_err("typecheck should fail");
        assert!(err.message.contains("`=?` requires Result"));
    }

    #[test]
    fn safebind_function_requires_result_return_type() {
        let resolved = resolve_with_builtin_prelude(
            r#"def bad() -> Int {
  num =? Ok(1)
  num
}"#,
        );

        let err = typecheck(resolved).expect_err("typecheck should fail");
        assert!(err
            .message
            .contains("can only be used in functions returning Result"));
    }

    #[test]
    fn safebind_top_ok_pattern_requires_nested_result_rhs() {
        let resolved = resolve_with_builtin_prelude(
            r#"value: Result<Int> = Ok(1)
Ok(num) =? value"#,
        );
        let err = typecheck(resolved).expect_err("typecheck should fail");
        assert!(err.message.contains("`Ok(...)` pattern requires Result"));
    }

    #[test]
    fn safebind_top_ok_pattern_accepts_nested_result_rhs() {
        let resolved = resolve_with_builtin_prelude(
            r#"value: Result<Result<Int>> = Ok(Ok(1))
Ok(num) =? value"#,
        );
        let typed = typecheck(resolved).expect("typecheck should succeed");
        assert!(matches!(
            typed.last().map(|node| &node.node),
            Some(TypedInner::SafeBind(_, _))
        ));
    }

    #[test]
    fn safebind_list_pattern_accepts_plain_list_rhs() {
        let resolved = resolve_with_builtin_prelude(
            r#"value = [1, 2, 3]
[head, ..tail] =? value"#,
        );
        let typed = typecheck(resolved).expect("typecheck should succeed");
        assert!(matches!(
            typed.last().map(|node| &node.node),
            Some(TypedInner::SafeBind(_, _))
        ));
    }

    #[test]
    fn safebind_list_pattern_accepts_nested_constructor_literals() {
        let resolved = resolve_with_builtin_prelude(
            r#"lr = [Ok(1), Ok(2), Ok(3)]
[Ok(1), Ok(2), _] =? lr"#,
        );
        let typed = typecheck(resolved).expect("typecheck should succeed");
        assert!(matches!(
            typed.last().map(|node| &node.node),
            Some(TypedInner::SafeBind(_, _))
        ));
    }

    #[test]
    fn forward_struct_type_annotation_and_literal_are_allowed() {
        let resolved = resolve_with_builtin_prelude(
            r#"user: User = User { name: "alice", age: 30 }
defstruct User {
  name: String,
  age: Int,
}"#,
        );
        let typed = typecheck(resolved).expect("forward struct reference should typecheck");
        assert!(typed
            .iter()
            .any(|node| matches!(node.node, TypedInner::StructDef(_, _, _))));
    }

    #[test]
    fn forward_deferror_value_can_flow_into_err() {
        let resolved = resolve_with_builtin_prelude(
            r#"ret: Result<Int> = Err(NotFound)
deferror NotFound {
  "not found"
}"#,
        );
        let typed = typecheck(resolved).expect("forward deferror constructor should typecheck");
        assert!(typed
            .iter()
            .any(|node| matches!(node.node, TypedInner::DeferrorDef(_, _, _, _, _))));
    }

    #[test]
    fn forward_reference_type_tags_are_deterministic_across_runs() {
        let source = r#"user: User = User { name: "alice", age: 30 }
pair = Pair(first: 1, second: "two")
ret: Result<Int> = Err(NotFound("404"))

defstruct User {
  name: String,
  age: Int,
}

defrecord Pair(first: Int, second: String)

deferror NotFound(code: String) {
  "missing #{code}"
}"#;

        let first = typecheck_with_builtin_prelude(source);
        let second = typecheck_with_builtin_prelude(source);

        fn collect_type_tags(nodes: &[TypedNode]) -> Vec<(String, u32)> {
            nodes
                .iter()
                .filter_map(|node| match &node.node {
                    TypedInner::StructDef(tag, name, _) | TypedInner::RecordDef(tag, name, _) => {
                        Some((name.clone(), *tag))
                    }
                    TypedInner::DeferrorDef(tag, _, id, _, _) => Some((id.name.clone(), *tag)),
                    _ => None,
                })
                .collect()
        }

        assert_eq!(collect_type_tags(&first), collect_type_tags(&second));
    }

    #[test]
    fn set_exit_code_is_allowed_in_script_rules() {
        let typed =
            typecheck_with_rules("set_exit_code(9)", SourceRules::script()).expect("must pass");
        assert!(matches!(
            typed.last().map(|node| &node.node),
            Some(TypedInner::App(_, _))
        ));
    }

    #[test]
    fn set_exit_code_is_forbidden_in_repl_chunk_rules() {
        let err = typecheck_with_rules("set_exit_code(9)", SourceRules::repl_chunk())
            .expect_err("must fail");
        assert!(err.message.contains("forbidden by source policy"));
    }

    #[test]
    fn set_exit_code_entry_only_policy_allows_only_entrypoint_function() {
        let entrypoint = EntryPoint::qualified("main");
        let rules = SourceRules::module()
            .with_set_exit_code_policy(SetExitCodePolicy::EntryOnly, Some(&entrypoint));

        let ok = typecheck_with_rules(
            r#"def main() -> Result<()> {
  set_exit_code(7)
  Ok(())
}"#,
            rules.clone(),
        )
        .expect("entrypoint body should allow set_exit_code");
        assert!(matches!(
            ok.iter()
                .find(|node| matches!(node.node, TypedInner::Def(_, _, _, _, _))),
            Some(_)
        ));

        let err = typecheck_with_rules(
            r#"def helper() -> Result<()> {
  set_exit_code(7)
  Ok(())
}"#,
            rules,
        )
        .expect_err("non-entrypoint function must fail");
        assert!(err.message.contains("only allowed inside entrypoint"));
    }

    #[test]
    fn generic_annotation_list_int_is_accepted() {
        let typed = typecheck_with_builtin_prelude("nums: List<Int> = [1, 2, 3]");
        assert!(matches!(
            typed.last().map(|node| &node.node),
            Some(TypedInner::Bind(_, _))
        ));
    }

    #[test]
    fn cyclic_type_definition_is_rejected() {
        let resolved = resolve_with_builtin_prelude(
            r#"defstruct Node {
  next: Node,
}"#,
        );
        let err = typecheck(resolved).expect_err("cyclic type must fail");
        assert!(err.message.contains("Cyclic type definition detected"));
    }

    #[test]
    fn match_binding_pattern_is_treated_as_exhaustive() {
        let resolved = resolve_with_builtin_prelude(
            r#"flag = True
answer = match flag {
  value => value,
}"#,
        );
        let typed = typecheck(resolved).expect("binding arm should be exhaustive");
        assert!(matches!(
            typed.last().map(|node| &node.node),
            Some(TypedInner::Bind(_, _))
        ));
    }

    #[test]
    fn struct_literal_rejects_extra_fields() {
        let resolved = resolve_with_builtin_prelude(
            r#"defstruct User {
  name: String,
  age: Int,
}
user = User { name: "alice", age: 20, extra: 1 }"#,
        );
        let err = typecheck(resolved).expect_err("extra fields must fail");
        assert!(err.message.contains("Unknown field 'extra' in User"));
    }

    #[test]
    fn constructor_named_args_reject_duplicate_fields() {
        let resolved = resolve_with_builtin_prelude(
            r#"defrecord Pair(first: Int, second: String)
pair = Pair(first: 1, first: 2)"#,
        );
        let err = typecheck(resolved).expect_err("duplicate named args must fail");
        assert!(err.message.contains("Duplicate field 'first' in Pair"));
    }

    #[test]
    fn deferror_show_type_mismatch_points_to_show_expression_span() {
        let source = r#"deferror NotFound(code: String) {
  123
}"#;
        let resolved = resolve_with_builtin_prelude(source);
        let err = typecheck(resolved).expect_err("show block must return String");
        let literal_start = source.find("123").expect("literal should exist in source");
        assert!(err
            .message
            .contains("deferror show block must return String"));
        assert_eq!(err.span.start, literal_start);
    }
}
