//! Manual process-warm and incremental Scar measurements. No VM execution.
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Warm,
    Incremental,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Api {
    Transactional,
    InPlace,
}
impl Api {
    fn name(self) -> &'static str {
        match self {
            Self::Transactional => "transactional",
            Self::InPlace => "in_place",
        }
    }
}
struct Options {
    mode: Mode,
    api: Api,
    iterations: usize,
    size: usize,
    input: Option<String>,
}

use scar::TypecheckContext;
use sha2::{Digest, Sha256};
use sigil::{ResolveEnvironment, ResolvedStagedProgram, SigilSession};
use spire::ast::Ast;
use std::sync::Arc;
use std::time::Instant;

struct Prepared {
    snapshot: Arc<xldr::DefaultStdlibSnapshot>,
    environment: ResolveEnvironment,
    prefix_metadata: ResolvedStagedProgram,
}
impl Prepared {
    fn new() -> Result<Self, String> {
        let snapshot = xldr::default_stdlib_semantic_snapshot().map_err(|e| format!("{e:?}"))?;
        let environment =
            ResolveEnvironment::from_stages(&snapshot.module_stages).map_err(|e| e.to_string())?;
        // Resolve prefix metadata once, outside sample timings. No Scar or VM work here.
        let mut prefix_metadata = sigil::resolve_staged_program_from_state(
            &snapshot.module_stages,
            Vec::new(),
            snapshot.declaration_index(),
            None,
            0,
            sigil::ResolveResumeState::default(),
        )
        .map_err(|e| e.to_string())?;
        prefix_metadata.stages.clear();
        Ok(Self {
            snapshot,
            environment,
            prefix_metadata,
        })
    }
    fn resolver(&self) -> Result<SigilSession, String> {
        SigilSession::from_environment(
            &self.environment,
            Some("ProfileHarness".into()),
            self.snapshot.resolve_state(),
        )
        .map_err(|e| e.to_string())
    }
    fn program(&self, resolved: Vec<sigil::resolved::Resolved>) -> ResolvedStagedProgram {
        let mut program = self.prefix_metadata.clone();
        program.stages = vec![sigil::ResolvedStage {
            stage_index: self.snapshot.module_stages.len(),
            resolved,
            process_specs: Vec::new(),
        }];
        program.resume_state = self.snapshot.resolve_state(); // Scar does not consume this field.
        program
    }
}
fn micros(start: Instant) -> f64 {
    start.elapsed().as_nanos() as f64 / 1000.0
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn declaration_count(ast: &[Ast]) -> usize {
    ast.iter()
        .filter(|node| {
            matches!(
                node,
                Ast::Def(..)
                    | Ast::ConstDef(..)
                    | Ast::ExtractorDef(..)
                    | Ast::StructDef(..)
                    | Ast::RecordDef(..)
                    | Ast::EnumDef(..)
                    | Ast::DeferrorDef(..)
                    | Ast::TraitDef(..)
                    | Ast::TraitImplDef(..)
                    | Ast::ImplDef(..)
            )
        })
        .count()
}
fn generated_definition(index: usize) -> String {
    let body = if index == 0 {
        "x".into()
    } else {
        format!("profile_stage{:03}(x)", index - 1)
    };
    format!("def profile_stage{index:03}(x: $A) -> $A {{ {body} }}\n")
}
fn generated_calls(index: usize) -> String {
    format!(
        "profile_stage{index:03}(7)\nprofile_stage{index:03}(\"x\")\nprofile_stage{index:03}(7)\n"
    )
}
fn generated_chunk(index: usize) -> String {
    generated_definition(index) + &generated_calls(index)
}
fn generated_warm(size: usize) -> String {
    let mut source: String = (0..size).map(generated_definition).collect();
    source.push_str(&generated_calls(size - 1));
    source
}
const TRAIT_CHUNK: &str = "defrecord ProfileRecord(value: Int)\ndeftrait ProfileRead { def read(self: Self) -> Int }\nimpl ProfileRead for ProfileRecord { def read(self: ProfileRecord) -> Int { self.value } }\nProfileRead::read(ProfileRecord(7))\n";
const REJECTED_CHUNK: &str = "def profile_discard(x: Int) -> Int { x }\ndef profile_invalid(x: Int) -> Int { \"invalid\" }\n";
fn recovery_chunk(size: usize) -> String {
    format!("def profile_discard(x: String) -> String {{ profile_stage{:03}(x) }}\nprofile_discard(\"recovered\")\nProfileRead::read(ProfileRecord(9))\n", size - 1)
}

struct Timings {
    parse: f64,
    resolve: f64,
    scar: f64,
    restore: f64,
    checkpoint: f64,
    rollback: f64,
    resolver_restore: f64,
    resolver_checkpoint: f64,
    resolver_rollback: f64,
}
impl Default for Timings {
    fn default() -> Self {
        Self {
            parse: 0.0,
            resolve: 0.0,
            scar: 0.0,
            restore: 0.0,
            checkpoint: 0.0,
            rollback: 0.0,
            resolver_restore: 0.0,
            resolver_checkpoint: 0.0,
            resolver_rollback: 0.0,
        }
    }
}
fn semantic_fingerprint(mut program: Value) -> String {
    fn normalize(value: &mut Value, bindings: &mut std::collections::HashMap<u64, usize>) {
        match value {
            Value::Object(object) => {
                // Compiler-created local binders use a process-global fresh UID supply.
                // Alpha-rename only these ResolvedId objects, preserving every other
                // field and one-to-one binding/reference relationships across the program.
                if object.get("compiler_generated") == Some(&Value::Bool(true))
                    && object.get("qualified_name") == Some(&Value::Null)
                    && object.contains_key("name")
                    && object.contains_key("span")
                {
                    if let Some(uid) = object.get("unique_id").and_then(Value::as_u64) {
                        let next = bindings.len();
                        let ordinal = *bindings.entry(uid).or_insert(next);
                        object.insert(
                            "unique_id".into(),
                            json!(format!("synthetic_binding:{ordinal}")),
                        );
                    }
                }
                // serde_json::Value uses sorted object keys; arrays retain source order.
                for child in object.values_mut() {
                    normalize(child, bindings);
                }
            }
            Value::Array(items) => {
                for child in items {
                    normalize(child, bindings);
                }
            }
            _ => {}
        }
    }
    normalize(&mut program, &mut std::collections::HashMap::new());
    hash(&serde_json::to_vec(&program).expect("JSON value serializes"))
}

fn record(
    options: &Options,
    iteration: usize,
    chunk: usize,
    case: &str,
    source: &str,
    before: usize,
    after: usize,
    typed: Option<&scar::typed::TypedProgram>,
    error: Option<&scar::error::TypeError>,
    timings: Timings,
    outer: f64,
) -> Result<Value, String> {
    let signature = typed
        .map(|typed| serde_json::to_value(typed).map(|value| semantic_fingerprint(value)))
        .transpose()
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "event": "sample", "mode": match options.mode { Mode::Warm => "warm", Mode::Incremental => "incremental" },
        "api": options.api.name(), "iteration": iteration, "size": options.size,
        "chunk": chunk, "case": case, "status": if error.is_some() { "expected_failure" } else { "success" },
        "input_bytes": source.len(), "source_sha256": hash(source.as_bytes()),
        "retained_user_definitions_before": before, "retained_user_definitions_after": after,
        "typed_top_level_nodes": typed.map(|p| p.nodes.len()), "semantic_signature": signature,
        "resolved_clone_in_scar": false, "resolved_clone_performed": false,
        "typed_metadata_in_scar": true, "session_restore_in_scar": false,
        "timings_us": { "parse": timings.parse, "resolve": timings.resolve, "scar": timings.scar,
            "restore": timings.restore, "checkpoint": timings.checkpoint, "rollback": timings.rollback,
            "resolver_restore": timings.resolver_restore, "resolver_checkpoint": timings.resolver_checkpoint,
            "resolver_rollback": timings.resolver_rollback, "resolved_clone": 0.0, "frontend_total": outer },
        "diagnostic": error.map(|e| json!({"reason": format!("{:?}", e.reason()),
            "message": e.message, "span_start": e.span.start, "span_end": e.span.end, "hint": e.hint}))
    }))
}
#[cfg(test)]
fn samples(options: &Options) -> Result<Vec<Value>, String> {
    samples_with_setup(options).map(|(_, rows)| rows)
}
fn samples_with_setup(options: &Options) -> Result<(Value, Vec<Value>), String> {
    if options.iterations == 0 || options.size == 0 {
        return Err("iterations and size must be positive".into());
    }
    if options.mode == Mode::Incremental
        && (options.api != Api::Transactional || options.input.is_some())
    {
        return Err("incremental uses transactional API and generated chunks".into());
    }
    let setup_start = Instant::now();
    let prepared = Prepared::new()?;
    let setup_elapsed = micros(setup_start);
    let setup = json!({
        "event": "setup", "mode": match options.mode { Mode::Warm => "warm", Mode::Incremental => "incremental" },
        "api": options.api.name(), "size": options.size, "iterations": options.iterations,
        "setup_elapsed_us": setup_elapsed,
        "stdlib_cache": xldr::stdlib_semantic_snapshot_cache_state(xldr::StdlibVariant::Default).map(|s| s.as_str()),
        "timing_unit": "microseconds", "stdlib_snapshot_prepared_once": true,
        "stdlib_preparation_in_samples": false, "resolved_input_cloned": false,
        "typed_metadata_in_scar": true,
        "semantic_signature_normalization": "alpha-renames compiler-generated unqualified ResolvedId bindings; preserves names, types, dispatch, literals, spans, function indices, tags, type variables, AST and array order",
        "retained_definition_unit": "accepted top-level user declarations including trait and impl declarations; excludes generated specializations and stdlib",
        "frontend_total_includes": ["parse", "resolve", "scar", "restore", "checkpoint", "rollback", "resolver_restore", "resolver_checkpoint", "resolver_rollback"],
        "frontend_total_excludes": ["stdlib preparation", "input generation and file I/O", "JSON and signature generation", "VM and Forge execution", "typed results, sessions, and unused success checkpoints dropped after each sample"],
        "scar_includes": ["API-owned transactional state clone when selected", "TypedProgram metadata generation"],
        "incremental_api": "transactional", "parser_context": "script (compiler-session workload; not REPL grammar)"
    });
    let mut rows = Vec::new();
    for iteration in 0..options.iterations {
        if options.mode == Mode::Warm {
            let generated;
            let source = if let Some(source) = &options.input {
                source
            } else {
                generated = generated_warm(options.size);
                &generated
            };
            let outer = Instant::now();
            let mut timings = Timings::default();
            let start = Instant::now();
            let ast = xldr::parse_file_source(
                source,
                spire::ParserContext::script(0),
                "session-profile.srt",
            )
            .map_err(|e| format!("parse: {e:?}"))?;
            timings.parse = micros(start);
            let declarations = declaration_count(&ast);
            let start = Instant::now();
            let mut program = sigil::resolve_staged_program_from_state(
                &prepared.snapshot.module_stages,
                ast,
                prepared.snapshot.declaration_index(),
                Some("ProfileHarness".into()),
                prepared.snapshot.default_stage_count,
                prepared.snapshot.resolve_state(),
            )
            .map_err(|e| format!("resolve: {e}"))?;
            // The restored Scar checkpoint owns prefix process dependencies.
            let mut boot = prepared.prefix_metadata.boot_plan.clone();
            boot.entries.extend(program.boot_plan.entries);
            boot.singletons.extend(program.boot_plan.singletons);
            boot.supervisors.extend(program.boot_plan.supervisors);
            program.boot_plan = boot;
            timings.resolve = micros(start);
            let start = Instant::now();
            let mut scar = prepared.snapshot.compile_prefix().restored_scar_session();
            timings.restore = micros(start);
            let start = Instant::now();
            let typed = match options.api {
                Api::Transactional => {
                    scar.typecheck_staged_program_with_context(program, TypecheckContext::default())
                }
                Api::InPlace => scar.typecheck_staged_program_in_place_with_context(
                    program,
                    TypecheckContext::default(),
                ),
            }
            .map_err(|e| format!("typecheck: {e}"))?;
            timings.scar = micros(start);
            let total = micros(outer);
            rows.push(record(
                options,
                iteration,
                0,
                "independent_suffix",
                source,
                0,
                declarations,
                Some(&typed),
                None,
                timings,
                total,
            )?);
        } else {
            let start = Instant::now();
            let mut resolver = prepared.resolver()?;
            let resolver_restore = micros(start);
            let start = Instant::now();
            let mut scar = prepared.snapshot.compile_prefix().restored_scar_session();
            let restore = micros(start);
            let mut retained = 0;
            for chunk in 0..options.size + 3 {
                let (case, source) = if chunk < options.size {
                    ("growing", generated_chunk(chunk))
                } else if chunk == options.size {
                    ("trait_impl", TRAIT_CHUNK.into())
                } else if chunk == options.size + 1 {
                    ("rejection", REJECTED_CHUNK.into())
                } else {
                    ("recovery", recovery_chunk(options.size))
                };
                let outer = Instant::now();
                let mut timings = Timings::default();
                if chunk == 0 {
                    timings.restore = restore;
                    timings.resolver_restore = resolver_restore;
                }
                let start = Instant::now();
                let scar_checkpoint = scar.checkpoint();
                timings.checkpoint = micros(start);
                let start = Instant::now();
                let resolver_checkpoint = resolver.checkpoint();
                timings.resolver_checkpoint = micros(start);
                let start = Instant::now();
                let ast = xldr::parse_file_source(
                    &source,
                    spire::ParserContext::script(0),
                    "session-profile.srt",
                )
                .map_err(|e| format!("parse: {e:?}"))?;
                timings.parse = micros(start);
                let added = declaration_count(&ast);
                let start = Instant::now();
                let resolved = resolver.resolve(ast).map_err(|e| format!("resolve: {e}"))?;
                let program = prepared.program(resolved);
                timings.resolve = micros(start);
                let start = Instant::now();
                let result = scar
                    .typecheck_staged_program_with_context(program, TypecheckContext::default());
                timings.scar = micros(start);
                let before = retained;
                let mut rolled_back = false;
                let (typed, error) = match result {
                    Ok(typed) if case != "rejection" => {
                        retained += added;
                        (Some(typed), None)
                    }
                    Ok(_) => return Err("expected late type rejection succeeded".into()),
                    Err(error) if case == "rejection" => {
                        let expected_start = REJECTED_CHUNK
                            .find("\"invalid\"")
                            .expect("fixed rejection literal");
                        if error.reason()
                            != Some(diagnostics::TypeDiagnosticReason::ReturnTypeMismatch)
                            || error.span.start != expected_start
                            || error.span.end != expected_start + "\"invalid\"".len()
                        {
                            return Err(format!("unexpected rejection diagnostic: {error}"));
                        }
                        // Resolver already committed names; both checkpoints must be restored.
                        if resolver.lookup_uid("profile_discard").is_none() {
                            return Err("rejection did not commit resolver binding".into());
                        }
                        let start = Instant::now();
                        scar.rollback(scar_checkpoint);
                        timings.rollback = micros(start);
                        let start = Instant::now();
                        resolver.rollback(resolver_checkpoint);
                        timings.resolver_rollback = micros(start);
                        rolled_back = resolver.lookup_uid("profile_discard").is_none()
                            && resolver.lookup_uid("profile_invalid").is_none();
                        if !rolled_back {
                            return Err("resolver rollback retained rejected names".into());
                        }
                        (None, Some(error))
                    }
                    Err(error) => return Err(format!("unexpected {case} rejection: {error}")),
                };
                let total = micros(outer)
                    + if chunk == 0 {
                        restore + resolver_restore
                    } else {
                        0.0
                    };
                let mut row = record(
                    options,
                    iteration,
                    chunk,
                    case,
                    &source,
                    before,
                    retained,
                    typed.as_ref(),
                    error.as_ref(),
                    timings,
                    total,
                )?;
                row["resolver_rollback_verified"] = json!(rolled_back);
                row["reused_failed_binding"] = json!(case == "recovery");
                rows.push(row);
            }
        }
    }
    // Fail the manual run if an independent repetition changes its semantic result.
    for row in &rows {
        if row["iteration"] != 0 {
            let baseline = rows
                .iter()
                .find(|candidate| candidate["iteration"] == 0 && candidate["chunk"] == row["chunk"])
                .ok_or("missing first-iteration sample")?;
            if row["semantic_signature"] != baseline["semantic_signature"]
                || row["diagnostic"] != baseline["diagnostic"]
            {
                return Err(format!(
                    "repetition changed semantic result at chunk {}",
                    row["chunk"]
                ));
            }
        }
    }
    Ok((setup, rows))
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options {
        mode: Mode::Warm,
        api: Api::Transactional,
        iterations: 10,
        size: 16,
        input: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        if flag == "--help" {
            return Err("usage: scar-session-profile --mode warm|incremental --api transactional|in_place --iterations N --size N [--input PATH]".into());
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--mode" => {
                options.mode = match value.as_str() {
                    "warm" => Mode::Warm,
                    "incremental" => Mode::Incremental,
                    _ => return Err("mode must be warm or incremental".into()),
                }
            }
            "--api" => {
                options.api = match value.as_str() {
                    "transactional" => Api::Transactional,
                    "in_place" => Api::InPlace,
                    _ => return Err("api must be transactional or in_place".into()),
                }
            }
            "--iterations" => {
                options.iterations = value.parse().map_err(|_| "invalid iterations")?
            }
            "--size" => options.size = value.parse().map_err(|_| "invalid size")?,
            "--input" => {
                options.input =
                    Some(std::fs::read_to_string(&value).map_err(|e| format!("{value}: {e}"))?)
            }
            _ => return Err(format!("unknown argument: {flag}")),
        }
    }
    Ok(options)
}
fn main() {
    let outcome = parse_options().and_then(|options| samples_with_setup(&options));
    match outcome {
        Ok((setup, rows)) => {
            println!("{setup}");
            for row in rows {
                println!("{row}");
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(mode: Mode, api: Api) -> Options {
        Options {
            mode,
            api,
            iterations: 2,
            size: 3,
            input: None,
        }
    }

    #[test]
    fn warm_replays_identical_suffix_from_independent_checkpoints() {
        for api in [Api::Transactional, Api::InPlace] {
            let rows = samples(&options(Mode::Warm, api)).unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0]["semantic_signature"], rows[1]["semantic_signature"]);
            assert_eq!(rows[0]["source_sha256"], rows[1]["source_sha256"]);
            for row in rows {
                assert_eq!(row["status"], "success");
                assert_eq!(row["retained_user_definitions_before"], 0);
                assert_eq!(row["retained_user_definitions_after"], 3);
                assert_eq!(row["resolved_clone_in_scar"], false);
                assert_eq!(row["typed_metadata_in_scar"], true);
            }
        }
    }

    #[test]
    fn warm_pattern_fixture_preserves_semantics_across_repetitions() {
        for api in [Api::Transactional, Api::InPlace] {
            let mut options = options(Mode::Warm, api);
            options.iterations = 4;
            options.input = Some(
                include_str!("../../../tests/profile/scar_compile/features/patterns.srt").into(),
            );
            let rows = samples(&options).unwrap();
            assert_eq!(rows.len(), 4);
            for row in &rows[1..] {
                assert_eq!(row["semantic_signature"], rows[0]["semantic_signature"]);
            }
        }
    }

    #[test]
    fn semantic_fingerprint_preserves_binding_wiring_and_semantic_fields() {
        fn id(uid: u64, generated: bool) -> Value {
            json!({"name": "__local", "qualified_name": null, "unique_id": uid,
                "compiler_generated": generated, "symbol_info": null, "span": {"start": 1, "end": 2}})
        }
        fn program(first: u64, second: u64) -> Value {
            json!({"binders": [id(first, true), id(second, true)],
                "references": [id(first, true), id(second, true)],
                "literal": {"Int": 42}, "ty": "Int",
                "dispatch": {"UserFunction": {"name": "Canonical::method", "fun_idx": 7}},
                "authored": id(10, false)})
        }
        let baseline = program(3_000_000_000, 3_000_000_001);
        let expected = semantic_fingerprint(baseline.clone());
        assert_eq!(
            expected,
            semantic_fingerprint(program(3_000_000_100, 3_000_000_101))
        );
        let mut rewired = baseline.clone();
        rewired["references"][1] = id(3_000_000_000, true);
        assert_ne!(expected, semantic_fingerprint(rewired));
        for (pointer, replacement) in [
            ("/literal/Int", json!(43)),
            ("/ty", json!("Str")),
            ("/dispatch/UserFunction/name", json!("Other::method")),
            ("/dispatch/UserFunction/fun_idx", json!(8)),
            ("/authored/unique_id", json!(11)),
            ("/references/0/span/end", json!(3)),
        ] {
            let mut changed = baseline.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert_ne!(expected, semantic_fingerprint(changed), "{pointer}");
        }
    }

    #[test]
    fn incremental_retains_bindings_and_restores_both_phases_after_late_failure() {
        let rows = samples(&options(Mode::Incremental, Api::Transactional)).unwrap();
        for iteration in 0..2 {
            let run = rows
                .iter()
                .filter(|row| row["iteration"] == iteration)
                .collect::<Vec<_>>();
            assert_eq!(run.len(), 6); // three growing chunks, trait/impl, rejection, recovery.
            for (chunk, row) in run.iter().take(3).enumerate() {
                assert_eq!(row["retained_user_definitions_before"], chunk);
                assert_eq!(row["retained_user_definitions_after"], chunk + 1);
                assert_eq!(row["status"], "success");
            }
            assert_eq!(run[3]["case"], "trait_impl");
            let failed = run[4];
            assert_eq!(failed["status"], "expected_failure");
            assert_eq!(failed["resolver_rollback_verified"], true);
            assert_eq!(failed["diagnostic"]["reason"], "Some(ReturnTypeMismatch)");
            let expected_start = REJECTED_CHUNK.find("\"invalid\"").unwrap();
            assert_eq!(failed["diagnostic"]["span_start"], expected_start);
            assert_eq!(
                failed["diagnostic"]["span_end"],
                expected_start + "\"invalid\"".len()
            );
            assert_eq!(run[5]["status"], "success");
            assert_eq!(run[5]["case"], "recovery");
            assert_eq!(run[5]["reused_failed_binding"], true);
        }
    }

    #[test]
    fn sibling_sessions_do_not_share_new_names_or_nominal_metadata() {
        let prepared = Prepared::new().unwrap();
        let mut original = prepared.resolver().unwrap();
        let mut sibling = original.clone();
        let mut original_scar = prepared.snapshot.compile_prefix().restored_scar_session();
        let mut sibling_scar = original_scar.clone();
        let ast = spire::parse("defrecord ProfileSibling(value: Int)").unwrap();
        let resolved = sibling.resolve(ast).unwrap();
        sibling_scar
            .typecheck_staged_program(prepared.program(resolved))
            .unwrap();
        assert!(sibling.lookup_uid("ProfileSibling").is_some());
        assert!(original.lookup_uid("ProfileSibling").is_none());
        assert!(sibling_scar.lookup_type_def("ProfileSibling").is_some());
        assert!(original_scar.lookup_type_def("ProfileSibling").is_none());
        let ast = spire::parse("defrecord ProfileSibling(value: String)").unwrap();
        let resolved = original.resolve(ast).unwrap();
        original_scar
            .typecheck_staged_program(prepared.program(resolved))
            .unwrap();
    }
}
