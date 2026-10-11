#[allow(dead_code)]
mod support;
use sindr::policy::RuntimeSourcePolicy;

#[test]
fn error_payload_schema_is_separate_from_constructor_inputs() {
    support::typecheck_with_rules(
        r#"deferror Trouble(code: Int) {
            |value: Int|
            code = value + 1
            Self(message: "trouble", code)
        }
        error: Error = Trouble(3)
        result: Int = match error { Trouble(code) => code, _ => 0 }
        "#,
        RuntimeSourcePolicy::script(),
    )
    .expect("constructor inputs and stored payload must be separate");
}

#[test]
fn nonempty_payload_rejects_string_body_and_nonterminal_construction() {
    for source in [
        r#"deferror Trouble(code: Int) { |value: Int| "trouble" }"#,
        r#"deferror Trouble(code: Int) { |value: Int| saved = Self(message: "trouble", code: value)
saved }"#,
        r#"deferror Trouble(code: Int) { |value: Int| Self(message: "trouble") }"#,
        r#"deferror Trouble(message: Int) { |value: Int| Self(message: "trouble") }"#,
    ] {
        support::typecheck_with_rules(source, RuntimeSourcePolicy::script()).expect_err(source);
    }
}

const PAIR: &str = r#"
deferror FirstPair(x: Int, y: Int) {
  |a: Int, b: Int|
  Self(message: "first", x: a, y: b)
}
deferror SecondPair(x: Int, y: Int) {
  |a: Int, b: Int|
  SecondPair("second", a, b)
}
"#;

#[test]
fn error_payload_local_reading_and_scope_boundaries() {
    for suffix in [
        r#"error = FirstPair(1, 2)
        path = FirstPair.x
        value = match error { FirstPair(x, y) @ e => { alias: FirstPair = e
        Facet::view(path, alias) + e.y + x + y }, _ => 0 }"#,
        r#"error = FirstPair(1, 2)
        value = if_let(error, FirstPair(x, y) @ e, e.x + x + y, 0)"#,
        r#"error = FirstPair(1, 2)
        value = match error { FirstPair @ e => { read = {|| e.x}
        alias = e
        read() + alias.y }, _ => 0 }"#,
        r#"a = 1
        b = 2
        error = FirstPair(a: a, b)
        value = match error { FirstPair(y: b, x) => x + b, _ => 0 }"#,
        r#"value = match SecondPair(1, 2) { FirstPair(x, y) | SecondPair(y, x) => x + y, _ => 0 }"#,
        r#"value = is_match(FirstPair(1, 2), FirstPair(1, 2))"#,
        r#"value = match FirstPair(1, 2) { FirstPair @ e: FirstPair => e.x, _ => 0 }"#,
        r#"value = match FirstPair(1, 2) { FirstPair @ e => { read = {|| e.x}
        e = SecondPair(3, 4)
        read() }, _ => 0 }"#,
        r#"def normalize(error: Error) -> Error { match error { FirstPair @ e => e, SecondPair @ e => e, other => other } }
        value: Error = normalize(FirstPair(1, 2))"#,
        r#"factory: (Int, Int -> Error) = &FirstPair
        error: Error = factory(1, 2)"#,
        r#"deferror Empty() { Self(message: "empty") }
        deferror Text { || "text" }
        empty: Error = Empty()
        text: Error = Text()"#,
    ] {
        let source = format!("{PAIR}\n{suffix}");
        support::typecheck_with_rules(&source, RuntimeSourcePolicy::script()).expect(&source);
    }
}

#[test]
fn error_facet_capture_distinguishes_common_and_concrete_roots() {
    let common = format!(
        "{PAIR}\nread_message: (Error -> String) = &Error.message\nread_kind: (Error -> String) = &Error.kind\nfactory: (Int, Int -> Error) = &FirstPair\nvalue = match factory(1, 2) {{ FirstPair @ e => {{ read = {{|| e.x}}\nread() }}, _ => 0 }}"
    );
    support::typecheck_with_rules(&common, RuntimeSourcePolicy::script())
        .expect("common Error information and ordinary lexical captures remain available");

    for suffix in [
        "read: (Error -> String) = &FirstPair.message",
        "read: (Error -> String) = &FirstPair.kind",
        "read: (Error -> Int) = &FirstPair.x",
        "value = match FirstPair(1, 2) { FirstPair @ e => { read: (Error -> String) = &e.message\nread(e) }, _ => \"none\" }",
    ] {
        let source = format!("{PAIR}\n{suffix}");
        let error = support::typecheck_with_rules(&source, RuntimeSourcePolicy::script())
            .expect_err(&source);
        assert!(
            error.message.contains("Concrete Error Facet paths cannot be captured"),
            "{source}\n{error:?}"
        );
    }
}

