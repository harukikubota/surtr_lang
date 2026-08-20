use super::*;
use sindr::names::{FacetRootKind, TypeIdentity};
use sindr::primitives::int;
use sindr::warning::WarningKind;
use spire::ast::{AstTy, BinOp, Lit};
use spire::parse;

fn permissive_module_rules() -> spire::ParseRules {
    spire::ParseRules::permissive_for_tests()
}

fn parse_module_ast(src: &str, module_path: &str) -> Vec<Ast> {
    let _ = module_path;
    spire::parse_with_context(
        src,
        spire::ParserContext::module(0, Some(module_path.to_string()))
            .with_rules(permissive_module_rules()),
    )
    .expect("definition source should parse")
}

fn parse_and_resolve(src: &str) -> Result<Vec<Resolved>, ResolveError> {
    let ast =
        spire::parse_with_context(src, spire::ParserContext::project(0)).expect("parse failed");
    resolve(ast)
}

fn parse_and_resolve_with_warnings(
    src: &str,
) -> Result<sindr::warning::PhaseOutput<Vec<Resolved>>, ResolveError> {
    let ast =
        spire::parse_with_context(src, spire::ParserContext::project(0)).expect("parse failed");
    resolve_with_warnings(ast)
}

#[test]
fn lower_module_source_ast_attaches_namespace_consts_to_fallback_module() {
    let ast = spire::parse_with_context(
        r#"
const APP_NAME: String = "surtr"
defmod App {
  def main() -> Int { 1 }
}
"#,
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::std_module()),
    )
    .expect("definition source should parse");

    let lowered = lower_module_source_ast(ast, Some("App"));

    assert_eq!(lowered.len(), 1);
    assert_eq!(lowered[0].module_path, "Global::App");
    assert!(matches!(
        lowered[0].ast.first(),
        Some(Ast::ConstDef(_, name, _, _, _)) if name == "APP_NAME"
    ));
}

#[test]
fn staged_modules_from_source_ast_projects_lowered_modules() {
    let ast = spire::parse_with_context(
        r#"
import Helper::help
impl User {
  def use() -> Int { help() }
}
"#,
        spire::ParserContext::module(0, Some("User".to_string()))
            .with_rules(spire::ParseRules::std_module()),
    )
    .expect("definition source should parse");

    let staged = staged_modules_from_source_ast(ast, Some("User"));

    assert_eq!(staged.len(), 1);
    assert_eq!(staged[0].module_path, "Global::User");
    assert!(matches!(staged[0].ast.first(), Some(Ast::Import(_, _, _))));
    assert!(matches!(
        staged[0].ast.get(1),
        Some(Ast::ImplDef(_, _, _, _))
    ));
}

#[test]
fn lower_module_source_ast_preserves_explicit_owner_provenance() {
    let ast = spire::parse_with_context(
        "defmod App { def run() { () } }",
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::module_source()),
    )
    .expect("definition source should parse");
    let expected_span = ast[0].span().clone();

    let lowered = lower_module_source_ast(ast, None);
    let owner = lowered[0]
        .owner
        .as_ref()
        .expect("explicit defmod should preserve its owner");

    assert_eq!(owner.canonical_path, "Global::App");
    assert_eq!(owner.span, expected_span);
    assert_eq!(owner.source_form, OwnerSourceForm::Defmod);
}

#[test]
fn extract_process_modules_from_user_ast_hoists_shared_imports_and_removes_process_defs() {
    let ast = spire::parse_with_context(
        r#"
import Helper::help
defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int) -> Result<Int> { Ok(state) }
}

def main() -> Int { help() }
"#,
        spire::ParserContext::script(0).with_rules(permissive_module_rules()),
    )
    .expect("script source should parse");

    let (process_modules, remaining_ast) = extract_process_modules_from_user_ast(ast);

    assert_eq!(process_modules.len(), 1);
    assert_eq!(process_modules[0].module_path, "Global::Counter");
    assert!(matches!(
        process_modules[0].ast.first(),
        Some(Ast::Import(_, _, _))
    ));
    assert!(process_modules[0].process_spec.is_some());
    assert!(remaining_ast
        .iter()
        .all(|stmt| !matches!(stmt, Ast::Defagent(_, _, _, _, _))));
    assert!(remaining_ast
        .iter()
        .any(|stmt| matches!(stmt, Ast::Import(_, _, _))));
    assert!(remaining_ast
        .iter()
        .any(|stmt| matches!(stmt, Ast::Def(..))));
}

#[test]
fn const_only_fallback_module_path_requires_const_only_source() {
    let const_only = parse_module_ast("import Env::env\nconst VERSION: Int = 1", "Config");
    assert_eq!(
        const_only_fallback_module_path(&const_only, Some("Config")),
        Some("Config")
    );

    let with_def = parse_module_ast(
        "const VERSION: Int = 1\ndef version() -> Int { VERSION }",
        "Config",
    );
    assert_eq!(
        const_only_fallback_module_path(&with_def, Some("Config")),
        None
    );
}

#[test]
fn test_dbg_special_form_resolves_without_name_lookup() {
    let resolved = parse_and_resolve(
        r#"dbg = {|x| x}
value = dbg!(dbg(1), 2)"#,
    )
    .expect("dbg special form should resolve");

    let dbg_node = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::Dbg(_, args) => Some(args),
                _ => None,
            },
            Resolved::Dbg(_, args) => Some(args),
            _ => None,
        })
        .expect("expected resolved dbg node");

    assert_eq!(dbg_node.len(), 2);
}

#[test]
fn test_warning_unused_local_binding_warns() {
    let output = parse_and_resolve_with_warnings(
        r#"def main() -> Int {
  unused = 1
  2
}"#,
    )
    .expect("program should resolve");

    let messages = warning_messages(&output, WarningKind::UnusedVariable);
    assert!(
        messages.iter().any(|message| message.contains("unused")),
        "warnings: {:?}",
        output.warnings
    );
}

#[test]
fn test_warning_used_local_binding_does_not_warn() {
    let output = parse_and_resolve_with_warnings(
        r#"def main() -> Int {
  used = 1
  used
}"#,
    )
    .expect("program should resolve");

    assert!(
        output.warnings.is_empty(),
        "warnings: {:?}",
        output.warnings
    );
}

#[test]
fn test_warning_function_param_and_match_alias_warn() {
    let output = parse_and_resolve_with_warnings(
        r#"def pick(value: Result<Int>, extra: Int) -> Int {
  match value {
    Ok(x) @ whole => x,
    Err(_) => 0,
  }
}"#,
    )
    .expect("program should resolve");

    let messages = warning_messages(&output, WarningKind::UnusedVariable);
    assert!(
        messages.iter().any(|message| message.contains("extra")),
        "warnings: {:?}",
        output.warnings
    );
    assert!(
        messages.iter().any(|message| message.contains("whole")),
        "warnings: {:?}",
        output.warnings
    );
}

#[test]
fn test_warning_wildcard_pattern_does_not_warn() {
    let output = parse_and_resolve_with_warnings(
        r#"def ignore(value: Int) -> Int {
  match value {
    _ => 0,
  }
}"#,
    )
    .expect("program should resolve");

    assert!(
        output.warnings.is_empty(),
        "warnings: {:?}",
        output.warnings
    );
}

fn staged_module(module_path: &str, ast: Vec<Ast>) -> StagedModuleAst {
    StagedModuleAst {
        module_path: module_path.to_string(),
        doc_module_path: None,
        ast,
        owner: None,
        module_doc: None,
        auto_import: matches!(module_path, "Bootstrap" | "Kernel" | "Result"),
        process_spec: None,
    }
}

fn staged_process_module(ast: Vec<Ast>) -> StagedModuleAst {
    match ast.into_iter().next().expect("process module should exist") {
        Ast::Defagent(span, module_path, ast, process_spec, attrs) => StagedModuleAst {
            owner: Some(OwnerDescriptor {
                canonical_path: module_path.clone(),
                span,
                source_form: OwnerSourceForm::Defagent,
            }),
            module_path,
            doc_module_path: None,
            ast,
            module_doc: attrs.doc,
            auto_import: attrs.auto_import,
            process_spec: Some(process_spec),
        },
        Ast::Defgenserver(span, module_path, ast, process_spec, attrs) => StagedModuleAst {
            owner: Some(OwnerDescriptor {
                canonical_path: module_path.clone(),
                span,
                source_form: OwnerSourceForm::Defgenserver,
            }),
            module_path,
            doc_module_path: None,
            ast,
            module_doc: attrs.doc,
            auto_import: attrs.auto_import,
            process_spec: Some(process_spec),
        },
        Ast::Defsupervisor(span, module_path, ast, process_spec, attrs) => StagedModuleAst {
            owner: Some(OwnerDescriptor {
                canonical_path: module_path.clone(),
                span,
                source_form: OwnerSourceForm::Defsupervisor,
            }),
            module_path,
            doc_module_path: None,
            ast,
            module_doc: attrs.doc,
            auto_import: attrs.auto_import,
            process_spec: Some(process_spec),
        },
        Ast::DefdynamicSupervisor(span, module_path, ast, process_spec, attrs) => StagedModuleAst {
            owner: Some(OwnerDescriptor {
                canonical_path: module_path.clone(),
                span,
                source_form: OwnerSourceForm::DefdynamicSupervisor,
            }),
            module_path,
            doc_module_path: None,
            ast,
            module_doc: attrs.doc,
            auto_import: attrs.auto_import,
            process_spec: Some(process_spec),
        },
        other => panic!("expected process module, got {other:?}"),
    }
}

fn staged_auto_import_module(module_path: &str, ast: Vec<Ast>) -> StagedModuleAst {
    StagedModuleAst {
        module_path: module_path.to_string(),
        doc_module_path: None,
        ast,
        owner: None,
        module_doc: None,
        auto_import: true,
        process_spec: None,
    }
}

fn resolve_user_with_modules(
    user_src: &str,
    module_stages: &[Vec<StagedModuleAst>],
) -> Result<Vec<Resolved>, ResolveError> {
    let user_ast = spire::parse_with_context(user_src, spire::ParserContext::project(0))
        .expect("user script should parse");
    let mut full_stages = vec![vec![staged_module(
        "Bootstrap",
        parse_module_ast(
            r#"@builtin def print(a: String) -> Unit
@builtin def to_string(a: $A) -> String
@builtin def inspect(a: $A) -> String
@builtin def safe_div(a: $A, b: $A) -> Result<$A, ZeroDivisionError>
@builtin def safe_mod(a: Int, b: Int) -> Result<Int, ZeroDivisionError>
@builtin def eprint(err: Error) -> Unit
@builtin def set_exit_code(code: Int) -> Unit
deferror NoneError { "none" }
deferror ZeroDivisionError { "division by zero" }
deferror EmptyList { "Empty List." }
deferror IndexOutOfBounds(detail: String) { detail }"#,
            "Bootstrap",
        ),
    )]];
    full_stages.push(vec![
        staged_module(
            "Agent",
            parse_module_ast(
                r#"@hidden
@builtin def pid(owner: $Owner, init: (-> Result<$State>)) -> PID<$Process>
@hidden
@builtin def state(pid: PID<$Process>) -> Result<$State>
@hidden
@builtin def store(pid: PID<$Process>, state: $State) -> Result<Unit>"#,
                "Agent",
            ),
        ),
        staged_module(
            "GenServer",
            parse_module_ast(
                r#"@hidden
@builtin def pid(owner: $Owner, init: (-> Result<$State>)) -> PID<$Process>
@hidden
@builtin def state(pid: PID<$Process>) -> Result<$State>
@hidden
@builtin def store(pid: PID<$Process>, state: $State) -> Result<Unit>"#,
                "GenServer",
            ),
        ),
        staged_module(
            "DynamicSupervisor",
            parse_module_ast(
                r#"@builtin def spawn(worker_init: (-> Result<$State>)) -> Result<PID<$Process>>"#,
                "DynamicSupervisor",
            ),
        ),
    ]);
    full_stages.extend(module_stages.iter().cloned());
    let declaration_index =
        precollect_declaration_index(&full_stages).expect("precollect should succeed");
    resolve_staged_program(
        &full_stages,
        user_ast,
        &declaration_index,
        Some("__Script::fixture".to_string()),
    )
}

fn resolve_user_with_modules_with_warnings(
    user_src: &str,
    module_stages: &[Vec<StagedModuleAst>],
) -> Result<sindr::warning::PhaseOutput<Vec<Resolved>>, ResolveError> {
    let user_ast = spire::parse_with_context(user_src, spire::ParserContext::project(0))
        .expect("user script should parse");
    let mut full_stages = vec![vec![staged_module(
        "Bootstrap",
        parse_module_ast(
            r#"@builtin def print(a: String) -> Unit
@builtin def to_string(a: $A) -> String
@builtin def inspect(a: $A) -> String
@builtin def safe_div(a: $A, b: $A) -> Result<$A, ZeroDivisionError>
@builtin def safe_mod(a: Int, b: Int) -> Result<Int, ZeroDivisionError>
@builtin def eprint(err: Error) -> Unit
@builtin def set_exit_code(code: Int) -> Unit
deferror NoneError { "none" }
deferror ZeroDivisionError { "division by zero" }
deferror EmptyList { "Empty List." }
deferror IndexOutOfBounds(detail: String) { detail }"#,
            "Bootstrap",
        ),
    )]];
    full_stages.push(vec![
        staged_module(
            "Agent",
            parse_module_ast(
                r#"@hidden
@builtin def pid(owner: $Owner, init: (-> Result<$State>)) -> PID<$Process>
@hidden
@builtin def state(pid: PID<$Process>) -> Result<$State>
@hidden
@builtin def store(pid: PID<$Process>, state: $State) -> Result<Unit>"#,
                "Agent",
            ),
        ),
        staged_module(
            "GenServer",
            parse_module_ast(
                r#"@hidden
@builtin def pid(owner: $Owner, init: (-> Result<$State>)) -> PID<$Process>
@hidden
@builtin def state(pid: PID<$Process>) -> Result<$State>
@hidden
@builtin def store(pid: PID<$Process>, state: $State) -> Result<Unit>"#,
                "GenServer",
            ),
        ),
        staged_module(
            "DynamicSupervisor",
            parse_module_ast(
                r#"@builtin def spawn(worker_init: (-> Result<$State>)) -> Result<PID<$Process>>"#,
                "DynamicSupervisor",
            ),
        ),
    ]);
    full_stages.extend(module_stages.iter().cloned());
    let declaration_index =
        precollect_declaration_index(&full_stages).expect("precollect should succeed");
    resolve_staged_program_with_warnings(
        &full_stages,
        user_ast,
        &declaration_index,
        Some("__Script::fixture".to_string()),
    )
}

fn warning_messages(
    output: &sindr::warning::PhaseOutput<Vec<Resolved>>,
    kind: WarningKind,
) -> Vec<String> {
    output
        .warnings
        .iter()
        .filter(|warning| warning.kind == kind)
        .map(|warning| warning.message.clone())
        .collect()
}

#[test]
fn test_precollect_declaration_index_succeeds_without_body_resolution() {
    let module_stages = vec![vec![staged_module(
        "Bootstrap",
        parse_module_ast(
            r#"def to_int(x: String) -> Int { unknown_name }
defrecord Pair(left: Int, right: Int)
deferror Oops(reason: String) { reason }"#,
            "Bootstrap",
        ),
    )]];

    let index = precollect_declaration_index(&module_stages).expect("precollect should succeed");
    assert!(index.contains_key("Bootstrap::to_int"));
    assert!(index.contains_key("Global::Pair"));
    assert!(index.contains_key("Global::Oops"));
}

#[test]
fn test_precollect_builtin_decl_in_module() {
    let module_stages = vec![vec![staged_module(
        "Int",
        parse_module_ast(
            r#"@builtin def shl(value: Int, bits: Int) -> Result<Int, NegativeShiftCount>"#,
            "Int",
        ),
    )]];

    let index = precollect_declaration_index(&module_stages).expect("precollect should succeed");
    assert!(index.contains_key("Int::shl"));
}

#[test]
fn test_resolve_staged_program_keeps_process_specs() {
    let kernel = staged_module(
        "Agent",
        parse_module_ast(
            r#"@hidden
@builtin def pid(owner: $Owner, init: (-> Result<$State>)) -> PID<$Process>

@hidden
@builtin def state(pid: PID<$Process>) -> Result<$State>

@hidden
@builtin def store(pid: PID<$Process>, state: $State) -> Result<Unit>"#,
            "Agent",
        ),
    );
    let ast = spire::parse_with_context(
        r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { 0 }

  @get
  def get(state: Int, _field: String) -> Result<Int> { state }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { next }
}"#,
        spire::ParserContext::module(0, Some("Counter".to_string()))
            .with_rules(permissive_module_rules()),
    )
    .expect("definition source should parse");

    let module = match ast.into_iter().next().expect("lowered module should exist") {
        Ast::Defagent(span, module_path, ast, process_spec, attrs) => StagedModuleAst {
            owner: Some(OwnerDescriptor {
                canonical_path: module_path.clone(),
                span,
                source_form: OwnerSourceForm::Defagent,
            }),
            module_path,
            doc_module_path: None,
            ast,
            module_doc: attrs.doc,
            auto_import: attrs.auto_import,
            process_spec: Some(process_spec),
        },
        other => panic!("expected defagent, got {other:?}"),
    };
    let module_stages = vec![vec![kernel, module]];
    let declaration_index =
        precollect_declaration_index(&module_stages).expect("precollect should succeed");
    let resolved =
        resolve_staged_program_with_state(&module_stages, Vec::new(), &declaration_index, None)
            .expect("resolve should succeed");

    assert_eq!(resolved.process_specs.len(), 1);
    let spec = &resolved.process_specs[0];
    assert_eq!(spec.module_path, "Global::Counter");
    assert_eq!(spec.process_name, "Global::Counter");
}

#[test]
fn test_precollect_declaration_index_rejects_duplicate_fully_qualified_name() {
    let module_stages = vec![vec![
        staged_module(
            "Std::Math",
            parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Std::Math"),
        ),
        staged_module(
            "Std::Math",
            parse_module_ast(r#"def add(a: Int, b: Int) -> Int { a + b }"#, "Std::Math"),
        ),
    ]];

    let err = precollect_declaration_index(&module_stages)
        .expect_err("duplicate fully-qualified declaration must fail");
    assert!(err
        .message
        .contains("Duplicate fully-qualified declaration: Std::Math::add"));
}

#[test]
fn test_precollect_owner_registry_rejects_record_then_module() {
    let ast = spire::parse_with_context(
        r#"defrecord Hoge(a: String)
defmod Hoge { def test() { () } }"#,
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::module_source()),
    )
    .expect("definition source should parse");
    let first_span = ast[0].span().clone();
    let conflicting_span = ast[1].span().clone();
    let staged = staged_modules_from_source_ast(ast, None);
    let module_stages = vec![staged];

    let err = precollect_declaration_index(&module_stages)
        .expect_err("record and module owners must share one namespace");

    assert_eq!(err.message, "Duplicate top-level owner: Hoge");
    assert_eq!(err.span, conflicting_span);
    assert!(err.related_labels.iter().any(|label| {
        label.span == first_span
            && label.message.starts_with("first ")
            && label.message.ends_with(" declaration")
    }));
}

#[test]
fn test_precollect_owner_registry_rejects_module_then_record() {
    let ast = spire::parse_with_context(
        r#"defmod Hoge { def test() { () } }
defrecord Hoge(a: String)"#,
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::module_source()),
    )
    .expect("definition source should parse");
    let first_span = ast[0].span().clone();
    let conflicting_span = ast[1].span().clone();
    let staged = staged_modules_from_source_ast(ast, None);
    let module_stages = vec![staged];

    let err = precollect_declaration_index(&module_stages)
        .expect_err("module and record owners must share one namespace");

    assert_eq!(err.message, "Duplicate top-level owner: Hoge");
    assert_eq!(err.span, conflicting_span);
    assert!(err.related_labels.iter().any(|label| {
        label.span == first_span
            && label.message.starts_with("first ")
            && label.message.ends_with(" declaration")
    }));
}

#[test]
fn test_precollect_owner_registry_maps_direct_and_promoted_identities() {
    let regular_ast = spire::parse_with_context(
        r#"defmod App { def run() { () } }
defstruct User { name: String }
defrecord Pair(left: Int, right: Int)
defenum Choice { First, Second }
deferror Oops { "oops" }
type Mapper<$A, $B> = ($A -> $B)
const FLAG: Int = 1
deftrait Show {
  def show(self: Self) -> String
}
deftrait Applicative
where
  Self: Functor
{
  def pure(value: $A) -> Self<$A>
}
deftrait Functor
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}"#,
        spire::ParserContext::project(0),
    )
    .expect("project source should parse");
    let standard_ast = spire::parse_with_context(
        r#"@builtin type Int
@builtin type List<$A>
@builtin defenum Option<$T> { Some($T), None }
@builtin defenum Result<$T> { Ok($T), Err(Error) }
@builtin defenum Boolean { True, False }"#,
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::std_module()),
    )
    .expect("standard source should parse");
    let regular_stage = regular_ast
        .into_iter()
        .flat_map(|stmt| staged_modules_from_source_ast(vec![stmt], None))
        .collect::<Vec<_>>();
    let standard_stage = standard_ast
        .into_iter()
        .flat_map(|stmt| staged_modules_from_source_ast(vec![stmt], None))
        .collect::<Vec<_>>();
    let staged_owner = |canonical_path: &str, source_form| StagedModuleAst {
        module_path: canonical_path.to_string(),
        doc_module_path: None,
        ast: Vec::new(),
        owner: Some(OwnerDescriptor {
            canonical_path: canonical_path.to_string(),
            span: Span { start: 0, end: 1 },
            source_form,
        }),
        module_doc: None,
        auto_import: false,
        process_spec: None,
    };
    let process_stage = vec![
        staged_owner("Global::Worker", OwnerSourceForm::Defagent),
        staged_owner("Global::Server", OwnerSourceForm::Defgenserver),
        staged_owner("Global::RootSup", OwnerSourceForm::Defsupervisor),
        staged_owner("Global::DynamicSup", OwnerSourceForm::DefdynamicSupervisor),
    ];
    let all_stages = vec![standard_stage, regular_stage, process_stage];

    let precollected =
        precollect_declarations(&all_stages).expect("distinct owners should precollect");
    let owners = &precollected.owner_registry;

    for (name, identity) in [
        ("Int", TypeIdentity::Type),
        ("List", TypeIdentity::TypeConstructor),
        ("Option", TypeIdentity::TypeConstructor),
        ("Result", TypeIdentity::TypeConstructor),
        ("Boolean", TypeIdentity::Enum),
        ("User", TypeIdentity::Struct),
        ("Pair", TypeIdentity::Record),
        ("Choice", TypeIdentity::Enum),
        ("Oops", TypeIdentity::Error),
        ("Mapper", TypeIdentity::Sig),
        ("FLAG", TypeIdentity::Const),
        ("App", TypeIdentity::Mod),
        ("Worker", TypeIdentity::Mod),
        ("Server", TypeIdentity::Mod),
        ("RootSup", TypeIdentity::Supervisor),
        ("DynamicSup", TypeIdentity::Supervisor),
        ("Show", TypeIdentity::Trait),
        ("Functor", TypeIdentity::TypeConstructor),
        ("Applicative", TypeIdentity::TypeConstructor),
    ] {
        assert_eq!(owners.identity_for_owner(name), Some(identity), "{name}");
    }
    assert_eq!(owners.get("Functor").unwrap().kind, OwnerKind::Trait);
    assert_eq!(owners.owner_ref("Option").unwrap().canonical_key, "Option");
    let mut resolver = Resolver::new();
    resolver.owner_registry = owners.clone();
    assert_eq!(
        resolver.owner_identity_for_declaration("App::run", &DeclarationKind::Def, Some("App"),),
        Some(TypeIdentity::Mod)
    );
    assert!(!precollected.declaration_index.contains_key("Global::App"));
    let scope = build_scope_for_module(&all_stages, None, all_stages.len())
        .expect("owner metadata should not change scope construction");
    assert_eq!(scope.lookup("App"), None);
}

