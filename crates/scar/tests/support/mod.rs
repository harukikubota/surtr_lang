use std::sync::OnceLock;

use scar::typed::TypedNode;
use scar::{
    typecheck_with_context as scar_typecheck_with_context, ScarCheckpoint, ScarSession,
    TypecheckContext,
};
use sindr::policy::RuntimeSourcePolicy;
use spire::ast::Ast;

const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../../lib/bootstrap.srt");
const SPECIAL_TYPES_SOURCE: &str = include_str!("../../../../lib/types/special_types.srt");
const FUNCTION_PRELUDE_SOURCE: &str = include_str!("../../../../lib/function.srt");
const KERNEL_PRELUDE_SOURCE: &str = include_str!("../../../../lib/kernel.srt");
const ADD_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/operator/add.srt");
const SUB_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/operator/sub.srt");
const MUL_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/operator/mul.srt");
const SHOW_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/show.srt");
const DEFAULT_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/default.srt");
const EQ_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/operator/eq.srt");
const COMPARE_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/operator/compare.srt");
const CONCAT_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/operator/concat.srt");
const FROM_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/from.srt");
const TRY_FROM_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/try_from.srt");
const ENCODE_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/encode.srt");
const DECODE_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/decode.srt");
const FUNCTOR_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/operator/functor.srt");
const BIFUNCTOR_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/operator/bifunctor.srt");
const APPLICATIVE_MODULE_SOURCE: &str =
    include_str!("../../../../lib/traits/operator/applicative.srt");
const MONAD_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/operator/monad.srt");
const ALTERNATIVE_MODULE_SOURCE: &str =
    include_str!("../../../../lib/traits/operator/alternative.srt");
const MONOID_MODULE_SOURCE: &str = include_str!("../../../../lib/types/monoid.srt");
const PIPE_APPLY_MODULE_SOURCE: &str =
    include_str!("../../../../lib/traits/operator/pipe_apply.srt");
const COMPOSE_MODULE_SOURCE: &str = include_str!("../../../../lib/traits/operator/compose.srt");
const COMPOSABLE_MODULE_SOURCE: &str =
    include_str!("../../../../lib/traits/operator/composable.srt");
const LIFT_COMPOSABLE_MODULE_SOURCE: &str =
    include_str!("../../../../lib/traits/operator/lift_composable.srt");
const KLEISLI_COMPOSABLE_MODULE_SOURCE: &str =
    include_str!("../../../../lib/traits/operator/kleisli_composable.srt");
const INT_MODULE_SOURCE: &str = include_str!("../../../../lib/types/int.srt");
const STRING_MODULE_SOURCE: &str = include_str!("../../../../lib/types/string.srt");
const REGEX_MODULE_SOURCE: &str = r#"@builtin type Regex
@builtin type RegexCaptures
@builtin type RegexMatch

impl Regex {}
impl RegexCaptures {}
impl RegexMatch {}"#;
const BOOLEAN_MODULE_SOURCE: &str = include_str!("../../../../lib/types/boolean.srt");
const ORDERING_MODULE_SOURCE: &str = include_str!("../../../../lib/types/ordering.srt");
const TUPLE_MODULE_SOURCE: &str = include_str!("../../../../lib/types/tuple.srt");
const ERROR_MODULE_SOURCE: &str = include_str!("../../../../lib/types/error.srt");
const LIST_MODULE_SOURCE: &str = include_str!("../../../../lib/types/list.srt");
const OPTION_MODULE_SOURCE: &str = include_str!("../../../../lib/types/option.srt");
const GENERATOR_MODULE_SOURCE: &str = r#"@builtin type Generator<$State, $Item>

impl Generator {}"#;
const HASH_MAP_MODULE_SOURCE: &str = include_str!("../../../../lib/types/hash_map.srt");
const RESULT_MODULE_SOURCE: &str = include_str!("../../../../lib/types/result.srt");
const DURATION_MODULE_SOURCE: &str = include_str!("../../../../lib/types/duration.srt");
const RANGE_MODULE_SOURCE: &str = include_str!("../../../../lib/types/range.srt");
const PROCESS_MODULE_SOURCE: &str = include_str!("../../../../lib/process.srt");
const FACET_MODULE_SOURCE: &str = include_str!("../../../../lib/facet.srt");
const FLOAT_MODULE_SOURCE: &str = include_str!("../../../../lib/types/float.srt");
const JSON_MODULE_SOURCE: &str = include_str!("../../../../lib/types/json.srt");
const RANDOM_MODULE_SOURCE: &str = include_str!("../../../../lib/Random.srt");
const FILE_MODULE_SOURCE: &str = include_str!("../../../../lib/file.srt");

