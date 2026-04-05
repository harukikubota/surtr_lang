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
    use spire::{EntryPoint, SetExitCodePolicy, SourceRules};

    const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../lib/bootstrap.srt");

    fn resolve_with_builtin_prelude(source: &str) -> Vec<sigil::resolved::Resolved> {
        let mut ast = spire::parse(BUILTIN_PRELUDE_SOURCE).expect("builtin prelude should parse");
        let mut user_ast = spire::parse(source).expect("source should parse");
        ast.append(&mut user_ast);
        sigil::resolve(ast).expect("source should resolve")
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
}