#[test]
fn test_owner_registry_does_not_bind_module_root_in_global_scope() {
    let ast = spire::parse_with_context("defmod Hoge {}", spire::ParserContext::project(0))
        .expect("module owner should parse");
    let module_stages = vec![ast
        .into_iter()
        .flat_map(|stmt| staged_modules_from_source_ast(vec![stmt], None))
        .collect::<Vec<_>>()];
    let precollected =
        precollect_declarations(&module_stages).expect("module owner should precollect");
    assert_eq!(
        precollected.owner_registry.identity_for_owner("Hoge"),
        Some(TypeIdentity::Mod)
    );

    let declaration_uids = assign_declaration_uids(&precollected.declaration_index);
    let scope = build_global_scope(&precollected.declaration_index, &declaration_uids);

    assert_eq!(scope.lookup("Hoge"), None);
}

#[test]
fn test_record_head_remains_bound_in_global_scope() {
    let ast = spire::parse_with_context(
        "defrecord Hoge(value: Int)",
        spire::ParserContext::project(0),
    )
    .expect("record owner should parse");
    let module_stages = vec![staged_modules_from_source_ast(ast, None)];
    let precollected =
        precollect_declarations(&module_stages).expect("record owner should precollect");
    let declaration_uids = assign_declaration_uids(&precollected.declaration_index);
    let expected_uid = declaration_uids
        .get("Global::Hoge")
        .copied()
        .expect("record head should have a declaration UID");

    let scope = build_global_scope(&precollected.declaration_index, &declaration_uids);

    assert_eq!(scope.lookup("Hoge"), Some(expected_uid));
}

#[test]
fn test_empty_module_owner_is_not_importable_from_registry_metadata_alone() {
    let ast = spire::parse_with_context("defmod Hoge {}", spire::ParserContext::project(0))
        .expect("module owner should parse");
    let module_stages = vec![staged_modules_from_source_ast(ast, None)];

    let err = resolve_user_with_modules("import Hoge", &module_stages)
        .expect_err("owner metadata alone must not make an empty module importable");

    assert_eq!(err.message, "Unknown module import: Hoge");
}

#[test]
fn test_precollect_namespaced_types_can_coexist() {
    let module_stages = vec![vec![staged_module(
        "",
        parse_module_ast(
            r#"namespace Auth { defrecord User(name: String) }
namespace Billing { defrecord User(name: String) }"#,
            "",
        ),
    )]];

    let index = precollect_declaration_index(&module_stages).expect("precollect should succeed");
    assert!(index.contains_key("Auth::User"));
    assert!(index.contains_key("Billing::User"));
}

#[test]
fn test_precollect_namespaced_duplicate_type_is_rejected() {
    let module_stages = vec![vec![staged_module(
        "",
        parse_module_ast(
            r#"namespace Auth {
  defrecord User(name: String)
  defrecord User(name: String)
}"#,
            "",
        ),
    )]];

    let err = precollect_declaration_index(&module_stages)
        .expect_err("duplicate namespaced type must fail");
    assert_eq!(err.message, "Duplicate top-level owner: Auth::User");
    assert!(err
        .related_labels
        .iter()
        .any(|label| label.message == "first Record declaration"));
}

#[test]
fn test_precollect_declaration_index_is_deterministic_when_stage_input_order_changes() {
    let mod_a = staged_module(
        "Std::A",
        parse_module_ast(r#"def same(x: Int) -> Int { x }"#, "Std::A"),
    );
    let mod_b = staged_module(
        "Std::B",
        parse_module_ast(r#"def same(x: Int) -> Int { x }"#, "Std::B"),
    );

    let index_first = precollect_declaration_index(&[vec![mod_a.clone(), mod_b.clone()]]).unwrap();
    let index_swapped =
        precollect_declaration_index(&[vec![mod_b.clone(), mod_a.clone()]]).unwrap();

    assert_eq!(index_first, index_swapped);
    assert!(index_first.contains_key("Std::A::same"));
    assert!(index_first.contains_key("Std::B::same"));
}

#[test]
fn test_precollect_declaration_index_tracks_bootstrap_std_user_stage_split() {
    let module_stages = vec![
        vec![staged_module(
            "Bootstrap",
            parse_module_ast(r#"deferror NoneError { "none" }"#, "Bootstrap"),
        )],
        vec![staged_module(
            "Std::Math",
            parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Std::Math"),
        )],
        vec![staged_module(
            "User::Main",
            parse_module_ast(r#"def main() -> Int { 1 }"#, "User::Main"),
        )],
    ];

    let index = precollect_declaration_index(&module_stages).expect("precollect should succeed");
    assert_eq!(index["Global::NoneError"].stage_index, 0);
    assert_eq!(index["Std::Math::add"].stage_index, 1);
    assert_eq!(index["User::Main::main"].stage_index, 2);
}

#[test]
fn declaration_uid_order_is_stage_then_fq_name() {
    let module_stages = vec![
        vec![staged_module(
            "User::B",
            parse_module_ast(r#"def beta() -> Int { 1 }"#, "User::B"),
        )],
        vec![
            staged_module(
                "User::Z",
                parse_module_ast(r#"def zeta() -> Int { 1 }"#, "User::Z"),
            ),
            staged_module(
                "User::A",
                parse_module_ast(r#"def alpha() -> Int { 1 }"#, "User::A"),
            ),
        ],
    ];

    let index = precollect_declaration_index(&module_stages).expect("precollect should succeed");

    assert_eq!(
        declaration_uid_order(&index),
        vec![
            "User::B::beta".to_string(),
            "User::A::alpha".to_string(),
            "User::Z::zeta".to_string(),
        ]
    );
}

#[test]
fn declaration_ordering_exposes_stage_metadata() {
    let module_stages = vec![
        vec![staged_module(
            "User::B",
            parse_module_ast(r#"def beta() -> Int { 1 }"#, "User::B"),
        )],
        vec![
            staged_module(
                "User::Z",
                parse_module_ast(r#"def zeta() -> Int { 1 }"#, "User::Z"),
            ),
            staged_module(
                "User::A",
                parse_module_ast(r#"def alpha() -> Int { 1 }"#, "User::A"),
            ),
        ],
    ];

    let index = precollect_declaration_index(&module_stages).expect("precollect should succeed");
    let ordering = declaration_stage_ordering(&index);

    assert_eq!(
        ordering.entries(),
        &[
            StageOrderedDeclaration {
                stage_index: 0,
                fq_name: "User::B::beta".to_string(),
            },
            StageOrderedDeclaration {
                stage_index: 1,
                fq_name: "User::A::alpha".to_string(),
            },
            StageOrderedDeclaration {
                stage_index: 1,
                fq_name: "User::Z::zeta".to_string(),
            },
        ]
    );
    assert_eq!(
        ordering.fq_names(),
        vec![
            "User::B::beta".to_string(),
            "User::A::alpha".to_string(),
            "User::Z::zeta".to_string(),
        ]
    );
}

#[test]
fn test_precollect_impl_methods_as_type_namespace_members() {
    let module_stages = vec![vec![staged_module(
        "",
        parse_module_ast(
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

  defextractor deconstruct(self: Self) -> Option<(String, Int)> {
    Option::None
  }
}"#,
            "",
        ),
    )]];

    let index = precollect_declaration_index(&module_stages).expect("precollect should succeed");
    let ctor = index
        .get("Global::User::new")
        .expect("new should be indexed");
    assert_eq!(ctor.module_path, "Global::User");
    assert_eq!(ctor.name, "new");
    assert_eq!(ctor.kind, DeclarationKind::ImplCtorNew);

    let normalize = index
        .get("Global::User::normalize")
        .expect("normalize should be indexed");
    assert_eq!(normalize.module_path, "Global::User");
    assert_eq!(normalize.name, "normalize");
    assert_eq!(normalize.kind, DeclarationKind::ImplMethod);

    let deconstruct = index
        .get("Global::User::deconstruct")
        .expect("deconstruct should be indexed");
    assert_eq!(deconstruct.module_path, "Global::User");
    assert_eq!(deconstruct.name, "deconstruct");
    assert_eq!(deconstruct.kind, DeclarationKind::Extractor);
}

#[test]
fn test_precollect_impl_extractors_for_enum_types() {
    let module_stages = vec![vec![staged_module(
        "",
        parse_module_ast(
            r#"defenum Light {
  Red,
  Green,
}

impl Light {
  defextractor stop_code(self: Self) -> Option<Int> {
    Option::None
  }
}"#,
            "",
        ),
    )]];

    let index = precollect_declaration_index(&module_stages).expect("precollect should succeed");
    let stop_code = index
        .get("Global::Light::stop_code")
        .expect("enum extractor should be indexed");
    assert_eq!(stop_code.module_path, "Global::Light");
    assert_eq!(stop_code.name, "stop_code");
    assert_eq!(stop_code.kind, DeclarationKind::Extractor);
}

#[test]
fn test_precollect_rejects_multiple_impl_blocks_for_same_type() {
    let module_stages = vec![vec![staged_module(
        "",
        parse_module_ast(
            r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
    User { name: name }
  }
}
impl User {
  def normalize(self) -> Self {
    self
  }
}"#,
            "",
        ),
    )]];

    let err = precollect_declaration_index(&module_stages).expect_err("duplicate impl must fail");
    assert!(err
        .message
        .contains("Multiple impl blocks for `User` are not allowed"));
    assert_eq!(err.related_labels.len(), 2);
    assert_eq!(err.related_labels[0].message, "first definition");
    assert_eq!(err.related_labels[1].message, "conflicting definition");
}

#[test]
fn test_precollect_allows_impl_target_defined_in_another_file_same_stage() {
    let module_stages = vec![vec![
        staged_module(
            "",
            parse_module_ast(
                r#"defstruct User {
  name: String,
}"#,
                "",
            ),
        ),
        staged_module(
            "",
            parse_module_ast(
                r#"impl User {
  def normalize(self) -> Self {
    self
  }
}"#,
                "",
            ),
        ),
    ]];

    let index = precollect_declaration_index(&module_stages).expect("precollect should succeed");
    let normalize = index
        .get("Global::User::normalize")
        .expect("impl method should be indexed");
    assert_eq!(normalize.module_path, "Global::User");
    assert_eq!(normalize.kind, DeclarationKind::ImplMethod);
}

#[test]
fn test_resolve_allows_impl_target_defined_in_another_file_same_stage() {
    let module_stages = vec![vec![
        staged_module(
            "",
            parse_module_ast(
                r#"defstruct User {
  name: String,
}"#,
                "",
            ),
        ),
        staged_module(
            "",
            parse_module_ast(
                r#"impl User {
  def normalize(self) -> Self {
    self
  }
}"#,
                "",
            ),
        ),
    ]];

    let resolved = resolve_user_with_modules("value = 0", &module_stages)
        .expect("split impl in same stage should resolve");
    assert!(resolved.iter().any(
        |node| matches!(node, Resolved::Def(_, id, ..) if id.name == "Global::User::normalize")
    ));
}

#[test]
fn test_resolve_allows_same_stage_import_independent_of_module_order() {
    let consumer = staged_module(
        "Consumer",
        parse_module_ast(
            r#"import Provider::value;

def use_value() -> Int {
  value()
}"#,
            "Consumer",
        ),
    );
    let provider = staged_module(
        "Provider",
        parse_module_ast(
            r#"def value() -> Int {
  41
}"#,
            "Provider",
        ),
    );

    resolve_user_with_modules(
        "print(to_string(Consumer::use_value()))",
        &[vec![consumer.clone(), provider.clone()]],
    )
    .expect("same-stage forward import should resolve");
    resolve_user_with_modules(
        "print(to_string(Consumer::use_value()))",
        &[vec![provider, consumer]],
    )
    .expect("same-stage backward import should resolve");
}

#[test]
fn test_resolve_allows_same_stage_auto_import() {
    let helper = staged_auto_import_module(
        "Helper",
        parse_module_ast(
            r#"def helper() -> Int {
  7
}"#,
            "Helper",
        ),
    );
    let consumer = staged_module(
        "Consumer",
        parse_module_ast(
            r#"def use_helper() -> Int {
  helper()
}"#,
            "Consumer",
        ),
    );

    resolve_user_with_modules(
        "print(to_string(Consumer::use_helper()))",
        &[vec![consumer, helper]],
    )
    .expect("same-stage auto import should resolve");
}

#[test]
fn test_resolve_rejects_future_stage_import() {
    let consumer = staged_module(
        "Consumer",
        parse_module_ast(
            r#"import Provider::value;

def use_value() -> Int {
  value()
}"#,
            "Consumer",
        ),
    );
    let provider = staged_module(
        "Provider",
        parse_module_ast(
            r#"def value() -> Int {
  41
}"#,
            "Provider",
        ),
    );

    let err = resolve_user_with_modules(
        "print(to_string(Consumer::use_value()))",
        &[vec![consumer], vec![provider]],
    )
    .expect_err("future-stage import should still be rejected");
    assert!(err.message.contains("Provider::value"));
}

#[test]
fn test_parallel_stage_resolve_rebases_local_ids() {
    let left = staged_module(
        "Left",
        parse_module_ast(
            r#"def value() -> Int {
  x = 1
  x
}"#,
            "Left",
        ),
    );
    let right = staged_module(
        "Right",
        parse_module_ast(
            r#"def value() -> Int {
  x = 2
  x
}"#,
            "Right",
        ),
    );

    let resolved = resolve_user_with_modules(
        "print(to_string(Left::value() + Right::value()))",
        &[vec![left, right]],
    )
    .expect("same-stage modules should resolve");
    let local_ids = resolved
        .iter()
        .filter_map(|node| match node {
            Resolved::Def(_, id, _, _, _, _, body, _) if id.name == "value" => first_bind_id(body),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(local_ids.len(), 2);
    assert_ne!(local_ids[0], local_ids[1]);
}

fn first_bind_id(node: &Resolved) -> Option<u32> {
    match node {
        Resolved::Bind(_, ResolvedPattern::Var(id), _) => Some(id.unique_id),
        Resolved::Block(_, nodes) => nodes.iter().find_map(first_bind_id),
        _ => None,
    }
}

#[test]
fn test_precollect_allows_impl_for_builtin_type_owner() {
    let module_stages = vec![vec![staged_module(
        "",
        parse_module_ast(
            r#"impl Int {
  def abs_alias(value: Int) -> Int {
    value
  }
}"#,
            "",
        ),
    )]];

    let index = precollect_declaration_index(&module_stages).expect("builtin impl should succeed");
    let method = index
        .get("Global::Int::abs_alias")
        .expect("builtin impl method should be indexed");
    assert_eq!(method.module_path, "Global::Int");
    assert_eq!(method.kind, DeclarationKind::ImplMethod);
}

#[test]
fn test_impl_owner_uses_target_name_not_declaring_module_path() {
    let module_stages = vec![vec![staged_module(
        "Types",
        parse_module_ast(
            r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
    User { name: name }
  }
  def normalize(self) -> Self {
    self
  }
}"#,
            "Types",
        ),
    )]];

    let declaration_index =
        precollect_declaration_index(&module_stages).expect("precollect should succeed");
    assert!(declaration_index.contains_key("Global::User::new"));
    assert!(declaration_index.contains_key("Global::User::normalize"));
    assert!(!declaration_index.contains_key("Types::User::normalize"));

    let resolved = resolve_user_with_modules(
        r#"user = User("alice")
normalized = User::normalize(user)"#,
        &module_stages,
    )
    .expect("qualified impl calls should resolve through type owner");
    assert!(resolved.iter().any(
        |node| matches!(node, Resolved::Def(_, id, ..) if id.name == "Global::User::normalize")
    ));
}

#[test]
fn test_precollect_trait_methods_as_trait_namespace_members() {
    let module_stages = vec![vec![staged_module(
        "Add",
        parse_module_ast(
            r#"deftrait Add {
  def add(self: Self, rhs: Self) -> Self
}"#,
            "Add",
        ),
    )]];

    let index = precollect_declaration_index(&module_stages).expect("precollect should succeed");
    let trait_entry = index.get("Add::Add").expect("trait should be indexed");
    assert_eq!(trait_entry.name, "Add");
    assert_eq!(trait_entry.kind, DeclarationKind::Trait);

    let add = index
        .get("Add::Add::add")
        .expect("trait method should be indexed");
    assert_eq!(add.module_path, "Add");
    assert_eq!(add.name, "Add::add");
    assert_eq!(add.kind, DeclarationKind::TraitMethod);
}

#[test]
fn test_resolve_rejects_multiple_trait_impl_blocks_for_same_pair() {
    let module_stages = vec![vec![staged_module(
        "Metric",
        parse_module_ast(
            r#"deftrait Metric {
  def add(self: Self, rhs: Self) -> Self
}

impl Metric for Int {
  def add(self: Self, rhs: Self) -> Self {
    self + rhs
  }
}

impl Metric for Int {
  def add(self: Self, rhs: Self) -> Self {
    self
  }
}"#,
            "Metric",
        ),
    )]];

    let declaration_index =
        precollect_declaration_index(&module_stages).expect("precollect should succeed");
    let err = resolve_staged_program(
        &module_stages,
        Vec::new(),
        &declaration_index,
        Some("__Script::fixture".to_string()),
    )
    .expect_err("duplicate trait impl pair must fail");
    assert!(err.message.contains("Multiple trait impl blocks for `"));
    assert!(err.message.contains("Metric"));
    assert!(err.message.contains("Int"));
    assert_eq!(err.related_labels.len(), 2);
    assert_eq!(err.related_labels[0].message, "first definition");
    assert_eq!(err.related_labels[1].message, "conflicting definition");
}

#[test]
fn test_resolve_rejects_duplicate_callable_in_trait_impl() {
    let result = parse_and_resolve(
        r#"deftrait Mark {
  def mark(self: Self) -> Int
}

impl Mark for Int {
  def mark(self: Self) -> Int { 1 }
  def mark(self: Self) -> Int { 2 }
}"#,
    );

    let err = result.expect_err("duplicate trait impl method must fail");
    assert!(err.message.contains("Duplicate function `mark`"));
    assert_eq!(err.related_labels.len(), 2);
}

#[test]
fn test_resolve_rejects_public_private_overload_in_inherent_impl() {
    let result = parse_and_resolve(
        r#"defstruct User { name: String }

impl User {
  def name(self: Self) -> String { self.name }
  defp name(self: Self) -> String { self.name }
}"#,
    );

    let err = result.expect_err("def and defp cannot overload each other");
    assert!(err.message.contains("Duplicate function `name`"));
    assert_eq!(err.related_labels.len(), 2);
}

#[test]
fn test_precollect_rejects_impl_target_for_record() {
    let module_stages = vec![vec![staged_module(
        "",
        parse_module_ast(
            r#"defrecord Pair(first: Int, second: Int)
impl Pair {
  def new(first: Int, second: Int) -> Self {
    Pair(first, second)
  }
}"#,
            "",
        ),
    )]];

    let err =
        precollect_declaration_index(&module_stages).expect_err("record impl should be rejected");
    assert!(err
        .message
        .contains("impl target `Global::Pair` must be a standard type, struct, or enum"));
}

#[test]
fn test_precollect_rejects_impl_target_for_cond_clauses_builtin_type() {
    let module_stages = vec![vec![staged_module(
        "",
        parse_module_ast(
            r#"impl CondClauses {
  def noop(self) -> Self { self }
}"#,
            "",
        ),
    )]];

    let err = precollect_declaration_index(&module_stages)
        .expect_err("CondClauses builtin clause type should reject inherent impl");
    assert!(err
        .message
        .contains("impl target `Global::CondClauses` must be a standard type owner or a struct/enum defined in the current stage"));
}

#[test]
fn test_import_new_from_impl_is_rejected() {
    let module_stages = vec![vec![staged_module(
        "",
        parse_module_ast(
            r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
    User { name: name }
  }
  def normalize(self) -> Self {
    self
  }
}"#,
            "",
        ),
    )]];

    let err = resolve_user_with_modules(
        r#"import User::new
User("alice")"#,
        &module_stages,
    )
    .expect_err("new import should fail");
    assert!(err.message.contains("is not importable"));
}

#[test]
fn test_import_root_struct_is_rejected() {
    let module_stages = vec![vec![staged_module(
        "",
        parse_module_ast(
            r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
    User { name: name }
  }
}"#,
            "",
        ),
    )]];

    let err = resolve_user_with_modules(
        r#"import User
value = User("alice")"#,
        &module_stages,
    )
    .expect_err("root struct import should fail");
    assert!(err
        .message
        .contains("Import target `User` is not importable"));
}

#[test]
fn test_root_struct_constructor_call_resolves_without_import() {
    let module_stages = vec![vec![staged_module(
        "",
        parse_module_ast(
            r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
    User { name: name }
  }
}"#,
            "",
        ),
    )]];

    let resolved = resolve_user_with_modules(r#"user = User("alice")"#, &module_stages)
        .expect("root struct constructor should resolve without import");

    let constructor_name = resolved.iter().find_map(|node| match node {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::ConstructorCall(_, rid, _) => Some(rid.name.clone()),
            _ => None,
        },
        _ => None,
    });

    assert_eq!(constructor_name.as_deref(), Some("User::new"));
}

#[test]
fn test_resolve_trait_def_and_impl_preserve_nodes() {
    let ast = parse_module_ast(
        r#"deftrait Add {
  def add(self: Self, rhs: Self) -> Self
}

impl Add for Int {
  def add(self: Self, rhs: Self) -> Self {
    self + rhs
  }
}"#,
        "Add",
    );

    let resolved = resolve(ast).expect("trait nodes should resolve");
    assert!(matches!(
        &resolved[0],
        Resolved::TraitDef(_, id, _, _, methods, _)
            if id.name == "Add"
                && methods.len() == 1
                && methods[0].id.qualified_name.as_deref() == Some("Add::add")
    ));
    assert!(matches!(
        &resolved[1],
        Resolved::TraitImplDef(_, id, _, AstTy::Named(_, target), _, methods)
            if id.name == "Add" && target == "Global::Int" && methods.len() == 1
    ));
}

