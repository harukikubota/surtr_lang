use super::context::ParseUnitKind;
use super::*;
use sindr::primitives::int;

#[test]
fn test_bind_and_var() {
    let ast = parse("x = 42").unwrap();
    assert_eq!(ast.len(), 1);
    match &ast[0] {
        Ast::Bind(_, AstPattern::Var(_, name), rhs) => {
            assert_eq!(name, "x");
            assert!(matches!(rhs.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(42)));
        }
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_annotated_bind() {
    let ast = parse("num: Int = 10").unwrap();
    match &ast[0] {
        Ast::Bind(_, AstPattern::Annotated(_, name, AstTy::Named(_, ty)), _) => {
            assert_eq!(name, "num");
            assert_eq!(ty, "Int");
        }
        _ => panic!("Expected annotated Bind"),
    }
}

#[test]
fn test_defp_marks_definition_private() {
    let ast = parse("defp helper() -> String { \"ok\" }").unwrap();
    match &ast[0] {
        Ast::Def(_, name, _, _, _, _, attrs) => {
            assert_eq!(name, "helper");
            assert_eq!(attrs.visibility, Visibility::Private);
        }
        _ => panic!("Expected Def"),
    }
}

#[test]
fn test_const_definition_surface() {
    let ast = parse("public const APP_NAME: String = \"surtr\"").unwrap();
    match &ast[0] {
        Ast::ConstDef(_, name, Some(AstTy::Named(_, ty)), rhs, attrs) => {
            assert_eq!(name, "APP_NAME");
            assert_eq!(ty, "String");
            assert_eq!(attrs.visibility, Visibility::Public);
            assert!(matches!(rhs.as_ref(), Ast::Lit(_, Lit::Str(value)) if value == "surtr"));
        }
        _ => panic!("Expected ConstDef"),
    }
}

#[test]
fn test_const_name_requires_cap_pattern() {
    let err = parse("const app_name = \"surtr\"").expect_err("expected parse error");
    assert!(err.message().contains("const name must match CAP_PATTERN"));
}

#[test]
fn test_private_field_modifier_is_preserved() {
    let ast = parse_with_context(
        "defstruct User { private password: String, name: String }",
        ParserContext::project(0),
    )
    .unwrap();
    match &ast[0] {
        Ast::StructDef(_, name, fields) => {
            assert_eq!(name, "User");
            assert_eq!(fields[0].name, "password");
            assert_eq!(fields[0].visibility, Visibility::Private);
            assert_eq!(fields[1].name, "name");
            assert_eq!(fields[1].visibility, Visibility::Public);
        }
        _ => panic!("Expected StructDef"),
    }
}

#[test]
fn test_safebind() {
    let ast = parse("num =? gen()").unwrap();
    match &ast[0] {
        Ast::SafeBind(_, AstPattern::Var(_, name), rhs) => {
            assert_eq!(name, "num");
            assert!(matches!(rhs.as_ref(), Ast::App(_, _, _)));
        }
        _ => panic!("Expected SafeBind"),
    }
}

#[test]
fn test_assignment_operator_is_non_associative() {
    let err = parse("x = y =? z").expect_err("Expected parse error");
    assert!(err.message().contains("non-associative"));
}

#[test]
fn test_function_call() {
    let ast = parse("print(to_string(num))").unwrap();
    match &ast[0] {
        Ast::App(_, func, args) => {
            assert!(matches!(func.as_ref(), Ast::Var(_, ref n) if n == "print"));
            assert_eq!(args.len(), 1);
            assert!(matches!(args[0], RecordLitArg::Positional(_)));
        }
        _ => panic!("Expected App"),
    }
}

#[test]
fn test_function_call_named_args() {
    let ast = parse("add(y: 2, x: 1)").unwrap();
    match &ast[0] {
        Ast::App(_, func, args) => {
            assert!(matches!(func.as_ref(), Ast::Var(_, ref n) if n == "add"));
            assert_eq!(args.len(), 2);
            assert!(matches!(&args[0], RecordLitArg::Named(n, _) if n == "y"));
            assert!(matches!(&args[1], RecordLitArg::Named(n, _) if n == "x"));
        }
        _ => panic!("Expected App"),
    }
}

#[test]
fn test_function_call_accepts_trailing_block_arg() {
    let ast = parse("if_then(True) { num = 10; num }").expect("trailing block call should parse");
    match &ast[0] {
        Ast::App(_, func, args) => {
            assert!(matches!(func.as_ref(), Ast::Var(_, ref n) if n == "if_then"));
            assert_eq!(args.len(), 2);
            assert!(matches!(
                &args[0],
                RecordLitArg::Positional(Ast::Lit(_, Lit::Bool(true)))
            ));
            assert!(matches!(
                &args[1],
                RecordLitArg::Positional(Ast::Closure(_, params, body))
                    if params.is_empty()
                    && matches!(
                        body.as_ref(),
                        Ast::Block(_, stmts)
                            if matches!(stmts.as_slice(), [Ast::Semi(_, _), Ast::Var(_, name)] if name == "num")
                    )
            ));
        }
        _ => panic!("Expected App"),
    }
}

#[test]
fn test_zero_arg_call_accepts_trailing_block_arg() {
    let ast = parse("run() { print(\"x\") }").expect("zero-arg trailing block call should parse");
    match &ast[0] {
        Ast::App(_, func, args) => {
            assert!(matches!(func.as_ref(), Ast::Var(_, ref n) if n == "run"));
            assert_eq!(args.len(), 1);
            assert!(matches!(
                &args[0],
                RecordLitArg::Positional(Ast::Closure(_, params, body))
                    if params.is_empty()
                    && matches!(body.as_ref(), Ast::App(_, _, inner_args) if inner_args.len() == 1)
            ));
        }
        _ => panic!("Expected App"),
    }
}

#[test]
fn test_brace_expression_is_zero_arg_closure() {
    let ast = parse("x = { tmp = 10; tmp * 10 }").expect("brace expression should parse");
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Closure(_, params, body) => {
                assert!(params.is_empty());
                assert!(matches!(
                    body.as_ref(),
                    Ast::Block(_, stmts)
                        if matches!(stmts.as_slice(), [Ast::Semi(_, _), Ast::BinOp(_, BinOp::Mul, _, _)])
                ));
            }
            other => panic!("Expected zero-arg Closure, got {:?}", other),
        },
        other => panic!("Expected Bind, got {:?}", other),
    }
}

#[test]
fn test_test_dsl_trailing_block_uses_zero_arg_closure() {
    let ast = parse(r#"test("suite") { it("case") { assert_true(True) } }"#)
        .expect("test DSL trailing block should parse");
    match &ast[0] {
        Ast::App(_, func, args) => {
            assert!(matches!(func.as_ref(), Ast::Var(_, ref n) if n == "test"));
            assert_eq!(args.len(), 2);
            assert!(matches!(
                &args[0],
                RecordLitArg::Positional(Ast::Lit(_, Lit::Str(value))) if value == "suite"
            ));
            assert!(matches!(
                &args[1],
                RecordLitArg::Positional(Ast::Closure(_, params, _)) if params.is_empty()
            ));
        }
        _ => panic!("Expected App"),
    }
}

#[test]
fn test_trailing_block_rejects_named_args() {
    let err = parse("add(x: 1) { 2 }").expect_err("named args with trailing block should fail");
    assert!(err
        .message()
        .contains("Trailing block sugar cannot follow named arguments"));
}

#[test]
fn test_constructor_call_rejects_trailing_block_arg() {
    let err = parse("Foo(1) { 2 }").expect_err("constructor call with trailing block should fail");
    assert!(err
        .message()
        .contains("Trailing block sugar is not supported for constructor calls"));

    let err = parse("Foo() { 2 }")
        .expect_err("zero-arg constructor call with trailing block should fail");
    assert!(err
        .message()
        .contains("Trailing block sugar is not supported for constructor calls"));
}

#[test]
fn test_match_scrutinee_does_not_consume_arm_block_as_trailing_call_arg() {
    let ast = parse("match noop() { _ => 1 }").expect("match scrutinee should parse");
    match &ast[0] {
        Ast::Match(_, scrutinee, arms) => {
            assert!(matches!(scrutinee.as_ref(), Ast::App(_, _, args) if args.is_empty()));
            assert_eq!(arms.len(), 1);
        }
        _ => panic!("Expected Match"),
    }
}

#[test]
fn test_zero_arg_call() {
    let ast = parse("x = noop()").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::App(_, func, args) => {
                assert!(matches!(func.as_ref(), Ast::Var(_, ref n) if n == "noop"));
                assert!(args.is_empty());
            }
            _ => panic!("Expected zero-arg App"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_function_def() {
    let ast = parse(
        r#"def add(x: Int, y: Int) -> Int { x + y }
def noop() {()}"#,
    )
    .unwrap();
    match &ast[0] {
        Ast::Def(_, name, _, params, ret_ty, body, attrs) => {
            assert_eq!(name, "add");
            assert_eq!(params.len(), 2);
            assert_eq!(attrs, &DeclAttrs::default());
            assert!(matches!(ret_ty, Some(AstTy::Named(_, ty)) if ty == "Int"));
            assert!(
                matches!(body.as_ref(), Ast::Block(_, stmts) if matches!(stmts.as_slice(), [Ast::BinOp(_, BinOp::Add, _, _)]))
            );
        }
        _ => panic!("Expected Def"),
    }
    match &ast[1] {
        Ast::Def(_, name, _, params, ret_ty, body, attrs) => {
            assert_eq!(name, "noop");
            assert_eq!(params.len(), 0);
            assert_eq!(attrs, &DeclAttrs::default());
            assert!(ret_ty.is_none());
            assert!(
                matches!(body.as_ref(), Ast::Block(_, stmts) if matches!(stmts.as_slice(), [Ast::Lit(_, Lit::Unit)]))
            );
        }
        _ => panic!("Expected Def"),
    }
}

#[test]
fn test_impl_parses_and_keeps_methods() {
    let ast = parse_with_context(
        r#"defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }

  def normalize(self) -> Self {
    self
  }
}"#,
        ParserContext::module(1, None),
    )
    .expect("impl should parse");

    let impl_node = ast
        .iter()
        .find(|node| matches!(node, Ast::ImplDef(_, _, _, _)))
        .expect("expected impl node");
    match impl_node {
        Ast::ImplDef(_, target, methods, attrs) => {
            assert_eq!(target, "User");
            assert_eq!(attrs, &DeclAttrs::default());
            assert_eq!(methods.len(), 2);
            assert!(matches!(
                &methods[0],
                Ast::Def(_, name, _, _, Some(AstTy::Named(_, ret)), _, _)
                    if name == "new" && ret == "Self"
            ));
            assert!(matches!(
                &methods[1],
                Ast::Def(_, name, _, _, Some(AstTy::Named(_, ret)), _, _)
                    if name == "normalize" && ret == "Self"
            ));
        }
        _ => panic!("Expected ImplDef"),
    }
}

#[test]
fn test_impl_parses_and_keeps_builtin_methods() {
    let ast = parse_with_context(
        r#"@@builtin type Int

impl Int {
  @@doc """Builtin int helper."""
  @@builtin def safe_mod(a: Int, b: Int) -> Result<Int, ZeroDivisionError>
}"#,
        ParserContext::module(1, None).with_rules(ParseRules::std_module()),
    )
    .expect("builtin impl method should parse");

    let impl_node = ast
        .iter()
        .find(|node| matches!(node, Ast::ImplDef(_, _, _, _)))
        .expect("expected impl node");
    match impl_node {
        Ast::ImplDef(_, target, methods, _) => {
            assert_eq!(target, "Int");
            assert!(matches!(
                methods.as_slice(),
                [Ast::BuiltinDecl(_, name, _, Some(AstTy::Generic(_, ret, _)), attrs)]
                    if name == "safe_mod"
                    && ret == "Result"
                    && attrs.doc.as_deref() == Some("Builtin int helper.")
            ));
        }
        _ => panic!("Expected ImplDef"),
    }
}

#[test]
fn test_trait_def_parses_method_signatures() {
    let ast = parse_with_context(
        r#"deftrait Numeric {
  def add(self: Self, rhs: Self) -> Self
  def abs(self: Self) -> Self
}"#,
        ParserContext::module(1, None),
    )
    .expect("trait should parse");

    match ast.as_slice() {
        [Ast::TraitDef(_, name, type_params, methods, attrs)] => {
            assert_eq!(name, "Numeric");
            assert_eq!(attrs, &DeclAttrs::default());
            assert!(type_params.is_empty());
            assert_eq!(methods.len(), 2);
            assert_eq!(methods[0].name, "add");
            assert_eq!(methods[0].params.len(), 2);
            assert!(methods[0].type_params.is_empty());
            assert!(matches!(methods[0].params[0].ty, AstTy::Named(_, ref ty) if ty == "Self"));
            assert!(matches!(methods[0].params[1].ty, AstTy::Named(_, ref ty) if ty == "Self"));
            assert!(matches!(methods[0].ret_ty, AstTy::Named(_, ref ty) if ty == "Self"));
            assert_eq!(methods[1].name, "abs");
            assert_eq!(methods[1].params.len(), 1);
        }
        _ => panic!("Expected TraitDef"),
    }
}

#[test]
fn test_trait_impl_parses_and_keeps_methods() {
    let ast = parse_with_context(
        r#"impl Numeric for Int {
  def add(self: Self, rhs: Self) -> Self {
    self + rhs
  }

  def abs(self: Self) -> Self {
    self
  }
}"#,
        ParserContext::module(1, None),
    )
    .expect("trait impl should parse");

    match ast.as_slice() {
        [Ast::TraitImplDef(_, trait_name, trait_args, AstTy::Named(_, target), methods, attrs)] => {
            assert_eq!(trait_name, "Numeric");
            assert!(trait_args.is_empty());
            assert_eq!(target, "Int");
            assert_eq!(attrs, &DeclAttrs::default());
            assert_eq!(methods.len(), 2);
            assert!(matches!(
                &methods[0],
                Ast::Def(_, name, _, _, Some(AstTy::Named(_, ret)), _, _)
                    if name == "add" && ret == "Self"
            ));
            assert!(matches!(
                &methods[1],
                Ast::Def(_, name, _, _, Some(AstTy::Named(_, ret)), _, _)
                    if name == "abs" && ret == "Self"
            ));
        }
        _ => panic!("Expected TraitImplDef"),
    }
}