#[test]
fn error_facet_api_capture_keeps_concrete_root_restriction() {
    let common = format!(
        "{PAIR}\nread: (Error -> String) = &Facet::view(Error.message, &1)\nvalue = match FirstPair(1, 2) {{ FirstPair @ e => {{ read = {{|| Facet::view(FirstPair.message, e)}}\nread() }}, _ => \"none\" }}"
    );
    support::typecheck_with_rules(&common, RuntimeSourcePolicy::script())
        .expect("common root API capture and ordinary closure field reads remain available");

    for suffix in [
        "read: (Error -> String) = &Facet::view(FirstPair.message, &1)",
        "read: (Error -> String) = &Facet::view(FirstPair.kind, &1)",
    ] {
        let source = format!("{PAIR}\n{suffix}");
        let error = support::typecheck_with_rules(&source, RuntimeSourcePolicy::script())
            .expect_err(&source);
        assert!(
            error
                .message
                .contains("Concrete Error Facet paths cannot be captured"),
            "{source}\n{error:?}"
        );
    }
}

#[test]
fn error_payload_rejects_casts_mutation_and_unallowed_consumers() {
    for suffix in [
        r#"e: FirstPair = FirstPair(1, 2)"#,
        r#"def expose(e: FirstPair) -> Error { e }"#,
        r#"e: List<FirstPair> = []"#,
        r#"factory: (Int -> FirstPair) = {|n| FirstPair(n, n)}"#,
        r#"value = match FirstPair(1, 2) { e: FirstPair => 1 }"#,
        r#"value = match FirstPair(1, 2) { _: FirstPair => 1 }"#,
        r#"(e: FirstPair, _) = (FirstPair(1, 2), 0)"#,
        r#"value = (FirstPair(1, 2)).x"#,
        r#"value = Facet::view(FirstPair.x, FirstPair(1, 2))"#,
        r#"value = match FirstPair(1, 2) { FirstPair @ e => { widened: Error = e
        widened.message }, _ => "none" }"#,
        r#"value = match FirstPair(1, 2) { FirstPair @ e => Facet::set(FirstPair.x, e, 3), _ => FirstPair(1, 2) }"#,
        r#"value = match FirstPair(1, 2) { FirstPair @ e => { saved = (e, 0)
        restored = saved._0
        restored.x }, _ => 0 }"#,
        r#"value = match FirstPair(1, 2) { FirstPair | SecondPair @ e => e.x, _ => 0 }"#,
        r#"value = is_match(FirstPair(1, 2), FirstPair(x, y))"#,
        r#"value = is_match(FirstPair(1, 2), FirstPair())"#,
        r#"value = is_match(FirstPair(1, 2), FirstPair @ e)"#,
        r#"FirstPair(x, y) =? FirstPair(1, 2)"#,
        r#"value = apply_pattern(FirstPair(1, 2), FirstPair(x, y))"#,
        r#"a = 1
        value = FirstPair(a: a, 2)"#,
        r#"value = match FirstPair(1, 2) { FirstPair(x: 1, _) => 0, _ => 1 }"#,
        r#"read: (Error -> Int) = &FirstPair.x"#,
        r#"value = match FirstPair(1, 2) { FirstPair @ e => { reader = &Facet::view(FirstPair.x, e)
        reader() }, _ => 0 }"#,
        r#"value = match FirstPair(1, 2) { FirstPair @ e => { reader = &e.x
        reader() }, _ => 0 }"#,
        r#"value = match FirstPair(1, 2) { FirstPair(x, x) => x, _ => 0 }"#,
        r#"returned = match FirstPair(1, 2) { FirstPair @ e => {|| e}, _ => {|| SecondPair(3, 4)} }
        escaped = returned()
        escaped.x"#,
        r#"defrecord Envelope(error: Error)
        path = Facet::compose(Envelope.error, FirstPair.x)
        value = Facet::view(path, Envelope(FirstPair(1, 2)))"#,
    ] {
        let source = format!("{PAIR}\n{suffix}");
        match support::resolve_with_builtin_prelude_result(&source) {
            Ok(resolved) => {
                support::typecheck(resolved).expect_err(&source);
            }
            Err(_) => {}
        }
    }
}