#[test]
fn test_resolved_trait_ids_use_direct_and_inherited_owner_identity() {
    let ast = parse_module_ast(
        r#"deftrait Functor
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>) -> Self<$A>
}

deftrait Applicative
where
  Self: Functor
{
  def pure(value: $A) -> Self<$A>
}

deftrait Show {
  def show(self: Self) -> String
}"#,
        "Traits",
    );
    let module_stages = vec![vec![staged_module("Traits", ast)]];
    let precollected = precollect_declarations(&module_stages).expect("traits should precollect");
    let declaration_uids = assign_declaration_uids(&precollected.declaration_index);
    let constructor_slots =
        collect_staged_trait_constructor_slots(&module_stages, &declaration_uids);

    for name in ["Functor", "Applicative"] {
        let uid = declaration_uids
            .get(&format!("Traits::{name}"))
            .copied()
            .expect("trait should have a declaration UID");
        assert_eq!(
            constructor_slots.get(&uid).map(Vec::as_slice),
            Some(["$A".to_string()].as_slice()),
            "{name} should retain its constructor slot"
        );
    }

    let resolved = resolve_staged_program(
        &module_stages,
        Vec::new(),
        &precollected.declaration_index,
        None,
    )
    .expect("traits should resolve");

    for (name, expected_identity) in [
        ("Functor", TypeIdentity::TypeConstructor),
        ("Applicative", TypeIdentity::TypeConstructor),
        ("Show", TypeIdentity::Trait),
    ] {
        let id = resolved
            .iter()
            .find_map(|node| match node {
                Resolved::TraitDef(_, id, ..) if id.name == name => Some(id),
                _ => None,
            })
            .expect("resolved trait should be present");
        assert_eq!(
            id.symbol_info.as_ref().map(|info| info.identity),
            Some(expected_identity),
            "{name} should use its owner registry identity"
        );
    }
}

#[test]
fn test_resolved_type_alias_carries_sig_identity_across_ir_boundary() {
    let resolved = parse_and_resolve("type Mapper<$A, $B> = ($A -> $B)")
        .expect("signature alias should resolve");
    let alias = resolved
        .iter()
        .find(|node| matches!(node, Resolved::TypeAlias(..)))
        .expect("resolved signature alias should be present");
    let serialized = serde_json::to_value(alias).expect("resolved alias should serialize");
    let fields = serialized
        .get("TypeAlias")
        .and_then(serde_json::Value::as_array)
        .expect("resolved alias should retain its tuple fields");

    assert_eq!(
        fields
            .get(4)
            .and_then(|identity| identity.get("identity"))
            .and_then(serde_json::Value::as_str),
        Some("Sig"),
        "Sig identity must survive after resolver-private registry state is dropped"
    );
}

#[test]
fn test_generic_trait_impl_member_uses_target_head_owner_identity() {
    let ast = parse_module_ast(
        r#"deftrait Functor
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>) -> Self<$A>
}

defstruct Boxed<$T> { value: $T }

impl Functor for Boxed<$T> {
  def fmap(self: Self<$A>) -> Self<$A> { self }
}"#,
        "GenericOwners",
    );

    let resolved = resolve(ast).expect("generic trait impl should resolve");
    let method_id = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::TraitImplDef(_, _, _, AstTy::Generic(_, target, _), _, methods)
                if target == "Global::Boxed" =>
            {
                methods.first().map(|method| &method.function_id)
            }
            _ => None,
        })
        .expect("generic trait impl method should be present");

    assert_eq!(method_id.name, "Global::Boxed<$T>::fmap");
    assert_eq!(
        method_id.symbol_info.as_ref().map(|info| info.identity),
        Some(TypeIdentity::Struct),
        "the generic application must resolve through the canonical Boxed owner"
    );
}

#[test]
fn test_resolved_members_use_enclosing_or_target_owner_identity() {
    let ast = spire::parse_with_context(
        r#"defmod App {
  def run() -> Int { 1 }
}

defenum Choice { First }

deftrait Show {
  def show(self: Self) -> String
}

defstruct Box { value: Int }

impl Box {
  def normalize(self: Self) -> Self { self }
}

impl Show for Box {
  def show(self: Self) -> String { "box" }
}"#,
        spire::ParserContext::project(0),
    )
    .expect("owners and members should parse");
    let module_stages = vec![ast
        .into_iter()
        .flat_map(|stmt| staged_modules_from_source_ast(vec![stmt], None))
        .collect::<Vec<_>>()];
    let precollected =
        precollect_declarations(&module_stages).expect("owners and members should precollect");
    let resolved = resolve_staged_program(
        &module_stages,
        Vec::new(),
        &precollected.declaration_index,
        None,
    )
    .expect("owners and members should resolve");

    let enum_variant = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::EnumDef(_, id, _, variants, _)
                if global_surface_name(&id.name) == "Choice" =>
            {
                variants.first().map(|variant| &variant.id)
            }
            _ => None,
        })
        .expect("enum variant should resolve");
    assert_eq!(
        enum_variant.symbol_info.as_ref().map(|info| info.identity),
        Some(TypeIdentity::Enum)
    );

    let (trait_id, trait_method) = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::TraitDef(_, id, _, _, methods, _) if id.name == "Show" => {
                Some((id, &methods[0].id))
            }
            _ => None,
        })
        .expect("trait and method should resolve");
    assert_eq!(
        trait_id.symbol_info.as_ref().map(|info| info.identity),
        Some(TypeIdentity::Trait)
    );
    assert_eq!(
        trait_method.symbol_info.as_ref().map(|info| info.identity),
        Some(TypeIdentity::Trait)
    );

    let module_member = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::Def(_, id, ..)
                if id.qualified_name.as_deref() == Some("Global::App::run") =>
            {
                Some(id)
            }
            _ => None,
        })
        .expect("module member should resolve");
    assert_eq!(
        module_member.symbol_info.as_ref().map(|info| info.identity),
        Some(TypeIdentity::Mod)
    );

    let inherent_member = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::Def(_, id, ..)
                if id.qualified_name.as_deref() == Some("Global::Box::normalize") =>
            {
                Some(id)
            }
            _ => None,
        })
        .expect("inherent impl member should resolve");
    assert_eq!(
        inherent_member
            .symbol_info
            .as_ref()
            .map(|info| info.identity),
        Some(TypeIdentity::Struct)
    );

    let (trait_impl_id, trait_impl_member) = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::TraitImplDef(_, id, _, AstTy::Named(_, target), _, methods)
                if id.name == "Show" && target == "Global::Box" =>
            {
                Some((id, &methods[0].function_id))
            }
            _ => None,
        })
        .expect("trait impl and member should resolve");
    assert_eq!(
        trait_impl_id.symbol_info.as_ref().map(|info| info.identity),
        Some(TypeIdentity::Trait)
    );
    assert_eq!(
        trait_impl_member
            .symbol_info
            .as_ref()
            .map(|info| info.identity),
        Some(TypeIdentity::Struct)
    );

    let bare = parse_and_resolve("def helper() -> Int { 1 }")
        .expect("bare top-level definition should resolve");
    let bare_id = match &bare[0] {
        Resolved::Def(_, id, ..) => id,
        other => panic!("expected bare top-level def, got {other:?}"),
    };
    assert_eq!(bare_id.symbol_info, None);
}

#[test]
fn test_resolve_where_clauses_preserve_constraint_kinds_and_trait_ids() {
    let ast = parse_module_ast(
        r#"deftrait Equal {
  def equal(self: Self, rhs: Self) -> Boolean
}

deftrait Functor
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
  where
    $A: Equal
}

def identity(value: $A) -> $A
where
  $A: Equal
{
  value
}

defstruct Boxed<$T> {
  value: $T,
}

impl Functor for Boxed<$T>
where
  $T: Functor.$A
{
  def fmap(self: Self, mapper: ($A -> $B)) -> Boxed<$B>
  where
    $B: Equal
  {
    Boxed { value: mapper(self.value) }
  }
}"#,
        "WhereConstraints",
    );

    let resolved = resolve(ast).expect("where clauses should resolve");
    let equal_id = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::TraitDef(_, id, _, _, _, _) if id.name == "Equal" => Some(id.clone()),
            _ => None,
        })
        .expect("Equal trait id");

    let functor_id = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::TraitDef(_, id, _, where_clause, methods, _) if id.name == "Functor" => {
                let clause = where_clause.as_ref().expect("Functor where clause");
                assert!(matches!(
                    clause.constraints[0].bounds.as_slice(),
                    [ResolvedWhereConstraintRhs::TypeConstructor { slots, .. }]
                        if matches!(slots.as_slice(), [AstTy::Named(_, slot)] if slot == "$A")
                ));
                let method_clause = methods[0]
                    .where_clause
                    .as_ref()
                    .expect("trait method where clause");
                assert!(matches!(
                    method_clause.constraints[0].bounds.as_slice(),
                    [ResolvedWhereConstraintRhs::Trait { trait_id: bound_id, .. }]
                        if bound_id.unique_id == equal_id.unique_id
                ));
                Some(id.clone())
            }
            _ => None,
        })
        .expect("Functor trait id");

    let function_clause = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::Def(_, id, _, _, _, where_clause, _, _) if id.name == "identity" => {
                where_clause.as_ref()
            }
            _ => None,
        })
        .expect("function where clause");
    assert!(matches!(
        function_clause.constraints[0].bounds.as_slice(),
        [ResolvedWhereConstraintRhs::Trait { trait_id: bound_id, .. }]
            if bound_id.unique_id == equal_id.unique_id
    ));

    let impl_clause = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::TraitImplDef(_, id, _, _, where_clause, _)
                if id.unique_id == functor_id.unique_id =>
            {
                where_clause.as_ref()
            }
            _ => None,
        })
        .expect("trait impl where clause");
    assert!(matches!(
        impl_clause.constraints[0].bounds.as_slice(),
        [ResolvedWhereConstraintRhs::TraitSlot {
            trait_id,
            slot_name,
            slot_ordinal,
            ..
        }] if trait_id.unique_id == functor_id.unique_id && slot_name == "$A" && *slot_ordinal == 0
    ));

    let impl_method_clause = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::TraitImplDef(_, id, _, _, _, methods)
                if id.unique_id == functor_id.unique_id =>
            {
                methods[0].where_clause.as_ref()
            }
            _ => None,
        })
        .expect("trait impl method where clause");
    assert!(matches!(
        impl_method_clause.constraints[0].bounds.as_slice(),
        [ResolvedWhereConstraintRhs::Trait { trait_id: bound_id, .. }]
            if bound_id.unique_id == equal_id.unique_id
    ));
}

#[test]
fn test_resolve_rejects_unknown_trait_constructor_slot() {
    let ast = parse_module_ast(
        r#"deftrait Functor
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}

defenum Boxed<$T> {
  Box($T),
}

impl Functor for Boxed<$T>
where
  $T: Functor.$Missing
{
  def fmap(self: Self, mapper: ($A -> $B)) -> Boxed<$B> { self }
}"#,
        "UnknownTraitSlot",
    );
    let err = resolve(ast).expect_err("unknown constructor slots must fail during resolution");
    assert!(err.message.contains("has no constructor slot $Missing"));
}

#[test]
fn test_resolve_trait_default_method_body_can_reference_later_sibling() {
    let ast = parse_module_ast(
        r#"deftrait Metric {
  @doc """Delegates to abs."""
  def magnitude(self: Self) -> Self {
    abs(self)
  }

  def abs(self: Self) -> Self
}"#,
        "Metric",
    );

    let resolved = resolve(ast).expect("trait default bodies should resolve");

    match &resolved[0] {
        Resolved::TraitDef(_, id, _, _, methods, _) => {
            assert_eq!(id.name, "Metric");
            assert_eq!(methods.len(), 2);
            assert_eq!(methods[0].attrs.doc.as_deref(), Some("Delegates to abs."));
            match methods[0].body.as_deref() {
                Some(Resolved::Block(_, stmts)) => match &stmts[0] {
                    Resolved::App(_, func, args) => {
                        assert_eq!(args.len(), 1);
                        match func.as_ref() {
                            Resolved::Var(_, rid) => {
                                assert_eq!(rid.name, "abs");
                                assert_eq!(rid.qualified_name.as_deref(), Some("Metric::abs"));
                            }
                            other => panic!("expected sibling trait method var, got {other:?}"),
                        }
                    }
                    other => panic!("expected app in default body, got {other:?}"),
                },
                other => panic!("expected resolved default body block, got {other:?}"),
            }
            assert!(methods[1].body.is_none());
        }
        other => panic!("Expected TraitDef, got {other:?}"),
    }
}

#[test]
fn test_resolve_trait_impl_builtin_method_preserves_private_name() {
    let ast = parse_module_ast(
        r#"deftrait Add {
  def add(self: Self, rhs: Self) -> Self
}

impl Add for Int {
  @builtin def add(self: Self, rhs: Self) -> Self
}"#,
        "Add",
    );

    let resolved = resolve(ast).expect("trait impl builtin method should resolve");
    assert!(matches!(
        &resolved[1],
        Resolved::TraitImplDef(_, id, _, AstTy::Named(_, target), _, methods)
            if id.name == "Add"
                && target == "Global::Int"
                && methods.len() == 1
                && methods[0].is_builtin
                && methods[0].function_id.qualified_name.as_deref().is_some_and(|name| {
                    name.contains("__traitimpl__") && name.contains("Add")
                })
    ));
}

#[test]
fn test_trait_qualified_call_resolves_via_trait_namespace() {
    let module_stages = vec![vec![staged_module(
        "Metric",
        parse_module_ast(
            r#"deftrait Metric {
  def safe_div(self: Self, rhs: Self) -> Result<Self, Error>
}"#,
            "Metric",
        ),
    )]];

    let resolved = resolve_user_with_modules(r#"result = Metric::safe_div(4, 2)"#, &module_stages)
        .expect("trait-qualified path should resolve");

    match &resolved.last().expect("expected bind node") {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, func, _) => match func.as_ref() {
                Resolved::Var(_, id) => assert_eq!(id.name, "Metric::safe_div"),
                _ => panic!("Expected resolved trait method path"),
            },
            _ => panic!("Expected app"),
        },
        _ => panic!("Expected bind"),
    }
}

#[test]
fn test_constructor_call_sugars_to_type_new_resolution() {
    let resolved = parse_and_resolve(
        r#"defstruct User {
  name: String,
  age: Int,
}
impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }
}
user = User("alice", 30)"#,
    )
    .expect("source should resolve");

    let constructor_name = resolved.iter().find_map(|node| match node {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::ConstructorCall(_, rid, _) => Some(rid.name.clone()),
            _ => None,
        },
        _ => None,
    });
    assert_eq!(constructor_name.as_deref(), Some("User::new"));
}

#[test]
fn test_simple_bind() {
    let resolved = parse_and_resolve("x = 10").unwrap();
    assert_eq!(resolved.len(), 1);
    match &resolved[0] {
        Resolved::Bind(_, ResolvedPattern::Var(id), _) => {
            assert_eq!(id.name, "x");
        }
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_builtin_ref() {
    let resolved = parse_and_resolve("print(to_string(42))").unwrap();
    match &resolved[0] {
        Resolved::App(_, func, _) => match func.as_ref() {
            Resolved::Var(_, id) => assert_eq!(id.name, "print"),
            _ => panic!("Expected Var for print"),
        },
        _ => panic!("Expected App"),
    }
}

#[test]
fn test_builtin_decl_resolution() {
    let ast = spire::parse_with_context(
        "@builtin def print(a: String) -> Unit",
        spire::ParserContext::module(0, Some("Bootstrap".into()))
            .with_rules(spire::ParseRules::std_module()),
    )
    .expect("std module should parse builtin declarations");
    let mut resolver = Resolver::new();
    let resolved = resolver
        .resolve_program(ast)
        .expect("builtin declaration should resolve");
    match &resolved[0] {
        Resolved::BuiltinDecl(_, id, params, ret_ty, _, attrs) => {
            assert_eq!(id.name, "print");
            assert_eq!(id.unique_id, 2); // 0=Ok, 1=Err, 2=print
            assert_eq!(params.len(), 1);
            assert_eq!(
                *attrs,
                ResolvedDeclAttrs {
                    builtin: true,
                    ..ResolvedDeclAttrs::default()
                }
            );
            assert!(matches!(
                ret_ty,
                Some(spire::ast::AstTy::Named(_, ty)) if ty == "Unit"
            ));
        }
        _ => panic!("Expected BuiltinDecl"),
    }
}

#[test]
fn test_hidden_builtin_decl_resolution_preserves_hidden_attr() {
    let ast = spire::parse_with_context(
        "@hidden\n@builtin def __process_sleep(duration: Duration) -> Result<Unit>",
        spire::ParserContext::module(0, Some("Process".into()))
            .with_rules(spire::ParseRules::std_module()),
    )
    .expect("std module should parse hidden builtin declarations");
    let mut resolver = Resolver::new();
    let resolved = resolver
        .resolve_program(ast)
        .expect("hidden builtin declaration should resolve");
    match &resolved[0] {
        Resolved::BuiltinDecl(_, id, _, _, _, attrs) => {
            assert_eq!(id.name, "__process_sleep");
            assert!(attrs.hidden);
        }
        _ => panic!("Expected BuiltinDecl"),
    }
}

#[test]
fn test_private_builtin_impl_member_is_private_surface() {
    let module_stages = vec![vec![staged_module(
        "Generator",
        parse_module_ast(
            r#"@builtin type Generator<$State, $Item>
impl Generator {
  @builtin defp gen_make(idx: Int, items: List<$Item>) -> Generator<$State, $Item>
  def from_list(items: List<$Item>) -> Generator<Unit, $Item> {
    gen_make(0, items)
  }
}"#,
            "Generator",
        ),
    )]];

    resolve_user_with_modules("g = Generator::from_list([1, 2])", &module_stages)
        .expect("public wrapper should resolve private builtin within the same impl");

    let err = resolve_user_with_modules("g = Generator::gen_make(0, [1, 2])", &module_stages)
        .expect_err("private builtin direct call should fail");

    assert!(err.message.contains("Generator::gen_make/2"));
    assert!(
        err.message.contains("is private"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_private_builtin_impl_member_import_is_rejected() {
    let module_stages = vec![vec![staged_module(
        "Generator",
        parse_module_ast(
            r#"@builtin type Generator<$State, $Item>
impl Generator {
  @builtin defp gen_make(idx: Int, items: List<$Item>) -> Generator<$State, $Item>
}"#,
            "Generator",
        ),
    )]];

    let err = resolve_user_with_modules("import Generator::gen_make", &module_stages)
        .expect_err("private builtin import should fail");

    assert!(
        err.message
            .contains("Import target `Generator::gen_make` is private"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_duration_literal_resolves_as_compiler_generated_struct_lit() {
    let ast = spire::parse_with_context(
        r#"defstruct Duration { private millis: Int }
100ms"#,
        spire::ParserContext::project(0),
    )
    .expect("duration literal should parse");
    let mut resolver = Resolver::new();
    let resolved = resolver
        .resolve_program(ast)
        .expect("duration literal should resolve");
    let lowered = resolved
        .iter()
        .find(|node| matches!(node, Resolved::StructLit(_, id, _) if id.compiler_generated))
        .expect("expected compiler-generated Duration struct literal");
    match lowered {
        Resolved::StructLit(_, id, fields) => {
            assert_eq!(id.name, "Duration");
            assert!(id.compiler_generated);
            assert!(matches!(
                fields.as_slice(),
                [ResolvedStructLitField::Explicit(name, Resolved::Lit(_, spire::ast::Lit::Int(value)))]
                    if name == "millis" && *value == sindr::primitives::int(100)
            ));
        }
        other => panic!(
            "Expected compiler-generated Duration struct literal, got {:?}",
            other
        ),
    }
}

#[test]
fn test_struct_literal_shorthand_resolves_to_same_named_local() {
    let ast = spire::parse_with_context(
        r#"defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name, age }
  }
}"#,
        spire::ParserContext::module(0, None),
    )
    .expect("struct shorthand should parse");
    let mut resolver = Resolver::new();
    let resolved = resolver
        .resolve_program(ast)
        .expect("struct shorthand should resolve");
    let lowered = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::Def(_, id, _, _, _, _, body, _)
                if id.qualified_name.as_deref() == Some("Global::User::new") =>
            {
                Some(body.as_ref())
            }
            _ => None,
        })
        .expect("expected impl method body");

    let Resolved::Block(_, stmts) = lowered else {
        panic!("expected block body");
    };
    let Resolved::StructLit(_, _, fields) = &stmts[0] else {
        panic!("expected struct literal");
    };
    assert!(matches!(
        fields.as_slice(),
        [
            ResolvedStructLitField::Shorthand(name, Resolved::Var(_, id1)),
            ResolvedStructLitField::Shorthand(age, Resolved::Var(_, id2))
        ] if name == "name" && age == "age" && id1.name == "name" && id2.name == "age"
    ));
}

#[test]
fn test_builtin_type_decl_resolution() {
    let ast = spire::parse_with_context(
        "@builtin type Int",
        spire::ParserContext::module(0, Some("Bootstrap".into()))
            .with_rules(spire::ParseRules::std_module()),
    )
    .expect("std module should parse builtin type declarations");
    let mut resolver = Resolver::new();
    let resolved = resolver
        .resolve_program(ast)
        .expect("builtin type declaration should resolve");
    match &resolved[0] {
        Resolved::BuiltinTypeDecl(_, id, params, attrs) => {
            assert_eq!(id.name, "Int");
            assert!(params.is_empty());
            assert_eq!(
                *attrs,
                ResolvedDeclAttrs {
                    builtin: true,
                    ..ResolvedDeclAttrs::default()
                }
            );
        }
        _ => panic!("Expected BuiltinTypeDecl"),
    }
}

#[test]
fn test_struct_readonly_metadata_and_fields_resolve() {
    let ast = spire::parse_with_context(
        "@readonly\ndefstruct User { private readonly password: String, readonly name: String }",
        spire::ParserContext::project(0),
    )
    .expect("readonly struct should parse");
    let mut resolver = Resolver::new();
    let resolved = resolver
        .resolve_program(ast)
        .expect("readonly struct should resolve");

    match &resolved[0] {
        Resolved::StructDef(_, id, type_params, fields, attrs) => {
            assert_eq!(id.name, "Global::User");
            assert!(type_params.is_empty());
            assert!(attrs.readonly);
            assert_eq!(fields[0].name, "password");
            assert_eq!(fields[0].visibility, spire::ast::Visibility::Private);
            assert!(fields[0].readonly);
            assert_eq!(fields[1].name, "name");
            assert_eq!(fields[1].visibility, spire::ast::Visibility::Public);
            assert!(fields[1].readonly);
        }
        other => panic!("Expected StructDef, got {other:?}"),
    }
}

#[test]
fn test_generic_struct_type_params_and_fields_resolve() {
    let ast = spire::parse_with_context(
        "defstruct Pair<$A, $B> { left: $A, right: $B }",
        spire::ParserContext::project(0),
    )
    .expect("generic struct should parse");
    let mut resolver = Resolver::new();
    let resolved = resolver
        .resolve_program(ast)
        .expect("generic struct should resolve");

    match &resolved[0] {
        Resolved::StructDef(_, id, type_params, fields, attrs) => {
            assert_eq!(id.name, "Global::Pair");
            assert_eq!(type_params.len(), 2);
            assert_eq!(type_params[0].name, "$A");
            assert_eq!(type_params[1].name, "$B");
            assert_eq!(fields[0].name, "left");
            assert!(matches!(fields[0].ty, spire::ast::AstTy::Named(_, ref ty) if ty == "$A"));
            assert_eq!(fields[1].name, "right");
            assert!(matches!(fields[1].ty, spire::ast::AstTy::Named(_, ref ty) if ty == "$B"));
            assert_eq!(*attrs, ResolvedDeclAttrs::default());
        }
        other => panic!("Expected StructDef, got {other:?}"),
    }
}

#[test]
fn test_module_builtin_can_be_resolved_by_qualified_name() {
    let module_stages = vec![vec![staged_module(
        "Int",
        parse_module_ast(
            r#"@builtin def shl(value: Int, bits: Int) -> Result<Int, NegativeShiftCount>"#,
            "Int",
        ),
    )]];

    let resolved = resolve_user_with_modules("value = Int::shl(2, 3)", &module_stages)
        .expect("qualified builtin should resolve");
    let bind = resolved
        .iter()
        .find(|node| matches!(node, Resolved::Bind(_, _, _)))
        .expect("expected bind in resolved output");
    match bind {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, func, _) => match func.as_ref() {
                Resolved::Var(_, id) => {
                    assert_eq!(id.name, "Int::shl");
                    assert_eq!(id.qualified_name.as_deref(), Some("Int::shl"));
                }
                _ => panic!("Expected builtin var"),
            },
            _ => panic!("Expected app"),
        },
        _ => panic!("Expected bind"),
    }
}

#[test]
fn test_impl_method_in_canonical_module_keeps_same_uid_for_qualified_calls() {
    let module_stages = vec![vec![staged_module(
        "Global::Int",
        parse_module_ast(
            r#"defenum IntBase {
  Dec,
}

impl IntBase {
  def radix(self: Self) -> Int {
    10
  }
}

def parse_base(base: IntBase) -> Int {
  IntBase::radix(base)
}"#,
            "Global::Int",
        ),
    )]];

    let resolved =
        resolve_user_with_modules("", &module_stages).expect("impl method module should resolve");

    let decl_uid = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::Def(_, id, _, _, _, _, _, _)
                if id.qualified_name.as_deref() == Some("Global::IntBase::radix") =>
            {
                Some(id.unique_id)
            }
            _ => None,
        })
        .expect("expected impl method declaration");

    let call_uid = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::Def(_, id, _, _, _, _, body, _) if id.name == "parse_base" => {
                assert_eq!(
                    id.qualified_name.as_deref(),
                    Some("Global::Int::parse_base")
                );
                match body.as_ref() {
                    Resolved::Block(_, stmts) => stmts.iter().find_map(|stmt| match stmt {
                        Resolved::App(_, func, _) => match func.as_ref() {
                            Resolved::Var(_, id)
                                if id.name == "IntBase::radix"
                                    && id.qualified_name.as_deref()
                                        == Some("Global::IntBase::radix") =>
                            {
                                Some(id.unique_id)
                            }
                            _ => None,
                        },
                        _ => None,
                    }),
                    Resolved::App(_, func, _) => match func.as_ref() {
                        Resolved::Var(_, id)
                            if id.name == "IntBase::radix"
                                && id.qualified_name.as_deref()
                                    == Some("Global::IntBase::radix") =>
                        {
                            Some(id.unique_id)
                        }
                        _ => None,
                    },
                    _ => None,
                }
            }
            _ => None,
        })
        .expect("expected qualified impl method call");

    assert_eq!(call_uid, decl_uid);
}

