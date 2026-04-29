use std::sync::OnceLock;
use std::thread;

use scar::typed::TypedNode;
use scar::{
    typecheck as scar_typecheck, typecheck_with_context as scar_typecheck_with_context,
    TypecheckContext,
};
use sindr::policy::RuntimeSourcePolicy;
use spire::ast::Ast;

const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../../lib/bootstrap.srt");
const SPECIAL_TYPES_SOURCE: &str = include_str!("../../../../lib/special_types.srt");
const KERNEL_PRELUDE_SOURCE: &str = include_str!("../../../../lib/kernel.srt");
const ADD_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/add.srt");
const SUB_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/sub.srt");
const MUL_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/mul.srt");
const NUMERIC_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/numeric.srt");
const SHOW_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/show.srt");
const EQ_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/eq.srt");
const NEQ_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/neq.srt");
const COMPARE_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/compare.srt");
const LT_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/lt.srt");
const LTE_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/lte.srt");
const GT_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/gt.srt");
const GTE_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/gte.srt");
const ORD_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/ord.srt");
const CONCAT_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/concat.srt");
const FROM_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/from.srt");
const TRY_FROM_MODULE_SOURCE: &str = include_str!("../../../../lib/trait/try_from.srt");
const INT_MODULE_SOURCE: &str = include_str!("../../../../lib/int.srt");
const STRING_MODULE_SOURCE: &str = include_str!("../../../../lib/string.srt");
const REGEX_MODULE_SOURCE: &str = include_str!("../../../../lib/regex.srt");
const BOOLEAN_MODULE_SOURCE: &str = include_str!("../../../../lib/boolean.srt");
const ORDERING_MODULE_SOURCE: &str = include_str!("../../../../lib/ordering.srt");
const ERROR_MODULE_SOURCE: &str = include_str!("../../../../lib/error.srt");
const LIST_MODULE_SOURCE: &str = include_str!("../../../../lib/list.srt");
const GENERATOR_MODULE_SOURCE: &str = r#"@@builtin type Generator<$State, $Item>

impl Generator {}"#;
const HASH_MAP_MODULE_SOURCE: &str = include_str!("../../../../lib/hash_map.srt");
const RESULT_MODULE_SOURCE: &str = include_str!("../../../../lib/result.srt");
const OPTION_MODULE_SOURCE: &str = include_str!("../../../../lib/option.srt");
const LENS_MODULE_SOURCE: &str = include_str!("../../../../lib/lens.srt");
const FLOAT_MODULE_SOURCE: &str = include_str!("../../../../lib/float.srt");
const TEST_STACK_SIZE: usize = 32 * 1024 * 1024;

pub(crate) fn run_with_large_stack<T>(label: &str, f: impl FnOnce() -> T + Send + 'static) -> T
where
    T: Send + 'static,
{
    thread::Builder::new()
        .name(format!("scar-test-{label}"))
        .stack_size(TEST_STACK_SIZE)
        .spawn(f)
        .unwrap_or_else(|e| panic!("failed to spawn large-stack test thread `{label}`: {e}"))
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
}

pub(crate) fn typecheck(
    resolved: Vec<sigil::resolved::Resolved>,
) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    run_with_large_stack("typecheck", move || scar_typecheck(resolved))
}

pub(crate) fn typecheck_with_context(
    resolved: Vec<sigil::resolved::Resolved>,
    context: TypecheckContext,
) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    run_with_large_stack("typecheck_with_context", move || {
        scar_typecheck_with_context(resolved, context)
    })
}