#[test]
fn test_trait_impl_accepts_builtin_def_method() {
    let ast = parse_with_context(
        r#"impl Add for Int {
  @@builtin def add(self: Self, rhs: Self) -> Self
}"#,
        ParserContext::module(1, None).with_rules(ParseRules::std_module()),
    )
    .expect("trait impl builtin method should parse");

    match ast.as_slice() {
        [Ast::TraitImplDef(_, trait_name, _, AstTy::Named(_, target), methods, _)] => {
            assert_eq!(trait_name, "Add");
            assert_eq!(target, "Int");
            assert!(matches!(
                methods.as_slice(),
                [Ast::BuiltinDecl(_, name, _, Some(AstTy::Named(_, ret)), _)]
                    if name == "add" && ret == "Self"
            ));
        }
        _ => panic!("Expected TraitImplDef"),
    }
}

#[test]
fn test_trait_impl_rejects_builtin_defp_method() {
    let err = parse_with_context(
        r#"impl Add for Int {
  @@builtin defp add(self: Self, rhs: Self) -> Self
}"#,
        ParserContext::module(1, None).with_rules(ParseRules::std_module()),
    )
    .expect_err("trait impl builtin private method should be rejected");
    assert!(err
        .message()
        .contains("@@builtin is not allowed before `defp` impl members"));
}

#[test]
fn test_doc_attributes_parse_for_trait_and_impl_decls() {
    let ast = parse_with_context(
        r#"@@doc """Trait docs."""
deftrait Numeric {
  def add(self: Self, rhs: Self) -> Self
}

@@doc """Impl docs."""
impl User {
  def new(name: String) -> Self {
    User { name: name }
  }
}"#,
        ParserContext::module(1, None),
    )
    .expect("annotated trait and impl should parse");

    match &ast[0] {
        Ast::TraitDef(_, name, _, _, attrs) => {
            assert_eq!(name, "Numeric");
            assert_eq!(attrs.doc.as_deref(), Some("Trait docs."));
        }
        _ => panic!("Expected TraitDef"),
    }

    match &ast[1] {
        Ast::ImplDef(_, target, _, attrs) => {
            assert_eq!(target, "User");
            assert_eq!(attrs.doc.as_deref(), Some("Impl docs."));
        }
        _ => panic!("Expected ImplDef"),
    }
}

#[test]
fn test_doc_attributes_parse_for_impl_methods() {
    let ast = parse_with_context(
        r#"defstruct User {
  name: String,
}

impl User {
  @@doc """Construct a user."""
  def new(name: String) -> Self {
    User { name: name }
  }

  @@doc """Normalize the user."""
  def normalize(self) -> Self {
    self
  }
}"#,
        ParserContext::module(1, None),
    )
    .expect("annotated impl methods should parse");

    match &ast[1] {
        Ast::ImplDef(_, target, methods, _) => {
            assert_eq!(target, "User");
            assert_eq!(methods.len(), 2);
            match &methods[0] {
                Ast::Def(_, name, _, _, _, _, attrs) => {
                    assert_eq!(name, "new");
                    assert_eq!(attrs.doc.as_deref(), Some("Construct a user."));
                }
                _ => panic!("Expected impl def"),
            }
            match &methods[1] {
                Ast::Def(_, name, _, _, _, _, attrs) => {
                    assert_eq!(name, "normalize");
                    assert_eq!(attrs.doc.as_deref(), Some("Normalize the user."));
                }
                _ => panic!("Expected impl def"),
            }
        }
        _ => panic!("Expected ImplDef"),
    }
}

#[test]
fn test_doc_attributes_parse_for_trait_impl_methods() {
    let ast = parse_with_context(
        r#"impl Show for Int {
  @@doc """Format the integer."""
  def to_string(self: Self) -> String {
    inspect(self)
  }
}"#,
        ParserContext::module(1, None),
    )
    .expect("annotated trait impl methods should parse");

    match ast.as_slice() {
        [Ast::TraitImplDef(_, trait_name, _, AstTy::Named(_, target), methods, _)] => {
            assert_eq!(trait_name, "Show");
            assert_eq!(target, "Int");
            match &methods[0] {
                Ast::Def(_, name, _, _, _, _, attrs) => {
                    assert_eq!(name, "to_string");
                    assert_eq!(attrs.doc.as_deref(), Some("Format the integer."));
                }
                _ => panic!("Expected trait impl def"),
            }
        }
        _ => panic!("Expected TraitImplDef"),
    }
}

#[test]
fn test_function_def_parses_bounded_type_params() {
    let ast = parse("def add<$N: Numeric>(x: $N, y: $N) -> $N { x }").unwrap();

    match ast.as_slice() {
        [Ast::Def(_, name, type_params, params, Some(AstTy::Named(_, ret_ty)), _, _)] => {
            assert_eq!(name, "add");
            assert_eq!(type_params.len(), 1);
            assert_eq!(type_params[0].name, "$N");
            assert_eq!(type_params[0].bound.as_deref(), Some("Numeric"));
            assert_eq!(params.len(), 2);
            assert!(matches!(params[0].ty, AstTy::Named(_, ref ty) if ty == "$N"));
            assert!(matches!(params[1].ty, AstTy::Named(_, ref ty) if ty == "$N"));
            assert_eq!(ret_ty, "$N");
        }
        _ => panic!("Expected Def with bounded type parameters"),
    }
}

#[test]
fn test_trait_def_parses_head_type_params() {
    let ast = parse_with_context(
        r#"deftrait From<$To> {
  def from(self: Self, to: TypeRef<$To>) -> $To
}"#,
        ParserContext::module(1, None),
    )
    .expect("generic trait should parse");

    match ast.as_slice() {
        [Ast::TraitDef(_, name, type_params, methods, _)] => {
            assert_eq!(name, "From");
            assert_eq!(type_params.len(), 1);
            assert_eq!(type_params[0].name, "$To");
            assert_eq!(methods.len(), 1);
            assert!(matches!(
                methods[0].params[1].ty,
                AstTy::Generic(_, ref name, ref args)
                    if name == "TypeRef"
                        && matches!(args.as_slice(), [AstTy::Named(_, arg)] if arg == "$To")
            ));
        }
        _ => panic!("Expected TraitDef"),
    }
}

#[test]
fn test_trait_impl_parses_trait_type_args() {
    let ast = parse_with_context(
        r#"impl From<String> for Int {
  def from(self: Self, to: TypeRef<String>) -> String {
    inspect(self)
  }
}"#,
        ParserContext::module(1, None),
    )
    .expect("generic trait impl should parse");

    match ast.as_slice() {
        [Ast::TraitImplDef(_, trait_name, trait_args, AstTy::Named(_, target), methods, attrs)] => {
            assert_eq!(trait_name, "From");
            assert!(matches!(trait_args.as_slice(), [AstTy::Named(_, name)] if name == "String"));
            assert_eq!(target, "Int");
            assert_eq!(attrs, &DeclAttrs::default());
            assert_eq!(methods.len(), 1);
        }
        _ => panic!("Expected TraitImplDef"),
    }
}

#[test]
fn test_function_def_parses_parameter_position_impl_trait() {
    let ast = parse("def abs(x: impl Numeric) -> Int { 0 }").unwrap();

    match ast.as_slice() {
        [Ast::Def(_, name, type_params, params, Some(AstTy::Named(_, ret_ty)), _, _)] => {
            assert_eq!(name, "abs");
            assert!(type_params.is_empty());
            assert_eq!(params.len(), 1);
            assert!(
                matches!(params[0].ty, AstTy::ImplTrait(_, ref trait_name) if trait_name == "Numeric")
            );
            assert_eq!(ret_ty, "Int");
        }
        _ => panic!("Expected Def with impl Trait parameter"),
    }
}

#[test]
fn test_return_position_impl_trait_is_rejected() {
    let err = parse("def bad(x: Int) -> impl Numeric { x }")
        .expect_err("return-position impl Trait should be rejected");
    assert!(err
        .message()
        .contains("return-position `impl Trait` is not supported"));
}

#[test]
fn test_where_clause_is_rejected() {
    let err = parse("def add(x: Int) -> Int where Int: Numeric { x }")
        .expect_err("where clauses should be rejected");
    assert!(err
        .message()
        .contains("`where` clauses are staged and not implemented yet"));
}

#[test]
fn test_impl_rejects_self_not_first_param() {
    let err = parse_with_context(
        r#"defstruct User {
  name: String,
}

impl User {
  def bad(x: Int, self: Self) -> Self {
    self
  }
}"#,
        ParserContext::module(1, None),
    )
    .expect_err("self after first parameter must fail");
    assert!(err
        .message()
        .contains("`self` is only allowed as the first parameter of impl methods"));
}

#[test]
fn test_impl_allows_self_rebinding_syntax() {
    let ast = parse_with_context(
        r#"defstruct User {
  name: String,
}

impl User {
  def bad(self) -> Self {
    self = self
    self
  }
}"#,
        ParserContext::module(1, None),
    )
    .expect("self rebinding should be parsed");
    assert!(ast
        .iter()
        .any(|node| matches!(node, Ast::ImplDef(_, _, _, _))));
}

#[test]
fn test_defmod_rejects_self_and_self_type() {
    let err = parse(
        r#"defmod UserTools {
  def bad(self: Int) -> Int { self }
}"#,
    )
    .expect_err("defmod must reject `self`");
    assert!(err
        .message()
        .contains("`self` is only allowed as the first parameter of impl methods"));

    let err = parse(
        r#"defmod UserTools {
  def bad(x: Self) -> Int { 1 }
}"#,
    )
    .expect_err("defmod must reject `Self`");
    assert!(err
        .message()
        .contains("`Self` can only be used inside impl methods"));
}

#[test]
fn test_builtin_decl() {
    let ast = parse_with_context(
        "@@builtin def to_string(a: $A) -> String",
        ParserContext::module(1, Some("Bootstrap".into())).with_rules(ParseRules::std_module()),
    )
    .expect("std module should accept builtin declarations");
    match &ast[0] {
        Ast::BuiltinDecl(_, name, params, ret_ty, attrs) => {
            assert_eq!(name, "to_string");
            assert_eq!(params.len(), 1);
            assert_eq!(attrs, &DeclAttrs::default());
            assert!(matches!(
                params[0].ty,
                AstTy::Named(_, ref name) if name == "$A"
            ));
            assert!(matches!(ret_ty, Some(AstTy::Named(_, ty)) if ty == "String"));
        }
        _ => panic!("Expected BuiltinDecl"),
    }
}