#[test]
fn test_qualified_func_literal_path_resolves_via_module_namespace() {
    let module_stages = vec![vec![staged_module(
        "Boolean",
        parse_module_ast(
            r#"def eq(lhs: Boolean, rhs: Boolean) -> Boolean { lhs }"#,
            "Boolean",
        ),
    )]];

    let resolved = resolve_user_with_modules("value = True `Boolean::eq` False", &module_stages)
        .expect("qualified func literal path should resolve");
    let bind = resolved
        .iter()
        .find(|node| matches!(node, Resolved::Bind(_, _, _)))
        .expect("expected bind");
    match bind {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, func, args) => {
                assert!(matches!(
                    func.as_ref(),
                    Resolved::Var(_, id)
                        if id.name == "Boolean::eq"
                            && id.qualified_name.as_deref() == Some("Boolean::eq")
                ));
                assert_eq!(args.len(), 2);
            }
            other => panic!("Expected app, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_named_args_resolution() {
    let resolved = parse_and_resolve(
        r#"def add(x: Int, y: Int) -> Int { x + y }
result = add(y: 2, x: 1)"#,
    )
    .unwrap();
    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, _, args) => {
                assert!(matches!(&args[0], ResolvedRecordLitArg::Named(n, _) if n == "y"));
                assert!(matches!(&args[1], ResolvedRecordLitArg::Named(n, _) if n == "x"));
            }
            _ => panic!("Expected App"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_function_def_resolution() {
    let resolved = parse_and_resolve(
        r#"def add(x: Int, y: Int) -> Int { x + y }
print(to_string(1))"#,
    )
    .unwrap();
    match &resolved[0] {
        Resolved::Def(_, id, type_params, params, ret_ty, _, body, attrs) => {
            assert_eq!(id.name, "add");
            assert!(type_params.is_empty());
            assert_eq!(params.len(), 2);
            assert_eq!(*attrs, ResolvedDeclAttrs::default());
            assert!(matches!(ret_ty, Some(spire::ast::AstTy::Named(_, ty)) if ty == "Int"));
            assert!(
                matches!(body.as_ref(), Resolved::Block(_, stmts) if matches!(stmts.as_slice(), [Resolved::BinOp(_, _, _, _)]))
            );
        }
        _ => panic!("Expected Def"),
    }
    match &resolved[1] {
        Resolved::App(_, func, _) => match func.as_ref() {
            Resolved::Var(_, id) => assert_eq!(id.name, "print"),
            _ => panic!("Expected Var"),
        },
        _ => panic!("Expected App"),
    }
}

#[test]
fn test_undefined_var() {
    let result = parse_and_resolve("print(unknown_var)");
    assert!(result.is_err());
}

#[test]
fn test_undefined_function_call_uses_callable_message() {
    let err = parse_and_resolve("print(missing_func(1))").expect_err("call to missing function");
    assert!(err.message.contains("Undefined function missing_func/1"));
}

#[test]
fn test_if_conversion() {
    let resolved = parse_and_resolve("x = if(True, 1, 2)").unwrap();
    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Resolved::If(_, _, _, Some(_))));
        }
        _ => panic!("Expected Bind with If"),
    }
}

#[test]
fn test_if_then_conversion() {
    let resolved = parse_and_resolve("x = if_then(True, 1)").unwrap();
    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Resolved::If(_, _, _, None)));
        }
        _ => panic!("Expected Bind with If"),
    }
}

#[test]
fn test_if_let_conversion() {
    let resolved = parse_and_resolve("x = if_let(Ok(1), Ok(v), v, 0)").unwrap();
    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Match(_, _, arms) => {
                assert_eq!(arms.len(), 2);
                assert!(matches!(
                    &arms[0].pattern,
                    ResolvedPattern::Constructor(_, _)
                ));
                assert!(matches!(&arms[1].pattern, ResolvedPattern::Wildcard(_)));
            }
            other => panic!("Expected Match for if_let(...), got {:?}", other),
        },
        _ => panic!("Expected Bind with Match"),
    }
}

#[test]
fn test_if_let_then_conversion() {
    let resolved = parse_and_resolve("x = if_let_then(Ok(1), Ok(v), print(\"ok\"))").unwrap();
    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Match(_, _, arms) => {
                assert_eq!(arms.len(), 2);
                assert!(matches!(&arms[0].body, Resolved::Block(_, _)));
                assert!(matches!(&arms[1].body, Resolved::Lit(_, Lit::Unit)));
            }
            other => panic!("Expected Match for if_let_then(...), got {:?}", other),
        },
        _ => panic!("Expected Bind with Match"),
    }
}

#[test]
fn test_is_match_conversion() {
    let resolved = parse_and_resolve("x = is_match(Ok(1), Ok(_))").unwrap();
    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Match(_, _, arms) => {
                assert_eq!(arms.len(), 2);
                assert!(matches!(&arms[0].body, Resolved::Lit(_, Lit::Bool(true))));
                assert!(matches!(&arms[1].body, Resolved::Lit(_, Lit::Bool(false))));
            }
            other => panic!("Expected Match for is_match(...), got {:?}", other),
        },
        _ => panic!("Expected Bind with Match"),
    }
}

#[test]
fn test_is_match_rejects_binding_variable_pattern() {
    let err = spire::parse_with_context(
        "x = is_match(Ok(1), Ok(v))",
        spire::ParserContext::project(0),
    )
    .expect_err("must fail");
    assert!(err.message().contains("does not allow binding variables"));
}

#[test]
fn test_assert_conversion() {
    let resolved = parse_and_resolve(
        r#"deferror SomeError { "boom" }
x = assert(True, SomeError)"#,
    )
    .unwrap();
    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Resolved::Assert(_, _, _)));
        }
        _ => panic!("Expected Bind with Assert"),
    }
}

#[test]
fn test_ensure_conversion() {
    let resolved = parse_and_resolve(
        r#"def is_even(n: Int) -> Boolean { True }
deferror SomeError { "boom" }
x = ensure(1, &is_even, SomeError)"#,
    )
    .unwrap();
    match &resolved[2] {
        Resolved::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Resolved::Ensure(_, _, _, _)));
        }
        _ => panic!("Expected Bind with Ensure"),
    }
}

#[test]
fn test_recover_kind_constructor_marker_conversion() {
    let resolved = parse_and_resolve(
        r#"deferror Timeout(detail: String) { detail }
x = Result::recover_kind(Err(Timeout("runtime")), Timeout("marker"), {|err| Ok(1)})"#,
    )
    .expect("recover_kind constructor marker should resolve");
    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => {
            assert!(matches!(rhs.as_ref(), Resolved::RecoverKind(_, _, _, _)));
        }
        other => panic!("Expected Bind with RecoverKind, got {other:?}"),
    }
}

#[test]
fn test_and_conversion() {
    let resolved = parse_and_resolve(
        r#"def rhs() -> Boolean { True }
x = and(False, rhs())"#,
    )
    .unwrap();
    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::If(_, cond, then_branch, Some(else_branch)) => {
                assert!(matches!(cond.as_ref(), Resolved::Lit(_, Lit::Bool(false))));
                assert!(matches!(then_branch.as_ref(), Resolved::App(_, _, _)));
                assert!(matches!(
                    else_branch.as_ref(),
                    Resolved::Lit(_, Lit::Bool(false))
                ));
            }
            other => panic!("Expected If for and(...), got {:?}", other),
        },
        _ => panic!("Expected Bind with If"),
    }
}

#[test]
fn test_or_conversion() {
    let resolved = parse_and_resolve(
        r#"def rhs() -> Boolean { False }
x = or(True, rhs())"#,
    )
    .unwrap();
    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::If(_, cond, then_branch, Some(else_branch)) => {
                assert!(matches!(cond.as_ref(), Resolved::Lit(_, Lit::Bool(true))));
                assert!(matches!(
                    then_branch.as_ref(),
                    Resolved::Lit(_, Lit::Bool(true))
                ));
                assert!(matches!(else_branch.as_ref(), Resolved::App(_, _, _)));
            }
            other => panic!("Expected If for or(...), got {:?}", other),
        },
        _ => panic!("Expected Bind with If"),
    }
}

#[test]
fn test_symbolic_and_conversion() {
    let resolved = parse_and_resolve(
        r#"def rhs() -> Boolean { True }
x = False && rhs()"#,
    )
    .unwrap();
    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::If(_, cond, then_branch, Some(else_branch)) => {
                assert!(matches!(cond.as_ref(), Resolved::Lit(_, Lit::Bool(false))));
                assert!(matches!(then_branch.as_ref(), Resolved::App(_, _, _)));
                assert!(matches!(
                    else_branch.as_ref(),
                    Resolved::Lit(_, Lit::Bool(false))
                ));
            }
            other => panic!("Expected If for &&, got {:?}", other),
        },
        _ => panic!("Expected Bind with If"),
    }
}

#[test]
fn test_symbolic_or_conversion() {
    let resolved = parse_and_resolve(
        r#"def rhs() -> Boolean { False }
x = True || rhs()"#,
    )
    .unwrap();
    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::If(_, cond, then_branch, Some(else_branch)) => {
                assert!(matches!(cond.as_ref(), Resolved::Lit(_, Lit::Bool(true))));
                assert!(matches!(
                    then_branch.as_ref(),
                    Resolved::Lit(_, Lit::Bool(true))
                ));
                assert!(matches!(else_branch.as_ref(), Resolved::App(_, _, _)));
            }
            other => panic!("Expected If for ||, got {:?}", other),
        },
        _ => panic!("Expected Bind with If"),
    }
}

#[test]
fn test_symbolic_and_ignores_local_and_binding() {
    let resolved = parse_and_resolve(
        r#"def rhs() -> Boolean { True }
def and(left: Boolean, right: Boolean) -> Boolean { right }
x = False && rhs()"#,
    )
    .expect("symbolic && should still resolve as builtin logic");
    match &resolved[2] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::If(_, cond, then_branch, Some(else_branch)) => {
                assert!(matches!(cond.as_ref(), Resolved::Lit(_, Lit::Bool(false))));
                assert!(matches!(then_branch.as_ref(), Resolved::App(_, _, _)));
                assert!(matches!(
                    else_branch.as_ref(),
                    Resolved::Lit(_, Lit::Bool(false))
                ));
            }
            other => panic!("Expected If for && despite local and, got {:?}", other),
        },
        other => panic!("Expected Bind for && regression, got {:?}", other),
    }
}

#[test]
fn test_symbolic_or_ignores_local_or_binding() {
    let resolved = parse_and_resolve(
        r#"def rhs() -> Boolean { False }
def or(left: Boolean, right: Boolean) -> Boolean { right }
x = True || rhs()"#,
    )
    .expect("symbolic || should still resolve as builtin logic");
    match &resolved[2] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::If(_, cond, then_branch, Some(else_branch)) => {
                assert!(matches!(cond.as_ref(), Resolved::Lit(_, Lit::Bool(true))));
                assert!(matches!(
                    then_branch.as_ref(),
                    Resolved::Lit(_, Lit::Bool(true))
                ));
                assert!(matches!(else_branch.as_ref(), Resolved::App(_, _, _)));
            }
            other => panic!("Expected If for || despite local or, got {:?}", other),
        },
        other => panic!("Expected Bind for || regression, got {:?}", other),
    }
}

#[test]
fn test_bare_and_call_still_uses_local_binding() {
    let resolved = parse_and_resolve(
        r#"def and(left: Boolean, right: Boolean) -> Boolean { right }
x = and(False, True)"#,
    )
    .expect("bare and(...) should still follow normal name resolution");
    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, func, args) => {
                assert!(matches!(
                    func.as_ref(),
                    Resolved::Var(_, id)
                        if id.name == "and" && id.qualified_name.as_deref() == Some("and")
                ));
                assert_eq!(args.len(), 2);
            }
            other => panic!("Expected plain App for bare and(...), got {:?}", other),
        },
        other => panic!("Expected Bind for bare and(...), got {:?}", other),
    }
}

#[test]
fn test_eq_helper_resolves_via_autoimport_trait() {
    let module_stages = vec![vec![staged_module(
        "Eq",
        parse_module_ast(
            r#"@autoimport
deftrait Eq {
  def eq(self: Self, rhs: Self) -> Boolean
}"#,
            "Eq",
        ),
    )]];

    let resolved = resolve_user_with_modules("x = eq(1, 2)", &module_stages)
        .expect("eq helper should resolve");
    let bind = resolved
        .iter()
        .find(|node| matches!(node, Resolved::Bind(_, _, _)))
        .expect("user bind should exist");
    match bind {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, func, args) => {
                assert_eq!(args.len(), 2);
                match func.as_ref() {
                    Resolved::Var(_, id) => {
                        assert_eq!(id.name, "eq");
                        assert_eq!(id.qualified_name.as_deref(), Some("Eq::Eq::eq"));
                    }
                    other => panic!("expected helper var, got {:?}", other),
                }
            }
            other => panic!("expected app, got {:?}", other),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn derive_expands_struct_into_trait_impl_nodes() {
    let resolved = parse_and_resolve(
        r#"def if(flag: Boolean, then_branch: $A, else_branch: $A) -> $A { then_branch }
deftrait Eq {
  def eq(self: Self, rhs: Self) -> Boolean
}
@derive Eq
defstruct User { name: String }
"#,
    )
    .expect("derive should resolve");

    assert!(matches!(resolved[2], Resolved::StructDef(..)));
    assert!(matches!(
        resolved[3],
        Resolved::TraitImplDef(_, ref trait_id, _, _, _, _)
            if trait_id.name == "Eq"
    ));
}

#[test]
fn test_neq_helper_resolves_via_autoimport_trait() {
    let module_stages = vec![vec![staged_module(
        "Neq",
        parse_module_ast(
            r#"@autoimport
deftrait Neq {
  def neq(self: Self, rhs: Self) -> Boolean
}"#,
            "Neq",
        ),
    )]];

    let resolved = resolve_user_with_modules("x = neq(1, 2)", &module_stages)
        .expect("neq helper should resolve");
    let bind = resolved
        .iter()
        .find(|node| matches!(node, Resolved::Bind(_, _, _)))
        .expect("user bind should exist");
    match bind {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, func, args) => {
                assert_eq!(args.len(), 2);
                match func.as_ref() {
                    Resolved::Var(_, id) => {
                        assert_eq!(id.name, "neq");
                        assert_eq!(id.qualified_name.as_deref(), Some("Neq::Neq::neq"));
                    }
                    other => panic!("expected helper var, got {:?}", other),
                }
            }
            other => panic!("expected app, got {:?}", other),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_compare_helper_resolves_via_autoimport_trait() {
    let module_stages = vec![vec![
        staged_module(
            "Ordering",
            parse_module_ast(
                r#"defenum Ordering {
  Less,
  Equal,
  Greater,
}"#,
                "Ordering",
            ),
        ),
        staged_module(
            "Compare",
            parse_module_ast(
                r#"@autoimport
deftrait Compare {
  def compare(self: Self, rhs: Self) -> Ordering
}"#,
                "Compare",
            ),
        ),
    ]];

    let resolved = resolve_user_with_modules("x = compare(1, 2)", &module_stages)
        .expect("compare helper should resolve");
    let bind = resolved
        .iter()
        .find(|node| matches!(node, Resolved::Bind(_, _, _)))
        .expect("user bind should exist");
    match bind {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, func, args) => {
                assert_eq!(args.len(), 2);
                match func.as_ref() {
                    Resolved::Var(_, id) => {
                        assert_eq!(id.name, "compare");
                        assert_eq!(
                            id.qualified_name.as_deref(),
                            Some("Compare::Compare::compare")
                        );
                    }
                    other => panic!("expected helper var, got {:?}", other),
                }
            }
            other => panic!("expected app, got {:?}", other),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_lt_helper_resolves_via_autoimport_trait() {
    let module_stages = vec![vec![staged_module(
        "Ord",
        parse_module_ast(
            r#"@autoimport
deftrait Ord {
  def lt(self: Self, rhs: Self) -> Boolean
  def lte(self: Self, rhs: Self) -> Boolean
  def gt(self: Self, rhs: Self) -> Boolean
  def gte(self: Self, rhs: Self) -> Boolean
}"#,
            "Ord",
        ),
    )]];

    let resolved = resolve_user_with_modules("x = lt(1, 2)", &module_stages)
        .expect("lt helper should resolve");
    let bind = resolved
        .iter()
        .find(|node| matches!(node, Resolved::Bind(_, _, _)))
        .expect("user bind should exist");
    match bind {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, func, args) => {
                assert_eq!(args.len(), 2);
                match func.as_ref() {
                    Resolved::Var(_, id) => {
                        assert_eq!(id.name, "lt");
                        assert_eq!(id.qualified_name.as_deref(), Some("Ord::Ord::lt"));
                    }
                    other => panic!("expected helper var, got {:?}", other),
                }
            }
            other => panic!("expected app, got {:?}", other),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_concat_helper_resolves_via_autoimport_trait() {
    let module_stages = vec![vec![staged_module(
        "Concat",
        parse_module_ast(
            r#"@autoimport
deftrait Concat {
  def concat(self: Self, rhs: Self) -> Self
}"#,
            "Concat",
        ),
    )]];

    let resolved = resolve_user_with_modules(r#"x = concat("a", "b")"#, &module_stages)
        .expect("concat helper should resolve");
    let bind = resolved
        .iter()
        .find(|node| matches!(node, Resolved::Bind(_, _, _)))
        .expect("user bind should exist");
    match bind {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, func, args) => {
                assert_eq!(args.len(), 2);
                match func.as_ref() {
                    Resolved::Var(_, id) => {
                        assert_eq!(id.name, "concat");
                        assert_eq!(id.qualified_name.as_deref(), Some("Concat::Concat::concat"));
                    }
                    other => panic!("expected helper var, got {:?}", other),
                }
            }
            other => panic!("expected app, got {:?}", other),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_and_named_arg_is_error() {
    let err =
        parse_and_resolve("x = and(left: True, right: False)").expect_err("named args must fail");
    assert!(err.message.contains("and does not accept named argument"));
}

#[test]
fn test_eq_wrong_arity_resolves_as_regular_app() {
    let module_stages = vec![vec![staged_module(
        "Eq",
        parse_module_ast(
            r#"@autoimport
deftrait Eq {
  def eq(self: Self, rhs: Self) -> Boolean
}"#,
            "Eq",
        ),
    )]];

    let resolved =
        resolve_user_with_modules("x = eq(1)", &module_stages).expect("eq call should resolve");
    let bind = resolved
        .iter()
        .find(|node| matches!(node, Resolved::Bind(_, _, _)))
        .expect("user bind should exist");
    match bind {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, _, args) => assert_eq!(args.len(), 1),
            other => panic!("expected app, got {:?}", other),
        },
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn test_concat_named_arg_resolves_as_regular_app() {
    let module_stages = vec![vec![staged_module(
        "Concat",
        parse_module_ast(
            r#"@autoimport
deftrait Concat {
  def concat(self: Self, rhs: Self) -> Self
}"#,
            "Concat",
        ),
    )]];

    let resolved =
        resolve_user_with_modules(r#"x = concat(left: "a", right: "b")"#, &module_stages)
            .expect("concat call should resolve");
    let bind = resolved
        .iter()
        .find(|node| matches!(node, Resolved::Bind(_, _, _)))
        .expect("user bind should exist");
    match bind {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, _, args) => assert!(matches!(
                args.as_slice(),
                [
                    ResolvedRecordLitArg::Named(_, _),
                    ResolvedRecordLitArg::Named(_, _)
                ]
            )),
            other => panic!("expected app, got {:?}", other),
        },
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn test_duplicate_top_level_def_is_error() {
    let result = parse_and_resolve("def f() -> Int { 1 }\ndef f() -> Int { 2 }");
    let err = result.expect_err("duplicate def must fail");
    assert!(err.message.contains("Duplicate top-level definition: f"));
}

#[test]
fn test_forward_reference_to_function_resolves_to_same_unique_id() {
    let resolved = parse_and_resolve(
        r#"result = add(1, 2)
def add(x: Int, y: Int) -> Int { x + y }"#,
    )
    .unwrap();

    let call_id = match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, func, _) => match func.as_ref() {
                Resolved::Var(_, id) => id.unique_id,
                _ => panic!("Expected function variable in App"),
            },
            _ => panic!("Expected App on forward function reference"),
        },
        _ => panic!("Expected Bind"),
    };

    let def_id = match &resolved[1] {
        Resolved::Def(_, id, _, _, _, _, _, _) => id.unique_id,
        _ => panic!("Expected Def"),
    };

    assert_eq!(call_id, def_id);
}

#[test]
fn test_forward_reference_to_struct_literal_resolves_to_same_unique_id() {
    let resolved = parse_and_resolve(
        r#"user = User { name: "alice", age: 30 }
defstruct User {
  name: String,
  age: Int,
}"#,
    )
    .unwrap();

    let lit_id = match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::StructLit(_, id, _) => id.unique_id,
            _ => panic!("Expected StructLit"),
        },
        _ => panic!("Expected Bind"),
    };

    let def_id = match &resolved[1] {
        Resolved::StructDef(_, id, ..) => id.unique_id,
        _ => panic!("Expected StructDef"),
    };

    assert_eq!(lit_id, def_id);
}

#[test]
fn test_forward_reference_to_record_constructor_resolves_to_same_unique_id() {
    let resolved = parse_and_resolve(
        r#"point = Point(1.0, 2.0)
defrecord Point(x: Float, y: Float)"#,
    )
    .unwrap();

    let ctor_id = match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::ConstructorCall(_, id, _) => id.unique_id,
            _ => panic!("Expected ConstructorCall"),
        },
        _ => panic!("Expected Bind"),
    };

    let def_id = match &resolved[1] {
        Resolved::RecordDef(_, id, _, _) => id.unique_id,
        _ => panic!("Expected RecordDef"),
    };

    assert_eq!(ctor_id, def_id);
}