pub(crate) fn typecheck(
    resolved: Vec<sigil::resolved::Resolved>,
) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    typecheck_with_context(resolved, TypecheckContext::default())
}

pub(crate) fn typecheck_with_context(
    resolved: Vec<sigil::resolved::Resolved>,
    context: TypecheckContext,
) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    let mut session = session_from_cached_std_prelude();
    session.typecheck_in_place_with_context(resolved, context)
}

fn parse_std_module_stage(source: &str, fallback_module_path: &str) -> Vec<sigil::StagedModuleAst> {
    let ast = spire::parse_with_context(
        source,
        spire::ParserContext::module(
            0,
            (fallback_module_path == "Facet").then(|| fallback_module_path.into()),
        )
        .with_rules(spire::ParseRules::std_module()),
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

    fn partition_nested_imports(body: Vec<Ast>) -> (Vec<Ast>, Vec<Ast>) {
        let mut imports = Vec::new();
        let mut rest = Vec::new();
        for stmt in body {
            if matches!(stmt, Ast::Import(_, _, _)) {
                imports.push(stmt);
            } else {
                rest.push(stmt);
            }
        }
        (imports, rest)
    }

    fn first_non_import_index(ast: &[Ast]) -> usize {
        ast.iter()
            .take_while(|stmt| matches!(stmt, Ast::Import(_, _, _)))
            .count()
    }

    fn surface_module_name(name: &str) -> &str {
        name.strip_prefix("Global::").unwrap_or(name)
    }

    fn find_result_owner_module(lowered: &[sigil::StagedModuleAst]) -> Option<usize> {
        lowered.iter().position(|module| {
            surface_module_name(&module.module_path) == "Result"
                && matches!(
                    module
                        .ast
                        .iter()
                        .find(|stmt| !matches!(stmt, Ast::Import(_, _, _))),
                    Some(Ast::ImplDef(_, target, _, _)) if surface_module_name(target) == "Result"
                )
        })
    }

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
                let (local_imports, methods) = partition_nested_imports(methods);
                module_ast.extend(local_imports);
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
            Ast::TraitImplDef(
                span,
                trait_name,
                trait_args,
                target_ty,
                where_clause,
                methods,
                attrs,
            ) => {
                let module_path = match &target_ty {
                    spire::ast::AstTy::Named(_, name)
                    | spire::ast::AstTy::ImplTrait(_, name)
                    | spire::ast::AstTy::Generic(_, name, _) => name.clone(),
                    _ => String::new(),
                };
                let mut module_ast = shared_imports.clone();
                let (local_imports, methods) = partition_nested_imports(methods);
                module_ast.extend(local_imports);
                module_ast.push(Ast::TraitImplDef(
                    span,
                    trait_name,
                    trait_args,
                    target_ty,
                    where_clause,
                    methods,
                    attrs.clone(),
                ));
                lowered.push(sigil::StagedModuleAst {
                    module_path,
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

    // Match the real xldr lowering strategy used by integration tests:
    // keep normal top-level std declarations in the global declaration
    // layer, but attach `Result` constructor contracts to the sole `defmod`
    // when present so Scar sees `Result::Ok` / `Result::Err`.
    if !shared_result_ctor_contracts.is_empty() {
        if let Some(idx) =
            find_result_owner_module(&lowered).or_else(|| (lowered.len() == 1).then_some(0))
        {
            let insert_at = first_non_import_index(&lowered[idx].ast);
            lowered[idx]
                .ast
                .splice(insert_at..insert_at, shared_result_ctor_contracts);
        } else {
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

pub(crate) fn std_module_stages() -> Vec<Vec<sigil::StagedModuleAst>> {
    cached_std_prelude().module_stages.clone()
}

struct CachedStdPrelude {
    module_stages: Vec<Vec<sigil::StagedModuleAst>>,
    declaration_index: sigil::DeclarationIndex,
    resolved_len: usize,
    resolve_resume_state: sigil::ResolveResumeState,
    checkpoint: ScarCheckpoint,
}

fn cached_std_prelude() -> &'static CachedStdPrelude {
    static CACHE: OnceLock<CachedStdPrelude> = OnceLock::new();

    CACHE.get_or_init(|| {
        let module_stages = build_std_module_stages(&[]);
        let declaration_index = sigil::precollect_declaration_index(&module_stages)
            .expect("std modules should precollect");
        let std_resolved = sigil::resolve_staged_program_with_state(
            &module_stages,
            Vec::new(),
            &declaration_index,
            None,
        )
        .expect("std modules should resolve");
        let resolved_len = std_resolved.resolved.len();
        let resolve_resume_state = std_resolved.resume_state.clone();
        let mut session = ScarSession::new();
        session
            .typecheck_with_context(
                std_resolved.resolved,
                TypecheckContext {
                    runtime_policy: RuntimeSourcePolicy::std_module(),
                    enforce_builtin_type_contracts: true,
                    allow_error_function_params: true,
                    allow_private_facet_inspection: false,
                },
            )
            .expect("std modules should typecheck");
        let checkpoint = session.checkpoint();

        CachedStdPrelude {
            module_stages,
            declaration_index,
            resolved_len,
            resolve_resume_state,
            checkpoint,
        }
    })
}

pub(crate) fn session_from_cached_std_prelude() -> ScarSession {
    let prelude = cached_std_prelude();
    let mut session = ScarSession::new();
    session.rollback(prelude.checkpoint.clone());
    session
}

fn parse_user_module_stage(source: &str) -> Vec<sigil::StagedModuleAst> {
    let ast = spire::parse_with_context(source, spire::ParserContext::module(0, None))
        .expect("definition source should parse");

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
            other => shared_global_defs.push(other),
        }
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

pub(crate) fn std_module_stages_with_overrides(
    overrides: &[(&str, &str)],
) -> Vec<Vec<sigil::StagedModuleAst>> {
    build_std_module_stages(overrides)
}

fn build_std_module_stages(overrides: &[(&str, &str)]) -> Vec<Vec<sigil::StagedModuleAst>> {
    vec![
        parse_std_module_stage(BUILTIN_PRELUDE_SOURCE, "Bootstrap"),
        [
            (
                "SpecialTypes",
                pick_override("SpecialTypes", SPECIAL_TYPES_SOURCE, overrides),
            ),
            (
                "Function",
                pick_override("Function", FUNCTION_PRELUDE_SOURCE, overrides),
            ),
            (
                "Kernel",
                pick_override("Kernel", KERNEL_PRELUDE_SOURCE, overrides),
            ),
            ("Add", pick_override("Add", ADD_MODULE_SOURCE, overrides)),
            ("Sub", pick_override("Sub", SUB_MODULE_SOURCE, overrides)),
            ("Mul", pick_override("Mul", MUL_MODULE_SOURCE, overrides)),
            ("Eq", pick_override("Eq", EQ_MODULE_SOURCE, overrides)),
            (
                "Compare",
                pick_override("Compare", COMPARE_MODULE_SOURCE, overrides),
            ),
            (
                "Concat",
                pick_override("Concat", CONCAT_MODULE_SOURCE, overrides),
            ),
            ("Show", pick_override("Show", SHOW_MODULE_SOURCE, overrides)),
            (
                "Default",
                pick_override("Default", DEFAULT_MODULE_SOURCE, overrides),
            ),
            (
                "Ordering",
                pick_override("Ordering", ORDERING_MODULE_SOURCE, overrides),
            ),
            (
                "Tuple",
                pick_override("Tuple", TUPLE_MODULE_SOURCE, overrides),
            ),
            ("From", pick_override("From", FROM_MODULE_SOURCE, overrides)),
            (
                "TryFrom",
                pick_override("TryFrom", TRY_FROM_MODULE_SOURCE, overrides),
            ),
            (
                "Encode",
                pick_override("Encode", ENCODE_MODULE_SOURCE, overrides),
            ),
            (
                "Decode",
                pick_override("Decode", DECODE_MODULE_SOURCE, overrides),
            ),
            (
                "Functor",
                pick_override("Functor", FUNCTOR_MODULE_SOURCE, overrides),
            ),
            (
                "Bifunctor",
                pick_override("Bifunctor", BIFUNCTOR_MODULE_SOURCE, overrides),
            ),
            (
                "Applicative",
                pick_override("Applicative", APPLICATIVE_MODULE_SOURCE, overrides),
            ),
            (
                "Monad",
                pick_override("Monad", MONAD_MODULE_SOURCE, overrides),
            ),
            (
                "Alternative",
                pick_override("Alternative", ALTERNATIVE_MODULE_SOURCE, overrides),
            ),
            (
                "Monoid",
                pick_override("Monoid", MONOID_MODULE_SOURCE, overrides),
            ),
            (
                "PipeApply",
                pick_override("PipeApply", PIPE_APPLY_MODULE_SOURCE, overrides),
            ),
            (
                "Compose",
                pick_override("Compose", COMPOSE_MODULE_SOURCE, overrides),
            ),
            (
                "Composable",
                pick_override("Composable", COMPOSABLE_MODULE_SOURCE, overrides),
            ),
            (
                "LiftComposable",
                pick_override("LiftComposable", LIFT_COMPOSABLE_MODULE_SOURCE, overrides),
            ),
            (
                "KleisliComposable",
                pick_override(
                    "KleisliComposable",
                    KLEISLI_COMPOSABLE_MODULE_SOURCE,
                    overrides,
                ),
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
                "Option",
                pick_override("Option", OPTION_MODULE_SOURCE, overrides),
            ),
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
                "Duration",
                pick_override("Duration", DURATION_MODULE_SOURCE, overrides),
            ),
            (
                "Range",
                pick_override("Range", RANGE_MODULE_SOURCE, overrides),
            ),
            (
                "Process",
                pick_override("Process", PROCESS_MODULE_SOURCE, overrides),
            ),
            (
                "Facet",
                pick_override("Facet", FACET_MODULE_SOURCE, overrides),
            ),
            (
                "Float",
                pick_override("Float", FLOAT_MODULE_SOURCE, overrides),
            ),
            ("Json", pick_override("Json", JSON_MODULE_SOURCE, overrides)),
            (
                "Random",
                pick_override("Random", RANDOM_MODULE_SOURCE, overrides),
            ),
            ("File", pick_override("File", FILE_MODULE_SOURCE, overrides)),
        ]
        .into_iter()
        .flat_map(|(name, source)| parse_std_module_stage(source, name))
        .collect(),
    ]
}

pub(crate) fn resolve_with_builtin_prelude_result(
    source: &str,
) -> Result<Vec<sigil::resolved::Resolved>, sigil::error::ResolveError> {
    let prelude = cached_std_prelude();
    let user_ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("source should parse");
    sigil::resolve_staged_program_from_state(
        &prelude.module_stages,
        user_ast,
        &prelude.declaration_index,
        None,
        prelude.module_stages.len(),
        prelude.resolve_resume_state.clone(),
    )
    .map(|resolved| resolved.resolved)
}

pub(crate) fn resolve_program_with_builtin_prelude(source: &str) -> Vec<sigil::resolved::Resolved> {
    let prelude = cached_std_prelude();
    let user_ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("source should parse");
    sigil::resolve_staged_program(
        &prelude.module_stages,
        user_ast,
        &prelude.declaration_index,
        None,
    )
    .expect("source should resolve")
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
    let prelude = cached_std_prelude();
    let user_ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("source should parse");
    sigil::resolve_staged_program_from_state(
        &prelude.module_stages,
        user_ast,
        &prelude.declaration_index,
        Some(module_path.to_owned()),
        prelude.module_stages.len(),
        prelude.resolve_resume_state.clone(),
    )
    .map(|resolved| resolved.resolved)
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
            allow_error_function_params: false,
            allow_private_facet_inspection: false,
        },
    )
}

pub(crate) fn typecheck_module_source_result(source: &str) -> Result<Vec<TypedNode>, String> {
    let prelude = cached_std_prelude();
    let mut module_stages = prelude.module_stages.clone();
    module_stages.push(parse_user_module_stage(source));
    let declaration_index = sigil::precollect_declaration_index(&module_stages)
        .map_err(|err| format!("resolve precollect failed: {}", err.message))?;
    let resolved = sigil::resolve_staged_program_with_state(
        &module_stages,
        Vec::new(),
        &declaration_index,
        None,
    )
    .map_err(|err| format!("resolve failed: {}", err.message))?;
    let user_resolved = sigil::ResolvedStagedProgram {
        resolved: resolved
            .resolved
            .into_iter()
            .skip(prelude.resolved_len)
            .collect(),
        process_specs: resolved.process_specs,
        boot_plan: resolved.boot_plan,
        resume_state: resolved.resume_state,
    };
    scar::typecheck_staged_program(user_resolved)
        .map(|program| program.nodes)
        .map_err(|err| err.message)
}

pub(crate) fn typecheck_std_modules_with_overrides(
    overrides: &[(&str, &str)],
) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    let module_stages = std_module_stages_with_overrides(overrides);
    let declaration_index =
        sigil::precollect_declaration_index(&module_stages).expect("std modules should precollect");
    let resolved =
        sigil::resolve_staged_program(&module_stages, Vec::new(), &declaration_index, None)
            .expect("std modules should resolve");
    scar_typecheck_with_context(
        resolved,
        TypecheckContext {
            runtime_policy: RuntimeSourcePolicy::std_module(),
            enforce_builtin_type_contracts: true,
            allow_error_function_params: true,
            allow_private_facet_inspection: false,
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