#[test]
fn test_builtin_type_decl() {
    let ast = parse_with_context(
        "@@builtin\ntype Int",
        ParserContext::module(1, Some("Bootstrap".into())).with_rules(ParseRules::std_module()),
    )
    .expect("std module should accept builtin type declarations");
    assert!(matches!(
        ast.as_slice(),
        [Ast::BuiltinTypeDecl(_, BuiltinTypeHead { name, params, .. }, attrs)]
            if name == "Int" && params.is_empty() && attrs == &DeclAttrs::default()
    ));
}

#[test]
fn test_doc_annotates_builtin_type_decl() {
    let ast = parse_with_context(
        "@@doc \"\"\"\nBuiltin Int.\n\"\"\"\n@@builtin type Int",
        ParserContext::module(1, Some("Bootstrap".into())).with_rules(ParseRules::std_module()),
    )
    .expect("doc + builtin type should parse");

    assert!(matches!(
        ast.as_slice(),
        [Ast::BuiltinTypeDecl(_, BuiltinTypeHead { name, .. }, DeclAttrs { doc: Some(doc), .. })]
            if name == "Int" && doc == "\nBuiltin Int.\n"
    ));
}

#[test]
fn test_doc_annotates_defmod() {
    let ast = parse_with_context(
            "@@doc \"\"\"Kernel docs\"\"\"\ndefmod Kernel {\n  def add(x: Int, y: Int) -> Int { x + y }\n}",
            ParserContext::module(1, None),
        )
        .expect("doc + defmod should parse");

    assert!(matches!(
        ast.as_slice(),
        [Ast::Defmod(_, name, _, DeclAttrs { doc: Some(doc), .. })]
            if name == "Kernel" && doc == "Kernel docs"
    ));
}

#[test]
fn test_autoimport_annotates_defmod() {
    let ast = parse_with_context(
        "@@autoimport\ndefmod Kernel { def add(x: Int, y: Int) -> Int { x + y } }",
        ParserContext::module(1, None),
    )
    .expect("autoimport + defmod should parse");

    assert!(matches!(
        ast.as_slice(),
        [Ast::Defmod(_, name, _, DeclAttrs { auto_import: true, .. })]
            if name == "Kernel"
    ));
}

#[test]
fn test_doc_annotates_deferror() {
    let ast = parse_with_context(
        "@@doc \"\"\"Missing value error\"\"\"\ndeferror NoneError { \"None Value.\" }",
        ParserContext::module(1, None),
    )
    .expect("doc + deferror should parse");

    assert!(matches!(
        ast.as_slice(),
        [Ast::DeferrorDef(_, name, _, _, DeclAttrs { doc: Some(doc), .. })]
            if name == "NoneError" && doc == "Missing value error"
    ));
}

#[test]
fn test_doc_requires_following_declaration() {
    let err = parse("@@doc \"\"\"dangling\"\"\"").expect_err("expected parse error");
    assert!(err.message().contains("declaration"));
}

#[test]
fn test_builtin_type_decl_preserves_generic_head() {
    let ast = parse_with_context(
        "@@builtin type Result<$T>",
        ParserContext::module(1, Some("Bootstrap".into())).with_rules(ParseRules::std_module()),
    )
    .expect("generic builtin type should parse");
    assert!(matches!(
        ast.as_slice(),
        [Ast::BuiltinTypeDecl(_, BuiltinTypeHead { name, params, .. }, _)]
            if name == "Result" && params.as_slice() == ["$T"]
    ));
}