#[test]
fn test_forward_reference_to_deferror_constructor_resolves_to_same_unique_id() {
    let resolved = parse_and_resolve(
        r#"err = PageNotFound("404")
deferror PageNotFound(html: String) {
  "Page Not Found. #{html}"
}"#,
    )
    .unwrap();

    let ctor_id = match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::ConstructorCall(_, id, _) => id.unique_id,
            _ => panic!("Expected ConstructorCall"),
        },
        _ => panic!("Expected Bind"),
    };

    let def_id = match &resolved[1] {
        Resolved::DeferrorDef(_, id, _, _) => id.unique_id,
        _ => panic!("Expected DeferrorDef"),
    };

    assert_eq!(ctor_id, def_id);
}

#[test]
fn test_forward_reference_unique_ids_are_deterministic_across_runs() {
    let source = r#"result = build_user("alice")
point = Point(1, 2)
err = NotFound("404")

def build_user(name: String) -> String { name }
defrecord Point(x: Int, y: Int)
deferror NotFound(code: String) {
  "missing #{code}"
}"#;

    let first = parse_and_resolve(source).unwrap();
    let second = parse_and_resolve(source).unwrap();

    fn collect_top_level_ids(nodes: &[Resolved]) -> Vec<u32> {
        nodes
            .iter()
            .flat_map(|node| match node {
                Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                    Resolved::App(_, func, _) => match func.as_ref() {
                        Resolved::Var(_, id) => vec![id.unique_id],
                        _ => Vec::new(),
                    },
                    Resolved::ConstructorCall(_, id, _) | Resolved::StructLit(_, id, _) => {
                        vec![id.unique_id]
                    }
                    _ => Vec::new(),
                },
                Resolved::Def(_, id, _, _, _, _, _, _)
                | Resolved::RecordDef(_, id, _, _)
                | Resolved::StructDef(_, id, ..)
                | Resolved::DeferrorDef(_, id, _, _) => vec![id.unique_id],
                _ => Vec::new(),
            })
            .collect()
    }

    assert_eq!(
        collect_top_level_ids(&first),
        collect_top_level_ids(&second)
    );
}

#[test]
fn test_unresolved_forward_constructor_is_error() {
    let result = parse_and_resolve(r#"value = MissingType(1)"#);
    let err = result.expect_err("unknown forward constructor must fail");
    assert!(err.message.contains("Undefined type: MissingType"));
}

#[test]
fn test_duplicate_top_level_struct_is_error() {
    let result = parse_and_resolve(
        r#"defstruct User { name: String }
defstruct User { name: String }"#,
    );
    let err = result.expect_err("duplicate struct must fail");
    assert_eq!(err.message, "Duplicate top-level owner: User");
    assert!(err
        .related_labels
        .iter()
        .any(|label| label.message == "first Struct declaration"));
}

#[test]
fn test_shadowing() {
    let resolved = parse_and_resolve("x = 1\nx = x + 1").unwrap();
    // The second x should have a different unique_id
    match (&resolved[0], &resolved[1]) {
        (
            Resolved::Bind(_, ResolvedPattern::Var(id1), _),
            Resolved::Bind(_, ResolvedPattern::Var(id2), _),
        ) => {
            assert_ne!(id1.unique_id, id2.unique_id);
        }
        _ => panic!("Expected two Binds"),
    }
}

#[test]
fn test_top_level_def_cannot_capture_top_level_value_binding() {
    let err = parse_and_resolve("x = 1\ndef f() -> Int { x }")
        .expect_err("top-level def capture must fail");
    assert!(
        err.message
            .contains("Top-level definition `f` cannot reference value binding `x`"),
        "{}",
        err.message
    );
}

#[test]
fn test_top_level_def_param_shadowing_still_resolves() {
    parse_and_resolve("x = 1\ndef f(x: Int) -> Int { x }")
        .expect("function params should shadow top-level bindings");
}

#[test]
fn test_match_wildcard_and_literals() {
    let resolved = parse_and_resolve(
        r#"s = "a"
x = match s {
  "a" => 1,
  2 => 2,
  _ => 0,
}"#,
    )
    .unwrap();
    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Match(_, _, arms) => {
                assert!(matches!(
                    &arms[0].pattern,
                    ResolvedPattern::StrLit(_, s) if s == "a"
                ));
                assert!(matches!(
                    &arms[1].pattern,
                    ResolvedPattern::IntLit(_, n) if n == &int(2)
                ));
                assert!(matches!(&arms[2].pattern, ResolvedPattern::Wildcard(_)));
            }
            _ => panic!("Expected Match"),
        },
        _ => panic!("Expected Bind with Match"),
    }
}

#[test]
fn test_match_qualified_boolean_constructor_patterns_resolve() {
    let resolved = parse_and_resolve(
        r#"flag = True
x = match flag {
  Boolean::True => 1,
  Boolean::False => 0,
}"#,
    )
    .unwrap();
    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Match(_, _, arms) => {
                assert!(matches!(
                    &arms[0].pattern,
                    ResolvedPattern::Constructor(ctor, inner)
                        if ctor.name == "Boolean::True" && inner.is_empty()
                ));
                assert!(matches!(
                    &arms[1].pattern,
                    ResolvedPattern::Constructor(ctor, inner)
                        if ctor.name == "Boolean::False" && inner.is_empty()
                ));
            }
            _ => panic!("Expected Match"),
        },
        _ => panic!("Expected Bind with Match"),
    }
}

#[test]
fn test_closure_and_capture_resolution() {
    let resolved = parse_and_resolve(
        r#"x = 1
f = {|y| x + y}
g = &print"#,
    )
    .unwrap();
    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Closure(_, params, captures, body) => {
                assert_eq!(params.len(), 1);
                assert_eq!(captures.len(), 1);
                assert!(matches!(
                    body.as_ref(),
                    Resolved::BinOp(_, BinOp::Add, _, _)
                ));
            }
            _ => panic!("Expected Closure"),
        },
        _ => panic!("Expected Bind"),
    }
    match &resolved[2] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Capture(_, target, args) => {
                assert!(args.is_empty());
                assert!(matches!(target.as_ref(), Resolved::Var(_, id) if id.name == "print"));
            }
            _ => panic!("Expected Capture"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_capture_placeholder_lowers_to_closure() {
    let resolved =
        parse_and_resolve("def add(x: Int, y: Int) -> Int { x + y }\ninc = &add(&1, 1)").unwrap();
    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Closure(_, params, _, body) => {
                assert_eq!(params.len(), 1);
                assert!(matches!(body.as_ref(), Resolved::App(_, _, _)));
            }
            other => panic!("Expected lowered closure, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_backtick_name_capture_resolves_like_plain_capture() {
    let resolved =
        parse_and_resolve("captured = &`print`").expect("backtick capture should resolve");
    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Capture(_, target, args) => {
                assert!(args.is_empty());
                assert!(matches!(target.as_ref(), Resolved::Var(_, id) if id.name == "print"));
            }
            other => panic!("Expected capture, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_backtick_qualified_capture_resolves_like_plain_capture() {
    let module_stages = vec![vec![staged_module(
        "Boolean",
        parse_module_ast(r#"def not(value: Boolean) -> Boolean { value }"#, "Boolean"),
    )]];

    let resolved = resolve_user_with_modules("captured = &`Boolean::not`", &module_stages)
        .expect("qualified backtick capture should resolve");
    let bind = resolved
        .iter()
        .find(|node| matches!(node, Resolved::Bind(_, _, _)))
        .expect("expected bind");
    match bind {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Capture(_, target, args) => {
                assert!(args.is_empty());
                assert!(matches!(
                    target.as_ref(),
                    Resolved::Var(_, id)
                        if id.name == "Boolean::not"
                            && id.qualified_name.as_deref() == Some("Boolean::not")
                ));
            }
            other => panic!("Expected capture, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_backtick_operator_capture_lowers_to_closure() {
    let resolved =
        parse_and_resolve("inc = &`+`(&1, 1)\nadd = &`+`").expect("operator capture should lower");

    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Closure(_, params, _, body) => {
                assert_eq!(params.len(), 1);
                assert!(matches!(
                    body.as_ref(),
                    Resolved::BinOp(_, BinOp::Add, _, _)
                ));
            }
            other => panic!("Expected lowered closure, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }

    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Closure(_, params, _, body) => {
                assert_eq!(params.len(), 2);
                assert!(matches!(
                    body.as_ref(),
                    Resolved::BinOp(_, BinOp::Add, _, _)
                ));
            }
            other => panic!("Expected lowered closure, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_backtick_pair_constructor_capture_and_pipeline_lower_to_tuple_closures() {
    let resolved = parse_and_resolve(
        "pair = &`(,)`\npiped = 1 |> `(,)`(\"one\")\nmapped = [1] |*> `(,)`(\"tag\")\nbound = Ok(1) |>= `(,)`(\"tag\")",
    )
    .expect("quoted pair constructor forms should resolve");

    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Closure(_, params, _, body) => {
                assert_eq!(params.len(), 2);
                assert!(
                    matches!(body.as_ref(), Resolved::TupleLiteral(_, items) if items.len() == 2)
                );
            }
            other => panic!("Expected pair closure, got {other:?}"),
        },
        other => panic!("Expected pair bind, got {other:?}"),
    }

    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Pipe(_, _, right) => match right.as_ref() {
                Resolved::Closure(_, params, _, body) => {
                    assert_eq!(params.len(), 1);
                    assert!(
                        matches!(body.as_ref(), Resolved::TupleLiteral(_, items) if items.len() == 2)
                    );
                }
                other => panic!("Expected pipeline pair closure, got {other:?}"),
            },
            other => panic!("Expected pair pipeline, got {other:?}"),
        },
        other => panic!("Expected pipeline bind, got {other:?}"),
    }

    for node in &resolved[2..] {
        let Resolved::Bind(_, _, rhs) = node else {
            panic!("Expected contextual pipeline bind, got {node:?}");
        };
        let right = match rhs.as_ref() {
            Resolved::ContextMap(_, _, right) | Resolved::ContextBind(_, _, right) => right,
            other => panic!("Expected contextual pipeline, got {other:?}"),
        };
        assert!(
            matches!(right.as_ref(), Resolved::Closure(_, params, _, body)
            if params.len() == 1
                && matches!(body.as_ref(), Resolved::TupleLiteral(_, items) if items.len() == 2))
        );
    }
}

#[test]
fn test_pipe_slot_lowers_to_closure() {
    let resolved =
        parse_and_resolve("def add(x: Int, y: Int) -> Int { x + y }\nout = 1 |> add(10, _1)")
            .unwrap();
    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Pipe(_, _, right) => {
                assert!(matches!(right.as_ref(), Resolved::Closure(_, params, _, _)
                    if params.len() == 1));
            }
            other => panic!("Expected pipe, got {:?}", other),
        },
        other => panic!("Expected bind, got {:?}", other),
    }
}

#[test]
fn test_nested_capture_argument_block_is_rejected_inside_placeholder_capture() {
    let err = parse_and_resolve("bad = &outer(&1, &inner(1))")
        .expect_err("nested capture argument block must fail");
    assert!(err
        .message
        .contains("nested capture argument blocks are not allowed"));
}

#[test]
fn test_pipe_slot_cannot_be_used_more_than_once() {
    let err = parse_and_resolve("def add(x: Int, y: Int) -> Int { x + y }\nbad = 1 |> add(_1, _1)")
        .expect_err("duplicate pipe slot must fail");
    assert!(err.message.contains("can only be used once"));
}

#[test]
fn test_grouped_pipe_rhs_resolution_preserves_call_marker() {
    let resolved = parse_and_resolve(
        r#"def mk() -> (Int -> Int) { {|x| x} }
out = 1 |> (mk())"#,
    )
    .unwrap();
    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Pipe(_, _, right) => assert!(matches!(
                right.as_ref(),
                Resolved::Grouped(_, inner) if matches!(inner.as_ref(), Resolved::App(_, _, _))
            )),
            other => panic!("Expected Pipe, got {:?}", other),
        },
        other => panic!("Expected Bind, got {:?}", other),
    }
}

#[test]
fn test_safebind_resolution() {
    let resolved = parse_and_resolve("num =? Ok(1)").unwrap();
    match &resolved[0] {
        Resolved::SafeBind(_, ResolvedPattern::Var(id), rhs) => {
            assert_eq!(id.name, "num");
            assert!(matches!(rhs.as_ref(), Resolved::ConstructorCall(_, _, _)));
        }
        _ => panic!("Expected SafeBind"),
    }
}

#[test]
fn test_underscore_prefixed_pattern_does_not_bind() {
    let err = parse_and_resolve("_discard = 1\n_discard")
        .expect_err("underscore-prefixed patterns must not introduce a name");

    assert!(err.message.contains("Undefined variable: _discard"));
}

#[test]
fn test_safebind_constructor_pattern_resolution() {
    let resolved = parse_and_resolve(
        r#"value: Result<Result<Int>> = Ok(Ok(1))
Ok(num) =? value"#,
    )
    .unwrap();
    match &resolved[0] {
        Resolved::Bind(_, _, _) => {}
        _ => panic!("Expected prelude bind"),
    }
    match &resolved[1] {
        Resolved::SafeBind(_, ResolvedPattern::Constructor(ctor, inner), rhs) => {
            assert_eq!(ctor.name, "Ok");
            assert!(matches!(inner.as_slice(), [ResolvedPattern::Var(id)] if id.name == "num"));
            assert!(matches!(rhs.as_ref(), Resolved::Var(_, id) if id.name == "value"));
        }
        _ => panic!("Expected SafeBind with constructor pattern"),
    }
}

#[test]
fn test_safebind_list_with_constructor_literal_pattern_resolution() {
    let resolved = parse_and_resolve(
        r#"lr: Result<List<Result<Int>>> = Ok([Ok(1), Ok(2)])
[Ok(1), ..tail] =? lr"#,
    )
    .unwrap();
    match &resolved[0] {
        Resolved::Bind(_, _, _) => {}
        _ => panic!("Expected prelude bind"),
    }
    match &resolved[1] {
        Resolved::SafeBind(_, ResolvedPattern::ListCons(head, tail), rhs) => {
            assert!(matches!(
                head.as_ref(),
                ResolvedPattern::Constructor(ctor, inner)
                    if ctor.name == "Ok"
                    && matches!(inner.as_slice(), [ResolvedPattern::IntLit(_, n)] if n == &int(1))
            ));
            assert!(matches!(tail.as_ref(), ResolvedPattern::Var(id) if id.name == "tail"));
            assert!(matches!(rhs.as_ref(), Resolved::Var(_, id) if id.name == "lr"));
        }
        _ => panic!("Expected SafeBind list constructor pattern"),
    }
}

#[test]
fn test_as_pattern_resolution() {
    let resolved = parse_and_resolve(
        r#"value: Result<List<Int>> = Ok([1, 2, 3])
[head, ..tail] @ list_dup: List<Int> =? value"#,
    )
    .unwrap();
    match &resolved[1] {
        Resolved::SafeBind(_, ResolvedPattern::As(inner, alias, Some(_)), rhs) => {
            assert_eq!(alias.name, "list_dup");
            assert!(matches!(inner.as_ref(), ResolvedPattern::ListCons(_, _)));
            assert!(matches!(rhs.as_ref(), Resolved::Var(_, id) if id.name == "value"));
        }
        _ => panic!("Expected SafeBind with as-pattern"),
    }
}

#[test]
fn test_duplicate_binding_in_pattern_is_error() {
    let err = parse_and_resolve(
        r#"value: Result<List<Int>> = Ok([1, 2, 3])
[head, ..tail] @ head =? value"#,
    )
    .expect_err("duplicate pattern binding should fail");
    assert!(err.message.contains("Duplicate binding in pattern: head"));
}

#[test]
fn test_duplicate_binding_collects_occurrences_in_preorder_up_to_five() {
    let err = parse_and_resolve("(x, x, x, x, x, x) = (1, 2, 3, 4, 5, 6)")
        .expect_err("duplicate pattern bindings should fail");

    assert_eq!(
        err.related_labels
            .iter()
            .map(|label| label.message.as_str())
            .collect::<Vec<_>>(),
        ["first", "second", "third", "fourth", "fifth"]
    );
    assert_eq!(
        err.related_labels
            .iter()
            .map(|label| (label.span.start, label.span.end))
            .collect::<Vec<_>>(),
        [(1, 2), (4, 5), (7, 8), (10, 11), (13, 14)]
    );
}

#[test]
fn test_duplicate_binding_uses_binding_detection_order() {
    let err = parse_and_resolve("((s, s) @ s, s) @ s = ((1, 3), 2)")
        .expect_err("duplicate pattern bindings should fail");

    assert_eq!(
        err.related_labels
            .iter()
            .map(|label| (label.message.as_str(), label.span.start, label.span.end))
            .collect::<Vec<_>>(),
        [
            ("first", 13, 14),
            ("second", 18, 19),
            ("third", 2, 3),
            ("fourth", 5, 6),
            ("fifth", 10, 11),
        ]
    );
}

#[test]
fn test_duplicate_binding_orders_enum_constructor_like_tuple_children() {
    let source = r#"defenum Pair { Pair(Int, Int), }
Pair(x @ x, x) @ x = Pair(1, 2)"#;
    let err = parse_and_resolve(source).expect_err("duplicate pattern bindings should fail");
    let spans = source
        .match_indices('x')
        .map(|(start, _)| (start, start + 1))
        .collect::<Vec<_>>();

    assert_eq!(
        err.related_labels
            .iter()
            .map(|label| (label.span.start, label.span.end))
            .collect::<Vec<_>>(),
        [spans[0], spans[2], spans[3], spans[1]],
    );
}

#[test]
fn test_duplicate_binding_moves_outer_as_after_left_as_inner_binding() {
    let err = parse_and_resolve("(s @ s, (s, s)) @ s = (1, (2, 3))")
        .expect_err("duplicate pattern bindings should fail");

    assert_eq!(
        err.related_labels
            .iter()
            .map(|label| (label.message.as_str(), label.span.start, label.span.end))
            .collect::<Vec<_>>(),
        [
            ("first", 1, 2),
            ("second", 18, 19),
            ("third", 5, 6),
            ("fourth", 9, 10),
            ("fifth", 12, 13),
        ]
    );
}

#[test]
fn test_duplicate_binding_applies_same_order_to_list_cons_elements() {
    let err = parse_and_resolve("[s, ..s @ s] @ s =? [1, 2]")
        .expect_err("duplicate list pattern bindings should fail");

    assert_eq!(
        err.related_labels
            .iter()
            .map(|label| (label.message.as_str(), label.span.start, label.span.end))
            .collect::<Vec<_>>(),
        [
            ("first", 1, 2),
            ("second", 6, 7),
            ("third", 15, 16),
            ("fourth", 10, 11),
        ]
    );
}

#[test]
fn test_duplicate_binding_flattens_list_children_before_parent_alias() {
    let source = "[x, ..[y, ..x]] @ x =? [1, 2]";
    let err = parse_and_resolve(source).expect_err("duplicate list pattern bindings should fail");
    let spans = source
        .match_indices('x')
        .map(|(start, _)| (start, start + 1))
        .collect::<Vec<_>>();

    assert_eq!(
        err.related_labels
            .iter()
            .map(|label| (label.span.start, label.span.end))
            .collect::<Vec<_>>(),
        spans,
    );
}

#[test]
fn test_duplicate_binding_keeps_recursive_detection_order_without_outer_as_pattern() {
    let err = parse_and_resolve("((s, s) @ s, s) = ((1, 3), 2)")
        .expect_err("duplicate pattern bindings should fail");

    assert_eq!(
        err.related_labels
            .iter()
            .map(|label| (label.message.as_str(), label.span.start, label.span.end))
            .collect::<Vec<_>>(),
        [
            ("first", 2, 3),
            ("second", 5, 6),
            ("third", 10, 11),
            ("fourth", 13, 14),
        ]
    );
}