#[test]
fn error_payload_suffix_preserves_checked_prefix_enum_templates() {
    let module_stages = support::std_module_stages();
    let declaration_index = sigil::precollect_declaration_index(&module_stages)
        .expect("standard declarations should precollect");
    let source = r#"defstruct PhantomBox<$Tag, $A> { value: $A }
    impl PhantomBox {
        def new::<$Tag>(value: $A) -> PhantomBox<$Tag, $A> { PhantomBox { value } }
    }
    deferror MaybeNumber(value: Option<Int>) {
        |value: Option<Int>|
        Self(message: "number", value)
    }"#;
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("suffix should parse");
    let resolved = support::resolve_staged_program_suffix_with_builtin_prelude(
        &module_stages,
        ast,
        &declaration_index,
        None,
    )
    .expect("suffix should resolve against prefix");
    let program = support::typecheck_resolved_program_suffix_with_builtin_prelude(resolved)
        .expect("suffix should check against prefix");
    assert!(!program
        .nodes
        .iter()
        .any(|node| matches!(node.node, scar::typed::TypedInner::EnumDef(_, _))));
    let (identity, variants) = program
        .enum_definitions
        .iter()
        .find(|(identity, _)| identity.rsplit("::").next() == Some("Option"))
        .expect("prefix Option template must be available even without an emitted EnumDef");
    let identity_template = program
        .nominal_definitions
        .iter()
        .find(|(identity, _)| identity.rsplit("::").next() == Some("Identity"))
        .map(|(_, definition)| definition)
        .expect("prefix Identity template must remain available");
    assert_eq!(identity_template.type_param_vars.len(), 1);
    assert!(identity_template
        .fields
        .iter()
        .any(|(_, ty)| *ty == scar::types::Ty::Var(identity_template.type_param_vars[0])));
    let phantom_template = program
        .nominal_definitions
        .iter()
        .find(|(identity, _)| identity.rsplit("::").next() == Some("PhantomBox"))
        .map(|(_, definition)| definition)
        .expect("new nominal template must remain available");
    assert_eq!(phantom_template.type_param_vars.len(), 2);
    assert_eq!(
        phantom_template.fields,
        vec![(
            "value".into(),
            scar::types::Ty::Var(phantom_template.type_param_vars[1])
        )]
    );
    assert_eq!(variants.len(), 2);
    for variant in variants {
        assert!(matches!(&variant.enum_ty, scar::types::Ty::Enum(name, args)
            if name == identity && args.len() == 1));
    }
    assert!(variants
        .iter()
        .any(|variant| matches!(&variant.payload_types[..], [scar::types::Ty::Var(_)])));
}

#[test]
fn error_payload_accepts_enum_recursion_with_growing_type_arguments() {
    support::typecheck_with_rules(
        r#"defenum Nest<$A> { End, More(Nest<List<$A>>) }
        deferror Nested(value: Nest<Int>) {
            |value: Nest<Int>|
            Self(message: "nested", value)
        }
        error: Error = Nested(Nest<Int>::End)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("a terminating enum variant permits recursive generic payloads");
}

#[test]
fn error_payload_rejects_compile_time_only_values_and_inputs() {
    for source in [
        r#"defrecord User(name: String)
        deferror HeldFacet(value: Facet<InfallibleStructural, User, String, _, _>) {
            Self(message: "held", value: User.name)
        }"#,
        r#"defrecord User(name: String)
        deferror FacetInput {
            |value: Facet<InfallibleStructural, User, String, _, _>|
            "ignored"
        }
        error = FacetInput(User.name)"#,
        r#"defrecord User(name: String)
        deferror NestedFacetInput {
            |value: List<Facet<InfallibleStructural, User, String, _, _>>|
            "ignored"
        }"#,
    ] {
        let error = support::typecheck_with_rules(source, RuntimeSourcePolicy::script())
            .expect_err("Error values and constructor inputs must not transport Facet values");
        assert!(
            error.message.contains("Facet") && error.message.contains("compile-time only"),
            "{error:?}"
        );
    }
    for source in [
        r#"deferror MatchInput { |value: MatchResult<Int>| "ignored" }"#,
        r#"deferror HeldMatch(value: MatchResult<Int>) { |value: MatchResult<Int>| Self(message: "held", value) }"#,
    ] {
        support::typecheck_with_rules(source, RuntimeSourcePolicy::script())
            .expect_err("Error must not transport MatchResult as an ordinary value");
    }
}