#[test]
fn test_std_module_result_ctor_decls_are_accepted() {
    let ast = parse_with_context(
        r#"@@doc """
Construct the success branch.
"""
def Ok($T) -> Result<$T>

@@doc """
Construct the error branch.
"""
def Err(Error) -> Result<$T>"#,
        ParserContext::module(1, None).with_rules(ParseRules::std_module()),
    )
    .expect("result constructor declarations should parse in std modules");

    assert_eq!(ast.len(), 2);
    assert!(matches!(
        &ast[0],
        Ast::ResultCtorDecl(_, name, AstTy::Named(_, param), AstTy::Generic(_, ret_name, args), DeclAttrs { doc: Some(doc), .. })
            if name == "Ok" && param == "$T" && ret_name == "Result" && args.len() == 1 && doc.contains("success")
    ));
    assert!(matches!(
        &ast[1],
        Ast::ResultCtorDecl(_, name, AstTy::Named(_, param), AstTy::Generic(_, ret_name, args), DeclAttrs { doc: Some(doc), .. })
            if name == "Err" && param == "Error" && ret_name == "Result" && args.len() == 1 && doc.contains("error")
    ));
}

#[test]
fn test_std_module_result_ctor_builtin_type_contracts_are_accepted() {
    let ast = parse_with_context(
        r#"@@doc """
Construct the success branch.
"""
@@builtin type Ok($T) -> Result<$T>

@@doc """
Construct the error branch.
"""
@@builtin type Err(Error) -> Result<$T>"#,
        ParserContext::module(1, None).with_rules(ParseRules::std_module()),
    )
    .expect("result constructor builtin contracts should parse in std modules");

    assert_eq!(ast.len(), 2);
    assert!(matches!(
        &ast[0],
        Ast::ResultCtorDecl(_, name, AstTy::Named(_, param), AstTy::Generic(_, ret_name, args), DeclAttrs { doc: Some(doc), .. })
            if name == "Ok" && param == "$T" && ret_name == "Result" && args.len() == 1 && doc.contains("success")
    ));
    assert!(matches!(
        &ast[1],
        Ast::ResultCtorDecl(_, name, AstTy::Named(_, param), AstTy::Generic(_, ret_name, args), DeclAttrs { doc: Some(doc), .. })
            if name == "Err" && param == "Error" && ret_name == "Result" && args.len() == 1 && doc.contains("error")
    ));
}

#[test]
fn test_type_keyword_cannot_be_used_as_function_name() {
    let err = parse("def type() -> Int { 0 }").expect_err("type should stay reserved");
    assert!(err.message().contains("Expected identifier"));
}

#[test]
fn test_builtin_decl_with_body_is_error() {
    let err = parse("@@builtin def print(a: String) -> Unit { print(a) }").expect_err("error");
    assert!(err.message().contains("must not have a function body"));
}

#[test]
fn test_builtin_if_decl_accepts_keyword_name_in_std_module_member() {
    let ast = parse_with_context(
        r#"defmod Kernel {
  @@builtin def if(flag: Boolean, then_branch: (-> $A), else_branch: (-> $A)) -> $A
}"#,
        ParserContext::module(1, None).with_rules(ParseRules::std_module()),
    )
    .expect("builtin if declaration should parse");

    match &ast[0] {
        Ast::Defmod(_, name, body, _) => {
            assert_eq!(name, "Kernel");
            assert!(matches!(
                &body[0],
                Ast::BuiltinDecl(_, builtin_name, params, Some(AstTy::Named(_, ret)), _)
                    if builtin_name == "if" && params.len() == 3 && ret == "$A"
            ));
        }
        other => panic!("expected defmod, got {:?}", other),
    }
}

#[test]
fn test_builtin_import_decl_accepts_keyword_name_in_std_module_member() {
    let ast = parse_with_context(
        r#"defmod Bootstrap {
  @@builtin def import() -> Unit
  @@builtin def include(path: String) -> Unit
}"#,
        ParserContext::module(1, None).with_rules(ParseRules::std_module()),
    )
    .expect("builtin import/include declarations should parse");

    match &ast[0] {
        Ast::Defmod(_, name, body, _) => {
            assert_eq!(name, "Bootstrap");
            assert!(matches!(
                &body[0],
                Ast::BuiltinDecl(_, builtin_name, params, Some(AstTy::Named(_, ret)), _)
                    if builtin_name == "import" && params.is_empty() && ret == "Unit"
            ));
            assert!(matches!(
                &body[1],
                Ast::BuiltinDecl(_, builtin_name, params, Some(AstTy::Named(_, ret)), _)
                    if builtin_name == "include" && params.len() == 1 && ret == "Unit"
            ));
        }
        other => panic!("expected defmod, got {:?}", other),
    }
}

#[test]
fn test_unknown_annotator_is_error() {
    let err = parse("@@memo def f()").expect_err("error");
    assert!(err.message().contains("Unknown annotator: @@memo"));
}

#[test]
fn test_binop() {
    let ast = parse("x = 10 + 5").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Ast::BinOp(_, BinOp::Add, _, _)));
        }
        _ => panic!("Expected Bind with BinOp"),
    }
}

#[test]
fn test_precedence() {
    // Expr-class operators are same-precedence and left-associative.
    let ast = parse("x = 1 + 2 * 3").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::BinOp(_, BinOp::Mul, left, right) => {
                assert!(matches!(right.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(3)));
                assert!(matches!(
                    left.as_ref(),
                    Ast::BinOp(_, BinOp::Add, ll, lr)
                        if matches!(ll.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(1))
                            && matches!(lr.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(2))
                ));
            }
            _ => panic!("Expected left-associative Expr-class parse at top"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_logical_precedence_is_lower_than_expr_class() {
    let ast = parse("x = a + b == c").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::BinOp(_, BinOp::Eq, left, right) => {
                assert!(matches!(right.as_ref(), Ast::Var(_, name) if name == "c"));
                assert!(matches!(
                    left.as_ref(),
                    Ast::BinOp(_, BinOp::Add, ll, lr)
                        if matches!(ll.as_ref(), Ast::Var(_, name) if name == "a")
                            && matches!(lr.as_ref(), Ast::Var(_, name) if name == "b")
                ));
            }
            other => panic!("Expected logical top-level parse, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_func_literal_name_lowers_to_binary_call() {
    let ast = parse("x = left `eq` right").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::App(_, func, args) => {
                assert!(matches!(func.as_ref(), Ast::Var(_, name) if name == "eq"));
                assert!(matches!(
                    args.as_slice(),
                    [RecordLitArg::Positional(left), RecordLitArg::Positional(right)]
                        if matches!(left, Ast::Var(_, name) if name == "left")
                            && matches!(right, Ast::Var(_, name) if name == "right")
                ));
            }
            other => panic!("Expected lowered App, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_func_literal_qualified_path_lowers_to_binary_call() {
    let ast = parse("x = left `Boolean::not_eq` right").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::App(_, func, args) => {
                assert!(matches!(
                    func.as_ref(),
                    Ast::Path(_, path)
                        if path.segments == vec!["Boolean".to_string(), "not_eq".to_string()]
                ));
                assert!(matches!(
                    args.as_slice(),
                    [RecordLitArg::Positional(left), RecordLitArg::Positional(right)]
                        if matches!(left, Ast::Var(_, name) if name == "left")
                            && matches!(right, Ast::Var(_, name) if name == "right")
                ));
            }
            other => panic!("Expected lowered App, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_func_literal_comparison_name_uses_logical_tier() {
    let ast = parse("x = a `eq` b + c").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::App(_, func, args) => {
                assert!(matches!(func.as_ref(), Ast::Var(_, name) if name == "eq"));
                assert!(matches!(
                    args.as_slice(),
                    [RecordLitArg::Positional(left), RecordLitArg::Positional(right)]
                        if matches!(left, Ast::Var(_, name) if name == "a")
                            && matches!(
                                right,
                                Ast::BinOp(_, BinOp::Add, rl, rr)
                                    if matches!(rl.as_ref(), Ast::Var(_, name) if name == "b")
                                        && matches!(rr.as_ref(), Ast::Var(_, name) if name == "c")
                            )
                ));
            }
            other => panic!(
                "Expected logical-tier comparison helper parse, got {:?}",
                other
            ),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_func_literal_and_is_lower_precedence_than_comparison_ops() {
    let ast = parse("x = 0 < num `and` num < 10").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::App(_, func, args) => {
                assert!(matches!(func.as_ref(), Ast::Var(_, name) if name == "and"));
                assert!(matches!(
                    args.as_slice(),
                    [RecordLitArg::Positional(left), RecordLitArg::Positional(right)]
                        if matches!(
                            left,
                            Ast::BinOp(_, BinOp::Lt, ll, lr)
                                if matches!(ll.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(0))
                                    && matches!(lr.as_ref(), Ast::Var(_, name) if name == "num")
                        ) && matches!(
                            right,
                            Ast::BinOp(_, BinOp::Lt, rl, rr)
                                if matches!(rl.as_ref(), Ast::Var(_, name) if name == "num")
                                    && matches!(rr.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(10))
                        )
                ));
            }
            other => panic!("Expected and(...) call, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_symbolic_and_is_lower_precedence_than_comparison_ops() {
    let ast = parse("x = 0 < num && num < 10").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::App(_, func, args) => {
                assert!(matches!(func.as_ref(), Ast::Var(_, name) if name == "and"));
                assert!(matches!(
                    args.as_slice(),
                    [RecordLitArg::Positional(left), RecordLitArg::Positional(right)]
                        if matches!(
                            left,
                            Ast::BinOp(_, BinOp::Lt, ll, lr)
                                if matches!(ll.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(0))
                                    && matches!(lr.as_ref(), Ast::Var(_, name) if name == "num")
                        ) && matches!(
                            right,
                            Ast::BinOp(_, BinOp::Lt, rl, rr)
                                if matches!(rl.as_ref(), Ast::Var(_, name) if name == "num")
                                    && matches!(rr.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(10))
                        )
                ));
            }
            other => panic!("Expected and(...) call, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_symbolic_or_lowers_to_call() {
    let ast = parse("x = left || right").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::App(_, func, args) => {
                assert!(matches!(func.as_ref(), Ast::Var(_, name) if name == "or"));
                assert!(matches!(
                    args.as_slice(),
                    [RecordLitArg::Positional(left), RecordLitArg::Positional(right)]
                        if matches!(left, Ast::Var(_, name) if name == "left")
                            && matches!(right, Ast::Var(_, name) if name == "right")
                ));
            }
            other => panic!("Expected lowered App, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_func_literal_and_or_chain_is_left_associative_with_comparisons() {
    let ast = parse("x = 0 < num `and` num < 10 `or` num == 42").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::App(_, or_func, or_args) => {
                assert!(matches!(or_func.as_ref(), Ast::Var(_, name) if name == "or"));
                assert!(matches!(
                    or_args.as_slice(),
                    [RecordLitArg::Positional(left), RecordLitArg::Positional(right)]
                        if matches!(
                            left,
                            Ast::App(_, and_func, and_args)
                                if matches!(and_func.as_ref(), Ast::Var(_, name) if name == "and")
                                    && matches!(
                                        and_args.as_slice(),
                                        [RecordLitArg::Positional(_), RecordLitArg::Positional(_)]
                                    )
                        ) && matches!(
                            right,
                            Ast::BinOp(_, BinOp::Eq, rl, rr)
                                if matches!(rl.as_ref(), Ast::Var(_, name) if name == "num")
                                    && matches!(rr.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(42))
                        )
                ));
            }
            other => panic!("Expected or(...) at top, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_func_literal_operator_lowers_to_binop() {
    let ast = parse("x = left `+` right").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(
                rhs.as_ref(),
                Ast::BinOp(_, BinOp::Add, left, right)
                    if matches!(left.as_ref(), Ast::Var(_, name) if name == "left")
                        && matches!(right.as_ref(), Ast::Var(_, name) if name == "right")
            ));
        }
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_func_literal_operator_comparison_uses_logical_tier() {
    let ast = parse("x = a `==` b + c").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::BinOp(_, BinOp::Eq, left, right) => {
                assert!(matches!(left.as_ref(), Ast::Var(_, name) if name == "a"));
                assert!(matches!(
                    right.as_ref(),
                    Ast::BinOp(_, BinOp::Add, rl, rr)
                        if matches!(rl.as_ref(), Ast::Var(_, name) if name == "b")
                            && matches!(rr.as_ref(), Ast::Var(_, name) if name == "c")
                ));
            }
            other => panic!("Expected logical-tier func literal parse, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_standalone_func_literal_is_error() {
    let err = parse("`eq`").expect_err("expected parse error");
    assert!(err
        .message()
        .contains("FuncLiteral must appear in infix position"));
}

#[test]
fn test_list_literal() {
    let ast = parse("nums = [1, 2, 3]").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::ListLiteral(_, elems) => assert_eq!(elems.len(), 3),
            _ => panic!("Expected ListLiteral"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_empty_list_with_annotation() {
    let ast = parse("empty: List<Int> = []").unwrap();
    match &ast[0] {
        Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Generic(_, name, args)), rhs) => {
            assert_eq!(name, "List");
            assert_eq!(args.len(), 1);
            assert!(matches!(args[0], AstTy::Named(_, ref n) if n == "Int"));
            assert!(matches!(rhs.as_ref(), Ast::ListNil(_)));
        }
        _ => panic!("Expected annotated Bind with empty List"),
    }
}

#[test]
fn test_list_cons_expr() {
    let ast = parse("nums = [1, ..tail]").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::ListCons(_, head, tail) => {
                assert!(matches!(head.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(1)));
                assert!(matches!(tail.as_ref(), Ast::Var(_, name) if name == "tail"));
            }
            _ => panic!("Expected ListCons"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_list_pattern_safebind() {
    let ast = parse("[head, ..tail] =? value").unwrap();
    match &ast[0] {
        Ast::SafeBind(_, pattern, rhs) => {
            assert!(matches!(
                pattern,
                AstPattern::ListCons(_, head, tail)
                    if matches!(head.as_ref(), AstPattern::Var(_, name) if name == "head")
                    && matches!(tail.as_ref(), AstPattern::Var(_, name) if name == "tail")
            ));
            assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "value"));
        }
        _ => panic!("Expected SafeBind"),
    }
}

#[test]
fn test_as_pattern_safebind_with_annotation() {
    let ast = parse("[head, ..tail] @ list_dup: List<Int> =? value").unwrap();
    match &ast[0] {
        Ast::SafeBind(_, pattern, rhs) => {
            assert!(matches!(
                pattern,
                AstPattern::As(_, inner, alias, Some(AstTy::Generic(_, name, args)))
                    if alias == "list_dup"
                    && name == "List"
                    && matches!(args.as_slice(), [AstTy::Named(_, elem)] if elem == "Int")
                    && matches!(inner.as_ref(), AstPattern::ListCons(_, _, _))
            ));
            assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "value"));
        }
        _ => panic!("Expected SafeBind"),
    }
}

#[test]
fn test_nested_as_pattern_safebind() {
    let ast = parse("[head, .. [e2, ..tail] @ tail_dup] @ list_dup =? value").unwrap();
    match &ast[0] {
        Ast::SafeBind(_, pattern, rhs) => {
            assert!(matches!(
                pattern,
                AstPattern::As(_, outer_inner, outer_alias, None)
                    if outer_alias == "list_dup"
                    && matches!(
                        outer_inner.as_ref(),
                        AstPattern::ListCons(_, _, tail_pattern)
                            if matches!(
                                tail_pattern.as_ref(),
                                AstPattern::As(_, inner_list, inner_alias, None)
                                    if inner_alias == "tail_dup"
                                    && matches!(inner_list.as_ref(), AstPattern::ListCons(_, _, _))
                            )
                    )
            ));
            assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "value"));
        }
        _ => panic!("Expected SafeBind"),
    }
}

#[test]
fn test_as_pattern_bind() {
    let ast = parse("[head, ..tail] @ list_dup = list").unwrap();
    match &ast[0] {
        Ast::Bind(_, pattern, rhs) => {
            assert!(matches!(
                pattern,
                AstPattern::As(_, inner, alias, None)
                    if alias == "list_dup"
                    && matches!(inner.as_ref(), AstPattern::ListCons(_, _, _))
            ));
            assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "list"));
        }
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_constructor_pattern_safebind() {
    let ast = parse("Ok(num) =? value").unwrap();
    match &ast[0] {
        Ast::SafeBind(_, pattern, rhs) => {
            assert!(matches!(
                pattern,
                AstPattern::Call(_, ctor, inner)
                    if ctor == "Ok"
                    && matches!(inner.as_slice(), [AstPattern::Var(_, name)] if name == "num")
            ));
            assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "value"));
        }
        _ => panic!("Expected SafeBind"),
    }
}

#[test]
fn test_wildcard_pattern_safebind() {
    let ast = parse("_ =? value").unwrap();
    match &ast[0] {
        Ast::SafeBind(_, pattern, rhs) => {
            assert!(matches!(pattern, AstPattern::Wildcard(_)));
            assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "value"));
        }
        _ => panic!("Expected SafeBind"),
    }
}

#[test]
fn test_integer_literal_pattern_safebind() {
    let ast = parse("1 =? value").unwrap();
    match &ast[0] {
        Ast::SafeBind(_, pattern, rhs) => {
            assert!(matches!(pattern, AstPattern::IntLit(_, n) if n == &int(1)));
            assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "value"));
        }
        _ => panic!("Expected SafeBind"),
    }
}

#[test]
fn test_list_pattern_with_nested_constructor_literals_safebind() {
    let ast = parse("[Ok(1), Ok(2), _] =? lr").unwrap();
    match &ast[0] {
        Ast::SafeBind(_, pattern, rhs) => {
            assert!(matches!(
                pattern,
                AstPattern::ListCons(_, first, rest)
                    if matches!(first.as_ref(),
                        AstPattern::Call(_, ctor, inner)
                        if ctor == "Ok" && matches!(inner.as_slice(), [AstPattern::IntLit(_, n)] if n == &int(1))
                    )
                    && matches!(rhs.as_ref(), Ast::Var(_, name) if name == "lr")
                    && matches!(rest.as_ref(), AstPattern::ListCons(_, _, _))
            ));
        }
        _ => panic!("Expected SafeBind"),
    }
}

#[test]
fn test_result_type_annotation() {
    let ast = parse("r: Result<Int> = Ok(42)").unwrap();
    match &ast[0] {
        Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Generic(_, name, args)), _) => {
            assert_eq!(name, "Result");
            assert!(matches!(args.as_slice(), [AstTy::Named(_, n)] if n == "Int"));
        }
        _ => panic!("Expected annotated Bind with Result type"),
    }
}

#[test]
fn test_result_unit_type_annotation_uses_unit_token() {
    let ast = parse("def main() -> Result<()> { Ok(()) }").unwrap();
    match &ast[0] {
        Ast::Def(_, _, _, _, Some(AstTy::Generic(_, name, args)), _, _) => {
            assert_eq!(name, "Result");
            assert!(matches!(args.as_slice(), [AstTy::Named(_, n)] if n == "Unit"));
        }
        _ => panic!("Expected def with Result<()> return type"),
    }
}

#[test]
fn test_generic_type_args_are_preserved_for_user_defined_type() {
    let ast = parse("v: Option<Result<Int, ParseError>> = value").unwrap();
    match &ast[0] {
        Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Generic(_, name, args)), _) => {
            assert_eq!(name, "Option");
            assert!(matches!(
                args.as_slice(),
                [AstTy::Generic(_, inner_name, inner_args)]
                    if inner_name == "Result"
                    && matches!(
                        inner_args.as_slice(),
                        [AstTy::Named(_, a), AstTy::Named(_, b)] if a == "Int" && b == "ParseError"
                    )
            ));
        }
        _ => panic!("Expected annotated bind with nested generic type"),
    }
}

#[test]
fn test_function_type_and_closure_literal() {
    let ast = parse("fun: (Int -> Unit) = {|val| do_something(val)}").unwrap();
    match &ast[0] {
        Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Func(_, params, ret)), rhs) => {
            assert_eq!(params.len(), 1);
            assert!(matches!(params[0], AstTy::Named(_, ref n) if n == "Int"));
            assert!(matches!(ret.as_ref(), AstTy::Named(_, ref n) if n == "Unit"));
            assert!(
                matches!(rhs.as_ref(), Ast::Closure(_, params, body) if params.len() == 1 && matches!(body.as_ref(), Ast::App(_, _, _)))
            );
        }
        _ => panic!("Expected annotated Bind with function type and closure"),
    }
}

#[test]
fn test_multiline_function_type_annotation() {
    let ast = parse(
        r#"handler: (
  Int,
  String
  -> Unit
) = {|x, y| print(y)}"#,
    )
    .unwrap();
    match &ast[0] {
        Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Func(_, params, ret)), rhs) => {
            assert_eq!(params.len(), 2);
            assert!(matches!(params[0], AstTy::Named(_, ref name) if name == "Int"));
            assert!(matches!(params[1], AstTy::Named(_, ref name) if name == "String"));
            assert!(matches!(ret.as_ref(), AstTy::Named(_, name) if name == "Unit"));
            assert!(matches!(rhs.as_ref(), Ast::Closure(_, params, _) if params.len() == 2));
        }
        _ => panic!("Expected multiline function type bind"),
    }
}

#[test]
fn test_capture_and_zero_arg_closure() {
    let ast = parse("f = &print\nnoop: (-> Unit) = {|| print(\"x\")}").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => {
            assert!(
                matches!(rhs.as_ref(), Ast::Capture(_, target, args) if args.is_empty() && matches!(target.as_ref(), Ast::Var(_, ref n) if n == "print"))
            );
        }
        _ => panic!("Expected Capture"),
    }
    match &ast[1] {
        Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Func(_, params, _)), rhs) => {
            assert!(params.is_empty());
            assert!(matches!(rhs.as_ref(), Ast::Closure(_, params, _) if params.is_empty()));
        }
        _ => panic!("Expected zero-arg closure"),
    }
}

#[test]
fn test_capture_placeholder_and_tuple_field_access_parse() {
    let ast = parse("inc = &add(&1, 1)\nsecond = pair._1").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Ast::Capture(_, target, args)
                if matches!(target.as_ref(), Ast::Var(_, name) if name == "add")
                && matches!(args.as_slice(), [Ast::CapturePlaceholder(_, 1), Ast::Lit(_, Lit::Int(_))])));
        }
        other => panic!("Expected capture placeholder bind, got {:?}", other),
    }
    match &ast[1] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Ast::FieldAccess(_, _, field) if field == "_1"));
        }
        other => panic!("Expected tuple field access bind, got {:?}", other),
    }
}

#[test]
fn test_identity_anonymous_capture_reports_id_hint() {
    let err = parse("f = &(&1)").expect_err("identity anonymous capture must fail");
    assert!(err.message().contains("use `&id` instead"));
}

#[test]
fn test_anonymous_capture_reports_named_function_hint() {
    let err = parse(
        r#"f = &(&1 + &2)
g = &(print("Hello"))"#,
    )
    .expect_err("anonymous capture forms must fail");
    assert!(err
        .message()
        .contains("capture it like `&fun_name(&1, &2)`"));
}

#[test]
fn test_immediate_anonymous_callable_calls_report_dedicated_error() {
    for source in [
        "f = &add(&1, 10)(4)",
        "f = (&add(&1, 10))(4)",
        "f = ({|x| x + 1})(4)",
        "f = (make())(4)",
        "f = (&noop)()",
    ] {
        let err = parse(source).expect_err("immediate anonymous callable call must fail");
        assert_eq!(
            err.message(),
            "Immediate calls on anonymous callable expressions are not supported; bind the callable to a name and call it as `fn(args)`"
        );
    }
}

#[test]
fn test_qualified_capture_and_flow_parse() {
    let ast = parse("reader = &User::get_name\nout = value |> trim() |*> normalize()").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Ast::Capture(_, target, args)
                    if args.is_empty() && matches!(target.as_ref(), Ast::Path(_, path) if path.segments == vec!["User".to_string(), "get_name".to_string()])));
        }
        _ => panic!("Expected qualified capture"),
    }
    match &ast[1] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::ContextMap(_, left, _) => {
                assert!(matches!(left.as_ref(), Ast::Pipe(_, _, _)));
            }
            other => panic!("Expected left-associative flow parse, got {:?}", other),
        },
        _ => panic!("Expected bind"),
    }
}

#[test]
fn test_backtick_qualified_capture_and_operator_capture_parse() {
    let ast = parse("reader = &`User::get_name`\ninc = &`+`(&1, 10)\nadd = &`+`").unwrap();

    match &ast[0] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(
                rhs.as_ref(),
                Ast::Capture(_, target, args)
                    if args.is_empty()
                        && matches!(target.as_ref(), Ast::Path(_, path)
                            if path.segments == vec!["User".to_string(), "get_name".to_string()])
            ));
        }
        other => panic!("Expected qualified capture bind, got {:?}", other),
    }

    match &ast[1] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(
                rhs.as_ref(),
                Ast::Capture(_, target, args)
                    if args.len() == 2
                        && matches!(target.as_ref(), Ast::FuncLiteralRef(_, func) if func.body == "+")
            ));
        }
        other => panic!("Expected operator capture bind, got {:?}", other),
    }

    match &ast[2] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(
                rhs.as_ref(),
                Ast::Capture(_, target, args)
                    if args.is_empty()
                        && matches!(target.as_ref(), Ast::FuncLiteralRef(_, func) if func.body == "+")
            ));
        }
        other => panic!("Expected bare operator capture bind, got {:?}", other),
    }
}

#[test]
fn test_pipe_rhs_call_stays_as_app() {
    let ast = parse("out = user |> User::get_name()").expect("pipe with method call should parse");
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Pipe(_, _, right) => {
                assert!(matches!(right.as_ref(), Ast::App(_, _, args) if args.is_empty()));
            }
            other => panic!("Expected pipe node, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_flow_operators_allow_elixir_style_line_breaks() {
    let ast = parse(
        "out = value\n|> trim()\n|*> normalize()\npipeline = &parse\n>* &render\nplain = &inc >>\n&inc",
    )
    .expect("flow operators should continue across newlines");

    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::ContextMap(_, left, _) => {
                assert!(matches!(left.as_ref(), Ast::Pipe(_, _, _)));
            }
            other => panic!("Expected multiline pipe/map chain, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }

    match &ast[1] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Ast::LiftedCompose(_, _, _)));
        }
        other => panic!("Expected multiline lifted compose, got {:?}", other),
    }

    match &ast[2] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Ast::Compose(_, _, _)));
        }
        other => panic!("Expected multiline compose, got {:?}", other),
    }
}

#[test]
fn test_nested_generic_type_closes_without_confusing_compose() {
    let ast = parse("value: Result<List<Int>> = Ok([])").expect("nested generic type should parse");
    match &ast[0] {
        Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Generic(_, name, outer_args)), rhs) => {
            assert_eq!(name, "Result");
            assert_eq!(outer_args.len(), 1);
            assert!(matches!(
                &outer_args[0],
                AstTy::Generic(_, inner_name, inner_args)
                    if inner_name == "List"
                        && inner_args.len() == 1
                        && matches!(&inner_args[0], AstTy::Named(_, ty) if ty == "Int")
            ));
            assert!(matches!(
                rhs.as_ref(),
                Ast::ConstructorCall(_, ctor, args) if ctor == "Ok" && args.len() == 1
            ));
        }
        other => panic!("Expected annotated Result<List<Int>> bind, got {:?}", other),
    }
}

#[test]
fn test_qualified_placeholder_capture_parses() {
    let ast = parse(r#"rename = &User::with_name("bob", &1)"#)
        .expect("qualified placeholder capture should parse");
    match &ast[0] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(
                rhs.as_ref(),
                Ast::Capture(_, target, args)
                    if args.len() == 2
                        && matches!(target.as_ref(), Ast::Path(_, path)
                            if path.segments == vec!["User".to_string(), "with_name".to_string()])
            ));
        }
        other => panic!("Expected qualified partial capture bind, got {:?}", other),
    }
}

#[test]
fn test_compose_chain_is_left_associative_at_same_precedence() {
    let ast =
        parse("pipeline = parse() >=> validate() >> render()").expect("compose chain should parse");
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Compose(_, left, right) => {
                assert!(matches!(right.as_ref(), Ast::App(_, _, args) if args.is_empty()));
                assert!(matches!(left.as_ref(), Ast::KleisliCompose(_, _, _)));
            }
            other => panic!("Expected outer compose node, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_legacy_pipe_compose_operator_is_rejected() {
    parse("pipeline = &parse |=> &render").expect_err("legacy pipe compose should be rejected");
}

#[test]
fn test_closure_body_accepts_semicolon_separated_statements() {
    let ast = parse("fun = {|num| x = x + 5;x+num}").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Closure(_, params, body) => {
                assert_eq!(params.len(), 1);
                assert!(matches!(params[0].name.as_str(), "num"));
                assert!(matches!(
                    body.as_ref(),
                    Ast::Block(_, stmts)
                        if matches!(stmts.as_slice(), [Ast::Semi(_, _), Ast::BinOp(_, _, _, _)])
                ));
            }
            _ => panic!("Expected Closure"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_closure_param_annotation_is_optional() {
    let ast = parse("fun = {|x: Int, y| y}").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Closure(_, params, _) => {
                assert_eq!(params.len(), 2);
                assert!(matches!(params[0].ty, Some(AstTy::Named(_, ref n)) if n == "Int"));
                assert!(params[1].ty.is_none());
            }
            _ => panic!("Expected Closure"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_semicolon_wraps_statement_in_semi() {
    let ast = parse("print(\"x\");").unwrap();
    assert!(matches!(
        &ast[0],
        Ast::Semi(_, inner) if matches!(inner.as_ref(), Ast::App(_, _, _))
    ));
}

#[test]
fn test_function_body_trailing_semicolon_is_explicit_unit() {
    let ast = parse("def fun() -> Unit { print(\"x\"); }").unwrap();
    match &ast[0] {
        Ast::Def(_, _, _, _, _, body, _) => {
            assert!(matches!(
                body.as_ref(),
                Ast::Block(_, stmts) if matches!(stmts.as_slice(), [Ast::Semi(_, inner)] if matches!(inner.as_ref(), Ast::App(_, _, _)))
            ));
        }
        _ => panic!("Expected Def"),
    }
}

#[test]
fn test_empty_def_body_is_error() {
    let err = parse("def noop() -> Unit {}").expect_err("Expected parse error");
    assert!(err.message().contains("Function body must not be empty"));
}

#[test]
fn test_multiline() {
    let ast = parse("x = 1\ny = 2\nprint(to_string(x))").unwrap();
    assert_eq!(ast.len(), 3);
}

#[test]
fn test_statements_on_same_line_require_separator() {
    let err = parse("[]1").expect_err("Expected parse error");
    assert!(err.message().contains("Expected newline or `;`"));
}

#[test]
fn test_safebind_rhs_requires_statement_separator() {
    let err = parse("[] =? []1").expect_err("Expected parse error");
    assert!(err.message().contains("Expected newline or `;`"));
}

#[test]
fn test_safebind_allows_trailing_semicolon() {
    let ast = parse("[] =? value;").unwrap();
    assert!(matches!(
        &ast[0],
        Ast::Semi(_, inner) if matches!(inner.as_ref(), Ast::SafeBind(_, _, _))
    ));
}

#[test]
fn test_string_concat() {
    let ast = parse(r#"msg = "hello" ++ " world""#).unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Ast::BinOp(_, BinOp::Concat, _, _)));
        }
        _ => panic!("Expected Bind with Concat"),
    }
}

#[test]
fn test_string_concat_is_left_associative() {
    let ast = parse(r#"msg = a ++ b ++ c"#).unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::BinOp(_, BinOp::Concat, left, right) => {
                assert!(matches!(right.as_ref(), Ast::Var(_, name) if name == "c"));
                assert!(matches!(
                    left.as_ref(),
                    Ast::BinOp(_, BinOp::Concat, ll, lr)
                        if matches!(ll.as_ref(), Ast::Var(_, name) if name == "a")
                            && matches!(lr.as_ref(), Ast::Var(_, name) if name == "b")
                ));
            }
            _ => panic!("Expected nested left-associative concat"),
        },
        _ => panic!("Expected Bind with chained Concat"),
    }
}

#[test]
fn test_interpolated_string_ast() {
    let ast = parse(r#"msg = "hi #{name}!""#).unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::InterpolatedStr(_, parts) => {
                assert!(matches!(parts.first(), Some(InterpolatedPart::Text(s)) if s == "hi "));
                assert!(matches!(parts.get(1), Some(InterpolatedPart::Expr(expr))
                            if matches!(expr.as_ref(), Ast::Var(_, name) if name == "name")));
                assert!(matches!(parts.get(2), Some(InterpolatedPart::Text(s)) if s == "!"));
            }
            _ => panic!("Expected InterpolatedStr"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_interpolated_string_allows_brace_in_inner_string_literal() {
    let ast = parse(r#"msg = '#{to_string("}")}'"#).unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::InterpolatedStr(_, parts) => {
                assert!(matches!(
                    parts.as_slice(),
                    [InterpolatedPart::Expr(expr)]
                        if matches!(expr.as_ref(), Ast::App(_, _, _))
                ));
            }
            _ => panic!("Expected InterpolatedStr"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_interpolation_escape_drops_backslash() {
    let ast = parse(r#"msg = "\#{name}""#).unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Ast::Lit(_, Lit::Str(s)) if s == "#{name}"));
        }
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_triple_quoted_string_parses_as_plain_string_expr() {
    let ast = parse("msg = \"\"\"\nhello\n\"\"\"").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Ast::Lit(_, Lit::Str(s)) if s == "\nhello\n"));
        }
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_triple_quoted_string_allows_interpolation() {
    let ast = parse("msg = \"\"\"\nhello #{name}\n\"\"\"").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::InterpolatedStr(_, parts) => {
                assert!(
                    matches!(parts.first(), Some(InterpolatedPart::Text(s)) if s == "\nhello ")
                );
                assert!(matches!(parts.get(1), Some(InterpolatedPart::Expr(expr))
                    if matches!(expr.as_ref(), Ast::Var(_, name) if name == "name")));
                assert!(matches!(parts.get(2), Some(InterpolatedPart::Text(s)) if s == "\n"));
            }
            _ => panic!("Expected InterpolatedStr"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_triple_quoted_string_dedents_from_starting_line_indent() {
    let ast = parse("def main() -> Unit {\n  msg = \"\"\"\n  hello\n  world\n  \"\"\"\n}");
    let ast = ast.unwrap();
    match &ast[0] {
        Ast::Def(_, _, _, _, _, body, _) => match body.as_ref() {
            Ast::Block(_, stmts) => {
                assert!(matches!(
                    stmts.as_slice(),
                    [Ast::Bind(_, _, rhs)]
                        if matches!(rhs.as_ref(), Ast::Lit(_, Lit::Str(s)) if s == "\nhello\nworld\n")
                ));
            }
            _ => panic!("Expected Block"),
        },
        _ => panic!("Expected Def"),
    }
}

#[test]
fn test_doc_attribute_rejects_interpolation() {
    let err = parse("@@doc \"\"\"\nhello #{name}\n\"\"\"\ndef main() -> Unit { () }")
        .expect_err("@@doc interpolation must fail");
    assert!(
        err.message()
            .contains("@@doc does not allow string interpolation"),
        "unexpected error: {}",
        err.message()
    );
}

#[test]
fn test_triple_quoted_string_rejects_content_shallower_than_starting_line() {
    let err = parse("def main() -> Unit {\n  msg = \"\"\"\n shallow\n  \"\"\"\n}");
    assert!(err
        .expect_err("expected indentation error")
        .message()
        .contains(
            "Triple-quoted string content must be indented at least as far as the starting line"
        ));
}

#[test]
fn test_regex_generated_literal_double_quote_lowers_to_compile_call() {
    let ast = parse(r#"rx = re"^a+$""#).unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::App(_, callee, args) => {
                assert!(matches!(
                    callee.as_ref(),
                    Ast::Path(_, path) if path.segments == vec!["Regex", "compile"]
                ));
                assert!(matches!(
                    args.as_slice(),
                    [RecordLitArg::Positional(Ast::Lit(_, Lit::Str(pat)))] if pat == "^a+$"
                ));
            }
            other => panic!("Expected compile App, got {:?}", other),
        },
        other => panic!("Expected Bind, got {:?}", other),
    }
}

#[test]
fn test_regex_generated_literal_single_quote_lowers_to_compile_call() {
    let ast = parse("rx = re'^a+$'").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::App(_, callee, args) => {
                assert!(matches!(
                    callee.as_ref(),
                    Ast::Path(_, path) if path.segments == vec!["Regex", "compile"]
                ));
                assert!(matches!(
                    args.as_slice(),
                    [RecordLitArg::Positional(Ast::Lit(_, Lit::Str(pat)))] if pat == "^a+$"
                ));
            }
            other => panic!("Expected compile App, got {:?}", other),
        },
        other => panic!("Expected Bind, got {:?}", other),
    }
}

#[test]
fn test_negative_int() {
    let ast = parse("x = -5").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(-5)));
        }
        _ => panic!("Expected Bind with negative Int"),
    }
}

#[test]
fn test_negative_variable_reports_phase1_guidance() {
    let err = parse("x = -value").expect_err("Expected parse error");
    assert!(err
        .message()
        .contains("write `0 - value` instead of `-value`"));
}

#[test]
fn test_tuple_literal() {
    let ast = parse("pair = (1, 2)").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::TupleLiteral(_, elems) => {
                assert_eq!(elems.len(), 2);
                assert!(matches!(elems[0], Ast::Lit(_, Lit::Int(ref n)) if n == &int(1)));
                assert!(matches!(elems[1], Ast::Lit(_, Lit::Int(ref n)) if n == &int(2)));
            }
            other => panic!("Expected TupleLiteral, got {:?}", other),
        },
        other => panic!("Expected Bind, got {:?}", other),
    }
}

#[test]
fn test_parenthesized_expression_is_not_tuple_literal() {
    let ast = parse("value = (1)").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(
                rhs.as_ref(),
                Ast::Grouped(_, inner)
                    if matches!(inner.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(1))
            ));
        }
        other => panic!("Expected Bind, got {:?}", other),
    }
}

#[test]
fn test_deep_parenthesized_expression_parses_without_stacker_feature() {
    let depth = 16;
    let mut source = String::from("value = ");
    source.push_str(&"(".repeat(depth));
    source.push('1');
    source.push_str(&")".repeat(depth));

    let ast = parse(&source).expect("deep parenthesized expression should parse");
    assert_eq!(ast.len(), 1);
}

#[test]
fn test_max_depth_parenthesized_expression_parses() {
    let depth = MAX_PARSE_NESTING;
    let mut source = String::from("value = ");
    source.push_str(&"(".repeat(depth));
    source.push('1');
    source.push_str(&")".repeat(depth));

    let ast = parse(&source).expect("maximum allowed parenthesized expression should parse");
    assert_eq!(ast.len(), 1);
}

#[test]
fn test_excessive_parenthesized_expression_is_parse_error() {
    let depth = MAX_PARSE_NESTING + 1;
    let mut source = String::from("value = ");
    source.push_str(&"(".repeat(depth));
    source.push('1');
    source.push_str(&")".repeat(depth));

    let err = parse(&source).expect_err("excessive parenthesized expression must fail");
    assert!(err.message().contains(MAX_PARSE_NESTING_MESSAGE));
}

#[test]
fn test_excessive_generic_type_nesting_is_parse_error() {
    let depth = MAX_PARSE_NESTING + 1;
    let mut ty = "Int".to_string();
    for _ in 0..depth {
        ty = format!("List<{}>", ty);
    }
    let source = format!("value: {} = []", ty);

    let err = parse(&source).expect_err("excessive generic type nesting must fail");
    assert!(err.message().contains(MAX_PARSE_NESTING_MESSAGE));
}

#[test]
fn test_excessive_list_expression_nesting_is_parse_error() {
    let depth = MAX_PARSE_NESTING + 1;
    let mut source = String::from("value = ");
    source.push_str(&"[".repeat(depth));
    source.push('1');
    source.push_str(&"]".repeat(depth));

    let err = parse(&source).expect_err("excessive list expression nesting must fail");
    assert!(err.message().contains(MAX_PARSE_NESTING_MESSAGE));
}

#[test]
fn test_excessive_block_expression_nesting_is_parse_error() {
    let depth = MAX_PARSE_NESTING + 1;
    let mut source = String::from("value = ");
    source.push_str(&"{".repeat(depth));
    source.push('1');
    source.push_str(&"}".repeat(depth));

    let err = parse(&source).expect_err("excessive block expression nesting must fail");
    assert!(err.message().contains(MAX_PARSE_NESTING_MESSAGE));
}

#[test]
fn test_excessive_pattern_nesting_is_parse_error() {
    let depth = MAX_PARSE_NESTING + 1;
    let mut source = "[".repeat(depth);
    source.push('_');
    source.push_str(&"]".repeat(depth));
    source.push_str(" = value");

    let err = parse(&source).expect_err("excessive pattern nesting must fail");
    assert!(err.message().contains(MAX_PARSE_NESTING_MESSAGE));
}

#[test]
fn test_grouped_pipe_rhs_preserves_callable_returning_call() {
    let ast = parse("out = value |> (make_closure(1))").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Pipe(_, _, right) => assert!(matches!(
                right.as_ref(),
                Ast::Grouped(_, inner) if matches!(inner.as_ref(), Ast::App(_, _, _))
            )),
            other => panic!("Expected pipe node, got {:?}", other),
        },
        other => panic!("Expected Bind, got {:?}", other),
    }
}

#[test]
fn test_tuple_pattern_bind() {
    let ast = parse("(left, right) = pair").unwrap();
    match &ast[0] {
        Ast::Bind(_, pattern, rhs) => {
            assert!(matches!(
                pattern,
                AstPattern::Tuple(_, items)
                    if matches!(items.as_slice(),
                        [AstPattern::Var(_, left), AstPattern::Var(_, right)]
                            if left == "left" && right == "right")
            ));
            assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "pair"));
        }
        other => panic!("Expected Bind, got {:?}", other),
    }
}

#[test]
fn test_tuple_type_annotation() {
    let ast = parse("pair: (Int, String) = value").unwrap();
    match &ast[0] {
        Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Tuple(_, items)), rhs) => {
            assert_eq!(items.len(), 2);
            assert!(matches!(&items[0], AstTy::Named(_, name) if name == "Int"));
            assert!(matches!(&items[1], AstTy::Named(_, name) if name == "String"));
            assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "value"));
        }
        other => panic!("Expected annotated tuple bind, got {:?}", other),
    }
}

#[test]
fn test_function_type_annotation_is_not_tuple_type() {
    let ast = parse("fun: (Int, String -> Unit) = {|x, y| ()}").unwrap();
    match &ast[0] {
        Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Func(_, params, ret)), rhs) => {
            assert_eq!(params.len(), 2);
            assert!(matches!(&params[0], AstTy::Named(_, name) if name == "Int"));
            assert!(matches!(&params[1], AstTy::Named(_, name) if name == "String"));
            assert!(matches!(ret.as_ref(), AstTy::Named(_, name) if name == "Unit"));
            assert!(matches!(rhs.as_ref(), Ast::Closure(_, params, _) if params.len() == 2));
        }
        other => panic!("Expected function type bind, got {:?}", other),
    }
}

#[test]
fn test_function_type_annotation_arrow_outside_parens_has_guided_error() {
    let err = parse("fun: (Int) -> String = value").expect_err("Expected parse error");
    let message = err.message();
    assert!(message.contains("choose tuple or function syntax"));
    assert!(message.contains("`, `") || message.contains("`,`"));
    assert!(message.contains("`->`"));
    assert!(message.contains("(Int -> String)"));
}

#[test]
fn test_parenthesized_single_type_annotation_is_rejected() {
    let err = parse("value: (Int) = input").expect_err("Expected parse error");
    let message = err.message();
    assert!(message.contains("one element"));
    assert!(message.contains("without parentheses"));
    assert!(message.contains("(T, U)"));
    assert!(message.contains("(T -> R)"));
}

#[test]
fn test_field_access() {
    let ast = parse("user.name").unwrap();
    assert!(matches!(&ast[0], Ast::FieldAccess(_, _, ref f) if f == "name"));
}

#[test]
fn test_tuple_index_field_access() {
    let ast = parse("first = pair._0").unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Ast::FieldAccess(_, _, field) if field == "_0"));
        }
        other => panic!("Expected Bind, got {:?}", other),
    }
}

#[test]
fn test_tuple_dot_numeric_index_is_rejected() {
    let err = parse("first = pair.0").expect_err("Expected parse error");
    assert!(err.message().contains("Tuple access uses ._0, ._1, ..."));
}

#[test]
fn test_one_tuple_literal_is_rejected() {
    let err = parse("pair = (1,)").expect_err("Expected parse error");
    assert!(err.message().contains("1-tuple literals are not supported"));
}

#[test]
fn test_one_tuple_pattern_is_rejected() {
    let err = parse("(x,) = pair").expect_err("Expected parse error");
    assert!(err.message().contains("1-tuple patterns are not supported"));
}

#[test]
fn test_one_tuple_type_is_rejected() {
    let err = parse("pair: (Int,) = value").expect_err("Expected parse error");
    assert!(err.message().contains("1-tuple types are not supported"));
}

#[test]
fn test_match_wildcard_and_int_pattern() {
    let ast = parse(
        r#"x = match n {
  1 => "one",
  _ => "other",
}"#,
    )
    .unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Match(_, _, arms) => {
                assert!(matches!(&arms[0].pattern, AstPattern::IntLit(_, n) if n == &int(1)));
                assert!(matches!(&arms[1].pattern, AstPattern::Wildcard(_)));
            }
            _ => panic!("Expected Match"),
        },
        _ => panic!("Expected Bind with Match"),
    }
}

#[test]
fn test_match_string_pattern() {
    let ast = parse(
        r#"x = match s {
  "a" => 1,
  _ => 0,
}"#,
    )
    .unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Match(_, _, arms) => {
                assert!(matches!(&arms[0].pattern, AstPattern::StrLit(_, s) if s == "a"));
            }
            _ => panic!("Expected Match"),
        },
        _ => panic!("Expected Bind with Match"),
    }
}

#[test]
fn test_match_arm_rhs_brace_block_is_block_expr() {
    let ast = parse(
        r#"x = match n {
  1 => {
    y = 2
    y + 3
  },
  _ => 0,
}"#,
    )
    .unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Match(_, _, arms) => {
                assert!(matches!(&arms[0].body, Ast::Block(_, stmts) if stmts.len() == 2));
            }
            other => panic!("Expected Match, got {:?}", other),
        },
        other => panic!("Expected Bind with Match, got {:?}", other),
    }
}

#[test]
fn test_match_arm_rhs_explicit_closure_stays_closure() {
    let ast = parse(
        r#"x = match n {
  1 => {|| 2},
  _ => {|| 0},
}"#,
    )
    .unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Match(_, _, arms) => {
                assert!(matches!(&arms[0].body, Ast::Closure(_, params, _) if params.is_empty()));
                assert!(matches!(&arms[1].body, Ast::Closure(_, params, _) if params.is_empty()));
            }
            other => panic!("Expected Match, got {:?}", other),
        },
        other => panic!("Expected Bind with Match, got {:?}", other),
    }
}

#[test]
fn test_match_negative_int_in_list_pattern() {
    let ast = parse(
        r#"x = match nums {
  [-1] => "neg",
  _ => "other",
}"#,
    )
    .unwrap();
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Match(_, _, arms) => {
                assert!(matches!(
                    &arms[0].pattern,
                    AstPattern::ListCons(_, head, tail)
                        if matches!(head.as_ref(), AstPattern::IntLit(_, n) if n == &int(-1))
                            && matches!(tail.as_ref(), AstPattern::ListNil(_))
                ));
            }
            _ => panic!("Expected Match"),
        },
        _ => panic!("Expected Bind with Match"),
    }
}

#[test]
fn test_empty_match_is_error() {
    let err = parse("x = match value {}").expect_err("Expected parse error");
    assert!(err
        .message()
        .contains("Match expression must contain at least one arm"));
}

#[test]
fn test_cond_desugars_to_nested_if_apps() {
    let ast = parse(
        r#"x = cond {
  a => 1,
  b => 2,
  True => 3,
}"#,
    )
    .expect("cond should parse");

    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::App(_, func, args) => {
                assert!(matches!(func.as_ref(), Ast::Var(_, name) if name == "if"));
                assert!(
                    matches!(&args[0], RecordLitArg::Positional(Ast::Var(_, name)) if name == "a")
                );
                assert!(
                    matches!(&args[1], RecordLitArg::Positional(Ast::Lit(_, Lit::Int(n))) if n == &int(1))
                );
                assert!(matches!(
                    &args[2],
                    RecordLitArg::Positional(Ast::App(_, inner_func, inner_args))
                        if matches!(inner_func.as_ref(), Ast::Var(_, name) if name == "if")
                            && matches!(&inner_args[0], RecordLitArg::Positional(Ast::Var(_, name)) if name == "b")
                            && matches!(&inner_args[1], RecordLitArg::Positional(Ast::Lit(_, Lit::Int(n))) if n == &int(2))
                            && matches!(&inner_args[2], RecordLitArg::Positional(Ast::Lit(_, Lit::Int(n))) if n == &int(3))
                ));
            }
            _ => panic!("Expected App"),
        },
        _ => panic!("Expected Bind with cond RHS"),
    }
}

#[test]
fn test_cond_accepts_zero_arg_closure_body() {
    let ast = parse(
        r#"x = cond {
  True => { print("ok"); 1 },
}"#,
    )
    .expect("cond with closure body should parse");

    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Closure(_, params, body) => {
                assert!(params.is_empty());
                assert!(matches!(
                    body.as_ref(),
                    Ast::Block(_, stmts)
                        if matches!(stmts.as_slice(), [Ast::Semi(_, _), Ast::Lit(_, Lit::Int(n))] if n == &int(1))
                ));
            }
            other => panic!(
                "Expected final True clause body to become closure, got {:?}",
                other
            ),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_empty_cond_is_error() {
    let err = parse("x = cond {}").expect_err("Expected parse error");
    assert!(err
        .message()
        .contains("Cond expression must contain at least one clause"));
}

#[test]
fn test_cond_requires_final_true_clause() {
    let err = parse(
        r#"x = cond {
  flag => 1,
}"#,
    )
    .expect_err("Expected parse error");
    assert!(err
        .message()
        .contains("Final cond clause must use `True` as its condition"));
}

#[test]
fn test_cond_rejects_non_final_true_clause() {
    let err = parse(
        r#"x = cond {
  True => 1,
  other => 2,
}"#,
    )
    .expect_err("Expected parse error");
    assert!(err
        .message()
        .contains("`True` clause must be the final cond clause"));
}

#[test]
fn test_match_constructor_pattern_is_accepted() {
    let ast = parse(
        r#"x = match value {
  Some(y) => y,
  _ => 0,
}"#,
    )
    .expect("constructor pattern should parse");
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Match(_, _, arms) => {
                assert!(matches!(
                    &arms[0].pattern,
                    AstPattern::Call(_, name, inner)
                        if name == "Some"
                            && matches!(inner.as_slice(), [AstPattern::Var(_, bound)] if bound == "y")
                ));
            }
            _ => panic!("Expected Match"),
        },
        _ => panic!("Expected Bind with Match"),
    }
}

#[test]
fn test_match_bare_uppercase_identifier_is_constructor_pattern() {
    let ast = parse(
        r#"x = match value {
  ParseError => 0,
  _ => 1,
}"#,
    )
    .expect("bare uppercase identifier should parse as a constructor pattern");
    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Match(_, _, arms) => {
                assert!(matches!(
                    &arms[0].pattern,
                    AstPattern::Constructor(_, name, args) if name == "ParseError" && args.is_empty()
                ));
            }
            _ => panic!("Expected Match"),
        },
        _ => panic!("Expected Bind with Match"),
    }
}

#[test]
fn test_match_as_and_annotated_pattern_is_accepted() {
    let ast = parse(
        r#"x = match value {
  [head, ..tail] @ whole: List<Int> => head,
  _ => 0,
}"#,
    )
    .expect("as-pattern and annotation in match should parse");

    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Match(_, _, arms) => {
                assert!(matches!(
                    &arms[0].pattern,
                    AstPattern::As(_, inner, alias, Some(AstTy::Generic(_, ty_name, ty_args)))
                        if alias == "whole"
                            && ty_name == "List"
                            && ty_args.len() == 1
                            && matches!(inner.as_ref(), AstPattern::ListCons(_, _, _))
                ));
            }
            _ => panic!("Expected Match"),
        },
        _ => panic!("Expected Bind with Match"),
    }
}

#[test]
fn test_match_or_pattern_expands_into_multiple_arms() {
    let ast = parse(
        r#"x = match value {
  "a" | "b" => 1,
  _ => 0,
}"#,
    )
    .expect("or pattern should parse");

    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Match(_, _, arms) => {
                assert_eq!(arms.len(), 3);
                assert!(matches!(
                    &arms[0].pattern,
                    AstPattern::StrLit(_, s) if s == "a"
                ));
                assert!(matches!(
                    &arms[1].pattern,
                    AstPattern::StrLit(_, s) if s == "b"
                ));
                assert!(matches!(&arms[2].pattern, AstPattern::Wildcard(_)));
            }
            _ => panic!("Expected Match"),
        },
        _ => panic!("Expected Bind with Match"),
    }
}

#[test]
fn test_match_guard_is_parsed_on_each_expanded_or_arm() {
    let ast = parse(
        r#"x = match n {
  1 | 2 when 0 < n `and` n < 10 => n,
  _ => 0,
}"#,
    )
    .expect("guarded or pattern should parse");

    match &ast[0] {
        Ast::Bind(_, _, rhs) => match rhs.as_ref() {
            Ast::Match(_, _, arms) => {
                assert_eq!(arms.len(), 3);
                assert!(arms[0].guard.is_some());
                assert!(arms[1].guard.is_some());
                assert!(arms[2].guard.is_none());
            }
            _ => panic!("Expected Match"),
        },
        _ => panic!("Expected Bind with Match"),
    }
}

#[test]
fn test_defmod_parses_module_body() {
    let ast = parse_with_context(
        r#"defmod Kernel {
  def add(x: Int, y: Int) -> Int { x + y }
}"#,
        ParserContext::module(1, None),
    )
    .expect("defmod should parse");

    match ast.as_slice() {
        [Ast::Defmod(_, name, body, _)] => {
            assert_eq!(name, "Kernel");
            assert!(matches!(body.as_slice(), [Ast::Def(..)]));
        }
        _ => panic!("Expected single defmod declaration"),
    }
}

#[test]
fn test_import_three_forms_parse() {
    let ast = parse(
        r#"import Kernel;
import Kernel::add;
import Kernel::{add, sub};"#,
    )
    .expect("imports should parse");

    assert!(matches!(
        ast[0],
        Ast::Import(_, AstPath { ref segments, .. }, ImportSpec::All)
            if segments.as_slice() == ["Kernel"]
    ));
    assert!(matches!(
        ast[1],
        Ast::Import(_, AstPath { ref segments, .. }, ImportSpec::Single(ref name))
            if segments.as_slice() == ["Kernel"] && name == "add"
    ));
    assert!(matches!(
        ast[2],
        Ast::Import(_, AstPath { ref segments, .. }, ImportSpec::List(ref names))
            if segments.as_slice() == ["Kernel"] && names.as_slice() == ["add", "sub"]
    ));
}

#[test]
fn test_include_parses_string_path() {
    let ast = parse("include './mylib.srt'").expect("include should parse");
    assert!(matches!(
        ast.as_slice(),
        [Ast::Include(_, path)] if path == "./mylib.srt"
    ));
}

#[test]
fn test_defenum_parses_variants_with_payload_and_discriminant() {
    let ast = parse_with_context(
        r#"defenum Direction {
  Up = 1,
  Down,
  Arrow(Int, Int),
}"#,
        ParserContext::module(1, None),
    )
    .expect("defenum should parse");

    match ast.as_slice() {
        [Ast::EnumDef(_, name, type_params, variants, _)] => {
            assert_eq!(name, "Direction");
            assert!(type_params.is_empty());
            assert_eq!(variants.len(), 3);
            assert_eq!(variants[0].name, "Up");
            assert_eq!(variants[0].discriminant, Some(int(1)));
            assert_eq!(variants[1].name, "Down");
            assert_eq!(variants[1].payload.len(), 0);
            assert_eq!(variants[2].name, "Arrow");
            assert_eq!(variants[2].payload.len(), 2);
        }
        other => panic!("Expected enum definition, got {:?}", other),
    }
}

#[test]
fn test_defenum_parses_generic_header() {
    let ast = parse_with_context(
        r#"defenum ReduceStep<$A> {
  Resume($A),
  Stop($A),
}"#,
        ParserContext::module(1, None),
    )
    .expect("generic defenum should parse");

    match ast.as_slice() {
        [Ast::EnumDef(_, name, type_params, variants, _)] => {
            assert_eq!(name, "ReduceStep");
            assert_eq!(type_params.len(), 1);
            assert_eq!(type_params[0].name, "$A");
            assert_eq!(variants.len(), 2);
        }
        other => panic!("Expected generic enum definition, got {:?}", other),
    }
}

#[test]
fn test_qualified_constructor_call_and_unit_constructor_parse() {
    let ast = parse(
        r#"x = Direction::Up
y = KeyInput::Arrow(Direction::Down)"#,
    )
    .expect("qualified constructors should parse");

    assert!(matches!(
        ast[0],
        Ast::Bind(_, _, ref rhs)
            if matches!(rhs.as_ref(), Ast::ConstructorCall(_, name, args) if name == "Direction::Up" && args.is_empty())
    ));
    assert!(matches!(
        ast[1],
        Ast::Bind(_, _, ref rhs)
            if matches!(rhs.as_ref(), Ast::ConstructorCall(_, name, args) if name == "KeyInput::Arrow" && args.len() == 1)
    ));
}

#[test]
fn test_nested_defmod_is_rejected() {
    let err = parse(
        r#"defmod Outer {
  defmod Inner {
    def run() -> Unit { () }
  }
}"#,
    )
    .expect_err("nested defmod must be rejected");
    assert!(err
        .message()
        .contains("Nested module declarations are not allowed"));
}

#[test]
fn test_defmod_body_rejects_top_level_expression() {
    let err = parse(
        r#"defmod Kernel {
  x = 42
}"#,
    )
    .expect_err("module body should reject top-level expressions");
    assert!(err
        .message()
        .contains("Top-level expressions are not allowed in module compile units"));
}

#[test]
fn test_module_compile_unit_rejects_top_level_bind() {
    let err = parse_with_context("x = 42", ParserContext::module(1, None))
        .expect_err("module compile unit should reject top-level binding");
    assert!(err
        .message()
        .contains("Top-level expressions are not allowed in module compile units"));
}

#[test]
fn test_module_compile_unit_rejects_top_level_def() {
    let err = parse_with_context(
        "def add(x: Int, y: Int) -> Int { x + y }",
        ParserContext::module(1, None),
    )
    .expect_err("module compile unit should require defmod wrappers for functions");
    assert!(err
        .message()
        .contains("This top-level declaration is not allowed in the current source policy"));
}

#[test]
fn test_module_compile_unit_rejects_top_level_defextractor() {
    let err = parse_with_context(
        "defextractor never(self: Int) -> MatchResult<Int, Error> { MatchResult::NoMatch }",
        ParserContext::module(1, None),
    )
    .expect_err("module compile unit should require defmod wrappers for extractors");
    assert!(err
        .message()
        .contains("This top-level declaration is not allowed in the current source policy"));
}

#[test]
fn test_module_compile_unit_accepts_top_level_defmod() {
    let ast = parse_with_context(
        "defmod Kernel { def add(x: Int, y: Int) -> Int { x + y } }",
        ParserContext::module(1, None),
    )
    .expect("module compile unit should accept defmod declarations");
    assert!(matches!(ast.as_slice(), [Ast::Defmod(_, _, _, _)]));
}

#[test]
fn test_namespace_block_lowers_type_and_module_heads() {
    let ast = parse_with_context(
        r#"namespace Auth {
  defrecord User(name: String)
  defmod Repo {
    def wrap(user: Auth::User) -> Auth::User { user }
  }
}"#,
        ParserContext::module(1, None),
    )
    .expect("namespace declarations should lower into ordinary top-level declarations");

    assert!(matches!(
        ast.as_slice(),
        [Ast::RecordDef(_, name, _), Ast::Defmod(_, module_name, _, _)]
            if name == "Auth::User" && module_name == "Auth::Repo"
    ));
}

#[test]
fn test_defmod_accepts_qualified_module_path() {
    let ast = parse_with_context(
        "defmod Auth::Repo { def name() -> String { \"repo\" } }",
        ParserContext::module(1, None),
    )
    .expect("qualified defmod path should parse");
    assert!(matches!(ast.as_slice(), [Ast::Defmod(_, name, _, _)] if name == "Auth::Repo"));
}

#[test]
fn test_impl_accepts_qualified_type_target() {
    let ast = parse_with_context(
        r#"impl Auth::User {
  def id(self: Self) -> Auth::User { self }
}"#,
        ParserContext::module(1, None),
    )
    .expect("qualified impl target should parse");
    assert!(
        matches!(ast.as_slice(), [Ast::ImplDef(_, target, methods, _)]
        if target == "Auth::User"
            && matches!(methods.as_slice(), [Ast::Def(_, _, _, _, Some(AstTy::Named(_, ret_ty)), _, _)] if ret_ty == "Auth::User"))
    );
}

#[test]
fn test_defmod_body_accepts_defextractor() {
    let ast = parse_with_context(
        r#"defmod Matchers {
  defextractor never(self: Int) -> MatchResult<Int, Error> {
    MatchResult::NoMatch
  }
}"#,
        ParserContext::module(1, None),
    )
    .expect("defmod should accept extractor declarations");
    assert!(matches!(
        ast.as_slice(),
        [Ast::Defmod(_, name, body, _)]
            if name == "Matchers"
                && matches!(body.as_slice(), [Ast::ExtractorDef(_, extractor_name, _, _, _, _, _)] if extractor_name == "never")
    ));
}

#[test]
fn test_module_compile_unit_accepts_import() {
    let ast = parse_with_context("import Kernel::add;", ParserContext::module(1, None))
        .expect("module compile unit should accept import declarations");
    assert!(matches!(
        ast.as_slice(),
        [Ast::Import(_, AstPath { segments, .. }, ImportSpec::Single(name))]
            if segments.as_slice() == ["Kernel"] && name == "add"
    ));
}

#[test]
fn test_defmod_body_rejects_non_function_declarations() {
    let err = parse(
        r#"defmod Kernel {
  defrecord Pair(left: Int, right: Int)
}"#,
    )
    .expect_err("defmod should only contain function declarations");
    assert!(err
        .message()
        .contains("This top-level declaration is not allowed in the current source policy"));
}

#[test]
fn test_module_compile_unit_rejects_builtin_decl() {
    let err = parse_with_context(
        "@@builtin def print(a: String) -> Unit",
        ParserContext::module(1, None),
    )
    .expect_err("user module compile unit should reject builtin declarations");
    assert!(err
        .message()
        .contains("This top-level declaration is not allowed in the current source policy"));
}

#[test]
fn test_module_compile_unit_rejects_builtin_type_decl() {
    let err = parse_with_context("@@builtin type Int", ParserContext::module(1, None))
        .expect_err("user module compile unit should reject builtin type declarations");
    assert!(err
        .message()
        .contains("This top-level declaration is not allowed in the current source policy"));
}

#[test]
fn test_std_module_compile_unit_accepts_builtin_decl() {
    let ast = parse_with_context(
        "defmod Bootstrap { @@builtin def print(a: String) -> Unit }",
        ParserContext::module(1, None).with_rules(ParseRules::std_module()),
    )
    .expect("std module compile unit should accept builtin declarations");
    assert!(
        matches!(ast.as_slice(), [Ast::Defmod(_, name, body, _)] if name == "Bootstrap"
            && matches!(body.as_slice(), [Ast::BuiltinDecl(_, _, _, _, _)]))
    );
}

#[test]
fn test_std_module_compile_unit_accepts_builtin_type_decl() {
    let ast = parse_with_context(
        "defmod Bootstrap { @@builtin type Int }",
        ParserContext::module(1, None).with_rules(ParseRules::std_module()),
    )
    .expect("std module compile unit should accept builtin type declarations");
    assert!(
        matches!(ast.as_slice(), [Ast::Defmod(_, name, body, _)] if name == "Bootstrap"
            && matches!(body.as_slice(), [Ast::BuiltinTypeDecl(_, BuiltinTypeHead { name: builtin_name, .. }, _)] if builtin_name == "Int"))
    );
}

#[test]
fn test_script_compile_unit_accepts_top_level_def_import_and_include() {
    let ast = parse_with_context(
        "def add(x: Int, y: Int) -> Int { x + y }\ninclude './mylib.srt'\nimport Kernel::add;",
        ParserContext::script(1),
    )
    .expect("script compile unit should accept top-level def, include, and import");
    assert!(matches!(
        ast.as_slice(),
        [
            Ast::Def(_, name, _, _, _, _, _),
            Ast::Include(_, include_path),
            Ast::Import(_, AstPath { segments, .. }, ImportSpec::Single(import_name))
        ] if name == "add"
            && include_path == "./mylib.srt"
            && segments.as_slice() == ["Kernel"]
            && import_name == "add"
    ));
}

#[test]
fn test_module_compile_unit_rejects_top_level_include() {
    let err = parse_with_context("include './mylib.srt'", ParserContext::module(1, None))
        .expect_err("module compile unit should reject include");
    assert!(err
        .message()
        .contains("This top-level declaration is not allowed in the current source policy"));
}

#[test]
fn test_script_compile_unit_rejects_top_level_struct_def() {
    let err = parse_with_context("defstruct User { name: String }", ParserContext::script(1))
        .expect_err("script compile unit should reject top-level type declarations");
    assert!(err
        .message()
        .contains("This top-level declaration is not allowed in the current source policy"));
}

#[test]
fn test_repl_compile_unit_rejects_top_level_impl_block() {
    let err = parse_with_context(
        "impl User { def new(name: String) -> Self { User { name: name } } }",
        ParserContext::repl(1),
    )
    .expect_err("repl chunk should reject top-level impl declarations");
    assert!(err
        .message()
        .contains("This top-level declaration is not allowed in the current source policy"));
}

#[test]
fn test_project_parser_context_sets_unit_kind() {
    let context = ParserContext::project(7);
    assert_eq!(context.unit_kind, ParseUnitKind::Project);
    assert_eq!(context.source_id, 7);
    assert_eq!(context.module_path, None);
}

#[test]
fn test_project_compile_unit_accepts_top_level_expression() {
    let ast = parse_with_context("x = 42", ParserContext::project(1))
        .expect("project compile unit should accept top-level expressions");
    assert!(matches!(ast.as_slice(), [Ast::Bind(_, _, _)]));
}

#[test]
fn test_project_compile_unit_rejects_top_level_defextractor() {
    let err = parse_with_context(
        "defextractor never(self: Int) -> MatchResult<Int, Error> { MatchResult::NoMatch }",
        ParserContext::project(1),
    )
    .expect_err("project compile unit should reject top-level extractor declarations");
    assert!(err
        .message()
        .contains("This top-level declaration is not allowed in the current source policy"));
}

#[test]
fn test_declaration_inside_function_body_is_rejected() {
    let err = parse(
        r#"def outer() -> Unit {
  def inner() -> Unit { () }
}"#,
    )
    .expect_err("declaration inside expression level should be rejected");
    assert!(err
        .message()
        .contains("Declarations are only allowed at the top level"));
}

#[test]
fn test_constructor_like_capture_parses_and_is_left_for_later_validation() {
    let ast = parse("f = &Some").expect("constructor-like capture should parse");
    assert!(matches!(
        ast.as_slice(),
        [Ast::Bind(_, _, rhs)]
            if matches!(rhs.as_ref(), Ast::Capture(_, target, args)
                if args.is_empty() && matches!(target.as_ref(), Ast::Var(_, name) if name == "Some"))
    ));
}

#[test]
fn test_assignment_is_rejected_in_argument_position() {
    let err = parse("f(x: y = 1)").expect_err("Expected parse error");
    assert!(err.message().contains("cannot appear in argument position"));
}

#[test]
fn test_many_top_level_declarations_parse_successfully() {
    let mut source = String::new();
    for idx in 0..256 {
        source.push_str(&format!("def value_{idx}() -> Int {{ {idx} }}\n"));
    }

    let ast = parse(&source).expect("many top-level declarations should parse");
    assert_eq!(ast.len(), 256);
}