#[test]
fn test_as_pattern_duplicate_uses_alias_token_span() {
    let err = parse_and_resolve("(x @ x) = (1, 2)")
        .expect_err("duplicate as-pattern binding should fail");

    assert_eq!(
        err.related_labels
            .iter()
            .map(|label| (label.message.as_str(), label.span.start, label.span.end))
            .collect::<Vec<_>>(),
        [("first", 1, 2), ("second", 5, 6)]
    );
}

#[test]
fn test_block_binding_does_not_escape() {
    let result = parse_and_resolve(
        r#"{
  x = 1
  x
}
x"#,
    );
    let err = result.expect_err("block-local binding must not escape");
    assert!(err.message.contains("Undefined variable: x"));
}

#[test]
fn test_match_arm_binding_does_not_escape() {
    let result = parse_and_resolve(
        r#"value: Result<Int> = Ok(1)
match value {
  Ok(x) => x,
  Err(e) => 0,
}
x"#,
    );
    let err = result.expect_err("match-arm binding must not escape");
    assert!(err.message.contains("Undefined variable: x"));
}

#[test]
fn test_match_arm_binding_does_not_leak_to_other_arms() {
    let result = parse_and_resolve(
        r#"value: Result<Int> = Ok(1)
match value {
  Ok(x) => x,
  Err(e) => x,
}"#,
    );
    let err = result.expect_err("match-arm binding must stay within its own arm");
    assert!(err.message.contains("Undefined variable: x"));
}

