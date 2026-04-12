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

        // Match the real xldr lowering strategy used by integration tests:
        // keep normal top-level std declarations in the global declaration
        // layer, but attach `Result` constructor contracts to the sole `defmod`
        // when present so Scar sees `Result::Ok` / `Result::Err`.
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
        std_module_stages_with_overrides(&[])
    }

    fn std_module_stages_with_overrides(
        overrides: &[(&str, &str)],
    ) -> Vec<Vec<sigil::StagedModuleAst>> {
        vec![
            parse_std_module_stage(BUILTIN_PRELUDE_SOURCE, "Bootstrap"),
            [
                (
                    "Kernel",
                    pick_override("Kernel", KERNEL_PRELUDE_SOURCE, overrides),
                ),
                ("Int", pick_override("Int", INT_MODULE_SOURCE, overrides)),
                (
                    "String",
                    pick_override("String", STRING_MODULE_SOURCE, overrides),
                ),
                (
                    "Boolean",
                    pick_override("Boolean", BOOLEAN_MODULE_SOURCE, overrides),
                ),
                (
                    "Error",
                    pick_override("Error", ERROR_MODULE_SOURCE, overrides),
                ),
                ("List", pick_override("List", LIST_MODULE_SOURCE, overrides)),
                (
                    "Result",
                    pick_override("Result", RESULT_MODULE_SOURCE, overrides),
                ),
                (
                    "Float",
                    pick_override("Float", FLOAT_MODULE_SOURCE, overrides),
                ),
            ]
            .into_iter()
            .flat_map(|(name, source)| parse_std_module_stage(source, name))
            .collect(),
        ]
    }

    fn resolve_with_builtin_prelude_result(
        source: &str,
    ) -> Result<Vec<sigil::resolved::Resolved>, sigil::error::ResolveError> {
        let module_stages = std_module_stages();
        let user_ast = spire::parse_with_context(source, spire::ParserContext::project(0))
            .expect("source should parse");
        let declaration_index = sigil::precollect_declaration_index(&module_stages)
            .expect("std modules should precollect");
        sigil::resolve_staged_program(&module_stages, user_ast, &declaration_index, None)
    }

    fn resolve_with_builtin_prelude(source: &str) -> Vec<sigil::resolved::Resolved> {
        resolve_with_builtin_prelude_result(source).expect("source should resolve")
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
        typecheck_with_context(
            resolved,
            TypecheckContext {
                source_rules,
                enforce_builtin_type_contracts: false,
            },
        )
    }

    fn typecheck_std_modules_with_overrides(
        overrides: &[(&str, &str)],
    ) -> Result<Vec<TypedNode>, crate::error::TypeError> {
        let module_stages = std_module_stages_with_overrides(overrides);
        let declaration_index = sigil::precollect_declaration_index(&module_stages)
            .expect("std modules should precollect");
        let resolved =
            sigil::resolve_staged_program(&module_stages, Vec::new(), &declaration_index, None)
                .expect("std modules should resolve");
        typecheck_with_context(
            resolved,
            TypecheckContext {
                source_rules: SourceRules::std_module(),
                enforce_builtin_type_contracts: true,
            },
        )
    }

    fn pick_override<'a>(
        name: &str,
        default_source: &'a str,
        overrides: &[(&str, &'a str)],
    ) -> &'a str {
        overrides
            .iter()
            .find(|(override_name, _)| *override_name == name)
            .map(|(_, source)| *source)
            .unwrap_or(default_source)
    }

    #[test]
    fn field_access_is_resolved_to_numeric_index() {
        let resolved = resolve_with_builtin_prelude(
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
    fn safebind_total_pattern_accepts_plain_rhs() {
        let resolved = resolve_with_builtin_prelude("num =? 10");
        let typed = typecheck(resolved).expect("typecheck should succeed");
        assert!(matches!(
            typed.last().map(|node| &node.node),
            Some(TypedInner::SafeBind(_, _))
        ));
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
    fn safebind_string_pattern_accepts_plain_string_rhs() {
        let resolved = resolve_with_builtin_prelude(
            r#"value = "source"
[head, ..tail] =? value"#,
        );
        let typed = typecheck(resolved).expect("typecheck should succeed");
        assert!(matches!(
            typed.last().map(|node| &node.node),
            Some(TypedInner::SafeBind(_, _))
        ));
    }

    #[test]
    fn match_string_requires_empty_and_uncons_arms_for_exhaustiveness() {
        let resolved = resolve_with_builtin_prelude(
            r#"value = "x"
print(match value {
  [head, ..tail] => head,
})"#,
        );

        let err = typecheck(resolved).expect_err("typecheck should fail");
        assert!(err.message.contains("Non-exhaustive match. Missing: []"));
    }

    #[test]
    fn match_string_accepts_empty_and_uncons_arms() {
        let resolved = resolve_with_builtin_prelude(
            r#"value = "x"
print(match value {
  [] => "empty",
  [head, ..tail] => tail,
})"#,
        );
        let typed = typecheck(resolved).expect("typecheck should succeed");
        assert!(!typed.is_empty());
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
    fn tuple_literal_and_field_access_typecheck() {
        let resolved = resolve_with_builtin_prelude(
            r#"pair = (1, "two")
first = pair.0
second = pair.1"#,
        );
        let typed = typecheck(resolved).expect("tuple access should typecheck");
        assert!(typed.iter().filter(|node| matches!(node.node, TypedInner::Bind(_, _))).count() >= 3);
    }

    #[test]
    fn tuple_bind_pattern_typechecks() {
        let resolved = resolve_with_builtin_prelude(
            r#"pair = (1, "two")
(left, right) = pair"#,
        );
        let typed = typecheck(resolved).expect("tuple bind should typecheck");
        assert!(matches!(
            typed.last().map(|node| &node.node),
            Some(TypedInner::Bind(_, _))
        ));
    }

    #[test]
    fn extractor_single_value_match_result_contract_typechecks() {
        let resolved = resolve_with_builtin_prelude(
            r#"defstruct Single {
  value: Int,
}
impl Single {
  def new(value: Int) -> Self {
    Single { value: value }
  }

  def deconstruct(self: Self) -> MatchResult<Int, Error> {
    MatchResult::Success(self.value)
  }
}

value = Single(1)
print(match value {
  Single(inner) => to_string(inner),
  _ => "bad",
})"#,
        );
        let typed = typecheck(resolved).expect("single-value extractor should typecheck");
        assert!(!typed.is_empty());
    }

    #[test]
    fn struct_matchblock_head_uses_attached_deconstruct_method() {
        let resolved = resolve_with_builtin_prelude(
            r#"defstruct User {
  name: String,
  age: Int,
}
impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }
  def deconstruct(self: Self) -> MatchResult<(String, Int), Error> {
    MatchResult::NoMatch
  }
}
user = User("alice", 30)
print(match user {
  User(name, age) => "bad",
  _ => "fallback",
})"#,
        );
        let typed = typecheck(resolved).expect("typecheck should succeed");
        assert!(!typed.is_empty());
    }

    #[test]
    fn struct_matchblock_head_requires_attached_deconstruct_method() {
        let err = resolve_with_builtin_prelude_result(
            r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
    User { name: name }
  }
}
user = User("alice")
print(match user {
  User(name) => name,
  _ => "fallback",
})"#,
        )
        .expect_err("resolve should fail");
        assert!(err.message.contains(
            "MatchBlock head `User` requires attached extractor `User::deconstruct`, but it is not defined"
        ));
    }

    #[test]
    fn forward_struct_type_annotation_and_literal_are_allowed() {
        let resolved = resolve_with_builtin_prelude(
            r#"user: User = User("alice", 30)
defstruct User {
  name: String,
  age: Int,
}
impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }
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
        let source = r#"user: User = User("alice", 30)
pair = Pair(first: 1, second: "two")
ret: Result<Int> = Err(NotFound("404"))

defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }
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
        assert!(ok
            .iter()
            .find(|node| matches!(node.node, TypedInner::Def(_, _, _, _, _)))
            .is_some());

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
    fn assert_special_form_typechecks_to_result_unit() {
        let typed = typecheck_with_builtin_prelude("guard = assert(True, NoneError)");
        let bind = typed.last().expect("binding should exist");
        match &bind.node {
            TypedInner::Bind(_, rhs) => {
                assert!(matches!(rhs.node, TypedInner::Assert(_, _)));
                assert!(matches!(
                    rhs.ty,
                    crate::types::Ty::Result(ref ok, ref err)
                        if matches!(ok.as_ref(), crate::types::Ty::Unit)
                            && matches!(err.as_ref(), crate::types::Ty::Error)
                ));
            }
            other => panic!("expected bind, got {:?}", other),
        }
    }

    #[test]
    fn bitwidth_zero_arg_variant_reference_reuses_std_enum_constructor_uid() {
        let resolved = resolve_with_builtin_prelude("width = BitWidth::W8");

        let use_uid = match resolved
            .last()
            .expect("user bind should be present after std modules")
        {
            sigil::resolved::Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                sigil::resolved::Resolved::ConstructorCall(_, id, args) => {
                    assert!(args.is_empty(), "W8 should be zero-arg");
                    id.unique_id
                }
                other => panic!("expected zero-arg constructor call, got {other:?}"),
            },
            other => panic!("expected user bind, got {other:?}"),
        };

        let variant_uid = resolved
            .iter()
            .find_map(|node| match node {
                sigil::resolved::Resolved::EnumDef(_, id, _, variants) if id.name == "BitWidth" => {
                    variants
                        .iter()
                        .find(|variant| variant.id.name == "BitWidth::W8")
                        .map(|variant| variant.id.unique_id)
                }
                _ => None,
            })
            .expect("BitWidth::W8 variant should exist");

        assert_eq!(use_uid, variant_uid);

        let colliding_defs = resolved
            .iter()
            .filter_map(|node| match node {
                sigil::resolved::Resolved::BuiltinDecl(_, id, _, _, _)
                    if id.unique_id == use_uid =>
                {
                    Some(format!("builtin {}", id.name))
                }
                sigil::resolved::Resolved::Def(_, id, _, _, _, _) if id.unique_id == use_uid => {
                    Some(format!("def {}", id.name))
                }
                sigil::resolved::Resolved::ExtractorDef(_, id, _, _, _, _)
                    if id.unique_id == use_uid =>
                {
                    Some(format!("extractor {}", id.name))
                }
                sigil::resolved::Resolved::StructDef(_, id, _) if id.unique_id == use_uid => {
                    Some(format!("struct {}", id.name))
                }
                sigil::resolved::Resolved::RecordDef(_, id, _) if id.unique_id == use_uid => {
                    Some(format!("record {}", id.name))
                }
                sigil::resolved::Resolved::DeferrorDef(_, id, _, _) if id.unique_id == use_uid => {
                    Some(format!("deferror {}", id.name))
                }
                sigil::resolved::Resolved::EnumDef(_, _, _, variants) => variants
                    .iter()
                    .find(|variant| variant.id.unique_id == use_uid)
                    .map(|variant| format!("enum variant {}", variant.id.name)),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            colliding_defs,
            vec!["enum variant BitWidth::W8".to_string()],
            "unexpected declarations sharing uid {use_uid}: {colliding_defs:?}"
        );
    }

    #[test]
    fn bitwidth_zero_arg_variant_typechecks_with_builtin_prelude() {
        let typed = typecheck_with_builtin_prelude("width = BitWidth::W8");
        assert!(matches!(
            typed.last().expect("user bind should be present").node,
            TypedInner::Bind(_, _)
        ));
    }

    #[test]
    fn ensure_special_form_typechecks_to_result_value() {
        let typed = typecheck_with_builtin_prelude(
            r#"def is_even(n: Int) -> Boolean { Int::is_even(n) }
guard = ensure(4, &is_even, NoneError)"#,
        );
        let bind = typed.last().expect("binding should exist");
        match &bind.node {
            TypedInner::Bind(_, rhs) => {
                assert!(matches!(rhs.node, TypedInner::Ensure(_, _, _)));
            }
            other => panic!("expected bind, got {:?}", other),
        }
    }

    #[test]
    fn and_special_form_typechecks_to_boolean_if() {
        let typed = typecheck_with_builtin_prelude("flag = and(True, False)");
        let bind = typed.last().expect("binding should exist");
        match &bind.node {
            TypedInner::Bind(_, rhs) => {
                assert!(matches!(rhs.node, TypedInner::If(_, _, Some(_))));
                assert!(matches!(rhs.ty, crate::types::Ty::Bool));
            }
            other => panic!("expected bind, got {:?}", other),
        }
    }

    #[test]
    fn eq_special_form_typechecks_as_binop() {
        let typed = typecheck_with_builtin_prelude("flag = eq(1, 1)");
        let bind = typed.last().expect("binding should exist");
        match &bind.node {
            TypedInner::Bind(_, rhs) => {
                assert!(matches!(
                    rhs.node,
                    TypedInner::BinOp(spire::ast::BinOp::Eq, _, _)
                ));
                assert!(matches!(rhs.ty, crate::types::Ty::Bool));
            }
            other => panic!("expected bind, got {:?}", other),
        }
    }

    #[test]
    fn lt_special_form_typechecks_as_binop() {
        let typed = typecheck_with_builtin_prelude("flag = lt(1, 2)");
        let bind = typed.last().expect("binding should exist");
        match &bind.node {
            TypedInner::Bind(_, rhs) => {
                assert!(matches!(
                    rhs.node,
                    TypedInner::BinOp(spire::ast::BinOp::Lt, _, _)
                ));
                assert!(matches!(rhs.ty, crate::types::Ty::Bool));
            }
            other => panic!("expected bind, got {:?}", other),
        }
    }

    #[test]
    fn concat_special_form_typechecks_as_binop() {
        let typed = typecheck_with_builtin_prelude(r#"value = concat("a", "b")"#);
        let bind = typed.last().expect("binding should exist");
        match &bind.node {
            TypedInner::Bind(_, rhs) => {
                assert!(matches!(
                    rhs.node,
                    TypedInner::BinOp(spire::ast::BinOp::Concat, _, _)
                ));
                assert!(matches!(rhs.ty, crate::types::Ty::Str));
            }
            other => panic!("expected bind, got {:?}", other),
        }
    }

    #[test]
    fn ensure_rejects_call_expression_predicate() {
        let err = typecheck_with_rules(
            r#"def is_even() -> (Int -> Boolean) { {|n| Int::is_even(n) } }
guard = ensure(4, is_even(), NoneError)"#,
            SourceRules::script(),
        )
        .expect_err("call expression predicate must fail");
        assert!(err.message.contains("ensure requires a closure or capture"));
    }

    #[test]
    fn assert_rejects_non_concrete_error_expression() {
        let err = typecheck_with_rules(
            r#"deferror SomeError(detail: String) { detail }
deferror OtherError(detail: String) { detail }

def make_error(flag: Boolean) -> Error {
  if(flag, SomeError("left"), OtherError("right"))
}

guard = assert(False, make_error(True))"#,
            SourceRules::script(),
        )
        .expect_err("plain Error expression must fail");
        assert!(err
            .message
            .contains("assert error branch must be a concrete deferror value"));
    }

    #[test]
    fn kernel_and_contract_rejects_lazy_signature() {
        let err = typecheck_std_modules_with_overrides(&[(
            "Kernel",
            r#"@@builtin type Unit

defmod Kernel {
  @@builtin def and(left: Boolean, right: (-> Boolean)) -> Boolean
}"#,
        )])
        .expect_err("lazy signature should violate canonical contract");
        assert!(err
            .message
            .contains("@@builtin def and(left: Boolean, right: Boolean) -> Boolean"));
    }

    #[test]
    fn special_form_builtin_decl_must_live_under_kernel() {
        let err = typecheck_std_modules_with_overrides(&[(
            "Boolean",
            r#"@@builtin type Boolean

defmod Boolean {
  @@builtin def and(left: Boolean, right: Boolean) -> Boolean
}"#,
        )])
        .expect_err("special-form declaration outside Kernel must fail");
        assert!(err
            .message
            .contains("Special-form declaration `and` is only allowed in std module `Kernel`."));
    }

    #[test]
    fn kernel_concat_contract_rejects_generic_signature() {
        let err = typecheck_std_modules_with_overrides(&[(
            "Kernel",
            r#"@@builtin type Unit

defmod Kernel {
  @@builtin def concat(left: $A, right: $A) -> String
}"#,
        )])
        .expect_err("generic concat signature should violate canonical contract");
        assert!(err
            .message
            .contains("@@builtin def concat(left: String, right: String) -> String"));
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
    fn generic_def_signature_instantiates_per_call_site() {
        let typed = typecheck_with_builtin_prelude(
            r#"def id(x: $A) -> $A { x }
left: Int = id(1)
right: String = id("ok")"#,
        );
        assert!(typed.len() >= 3);
        assert!(typed.iter().rev().take(3).all(|node| matches!(
            node.node,
            TypedInner::Bind(_, _) | TypedInner::Def(_, _, _, _, _)
        )));
    }

    #[test]
    fn generic_defenum_constructor_and_match_typecheck() {
        let typed = typecheck_with_builtin_prelude(
            r#"defenum StepSignal<$A> {
  Resume($A),
  Stop($A),
}

step: StepSignal<Int> = StepSignal::Resume(1)
value = match step {
  StepSignal::Resume(v) => v,
  StepSignal::Stop(v) => v,
}"#,
        );
        assert!(typed
            .iter()
            .any(|node| matches!(node.node, TypedInner::EnumDef(_, _))));
        assert!(matches!(
            typed.last().map(|node| &node.node),
            Some(TypedInner::Bind(_, _))
        ));
    }

    #[test]
    fn closure_param_annotation_without_expected_type_constrains_calls() {
        let resolved = resolve_with_builtin_prelude(
            r#"id = {|value: Int| value}
answer = id("oops")"#,
        );
        let err = typecheck(resolved).expect_err("annotation should reject String call");
        assert!(err.message.contains("expected Int, got String"));
    }

    #[test]
    fn closure_param_annotation_must_match_expected_signature() {
        let resolved =
            resolve_with_builtin_prelude(r#"id: (String -> String) = {|value: Int| value}"#);
        let err = typecheck(resolved).expect_err("mismatched expected signature must fail");
        assert!(err
            .message
            .contains("closure parameter `value` expected String, got Int"));
    }

    #[test]
    fn sibling_closures_keep_substitution_state_local() {
        let typed = typecheck_with_builtin_prelude(
            r#"int_id: (Int -> Int) = {|value| value}
str_id: (String -> String) = {|value| value}
left: Int = int_id(1)
right: String = str_id("ok")"#,
        );
        assert!(typed.len() >= 4);
        assert!(typed
            .iter()
            .rev()
            .take(4)
            .all(|node| matches!(node.node, TypedInner::Bind(_, _))));
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
    fn enum_cycle_is_allowed_when_not_shared_by_all_variants() {
        let resolved = resolve_with_builtin_prelude(
            r#"defenum Loop {
  End,
  Next(Loop),
}
value: Loop = Loop::End"#,
        );
        let typed = typecheck(resolved).expect("enum should allow conditional recursion");
        assert!(typed
            .iter()
            .any(|node| matches!(node.node, TypedInner::EnumDef(_, _))));
    }

    #[test]
    fn enum_cycle_is_rejected_when_shared_by_all_variants() {
        let resolved = resolve_with_builtin_prelude(
            r#"defenum Loop {
  A(Loop),
  B(Loop),
}"#,
        );
        let err = typecheck(resolved).expect_err("enum cycle must fail");
        assert!(err.message.contains("Cyclic type definition detected"));
    }

    #[test]
    fn enum_field_access_is_rejected() {
        let resolved = resolve_with_builtin_prelude(
            r#"defenum Direction {
  Up,
  Down,
}
up: Direction = Direction::Up
x = up.idx"#,
        );
        let err = typecheck(resolved).expect_err("enum field access must fail");
        assert!(err.message.contains("Cannot access field on Direction"));
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
    fn match_tuple_binding_pattern_is_treated_as_exhaustive() {
        let resolved = resolve_with_builtin_prelude(
            r#"pair = (1, "two")
answer = match pair {
  (left, right) => right,
}"#,
        );
        let typed = typecheck(resolved).expect("tuple binding arm should be exhaustive");
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
impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age, extra: 1 }
  }
}
user = User("alice", 20)"#,
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
    fn struct_requires_impl_new() {
        let resolved = resolve_with_builtin_prelude(
            r#"defstruct User {
  name: String,
}
user = User("alice")"#,
        );
        let err = typecheck(resolved).expect_err("struct without new should fail");
        assert!(err.message.contains("must define `new` in its impl block"));
    }

    #[test]
    fn struct_literal_is_rejected_outside_impl_body() {
        let resolved = resolve_with_builtin_prelude(
            r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
    User { name: name }
  }
}
user = User { name: "alice" }"#,
        );
        let err = typecheck(resolved).expect_err("struct literal outside impl should fail");
        assert!(err
            .message
            .contains("Struct literal `User` is only allowed inside"));
    }

    #[test]
    fn user_function_call_rejects_mixed_named_and_positional_args() {
        let resolved = resolve_with_builtin_prelude(
            r#"def add3(x: Int, y: Int, z: Int) -> Int { x + y + z }
value = add3(1, y: 2, z: 3)"#,
        );
        let err = typecheck(resolved).expect_err("mixed args should fail");
        assert!(err
            .message
            .contains("Cannot mix positional and named arguments"));
    }

    #[test]
    fn impl_self_rebinding_allows_self_type() {
        let resolved = resolve_with_builtin_prelude(
            r#"defstruct User {
  name: String,
}

impl User {
  def new(name: String) -> Self {
    User { name: name }
  }

  def keep(self) -> Self {
    self = self
    self
  }
}

user = User("alice")
print(to_string(User::keep(user).name))"#,
        );
        let _typed = typecheck(resolved).expect("self rebinding with Self should pass");
    }

    #[test]
    fn impl_self_rebinding_rejects_non_self_type() {
        let resolved = resolve_with_builtin_prelude(
            r#"defstruct User {
  name: String,
}

impl User {
  def new(name: String) -> Self {
    User { name: name }
  }

  def bad(self) -> Self {
    self = 1
    self
  }
}"#,
        );
        let err = typecheck(resolved).expect_err("self rebinding with non-Self must fail");
        assert!(err.message.contains("`self` rebinding requires Self type"));
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