fn strip_test_annotations(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("@@test"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_std_module_stage(
    source: &str,
    _fallback_module_path: &str,
) -> Vec<sigil::StagedModuleAst> {
    let ast = spire::parse_with_context(
        &strip_test_annotations(source),
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::std_module()),
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
                    auto_import: attrs.auto_import,
                });
            }
            Ast::ImplDef(span, target, methods, attrs) => {
                let mut module_ast = shared_imports.clone();
                module_ast.push(Ast::ImplDef(span, target.clone(), methods, attrs.clone()));
                lowered.push(sigil::StagedModuleAst {
                    module_path: target,
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
            module_path: String::new(),
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

pub(crate) fn std_module_stages() -> Vec<Vec<sigil::StagedModuleAst>> {
    std_module_stages_with_overrides(&[])
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

fn parse_user_module_stage(source: &str) -> Vec<sigil::StagedModuleAst> {
    let ast = spire::parse_with_context(source, spire::ParserContext::module(0, None))
        .expect("module source should parse");

    let shared_imports = ast
        .iter()
        .filter_map(|stmt| match stmt {
            Ast::Import(_, _, _) => Some(stmt.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut lowered = Vec::new();
    let mut shared_global_defs = Vec::new();

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
            Ast::ImplDef(span, target, methods, attrs) => {
                let mut module_ast = shared_imports.clone();
                module_ast.push(Ast::ImplDef(span, target.clone(), methods, attrs.clone()));
                lowered.push(sigil::StagedModuleAst {
                    module_path: target,
                    ast: module_ast,
                    module_doc: attrs.doc,
                    auto_import: attrs.auto_import,
                });
            }
            Ast::Import(_, _, _) => {}
            other => shared_global_defs.push(other),
        }
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

pub(crate) fn std_module_stages_with_overrides(
    overrides: &[(&str, &str)],
) -> Vec<Vec<sigil::StagedModuleAst>> {
    vec![
        parse_std_module_stage(BUILTIN_PRELUDE_SOURCE, "Bootstrap"),
        [
            (
                "SpecialTypes",
                pick_override("SpecialTypes", SPECIAL_TYPES_SOURCE, overrides),
            ),
            (
                "Kernel",
                pick_override("Kernel", KERNEL_PRELUDE_SOURCE, overrides),
            ),
            ("Add", pick_override("Add", ADD_MODULE_SOURCE, overrides)),
            ("Sub", pick_override("Sub", SUB_MODULE_SOURCE, overrides)),
            ("Mul", pick_override("Mul", MUL_MODULE_SOURCE, overrides)),
            ("Eq", pick_override("Eq", EQ_MODULE_SOURCE, overrides)),
            ("Neq", pick_override("Neq", NEQ_MODULE_SOURCE, overrides)),
            (
                "Compare",
                pick_override("Compare", COMPARE_MODULE_SOURCE, overrides),
            ),
            ("Lt", pick_override("Lt", LT_MODULE_SOURCE, overrides)),
            ("Lte", pick_override("Lte", LTE_MODULE_SOURCE, overrides)),
            ("Gt", pick_override("Gt", GT_MODULE_SOURCE, overrides)),
            ("Gte", pick_override("Gte", GTE_MODULE_SOURCE, overrides)),
            (
                "Concat",
                pick_override("Concat", CONCAT_MODULE_SOURCE, overrides),
            ),
            (
                "Numeric",
                pick_override("Numeric", NUMERIC_MODULE_SOURCE, overrides),
            ),
            ("Show", pick_override("Show", SHOW_MODULE_SOURCE, overrides)),
            (
                "Ordering",
                pick_override("Ordering", ORDERING_MODULE_SOURCE, overrides),
            ),
            ("Ord", pick_override("Ord", ORD_MODULE_SOURCE, overrides)),
            ("From", pick_override("From", FROM_MODULE_SOURCE, overrides)),
            (
                "TryFrom",
                pick_override("TryFrom", TRY_FROM_MODULE_SOURCE, overrides),
            ),
            ("Int", pick_override("Int", INT_MODULE_SOURCE, overrides)),
            (
                "String",
                pick_override("String", STRING_MODULE_SOURCE, overrides),
            ),
            (
                "Regex",
                pick_override("Regex", REGEX_MODULE_SOURCE, overrides),
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
                "Generator",
                pick_override("Generator", GENERATOR_MODULE_SOURCE, overrides),
            ),
            (
                "HashMap",
                pick_override("HashMap", HASH_MAP_MODULE_SOURCE, overrides),
            ),
            (
                "Result",
                pick_override("Result", RESULT_MODULE_SOURCE, overrides),
            ),
            (
                "Option",
                pick_override("Option", OPTION_MODULE_SOURCE, overrides),
            ),
            ("Lens", pick_override("Lens", LENS_MODULE_SOURCE, overrides)),
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

pub(crate) fn resolve_with_builtin_prelude_result(
    source: &str,
) -> Result<Vec<sigil::resolved::Resolved>, sigil::error::ResolveError> {
    let source = source.to_owned();
    run_with_large_stack("resolve_with_builtin_prelude_result", move || {
        let (module_stages, declaration_index) = cached_std_modules_and_declarations();
        let user_ast = spire::parse_with_context(&source, spire::ParserContext::project(0))
            .expect("source should parse");
        sigil::resolve_staged_program(module_stages, user_ast, declaration_index, None)
    })
}

pub(crate) fn resolve_with_builtin_prelude_in_script_module(
    source: &str,
) -> Result<Vec<sigil::resolved::Resolved>, sigil::error::ResolveError> {
    resolve_with_builtin_prelude_in_module(source, "__Script::fixture")
}

pub(crate) fn resolve_with_builtin_prelude_in_module(
    source: &str,
    module_path: &str,
) -> Result<Vec<sigil::resolved::Resolved>, sigil::error::ResolveError> {
    let source = source.to_owned();
    let module_path = module_path.to_owned();
    run_with_large_stack("resolve_with_builtin_prelude_in_module", move || {
        let (module_stages, declaration_index) = cached_std_modules_and_declarations();
        let user_ast = spire::parse_with_context(&source, spire::ParserContext::project(0))
            .expect("source should parse");
        sigil::resolve_staged_program(
            module_stages,
            user_ast,
            declaration_index,
            Some(module_path),
        )
    })
}

pub(crate) fn resolve_with_builtin_prelude(source: &str) -> Vec<sigil::resolved::Resolved> {
    resolve_with_builtin_prelude_result(source).expect("source should resolve")
}

pub(crate) fn typecheck_with_builtin_prelude(source: &str) -> Vec<TypedNode> {
    let resolved = resolve_with_builtin_prelude(source);
    typecheck(resolved).expect("source should typecheck")
}

pub(crate) fn typecheck_with_builtin_prelude_in_script_module(source: &str) -> Vec<TypedNode> {
    let resolved = resolve_with_builtin_prelude_in_script_module(source)
        .expect("source should resolve inside script module");
    typecheck(resolved).expect("source should typecheck inside script module")
}

pub(crate) fn typecheck_with_rules(
    source: &str,
    runtime_policy: RuntimeSourcePolicy,
) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    let resolved = resolve_with_builtin_prelude(source);
    typecheck_with_context(
        resolved,
        TypecheckContext {
            runtime_policy,
            enforce_builtin_type_contracts: false,
        },
    )
}

pub(crate) fn typecheck_module_source_result(source: &str) -> Result<Vec<TypedNode>, String> {
    let source = source.to_owned();
    run_with_large_stack("typecheck_module_source_result", move || {
        let mut module_stages = std_module_stages();
        module_stages.push(parse_user_module_stage(&source));
        let declaration_index = sigil::precollect_declaration_index(&module_stages)
            .map_err(|err| format!("resolve precollect failed: {}", err.message))?;
        let resolved =
            sigil::resolve_staged_program(&module_stages, Vec::new(), &declaration_index, None)
                .map_err(|err| format!("resolve failed: {}", err.message))?;
        typecheck(resolved).map_err(|err| err.message)
    })
}

pub(crate) fn typecheck_std_modules_with_overrides(
    overrides: &[(&str, &str)],
) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    let overrides = overrides
        .iter()
        .map(|(name, source)| ((*name).to_owned(), (*source).to_owned()))
        .collect::<Vec<_>>();
    run_with_large_stack("typecheck_std_modules_with_overrides", move || {
        let override_refs = overrides
            .iter()
            .map(|(name, source)| (name.as_str(), source.as_str()))
            .collect::<Vec<_>>();
        let module_stages = std_module_stages_with_overrides(&override_refs);
        let declaration_index = sigil::precollect_declaration_index(&module_stages)
            .expect("std modules should precollect");
        let resolved =
            sigil::resolve_staged_program(&module_stages, Vec::new(), &declaration_index, None)
                .expect("std modules should resolve");
        scar_typecheck_with_context(
            resolved,
            TypecheckContext {
                runtime_policy: RuntimeSourcePolicy::std_module(),
                enforce_builtin_type_contracts: true,
            },
        )
    })
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