#[test]
fn test_nested_closure_does_not_overcapture_outer_local() {
    let resolved = parse_and_resolve(r#"f = {|x| {|y| x + y}}"#).unwrap();
    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Closure(_, outer_params, outer_captures, outer_body) => {
                assert_eq!(outer_params.len(), 1);
                assert!(outer_captures.is_empty());
                match outer_body.as_ref() {
                    Resolved::Closure(_, inner_params, inner_captures, inner_body) => {
                        assert_eq!(inner_params.len(), 1);
                        assert_eq!(inner_captures.len(), 1);
                        assert_eq!(inner_captures[0].name, "x");
                        assert!(matches!(
                            inner_body.as_ref(),
                            Resolved::BinOp(_, BinOp::Add, _, _)
                        ));
                    }
                    _ => panic!("Expected inner Closure"),
                }
            }
            _ => panic!("Expected outer Closure"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_closure_param_annotations_are_preserved() {
    let resolved = parse_and_resolve(r#"f = {|x: Int, y| x}"#).unwrap();
    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Closure(_, params, captures, _) => {
                assert!(captures.is_empty());
                assert_eq!(params.len(), 2);
                assert!(matches!(
                    params[0].ty.as_ref(),
                    Some(AstTy::Named(_, name)) if name == "Int"
                ));
                assert_eq!(params[1].ty, None);
            }
            _ => panic!("Expected Closure"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_std_module_is_auto_imported_from_module_attribute() {
    let module_stages = vec![vec![staged_auto_import_module(
        "Prelude",
        parse_module_ast(r#"def greet() -> String { "hi" }"#, "Prelude"),
    )]];

    let resolved = resolve_user_with_modules(r#"value = greet()"#, &module_stages)
        .expect("auto-import module should inject members");

    let bind = resolved
        .iter()
        .find(|node| matches!(node, Resolved::Bind(_, _, _)))
        .expect("user bind should exist");

    match bind {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, func, _) => match func.as_ref() {
                Resolved::Var(_, id) => {
                    assert_eq!(id.name, "greet");
                    assert_eq!(id.qualified_name.as_deref(), Some("Prelude::greet"));
                }
                other => panic!("expected imported function var, got {:?}", other),
            },
            other => panic!("expected app, got {:?}", other),
        },
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn test_impl_type_helpers_are_auto_imported_from_owner_surface() {
    let module_stages = vec![vec![staged_auto_import_module(
        "User",
        parse_module_ast(
            r#"defstruct User {
  name: String,
}

impl User {
  def greet() -> String { "hi" }
}"#,
            "User",
        ),
    )]];

    let resolved = resolve_user_with_modules(r#"value = greet()"#, &module_stages)
        .expect("auto-import impl owner surface should inject helpers");

    let bind = resolved
        .iter()
        .find(|node| matches!(node, Resolved::Bind(_, _, _)))
        .expect("user bind should exist");

    match bind {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, func, _) => match func.as_ref() {
                Resolved::Var(_, id) => {
                    assert_eq!(id.name, "greet");
                    assert_eq!(id.qualified_name.as_deref(), Some("Global::User::greet"));
                }
                other => panic!("expected imported impl helper var, got {:?}", other),
            },
            other => panic!("expected app, got {:?}", other),
        },
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn test_explicit_import_of_autoimport_module_is_allowed() {
    let module_stages = vec![vec![staged_auto_import_module(
        "Prelude",
        parse_module_ast(r#"def greet() -> String { "hi" }"#, "Prelude"),
    )]];

    let resolved = resolve_user_with_modules(
        r#"import Prelude;
value = greet()"#,
        &module_stages,
    )
    .expect("explicit import of autoimport module should be allowed");

    assert!(resolved
        .iter()
        .any(|node| matches!(node, Resolved::Bind(_, _, _))));
}

#[test]
fn test_std_trait_method_is_auto_imported_from_trait_attribute() {
    let module_stages = vec![vec![staged_module(
        "Metric",
        parse_module_ast(
            r#"@autoimport
deftrait Metric {
  def add(self: Self, rhs: Self) -> Self
}"#,
            "Metric",
        ),
    )]];

    let resolved = resolve_user_with_modules("value = add(1, 2)", &module_stages)
        .expect("std trait method should auto-import");

    let declaration_id = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::TraitDef(_, id, _, _, methods, _) if id.name == "Metric" => {
                methods.first().map(|method| &method.id)
            }
            _ => None,
        })
        .expect("trait method declaration should exist");

    let bind = resolved
        .iter()
        .find(|node| matches!(node, Resolved::Bind(_, _, _)))
        .expect("user bind should exist");

    match bind {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::App(_, func, args) => {
                assert_eq!(args.len(), 2);
                match func.as_ref() {
                    Resolved::Var(_, id) => {
                        assert_eq!(id.name, "add");
                        assert!(id
                            .qualified_name
                            .as_deref()
                            .is_some_and(|name| name.ends_with("::add")));
                        assert_eq!(id.unique_id, declaration_id.unique_id);
                        assert_eq!(
                            id.symbol_info.as_ref().map(|info| info.identity),
                            Some(TypeIdentity::Trait),
                            "a bare auto-import alias must retain its canonical trait owner"
                        );
                    }
                    other => panic!("expected trait method var, got {:?}", other),
                }
            }
            other => panic!("expected app, got {:?}", other),
        },
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn test_explicit_import_of_autoimport_trait_is_allowed() {
    let module_stages = vec![vec![staged_module(
        "Metric",
        parse_module_ast(
            r#"@autoimport
deftrait Metric {
  def add(self: Self, rhs: Self) -> Self
}"#,
            "Metric",
        ),
    )]];

    let resolved = resolve_user_with_modules(
        r#"import Metric;
value = add(1, 2)"#,
        &module_stages,
    )
    .expect("explicit import of autoimport trait should be allowed");

    let declaration_id = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::TraitDef(_, id, _, _, methods, _) if id.name == "Metric" => {
                methods.first().map(|method| &method.id)
            }
            _ => None,
        })
        .expect("trait method declaration should exist");
    let callee_id = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::App(_, func, _) => match func.as_ref() {
                    Resolved::Var(_, id) => Some(id),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .expect("explicitly imported trait method call should exist");

    assert_eq!(callee_id.name, "add");
    assert_eq!(callee_id.unique_id, declaration_id.unique_id);
    assert_eq!(
        callee_id.symbol_info.as_ref().map(|info| info.identity),
        Some(TypeIdentity::Trait),
        "a bare explicit-import alias must retain its canonical trait owner"
    );
}

#[test]
fn test_autoimport_trait_helper_conflict_is_rejected() {
    let module_stages = vec![vec![
        staged_module(
            "Concat",
            parse_module_ast(
                r#"@autoimport
deftrait Concat {
  def concat(self: Self, rhs: Self) -> Self
}"#,
                "Concat",
            ),
        ),
        staged_module(
            "Fake",
            parse_module_ast(
                r#"@autoimport
deftrait Fake {
  def concat(self: Self) -> Self
}"#,
                "Fake",
            ),
        ),
    ]];

    let err = resolve_user_with_modules("value = 1", &module_stages)
        .expect_err("conflicting auto-import trait helpers must fail");
    assert!(err.message.contains("Auto-import conflict"));
    assert!(err.message.contains("concat"));
    assert!(err.message.contains("Concat"));
    assert!(err.message.contains("Fake"));
}

#[test]
fn test_effective_auto_import_entries_include_autoimport_trait_methods_only() {
    let module_stages = vec![vec![staged_module(
        "Show",
        parse_module_ast(
            r#"@autoimport
deftrait Show {
  def to_string(self: Self) -> String
}"#,
            "Show",
        ),
    )]];

    let entries = crate::effective_auto_import_entries(&module_stages, None, 0)
        .expect("effective auto-import query should succeed");
    let names = entries
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();

    assert!(
        names.iter().any(|name| name == "Show::to_string"),
        "actual entries: {names:?}"
    );
    assert!(
        names.iter().all(|name| name != "Show"),
        "trait owner should not be reported as imported member: {names:?}"
    );
}

#[test]
fn test_effective_visible_entries_include_explicit_imported_short_name() {
    let module_stages = vec![vec![staged_module(
        "Global::Helper",
        parse_module_ast(
            r#"def helper(value: Int) -> String { "" }"#,
            "Global::Helper",
        ),
    )]];
    let ast = parse("import Helper::helper\nhe").expect("parse should succeed");

    let visible = crate::effective_visible_entries(&module_stages, &ast, None, 0)
        .expect("effective visible query should succeed");
    let imported = visible
        .iter()
        .find(|entry| entry.visible_name == "helper")
        .expect("explicit import should expose short name");

    assert_eq!(imported.entry.name, "helper");
    assert!(imported.via_import);
    assert!(!imported.via_auto_import);
    assert!(imported.importable);
    assert!(imported.callable);
}

#[test]
fn test_effective_visible_entries_mark_explicit_import_shadowing_auto_import() {
    let module_stages = vec![vec![
        staged_auto_import_module(
            "Kernel",
            parse_module_ast(r#"def helper() -> Int { 1 }"#, "Kernel"),
        ),
        staged_module(
            "UserHelpers",
            parse_module_ast(r#"def helper() -> Int { 2 }"#, "UserHelpers"),
        ),
    ]];
    let ast = parse("import UserHelpers::helper\nhelper").expect("parse should succeed");

    let visible = crate::effective_visible_entries(&module_stages, &ast, None, 0)
        .expect("effective visible query should succeed");
    let imported = visible
        .iter()
        .find(|entry| entry.visible_name == "helper" && entry.entry.module_path == "UserHelpers")
        .expect("explicit import should expose short name");

    assert!(imported.via_import);
    assert!(!imported.via_auto_import);
    assert!(imported.shadowed_auto_import);
}

#[test]
fn test_duplicate_module_import_is_rejected() {
    let module_stages = vec![vec![staged_module(
        "Helper",
        parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
    )]];

    let err = resolve_user_with_modules(
        r#"import Helper;
import Helper;
print(to_string(add(1, 2)))"#,
        &module_stages,
    )
    .expect_err("duplicate module import must fail");
    assert!(
        err.message.contains("Duplicate import"),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Helper"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_duplicate_module_then_member_import_is_rejected() {
    let module_stages = vec![vec![staged_module(
        "Helper",
        parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
    )]];

    let err = resolve_user_with_modules(
        r#"import Helper;
import Helper::add;
print(to_string(add(1, 2)))"#,
        &module_stages,
    )
    .expect_err("module followed by member import must fail");
    assert!(
        err.message.contains("Duplicate import"),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Helper::add"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_duplicate_member_then_module_import_is_rejected() {
    let module_stages = vec![vec![staged_module(
        "Helper",
        parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
    )]];

    let err = resolve_user_with_modules(
        r#"import Helper::add;
import Helper;
print(to_string(add(1, 2)))"#,
        &module_stages,
    )
    .expect_err("member followed by module import must fail");
    assert!(
        err.message.contains("Duplicate import"),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Helper"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_duplicate_member_import_is_rejected() {
    let module_stages = vec![vec![staged_module(
        "Helper",
        parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
    )]];

    let err = resolve_user_with_modules(
        r#"import Helper::add;
import Helper::add;
print(to_string(add(1, 2)))"#,
        &module_stages,
    )
    .expect_err("duplicate member import must fail");
    assert!(
        err.message.contains("Duplicate import"),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Helper::add"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_explicit_import_shadows_auto_imported_kernel_function() {
    let module_stages = vec![
        vec![staged_module(
            "Kernel",
            parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Kernel"),
        )],
        vec![staged_module(
            "Helper",
            parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x - y }"#, "Helper"),
        )],
    ];

    let resolved = resolve_user_with_modules(
        r#"import Helper::add;
print(to_string(add(7, 3)))"#,
        &module_stages,
    )
    .expect("explicit import should shadow auto-imported function");

    let helper_add_uid = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::Def(_, id, _, _, _, _, _, _)
                if id.qualified_name.as_deref() == Some("Helper::add") =>
            {
                Some(id.unique_id)
            }
            _ => None,
        })
        .expect("helper add should be resolved");

    let imported_add_uid = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::App(_, print_func, print_args) => {
                if !matches!(print_func.as_ref(), Resolved::Var(_, id) if id.name == "print") {
                    return None;
                }
                let call = match print_args.first()? {
                    ResolvedRecordLitArg::Positional(inner) => inner,
                    _ => return None,
                };
                let call = match call {
                    Resolved::App(_, func, args) => {
                        if !matches!(func.as_ref(), Resolved::Var(_, id) if id.name == "to_string")
                        {
                            return None;
                        }
                        match args.first()? {
                            ResolvedRecordLitArg::Positional(inner) => inner,
                            _ => return None,
                        }
                    }
                    _ => return None,
                };
                match call {
                    Resolved::App(_, func, _) => match func.as_ref() {
                        Resolved::Var(_, id) if id.name == "add" => Some(id.unique_id),
                        _ => None,
                    },
                    _ => None,
                }
            }
            _ => None,
        })
        .expect("user call should resolve imported add");

    assert_eq!(imported_add_uid, helper_add_uid);
}

#[test]
fn test_warning_explicit_unused_function_import_warns() {
    let module_stages = vec![vec![staged_module(
        "Helper",
        parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
    )]];

    let output = resolve_user_with_modules_with_warnings(
        r#"import Helper::add;
print("ok")"#,
        &module_stages,
    )
    .expect("program should resolve");

    let messages = warning_messages(&output, WarningKind::UnusedImportFunction);
    assert!(
        messages
            .iter()
            .any(|message| message.contains("Helper::add")),
        "warnings: {:?}",
        output.warnings
    );
}

#[test]
fn test_warning_auto_imported_function_does_not_warn() {
    let module_stages = vec![vec![staged_auto_import_module(
        "Helper",
        parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
    )]];

    let output = resolve_user_with_modules_with_warnings(r#"print("ok")"#, &module_stages)
        .expect("program should resolve");

    assert!(
        !output
            .warnings
            .iter()
            .any(|warning| warning.kind == WarningKind::UnusedImportFunction),
        "warnings: {:?}",
        output.warnings
    );
}

#[test]
fn test_warning_qualified_call_does_not_consume_explicit_short_import() {
    let module_stages = vec![vec![staged_module(
        "Helper",
        parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
    )]];

    let output = resolve_user_with_modules_with_warnings(
        r#"import Helper::add;
print(to_string(Helper::add(1, 2)))"#,
        &module_stages,
    )
    .expect("program should resolve");

    let messages = warning_messages(&output, WarningKind::UnusedImportFunction);
    assert!(
        messages
            .iter()
            .any(|message| message.contains("Helper::add")),
        "warnings: {:?}",
        output.warnings
    );
}

#[test]
fn test_staged_resolution_keeps_local_binder_ids_distinct_from_user_defs() {
    let module_stages = vec![vec![staged_module(
        "Helper",
        parse_module_ast(r#"def keep(x: Int) -> Int { x }"#, "Helper"),
    )]];

    let resolved = resolve_user_with_modules(
        r#"def add1(x: Int) -> Int { x + 1 }
value = add1(41)"#,
        &module_stages,
    )
    .expect("script module should resolve");

    let mut helper_param_uid = None;
    let mut user_def_uid = None;
    let mut user_param_uid = None;
    for node in &resolved {
        if let Resolved::Def(_, id, _, params, _, _, _, _) = node {
            match id.qualified_name.as_deref() {
                Some("Helper::keep") => {
                    helper_param_uid = params.first().map(|param| param.id.unique_id);
                }
                Some("__Script::fixture::add1") => {
                    user_def_uid = Some(id.unique_id);
                    user_param_uid = params.first().map(|param| param.id.unique_id);
                }
                _ => {}
            }
        }
    }

    let helper_param_uid = helper_param_uid.expect("helper param should exist");
    let user_def_uid = user_def_uid.expect("user def should exist");
    let user_param_uid = user_param_uid.expect("user param should exist");
    assert_ne!(helper_param_uid, user_def_uid);
    assert_ne!(helper_param_uid, user_param_uid);
    assert_ne!(user_def_uid, user_param_uid);

    let call_uid = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::App(_, func, _) => match func.as_ref() {
                    Resolved::Var(_, id) if id.name == "add1" => Some(id.unique_id),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .expect("user call should resolve to add1");
    assert_eq!(call_uid, user_def_uid);
}

#[test]
fn test_capture_prefers_shadowed_local_function_name() {
    let resolved = parse_and_resolve(
        r#"print = {|x| x}
captured = &print"#,
    )
    .expect("shadowing + capture should resolve");

    let local_print_id = match &resolved[0] {
        Resolved::Bind(_, ResolvedPattern::Var(id), _) => id.unique_id,
        _ => panic!("Expected local print binding"),
    };

    let captured_target_id = match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Capture(_, target, _) => match target.as_ref() {
                Resolved::Var(_, id) => id.unique_id,
                _ => panic!("Expected captured var target"),
            },
            _ => panic!("Expected capture expression"),
        },
        _ => panic!("Expected captured binding"),
    };

    assert_eq!(captured_target_id, local_print_id);
}

// --- SigilSession tests ---

#[test]
fn test_sigil_session_basic_resolve() {
    let mut session = SigilSession::new();
    let ast = spire::parse("x = 1").expect("parse failed");
    let resolved = session.resolve(ast).expect("resolve failed");
    assert_eq!(resolved.len(), 1);
    assert!(
        matches!(&resolved[0], Resolved::Bind(_, ResolvedPattern::Var(id), _) if id.name == "x")
    );
}

#[test]
fn test_sigil_session_scope_persists_across_calls() {
    let mut session = SigilSession::new();

    let ast1 = spire::parse("x = 1").expect("parse failed");
    session.resolve(ast1).expect("first resolve failed");

    // x must be in scope for the second call
    let ast2 = spire::parse("y = x + 1").expect("parse failed");
    let resolved = session.resolve(ast2).expect("second resolve failed");
    assert!(
        matches!(&resolved[0], Resolved::Bind(_, ResolvedPattern::Var(id), _) if id.name == "y")
    );
}

#[test]
fn test_sigil_session_top_level_def_cannot_capture_prior_value_binding() {
    let mut session = SigilSession::with_module_path(Some("__Repl::Session".to_string()));
    let first = spire::parse("x = 1").expect("parse failed");
    session.resolve(first).expect("bind should resolve");

    let second = spire::parse("def f() -> Int { x }").expect("parse failed");
    let err = session
        .resolve(second)
        .expect_err("top-level def capture must fail across session chunks");
    assert!(
        err.message
            .contains("Top-level definition `f` cannot reference value binding `x`"),
        "{}",
        err.message
    );
}

#[test]
fn test_sigil_session_lookup_uid_returns_bound_id() {
    let mut session = SigilSession::new();
    let ast = spire::parse("answer = 42").expect("parse failed");
    let resolved = session.resolve(ast).expect("resolve failed");

    let expected_id = match &resolved[0] {
        Resolved::Bind(_, ResolvedPattern::Var(id), _) => id.unique_id,
        _ => panic!("Expected Bind"),
    };

    assert_eq!(session.lookup_uid("answer"), Some(expected_id));
}

#[test]
fn test_sigil_session_checkpoint_rollback_removes_later_bindings() {
    let mut session = SigilSession::new();

    // Define x
    let ast1 = spire::parse("x = 1").expect("parse failed");
    session.resolve(ast1).expect("first resolve failed");
    let x_id = session
        .lookup_uid("x")
        .expect("x should be defined after first resolve");

    // Save checkpoint before defining y
    let checkpoint = session.checkpoint();

    // Define y
    let ast2 = spire::parse("y = 2").expect("parse failed");
    session.resolve(ast2).expect("second resolve failed");
    assert!(
        session.lookup_uid("y").is_some(),
        "y should be visible before rollback"
    );

    // Rollback to before y was added
    session.rollback(checkpoint);

    assert!(
        session.lookup_uid("y").is_none(),
        "y should be gone after rollback"
    );
    assert_eq!(
        session.lookup_uid("x"),
        Some(x_id),
        "x should remain after rollback"
    );
}

#[test]
fn test_sigil_session_checkpoint_rollback_restores_owner_registry() {
    let mut session = SigilSession::new();
    let checkpoint = session.checkpoint();

    session
        .resolve(spire::parse("defrecord Saved(value: Int)").expect("parse failed"))
        .expect("owner declaration should resolve");
    assert_eq!(
        session.owner_registry().identity_for_owner("Saved"),
        Some(TypeIdentity::Record)
    );

    session.rollback(checkpoint);
    assert_eq!(session.owner_registry().identity_for_owner("Saved"), None);
    session
        .resolve(spire::parse("defrecord Saved(value: Int)").expect("parse failed"))
        .expect("rolled-back owner should be declarable again");
}

#[test]
fn test_sigil_session_rejects_cross_chunk_owner_collision() {
    let mut session = SigilSession::new();
    session
        .resolve(spire::parse("defrecord Shared(value: Int)").expect("parse failed"))
        .expect("first owner should resolve");

    let err = session
        .resolve(spire::parse("type Shared = (Int -> Int)").expect("parse failed"))
        .expect_err("signature alias must collide with the existing record owner");
    assert_eq!(err.message, "Duplicate top-level owner: Shared");
    assert!(err
        .related_labels
        .iter()
        .any(|label| label.message == "first Record declaration"));
}

#[test]
fn test_sigil_session_promotes_inherited_constructor_traits_across_chunks() {
    let mut session = SigilSession::new();
    session
        .resolve(
            spire::parse(
                r#"deftrait RootConstructor
where
  Self: Type<$A>
{
  def map_root(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}"#,
            )
            .expect("root trait should parse"),
        )
        .expect("root constructor trait should resolve");

    session
        .resolve(
            spire::parse(
                r#"deftrait MiddleConstructor
where
  Self: RootConstructor
{
  def map_middle(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}"#,
            )
            .expect("middle trait should parse"),
        )
        .expect("middle constructor trait should resolve");
    assert_eq!(
        session
            .owner_registry()
            .identity_for_owner("MiddleConstructor"),
        Some(TypeIdentity::TypeConstructor)
    );

    session
        .resolve(
            spire::parse(
                r#"deftrait LeafConstructor
where
  Self: MiddleConstructor
{
  def map_leaf(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}"#,
            )
            .expect("leaf trait should parse"),
        )
        .expect("leaf constructor trait should resolve");
    assert_eq!(
        session
            .owner_registry()
            .identity_for_owner("LeafConstructor"),
        Some(TypeIdentity::TypeConstructor)
    );
}

#[test]
fn test_sigil_session_failed_resolve_does_not_pollute_scope() {
    let mut session = SigilSession::new();

    // Define x
    let ast1 = spire::parse("x = 1").expect("parse failed");
    session.resolve(ast1).expect("first resolve failed");
    let x_id = session.lookup_uid("x").expect("x should be defined");

    // Attempt to resolve something with an undefined variable — must fail
    let ast_fail = spire::parse("y = undefined_name + 1").expect("parse failed");
    assert!(
        session.resolve(ast_fail).is_err(),
        "resolve of undefined var must fail"
    );

    // x should survive; y must not be committed to scope
    assert_eq!(
        session.lookup_uid("x"),
        Some(x_id),
        "x should remain after failed resolve"
    );
    assert!(
        session.lookup_uid("y").is_none(),
        "y must not be in scope after a failed resolve"
    );
}

#[test]
fn test_sigil_session_allows_top_level_shadowing_of_imported_name() {
    let mut session = SigilSession::new();
    session.define_with_id("add", 99);

    let ast = spire::parse("def add(x: Int, y: Int) -> Int { x + y }").expect("parse failed");
    let resolved = session.resolve(ast).expect("resolve failed");

    let def_id = match &resolved[0] {
        Resolved::Def(_, id, _, _, _, _, _, _) => id.unique_id,
        other => panic!("Expected Def, got {:?}", other),
    };

    assert_eq!(session.lookup_uid("add"), Some(def_id));
    assert_ne!(def_id, 99);
}

#[test]
fn test_sigil_session_visible_entries_filter_hidden_surfaces_and_keep_visible_aliases() {
    let mut declaration_index = DeclarationIndex::new();
    declaration_index.insert(
        "Global::Helper::helper".to_string(),
        DeclarationEntry {
            module_path: "Global::Helper".to_string(),
            name: "helper".to_string(),
            fq_name: "Global::Helper::helper".to_string(),
            kind: DeclarationKind::Def,
            stage_index: 0,
            auto_import: false,
            hidden: false,
            visibility: Visibility::Public,
            user_importable: true,
            user_callable: true,
        },
    );
    declaration_index.insert(
        "Global::Kernel::hidden_pid".to_string(),
        DeclarationEntry {
            module_path: "Global::Kernel".to_string(),
            name: "hidden_pid".to_string(),
            fq_name: "Global::Kernel::hidden_pid".to_string(),
            kind: DeclarationKind::Def,
            stage_index: 0,
            auto_import: false,
            hidden: true,
            visibility: Visibility::Public,
            user_importable: true,
            user_callable: true,
        },
    );

    let declaration_uids = assign_declaration_uids(&declaration_index);
    let mut scope = Scope::new();
    scope.define_with_id(
        "helper",
        *declaration_uids
            .get("Global::Helper::helper")
            .expect("helper uid should exist"),
    );
    scope.define_with_id(
        "hidden_pid",
        *declaration_uids
            .get("Global::Kernel::hidden_pid")
            .expect("hidden uid should exist"),
    );

    let mut session = SigilSession::new();
    session.replace_scope_with_declarations(scope, &declaration_index);

    let visible = session.visible_declaration_entries();

    assert!(
        visible.iter().any(|entry| {
            entry.visible_name == "helper" && entry.entry.fq_name == "Global::Helper::helper"
        }),
        "visible entries should preserve the visible alias: {visible:?}"
    );
    assert!(
        visible
            .iter()
            .all(|entry| entry.entry.fq_name != "Global::Kernel::hidden_pid"),
        "hidden surfaces should not be exposed: {visible:?}"
    );
}

// --- Expression resolution tests ---

#[test]
fn test_interpolated_string_resolves_embedded_variable() {
    let resolved = parse_and_resolve(
        r#"name = "alice"
greeting = "Hello #{name}!""#,
    )
    .unwrap();

    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::InterpolatedStr(_, parts) => {
                let has_text = parts
                    .iter()
                    .any(|p| matches!(p, ResolvedInterpolatedPart::Text(s) if s.contains("Hello")));
                let has_name_var = parts.iter().any(|p| {
                    matches!(p, ResolvedInterpolatedPart::Expr(e)
                            if matches!(e.as_ref(), Resolved::Var(_, id) if id.name == "name"))
                });
                assert!(
                    has_text,
                    "expected 'Hello' text part in interpolated string"
                );
                assert!(
                    has_name_var,
                    "expected resolved `name` variable in interpolated string"
                );
            }
            _ => panic!("Expected InterpolatedStr, got {:?}", rhs),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_field_access_resolves_correct_target() {
    let resolved = parse_and_resolve(
        r#"defstruct Point { x: Int, y: Int }
p = Point { x: 1, y: 2 }
val = p.x"#,
    )
    .unwrap();

    match &resolved[2] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::FieldAccess(_, expr, field) => {
                assert_eq!(field, "x");
                assert!(
                    matches!(expr.as_ref(), Resolved::Var(_, id) if id.name == "p"),
                    "field access target should be `p`"
                );
            }
            _ => panic!("Expected FieldAccess"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_tuple_type_root_resolves_in_field_access() {
    let resolved = parse_and_resolve("facet = Tuple._0").unwrap();
    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::FieldAccess(_, expr, field) => {
                assert_eq!(field, "_0");
                let id = resolved_var_id(expr.as_ref());
                assert_eq!(id.name, "Tuple");
                assert_symbol_info_facet_root(id, Some(FacetRootKind::Tuple));
            }
            other => panic!("Expected FieldAccess, got {:?}", other),
        },
        other => panic!("Expected Bind, got {:?}", other),
    }
}

fn bind_rhs(node: &Resolved) -> &Resolved {
    match node {
        Resolved::Bind(_, _, rhs) => rhs.as_ref(),
        other => panic!("Expected Bind, got {:?}", other),
    }
}

fn resolved_var_id(node: &Resolved) -> &ResolvedId {
    match node {
        Resolved::Var(_, id) => id,
        other => panic!("Expected Var, got {:?}", other),
    }
}

fn assert_symbol_info_facet_root(id: &ResolvedId, expected: Option<FacetRootKind>) {
    let info = id
        .symbol_info
        .as_ref()
        .expect("resolved id should carry symbol identity info");
    assert_eq!(info.capabilities.facet_root_path, expected);
}

#[test]
fn test_container_facet_roots_carry_symbol_identity_info() {
    let resolved = parse_and_resolve(
        r#"tuple_facet = Tuple._0
list_facet = List.[0]
map_facet = HashMap.["k"]"#,
    )
    .unwrap();

    match bind_rhs(&resolved[0]) {
        Resolved::FieldAccess(_, expr, field) => {
            assert_eq!(field, "_0");
            assert_symbol_info_facet_root(resolved_var_id(expr), Some(FacetRootKind::Tuple));
        }
        other => panic!("Expected Tuple FieldAccess, got {:?}", other),
    }

    match bind_rhs(&resolved[1]) {
        Resolved::FacetSegmentAccess(_, expr, ResolvedFacetPathSegment::Bracket(_)) => {
            assert_symbol_info_facet_root(resolved_var_id(expr), Some(FacetRootKind::List));
        }
        other => panic!("Expected List FacetSegmentAccess, got {:?}", other),
    }

    match bind_rhs(&resolved[2]) {
        Resolved::FacetSegmentAccess(_, expr, ResolvedFacetPathSegment::Bracket(_)) => {
            assert_symbol_info_facet_root(resolved_var_id(expr), Some(FacetRootKind::HashMap));
        }
        other => panic!("Expected HashMap FacetSegmentAccess, got {:?}", other),
    }
}

#[test]
fn test_primitive_facet_roots_carry_symbol_identity_info_without_facet_path() {
    let resolved = parse_and_resolve("string_facet = String.len").unwrap();

    match bind_rhs(&resolved[0]) {
        Resolved::FieldAccess(_, expr, field) => {
            assert_eq!(field, "len");
            assert_symbol_info_facet_root(resolved_var_id(expr), None);
        }
        other => panic!("Expected String FieldAccess, got {:?}", other),
    }
}

#[test]
fn test_boolean_variant_facet_root_carries_type_root_symbol_identity_info() {
    let resolved = parse_and_resolve("true_facet = Boolean.True").unwrap();

    match bind_rhs(&resolved[0]) {
        Resolved::FieldAccess(_, expr, field) => {
            assert_eq!(field, "True");
            assert_symbol_info_facet_root(resolved_var_id(expr), Some(FacetRootKind::TypeRoot));
        }
        other => panic!("Expected Boolean FieldAccess, got {:?}", other),
    }
}

#[test]
fn test_user_type_declarations_carry_symbol_identity_info() {
    let resolved = parse_and_resolve(
        r#"defstruct User { name: String }
defrecord Pair(left: Int, right: Int)
defenum Light {
  Red,
  Green,
}
deferror Oops(reason: String) { reason }"#,
    )
    .unwrap();

    match &resolved[0] {
        Resolved::StructDef(_, id, ..) => {
            let info = id.symbol_info.as_ref().expect("struct should carry info");
            assert_eq!(info.identity, TypeIdentity::Struct);
            assert!(info.capabilities.type_annotation);
            assert!(info.capabilities.module_owner);
            assert!(info.capabilities.impl_target);
            assert_eq!(
                info.capabilities.facet_root_path,
                Some(FacetRootKind::TypeRoot)
            );
        }
        other => panic!("Expected StructDef, got {:?}", other),
    }

    match &resolved[1] {
        Resolved::RecordDef(_, id, _, _) => {
            let info = id.symbol_info.as_ref().expect("record should carry info");
            assert_eq!(info.identity, TypeIdentity::Record);
            assert!(info.capabilities.type_annotation);
            assert!(info.capabilities.module_owner);
            assert!(info.capabilities.impl_target);
            assert_eq!(
                info.capabilities.facet_root_path,
                Some(FacetRootKind::TypeRoot)
            );
        }
        other => panic!("Expected RecordDef, got {:?}", other),
    }

    match &resolved[2] {
        Resolved::EnumDef(_, id, ..) => {
            let info = id.symbol_info.as_ref().expect("enum should carry info");
            assert_eq!(info.identity, TypeIdentity::Enum);
            assert!(info.capabilities.type_annotation);
            assert!(info.capabilities.module_owner);
            assert!(info.capabilities.impl_target);
            assert_eq!(
                info.capabilities.facet_root_path,
                Some(FacetRootKind::TypeRoot)
            );
        }
        other => panic!("Expected EnumDef, got {:?}", other),
    }

    match &resolved[3] {
        Resolved::DeferrorDef(_, id, _, _) => {
            let info = id.symbol_info.as_ref().expect("deferror should carry info");
            assert_eq!(info.identity, TypeIdentity::Error);
            assert!(info.capabilities.type_annotation);
            assert!(!info.capabilities.module_owner);
            assert!(!info.capabilities.impl_target);
            assert_eq!(info.capabilities.facet_root_path, None);
        }
        other => panic!("Expected DeferrorDef, got {:?}", other),
    }
}

#[test]
fn test_builtin_special_variant_aliases_cannot_be_declared_as_owners() {
    for alias in ["Ok", "Err", "True", "False"] {
        let module_path = format!("Global::{alias}");
        let err = precollect_declaration_index(&[vec![staged_module(&module_path, Vec::new())]])
            .expect_err("builtin-special variant alias should not be usable as a module owner");
        assert!(
            err.message
                .contains("reserved for builtin-special enum variant sugar"),
            "{alias}: {}",
            err.message
        );

        let err = precollect_declaration_index(&[vec![staged_module(
            "User",
            vec![Ast::EnumDef(
                Span { start: 0, end: 0 },
                format!("Global::{alias}"),
                Vec::new(),
                vec![spire::ast::EnumVariant {
                    name: "Value".to_string(),
                    payload: Vec::new(),
                    discriminant: None,
                    span: Span { start: 0, end: 0 },
                }],
                DeclAttrs::default(),
            )],
        )]])
        .expect_err("builtin-special variant alias should not be usable as a type owner");
        assert!(
            err.message
                .contains("reserved for builtin-special enum variant sugar"),
            "{alias}: {}",
            err.message
        );
    }
}

#[test]
fn resolves_inferred_facet_capture_with_map_key() {
    let resolved = parse_and_resolve(
        r#"users = []
names = users |*> _.score.["talk"]"#,
    )
    .unwrap();
    let rendered = format!("{resolved:?}");
    assert!(rendered.contains("InferredFacetCapture"), "{rendered}");
    assert!(rendered.contains("Bracket"), "{rendered}");
}

#[test]
fn resolves_container_root_facet_paths() {
    let module_stages = vec![vec![staged_module(
        "Facet",
        parse_module_ast(
            r#"@builtin def view(facet: Facet<ReadablePath, $S, $A, _, _>, source: $S) -> Result<$A>"#,
            "Facet",
        ),
    )]];

    let resolved = resolve_user_with_modules(
        r#"map = ()
value = Facet::view(HashMap.["taro"], map)"#,
        &module_stages,
    )
    .unwrap();
    let rendered = format!("{resolved:?}");
    assert!(rendered.contains("HashMap"), "{rendered}");
    assert!(rendered.contains("Bracket"), "{rendered}");

    let resolved = resolve_user_with_modules(
        r#"values = []
value = Facet::view(List.[0], values)"#,
        &module_stages,
    )
    .unwrap();
    let rendered = format!("{resolved:?}");
    assert!(rendered.contains("List"), "{rendered}");
    assert!(rendered.contains("Bracket"), "{rendered}");
}

#[test]
fn resolves_bulk_update_case_actions_as_facet_calls() {
    let module_stages = vec![vec![staged_module(
        "Facet",
        parse_module_ast(
            r#"@builtin def case_set(facet: Facet<CasePath, $S, $A, $T, $B>, source: $S, value: $B) -> Result<$T>
@builtin def set(facet: Facet<WritablePath, $S, $A, $T, $B>, source: $S, value: $B) -> Result<$T>"#,
            "Facet",
        ),
    )]];
    let resolved = resolve_user_with_modules(
        r#"user = ()
user2 =? Facet::bulk_update(user) {
  nickname.Some <- case_set("alice")
  scores.[1] <- set(500)
}"#,
        &module_stages,
    )
    .unwrap();
    let rendered = format!("{resolved:?}");
    assert!(
        rendered.contains("Facet::case_set") || rendered.contains("case_set"),
        "{rendered}"
    );
    assert!(rendered.contains("FacetCapture"), "{rendered}");
}

#[test]
fn test_list_literal_resolves_all_elements() {
    let resolved = parse_and_resolve("items = [1, 2, 3]").unwrap();
    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::ListLiteral(_, elems) => {
                assert_eq!(elems.len(), 3);
                assert!(matches!(&elems[0], Resolved::Lit(_, Lit::Int(n)) if n == &int(1)));
                assert!(matches!(&elems[1], Resolved::Lit(_, Lit::Int(n)) if n == &int(2)));
                assert!(matches!(&elems[2], Resolved::Lit(_, Lit::Int(n)) if n == &int(3)));
            }
            _ => panic!("Expected ListLiteral"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_range_literal_resolves_endpoints() {
    let resolved = parse_and_resolve("items = [1..3]").unwrap();
    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::RangeLiteral(_, start, stop) => {
                assert!(matches!(start.as_ref(), Resolved::Lit(_, Lit::Int(n)) if *n == int(1)));
                assert!(matches!(stop.as_ref(), Resolved::Lit(_, Lit::Int(n)) if *n == int(3)));
            }
            other => panic!("Expected RangeLiteral, got {other:?}"),
        },
        other => panic!("Expected Bind, got {other:?}"),
    }
}

#[test]
fn test_semicolon_expression_wraps_inner_node() {
    let resolved = parse_and_resolve(r#"print("hello");"#).unwrap();
    match &resolved[0] {
        Resolved::Semi(_, inner) => match inner.as_ref() {
            Resolved::App(_, func, _) => match func.as_ref() {
                Resolved::Var(_, id) => assert_eq!(id.name, "print"),
                _ => panic!("Expected Var(print) inside Semi"),
            },
            _ => panic!("Expected App inside Semi"),
        },
        _ => panic!("Expected Semi at top level"),
    }
}

// --- Import error tests ---

#[test]
fn test_unknown_import_member_is_error() {
    let module_stages = vec![vec![staged_module(
        "Helper",
        parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
    )]];

    let err = resolve_user_with_modules(
        r#"import Helper::nonexistent;
print("ok")"#,
        &module_stages,
    )
    .expect_err("importing a non-existent member must fail");

    assert!(
        err.message.contains("Unknown import member"),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Helper::nonexistent"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_import_list_groups_private_not_importable_and_unknown_members() {
    let module_stages = vec![vec![
        staged_module(
            "User",
            parse_module_ast(
                r#"defstruct User { name: String }
impl User {
  def new(name: String) -> Self { User(name: name) }
  defextractor deconstruct(self: Self) -> User { self }
}"#,
                "User",
            ),
        ),
        staged_module(
            "Secrets",
            parse_module_ast(
                r#"defp secret_suffix() -> String { "::private" }
def public_secret() -> String { "module" ++ secret_suffix() }"#,
                "Secrets",
            ),
        ),
    ]];

    let err = resolve_user_with_modules(
        r#"import User::{new, deconstruct}
import Secrets::{secret_suffix, missing_fun}
print("ok")"#,
        &module_stages,
    )
    .expect_err("invalid list import should fail");

    assert!(
        err.message.contains("Invalid import members in `User`."),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Error: not importable members."),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("User::new"),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("User::deconstruct"),
        "actual error: {}",
        err.message
    );

    let err = resolve_user_with_modules(
        r#"import Secrets::{secret_suffix, missing_fun}
print("ok")"#,
        &module_stages,
    )
    .expect_err("private and unknown list import should fail");

    assert!(
        err.message.contains("Invalid import members in `Secrets`."),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Error: private functions."),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Secrets::secret_suffix"),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Error: unknown import members."),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Secrets::missing_fun"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_import_list_groups_hidden_builtin_members() {
    let module_stages = vec![vec![staged_module(
        "Process",
        parse_module_ast(
            r#"@hidden
@builtin def __process_self() -> PID<$Process>

def visible() -> Int { 1 }"#,
            "Process",
        ),
    )]];

    let err = resolve_user_with_modules(
        r#"import Process::{visible, __process_self}
print("ok")"#,
        &module_stages,
    )
    .expect_err("hidden builtin in list import should fail");

    assert!(
        err.message.contains("Invalid import members in `Process`."),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Error: hidden builtins."),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Process::__process_self"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_import_list_groups_future_stage_members() {
    let consumer = staged_module(
        "Consumer",
        parse_module_ast(
            r#"import Provider::{value, missing};

def use_value() -> Int {
  value()
}"#,
            "Consumer",
        ),
    );
    let provider = staged_module(
        "Provider",
        parse_module_ast(
            r#"def value() -> Int {
  41
}"#,
            "Provider",
        ),
    );

    let err = resolve_user_with_modules(
        "print(to_string(Consumer::use_value()))",
        &[vec![consumer], vec![provider]],
    )
    .expect_err("future-stage list import should fail");

    assert!(
        err.message
            .contains("Invalid import members in `Provider`."),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Error: unavailable import members."),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Provider::value"),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Error: unknown import members."),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Provider::missing"),
        "actual error: {}",
        err.message
    );
}

// --- Match arm binding tests ---

#[test]
fn test_match_arm_constructor_binding_resolves_to_same_uid_in_body() {
    let resolved = parse_and_resolve(
        r#"value: Result<Int> = Ok(42)
result = match value {
  Ok(x) => x,
  Err(e) => 0,
}"#,
    )
    .unwrap();

    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Match(_, _, arms) => match &arms[0] {
                ResolvedMatchArm {
                    pattern: ResolvedPattern::Constructor(ctor_id, inner),
                    guard: None,
                    body,
                } => {
                    assert_eq!(ctor_id.name, "Ok");
                    let binding_id = match inner.as_slice() {
                        [ResolvedPattern::Var(binding_id)] => binding_id,
                        _ => panic!("Expected constructor inner var binding"),
                    };
                    assert_eq!(binding_id.name, "x");
                    // The arm body `x` must refer to the same uid as the pattern binding
                    match body {
                        Resolved::Var(_, var_id) => {
                            assert_eq!(
                                var_id.unique_id, binding_id.unique_id,
                                "body var uid must match pattern binding uid"
                            );
                        }
                        _ => panic!("Expected Var as match arm body"),
                    }
                }
                _ => panic!("Expected Constructor arm pattern with binding"),
            },
            _ => panic!("Expected Match"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_match_first_binding_pattern_binds_and_is_visible_in_body() {
    let resolved = parse_and_resolve(
        r#"result = match 42 {
  fallback => fallback,
  _ => 0,
}"#,
    )
    .unwrap();

    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Match(_, _, arms) => match &arms[0] {
                ResolvedMatchArm {
                    pattern: ResolvedPattern::Var(binding_id),
                    guard: None,
                    body: Resolved::Var(_, body_id),
                } => {
                    assert_eq!(binding_id.name, "fallback");
                    assert_eq!(binding_id.unique_id, body_id.unique_id);
                }
                _ => panic!("Expected first arm to be a binding pattern"),
            },
            _ => panic!("Expected Match"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_match_as_pattern_and_annotation_resolve_end_to_end() {
    let resolved = parse_and_resolve(
        r#"value = [1, 2]
result = match value {
  [head, ..tail] @ whole: List<Int> => head,
  _ => 0,
}"#,
    )
    .unwrap();

    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Match(_, _, arms) => match &arms[0] {
                ResolvedMatchArm {
                    pattern:
                        ResolvedPattern::As(inner, alias, Some(AstTy::Generic(_, ty_name, ty_args))),
                    guard: None,
                    body,
                } => {
                    assert_eq!(alias.name, "whole");
                    assert_eq!(ty_name, "List");
                    assert_eq!(ty_args.len(), 1);
                    assert!(matches!(inner.as_ref(), ResolvedPattern::ListCons(_, _)));
                    assert!(matches!(body, Resolved::Var(_, id) if id.name == "head"));
                }
                _ => panic!("Expected as-pattern with generic annotation"),
            },
            _ => panic!("Expected Match"),
        },
        _ => panic!("Expected Bind"),
    }
}

#[test]
fn test_match_guard_can_reference_pattern_binding() {
    let resolved = parse_and_resolve(
        r#"result = match 5 {
  num when num `==` 5 => num,
  _ => 0,
}"#,
    )
    .unwrap();

    match &resolved[0] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Match(_, _, arms) => match &arms[0] {
                ResolvedMatchArm {
                    pattern: ResolvedPattern::Var(binding_id),
                    guard: Some(Resolved::BinOp(_, _, left, _)),
                    body: Resolved::Var(_, body_id),
                } => {
                    let Resolved::Var(_, guard_left_id) = left.as_ref() else {
                        panic!("Expected guard left operand to be bound variable");
                    };
                    assert_eq!(binding_id.unique_id, guard_left_id.unique_id);
                    assert_eq!(binding_id.unique_id, body_id.unique_id);
                }
                _ => panic!("Expected guarded variable arm"),
            },
            _ => panic!("Expected Match"),
        },
        _ => panic!("Expected Bind"),
    }
}

// --- build_scope_for_module tests ---

#[test]
fn test_build_scope_for_module_includes_prior_stage_declarations() {
    let module_stages = vec![
        vec![staged_module(
            "Util",
            parse_module_ast(r#"def helper(x: Int) -> Int { x }"#, "Util"),
        )],
        vec![staged_module(
            "App",
            parse_module_ast(r#"def main() -> Int { 0 }"#, "App"),
        )],
    ];

    // Stage index 1 (App) — Util::helper from stage 0 should appear by fully-qualified name
    let scope = build_scope_for_module(&module_stages, Some("App"), 1)
        .expect("build_scope_for_module should succeed");

    assert!(
        scope.lookup("Util::helper").is_some(),
        "Util::helper should be accessible by qualified name in App's scope"
    );
}

#[test]
fn test_build_scope_for_module_includes_qualified_public_const() {
    let module_stages = vec![
        vec![staged_module(
            "AppConfig",
            parse_module_ast(r#"const APP_NAME = "surtr""#, "AppConfig"),
        )],
        vec![staged_module(
            "App",
            parse_module_ast(r#"def main() -> Int { 0 }"#, "App"),
        )],
    ];

    let scope = build_scope_for_module(&module_stages, Some("App"), 1)
        .expect("build_scope_for_module should succeed");

    assert!(
        scope.lookup("AppConfig::APP_NAME").is_some(),
        "AppConfig::APP_NAME should be accessible by qualified name in App's scope"
    );
    assert!(
        scope.lookup("APP_NAME").is_some(),
        "APP_NAME should remain accessible by bare public-const name"
    );
}

#[test]
fn test_nested_import_inside_defmod_resolves_within_module_scope() {
    let module_stages = vec![vec![
        staged_module(
            "String",
            parse_module_ast(r#"def trim(text: String) -> String { text }"#, "String"),
        ),
        staged_module(
            "Parser",
            parse_module_ast(
                r#"import String;
def parse(line: String) -> String { trim(line) }"#,
                "Parser",
            ),
        ),
    ]];

    let resolved = resolve_user_with_modules("print(Parser::parse(\" ok \"))", &module_stages)
        .expect("nested defmod import should resolve");
    assert!(!resolved.is_empty());
}

#[test]
fn test_nested_import_inside_inherent_impl_resolves_without_leaking() {
    let module_stages = vec![vec![
        staged_module(
            "String",
            parse_module_ast(r#"def trim(text: String) -> String { text }"#, "String"),
        ),
        staged_module(
            "User",
            parse_module_ast(
                r#"import String;
defstruct User { name: String }
impl User {
  def normalize(self: Self, name: String) -> String { trim(name) }
}"#,
                "User",
            ),
        ),
    ]];

    let ok = resolve_user_with_modules(
        r#"print(User::normalize(User(name: "x"), " ok "))"#,
        &module_stages,
    )
    .expect("impl-local import should resolve inside impl methods");
    assert!(!ok.is_empty());

    let leak_err = resolve_user_with_modules(
        r#"def helper(name: String) -> String { trim(name) }
print(helper("ok"))"#,
        &module_stages,
    )
    .expect_err("impl-local import must not leak to sibling top-level defs");
    assert!(
        leak_err.message.contains("Undefined function trim/1")
            || leak_err.message.contains("Undefined variable: trim"),
        "actual error: {}",
        leak_err.message
    );
}

#[test]
fn test_nested_import_inside_trait_impl_resolves() {
    let module_stages = vec![vec![
        staged_module(
            "String",
            parse_module_ast(r#"def trim(text: String) -> String { text }"#, "String"),
        ),
        staged_module(
            "Show",
            parse_module_ast(
                r#"deftrait Show {
  def to_string(self: Self) -> String
}"#,
                "Show",
            ),
        ),
        staged_module(
            "User",
            parse_module_ast(
                r#"import String;
defstruct User { name: String }
impl Show::Show for User {
  def to_string(self: Self) -> String { trim(self.name) }
}"#,
                "User",
            ),
        ),
    ]];

    let resolved = resolve_user_with_modules(
        r#"value = User(name: "ok")
print(Show::Show::to_string(value))"#,
        &module_stages,
    )
    .expect("trait-impl-local import should resolve inside trait impl methods");
    assert!(!resolved.is_empty());
}

#[test]
fn test_private_function_direct_qualified_call_reports_private() {
    let module_stages = vec![vec![staged_module(
        "OuterMod",
        parse_module_ast(r#"defp priv_fun(x: Int) -> Int { x }"#, "OuterMod"),
    )]];

    let err = resolve_user_with_modules("print(OuterMod::priv_fun(1))", &module_stages)
        .expect_err("qualified private function call should fail");

    assert!(err.message.contains("OuterMod::priv_fun/1"));
    assert!(
        err.message.contains("is private"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_private_function_from_imported_module_suggests_private_candidate() {
    let module_stages = vec![vec![staged_module(
        "OuterMod",
        parse_module_ast(
            r#"def keep(x: Int) -> Int { x }
defp priv_fun(x: Int) -> Int { x }"#,
            "OuterMod",
        ),
    )]];

    let err = resolve_user_with_modules(
        r#"import OuterMod
print(priv_fun(1))"#,
        &module_stages,
    )
    .expect_err("bare private function call should fail with guidance");

    assert!(err.message.contains("Undefined function priv_fun/1"));
    assert!(
        err.message
            .contains("Help: `OuterMod::priv_fun/1` is private"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_worker_process_init_surface_is_importable() {
    let module_stages = vec![vec![staged_process_module(parse_module_ast(
        r#"defagent FibWorker {
  meta {
    instance: Worker
    init_policy: Eager
    state: Int
  }

  @init
  def init(seed: Int) -> Result<Int> { Ok(seed) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
        "FibWorker",
    ))]];

    resolve_user_with_modules(
        r#"import FibWorker::init
worker =? init(1)
print(inspect(worker))"#,
        &module_stages,
    )
    .expect("worker init route should be importable");
}

#[test]
fn test_singleton_agent_pid_surface_is_visible_to_user_code() {
    let module_stages = vec![vec![staged_process_module(parse_module_ast(
        r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
        "Counter",
    ))]];

    resolve_user_with_modules("pid = Counter::pid()", &module_stages)
        .expect("singleton agent pid surface should resolve");
}

#[test]
fn test_singleton_agent_pid_surface_can_be_imported() {
    let module_stages = vec![vec![staged_process_module(parse_module_ast(
        r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
        "Counter",
    ))]];

    resolve_user_with_modules(
        r#"import Counter::pid
handle = pid()"#,
        &module_stages,
    )
    .expect("singleton agent pid surface should be importable");
}

#[test]
fn test_singleton_process_init_surface_is_not_importable() {
    let module_stages = vec![vec![staged_process_module(parse_module_ast(
        r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
        "Counter",
    ))]];

    let err = resolve_user_with_modules(
        r#"import Counter::init
print("ok")"#,
        &module_stages,
    )
    .expect_err("singleton init route import should fail");

    assert!(
        err.message.contains("Counter::init"),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("cannot be imported"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_compiler_generated_spawn_surface_is_not_exposed_to_user_imports() {
    let module_stages = vec![vec![staged_process_module(parse_module_ast(
        r#"defagent FibWorker {
  meta {
    instance: Worker
    init_policy: Eager
    state: Int
  }

  @init
  def init(seed: Int) -> Result<Int> { Ok(seed) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
        "FibWorker",
    ))]];

    let err = resolve_user_with_modules(
        r#"import FibWorker::spawn
print("ok")"#,
        &module_stages,
    )
    .expect_err("compiler-generated spawn should not be exposed for import");

    assert!(
        err.message.contains("FibWorker::spawn"),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("Unknown import member"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_worker_genserver_init_surface_is_importable() {
    let module_stages = vec![vec![
        staged_module(
            "ProcessTypes",
            parse_module_ast(
                r#"defenum CallResult<$Reply, $State> {
  Reply($Reply, $State),
  ReplyLater($State, (-> Result<$Reply>)),
  Stop(StopReply<$Reply>),
}

defenum StopReply<$Reply> {
  Normal($Reply),
  Error(Error),
}

defenum CastResult<$State> {
  Next($State),
  Stop(StopReason),
}

defenum StopReason {
  Normal,
  Error(Error),
}"#,
                "ProcessTypes",
            ),
        ),
        staged_process_module(parse_module_ast(
            r#"defgenserver QueueServer {
  meta {
    instance: Worker
    init_policy: Eager
    state: Int
  }

  @init
  def boot(seed: Int) -> Result<Int> { Ok(seed) }

  @call
  def size(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Reply(state, state))
  }
}"#,
            "QueueServer",
        )),
    ]];

    resolve_user_with_modules(
        r#"import QueueServer::boot
pid =? boot(1)
print(inspect(pid))"#,
        &module_stages,
    )
    .expect("worker genserver init route should be importable");
}

#[test]
fn test_singleton_genserver_pid_surface_is_visible_to_user_code() {
    let module_stages = vec![vec![
        staged_module(
            "ProcessTypes",
            parse_module_ast(
                r#"defenum CallResult<$Reply, $State> {
  Reply($Reply, $State),
  ReplyLater($State, (-> Result<$Reply>)),
  Stop(StopReply<$Reply>),
}

defenum StopReply<$Reply> {
  Normal($Reply),
  Error(Error),
}"#,
                "ProcessTypes",
            ),
        ),
        staged_process_module(parse_module_ast(
            r#"defgenserver QueueServer {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @call
  def size(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Reply(state, state))
  }
}"#,
            "QueueServer",
        )),
    ]];

    resolve_user_with_modules("pid = QueueServer::pid()", &module_stages)
        .expect("singleton genserver pid surface should resolve");
}

#[test]
fn test_singleton_genserver_pid_surface_can_be_imported() {
    let module_stages = vec![vec![
        staged_module(
            "ProcessTypes",
            parse_module_ast(
                r#"defenum CallResult<$Reply, $State> {
  Reply($Reply, $State),
  ReplyLater($State, (-> Result<$Reply>)),
  Stop(StopReply<$Reply>),
}

defenum StopReply<$Reply> {
  Normal($Reply),
  Error(Error),
}"#,
                "ProcessTypes",
            ),
        ),
        staged_process_module(parse_module_ast(
            r#"defgenserver QueueServer {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @call
  def size(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Reply(state, state))
  }
}"#,
            "QueueServer",
        )),
    ]];

    resolve_user_with_modules(
        r#"import QueueServer::pid
handle = pid()"#,
        &module_stages,
    )
    .expect("singleton genserver pid surface should be importable");
}

#[test]
fn test_singleton_process_init_surface_is_not_callable_from_user_code() {
    let module_stages = vec![vec![staged_process_module(parse_module_ast(
        r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
        "Counter",
    ))]];

    let err = resolve_user_with_modules("print(inspect(Counter::init()))", &module_stages)
        .expect_err("singleton init route call should fail");

    assert!(
        err.message.contains("Counter::init/0"),
        "actual error: {}",
        err.message
    );
    assert!(
        err.message.contains("cannot be called"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_nested_import_duplicate_conflict_still_errors() {
    let module_stages = vec![vec![
        staged_module(
            "String",
            parse_module_ast(r#"def trim(text: String) -> String { text }"#, "String"),
        ),
        staged_module(
            "Names",
            parse_module_ast(r#"def trim(text: String) -> String { text }"#, "Names"),
        ),
        staged_module(
            "Parser",
            parse_module_ast(
                r#"import String;
import Names;
def parse(line: String) -> String { trim(line) }"#,
                "Parser",
            ),
        ),
    ]];

    let err = resolve_user_with_modules("print(Parser::parse(\"ok\"))", &module_stages)
        .expect_err("conflicting nested imports should fail");
    assert!(
        err.message.contains("Import conflict") || err.message.contains("Duplicate import"),
        "actual error: {}",
        err.message
    );
}

#[test]
fn test_nested_import_shadows_auto_import_within_body_only() {
    fn find_called_uid(node: &Resolved, name: &str) -> Option<u32> {
        match node {
            Resolved::App(_, func, args) => match func.as_ref() {
                Resolved::Var(_, called_id) if called_id.name == name => Some(called_id.unique_id),
                _ => args.iter().find_map(|arg| match arg {
                    ResolvedRecordLitArg::Positional(inner)
                    | ResolvedRecordLitArg::Named(_, inner) => find_called_uid(inner, name),
                }),
            },
            Resolved::Block(_, nodes) => nodes.iter().find_map(|node| find_called_uid(node, name)),
            _ => None,
        }
    }

    let module_stages = vec![
        vec![staged_module(
            "Kernel",
            parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Kernel"),
        )],
        vec![staged_module(
            "Helper",
            parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x - y }"#, "Helper"),
        )],
        vec![staged_module(
            "Parser",
            parse_module_ast(
                r#"import Helper::add;
def parse() -> Int { add(7, 3) }"#,
                "Parser",
            ),
        )],
    ];

    let resolved =
        resolve_user_with_modules(r#"print(to_string(Parser::parse()))"#, &module_stages)
            .expect("nested explicit import should shadow auto-import inside that body");

    let helper_add_uid = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::Def(_, id, _, _, _, _, _, _)
                if id.qualified_name.as_deref() == Some("Helper::add") =>
            {
                Some(id.unique_id)
            }
            _ => None,
        })
        .expect("helper add should be resolved");

    let parser_add_uid = resolved.iter().find_map(|node| match node {
        Resolved::Def(_, id, _, _, _, _, body, _)
            if id.qualified_name.as_deref() == Some("Parser::parse") =>
        {
            find_called_uid(body.as_ref(), "add")
        }
        _ => None,
    });

    assert_eq!(parser_add_uid, Some(helper_add_uid));
}

#[test]
fn test_pipeline_rhs_desugars_partial_special_forms_into_closures() {
    let resolved = parse_and_resolve(
        r#"deferror GuardError { "guard" }

def pred(n: Int) -> Boolean {
  n > 0
}

checked = Ok(3) |>= ensure(&pred, GuardError)
flagged = True |> and(False)
verified = Ok(True) |>= assert(GuardError)
replaced = Err(GuardError) |> map_err(GuardError)
wrapped = Err(GuardError) |> cause(GuardError)"#,
    )
    .expect("pipeline partial special forms should resolve");

    let mut checked_ok = false;
    let mut flagged_ok = false;
    let mut verified_ok = false;
    let mut replaced_ok = false;
    let mut wrapped_ok = false;

    for node in resolved {
        let Resolved::Bind(_, pat, rhs) = node else {
            continue;
        };
        let ResolvedPattern::Var(id) = pat else {
            continue;
        };
        match id.name.as_str() {
            "checked" => match rhs.as_ref() {
                Resolved::ContextBind(_, _, right) => match right.as_ref() {
                    Resolved::Closure(_, params, _, body) => {
                        assert_eq!(params.len(), 1, "partial ensure must become unary closure");
                        assert!(
                            matches!(body.as_ref(), Resolved::Ensure(_, _, _, _)),
                            "partial ensure closure body must resolve to Ensure"
                        );
                        checked_ok = true;
                    }
                    other => panic!("expected closure on checked rhs, got {:?}", other),
                },
                other => panic!("expected context bind for checked, got {:?}", other),
            },
            "flagged" => match rhs.as_ref() {
                Resolved::Pipe(_, _, right) => match right.as_ref() {
                    Resolved::Closure(_, params, _, body) => {
                        assert_eq!(params.len(), 1, "partial and must become unary closure");
                        assert!(
                            matches!(body.as_ref(), Resolved::If(_, _, _, _)),
                            "partial and closure body must resolve to If"
                        );
                        flagged_ok = true;
                    }
                    other => panic!("expected closure on flagged rhs, got {:?}", other),
                },
                other => panic!("expected pipe for flagged, got {:?}", other),
            },
            "verified" => match rhs.as_ref() {
                Resolved::ContextBind(_, _, right) => match right.as_ref() {
                    Resolved::Closure(_, params, _, body) => {
                        assert_eq!(params.len(), 1, "partial assert must become unary closure");
                        assert!(
                            matches!(body.as_ref(), Resolved::Assert(_, _, _)),
                            "partial assert closure body must resolve to Assert"
                        );
                        verified_ok = true;
                    }
                    other => panic!("expected closure on verified rhs, got {:?}", other),
                },
                other => panic!("expected context bind for verified, got {:?}", other),
            },
            "replaced" => match rhs.as_ref() {
                Resolved::Pipe(_, _, right) => match right.as_ref() {
                    Resolved::Closure(_, params, _, body) => {
                        assert_eq!(params.len(), 1, "partial map_err must become unary closure");
                        assert!(
                            matches!(body.as_ref(), Resolved::MapErr(_, _, _)),
                            "partial map_err closure body must resolve to MapErr"
                        );
                        replaced_ok = true;
                    }
                    other => panic!("expected closure on replaced rhs, got {:?}", other),
                },
                other => panic!("expected pipe for replaced, got {:?}", other),
            },
            "wrapped" => match rhs.as_ref() {
                Resolved::Pipe(_, _, right) => match right.as_ref() {
                    Resolved::Closure(_, params, _, body) => {
                        assert_eq!(params.len(), 1, "partial cause must become unary closure");
                        assert!(
                            matches!(body.as_ref(), Resolved::Cause(_, _, _)),
                            "partial cause closure body must resolve to Cause"
                        );
                        wrapped_ok = true;
                    }
                    other => panic!("expected closure on wrapped rhs, got {:?}", other),
                },
                other => panic!("expected pipe for wrapped, got {:?}", other),
            },
            _ => {}
        }
    }

    assert!(checked_ok, "missing checked bind assertion");
    assert!(flagged_ok, "missing flagged bind assertion");
    assert!(verified_ok, "missing verified bind assertion");
    assert!(replaced_ok, "missing replaced bind assertion");
    assert!(wrapped_ok, "missing wrapped bind assertion");
}

#[test]
fn test_pipeline_partial_special_form_does_not_trigger_for_shadowed_local_binding() {
    let resolved = parse_and_resolve(
        r#"map_err = {|value: Int, suffix: Int| value + suffix}
out = 1 |> map_err(2)"#,
    )
    .expect("shadowed local map_err should resolve like an ordinary pipe call");

    match &resolved[1] {
        Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            Resolved::Pipe(_, _, right) => match right.as_ref() {
                Resolved::App(_, func, args) => {
                    assert!(matches!(func.as_ref(), Resolved::Var(_, id) if id.name == "map_err"));
                    assert_eq!(
                        args.len(),
                        1,
                        "ordinary shadowed call should keep its explicit arg"
                    );
                }
                other => panic!("expected ordinary app on shadowed rhs, got {:?}", other),
            },
            other => panic!("expected pipe on shadowed binding, got {:?}", other),
        },
        other => panic!("expected bind for shadowed pipeline, got {:?}", other),
    }
}

#[test]
fn test_pipeline_partial_special_form_does_not_trigger_for_shadowed_parameter() {
    fn find_pipe_rhs(node: &Resolved) -> Option<&Resolved> {
        match node {
            Resolved::Pipe(_, _, right) => Some(right.as_ref()),
            Resolved::Block(_, nodes) => nodes.iter().find_map(find_pipe_rhs),
            Resolved::Bind(_, _, rhs) | Resolved::SafeBind(_, _, rhs) => find_pipe_rhs(rhs),
            _ => None,
        }
    }

    let resolved = parse_and_resolve(
        r#"def apply(map_err: (Int -> Int)) -> Int {
  1 |> map_err(2)
}"#,
    )
    .expect("shadowed parameter map_err should resolve like an ordinary pipe call");

    let pipe_rhs = resolved
        .iter()
        .find_map(|node| match node {
            Resolved::Def(_, id, _, _, _, _, body, _) if id.name == "apply" => find_pipe_rhs(body),
            _ => None,
        })
        .expect("expected pipe rhs inside apply body");

    match pipe_rhs {
        Resolved::App(_, func, args) => {
            assert!(matches!(func.as_ref(), Resolved::Var(_, id) if id.name == "map_err"));
            assert_eq!(
                args.len(),
                1,
                "ordinary shadowed parameter call should keep its explicit arg"
            );
        }
        other => panic!(
            "expected ordinary app on shadowed parameter rhs, got {:?}",
            other
        ),
    }
}

#[test]
fn explicit_type_apply_resolves_callable_identity_and_type_arguments() {
    let resolved = parse_and_resolve(
        r#"def identity(value: $A) -> $A { value }
value = identity::<Int>(1)
fn = &identity::<Int>"#,
    )
    .expect("explicit type application should resolve");

    let def_id = match &resolved[0] {
        Resolved::Def(_, id, ..) => id.unique_id,
        other => panic!("expected definition, got {other:?}"),
    };
    for node in [&resolved[1], &resolved[2]] {
        let Resolved::Bind(_, _, rhs) = node else {
            panic!("expected binding, got {node:?}");
        };
        let applied = match rhs.as_ref() {
            Resolved::App(_, callee, _) => callee.as_ref(),
            Resolved::Capture(_, target, _) => target.as_ref(),
            other => panic!("expected call or capture, got {other:?}"),
        };
        assert!(matches!(
            applied,
            Resolved::TypeApply(_, target, args)
                if matches!(target.as_ref(), Resolved::Var(_, id) if id.unique_id == def_id)
                    && matches!(args.as_slice(), [AstTy::Named(_, name)] if name == "Int")
        ));
    }
}
