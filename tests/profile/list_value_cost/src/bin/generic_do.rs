//! Manual profile of real standard Monad::bind and generic do compilation.
//! Run one workload per process; PROBE_VERIFY is a separate correctness run.
use list_value_cost_probe::metrics::{emit, measured};
use serde_json::json;
use sindr::policy::{CompileUnitKind, SourceKind};
use sindr::primitives::int;
use sindr::runtime::Value;

const FILE_NAME: &str = "list_value_cost_probe.srt";
const STRING_PADDING: usize = 256;

fn source(case: &str, n: usize, payload: &str, depth: usize) -> String {
    let (ty, item, seed) = match payload {
        "int" => ("Int", "remaining".to_owned(), "0".to_owned()),
        "string" => (
            "String",
            format!("\"#{{remaining}}:{}\"", "x".repeat(STRING_PADDING)),
            "\"seed\"".to_owned(),
        ),
        _ => panic!("payload must be int or string"),
    };
    let (output_ty, body) = match case {
        "singleton" => (
            "List<$A>",
            "do::<List> { value <- input\n Monad::return(value) }".to_owned(),
        ),
        "empty" => ("List<$A>", "Monad::bind(input, {|_value| []})".to_owned()),
        "huge" => (
            "List<$A>",
            "Monad::bind([seed], {|_value| input})".to_owned(),
        ),
        "chain" => {
            let mut body = "step0: List<$A> = input\n".to_owned();
            for i in 1..=depth {
                body.push_str(&format!(
                    "step{i}: List<$A> = Monad::bind(step{}, {{|value| Monad::return(value)}})\n",
                    i - 1
                ));
            }
            body.push_str(&format!("step{depth}"));
            ("List<$A>", body)
        }
        "nested" => {
            let mut body = "input".to_owned();
            for _ in 0..depth {
                body = format!("do::<List> {{ _value <- [seed]\n {body} }}");
            }
            ("List<$A>", body)
        }
        "product" => (
            "List<($A, $A, $A)>",
            "do::<List> { x <- input\n y <- input\n z <- input\n Monad::return((x, y, z)) }"
                .to_owned(),
        ),
        _ => panic!("unknown case: {case}"),
    };
    format!(
        "def build_input(remaining: Int, tail: List<{ty}>) -> List<{ty}> {{\n\
         if(remaining == 0, tail, build_input(remaining - 1, List::cons({item}, tail)))\n\
         }}\n\
         def run_probe(input: List<$A>, seed: $A) -> {output_ty} {{\n{body}\n}}\n\
         run_probe(build_input({n}, []), {seed})\n"
    )
}

fn compile(
    sources: &xldr::CompileSources,
    snapshot: &xldr::DefaultStdlibSnapshot,
) -> forge::bytecode::Bytecode {
    // Same staged-prefix public API as Rune; no hand-written List lowering.
    let context = xldr::derive_parser_context(
        sources.user_source_id.0,
        SourceKind::Script,
        CompileUnitKind::Script,
        Some(sources.user_module_path.clone()),
    );
    let ast = xldr::parse_file_source(
        sources.sources.source(sources.user_source_id).unwrap(),
        context,
        FILE_NAME,
    )
    .unwrap_or_else(|e| panic!("parse: {e:?}"));
    let resolved = sigil::resolve_staged_program_from_state(
        &snapshot.module_stages,
        ast,
        snapshot.declaration_index(),
        Some(sources.user_module_path.clone()),
        snapshot.default_stage_count,
        snapshot.resolve_state(),
    )
    .unwrap_or_else(|e| panic!("resolve: {e:?}"));
    let mut checker = snapshot.compile_prefix().restored_scar_session();
    let typed = checker
        .typecheck_staged_program_in_place_with_context(
            resolved,
            scar::TypecheckContext::from_source_policy(
                SourceKind::Script.policy(CompileUnitKind::Script, None),
            ),
        )
        .unwrap_or_else(|e| panic!("typecheck: {e:?}"));
    let mut codegen = snapshot.compile_prefix().forge_session();
    let (chunk, _) = codegen
        .codegen_chunk_typed_program(typed)
        .unwrap_or_else(|e| panic!("codegen: {e:?}"));
    forge::compose_bytecode_with_chunk(snapshot.bytecode().clone(), chunk)
        .unwrap_or_else(|e| panic!("compose: {e:?}"))
}

fn cardinalities(case: &str, n: usize, depth: usize) -> (usize, usize, usize) {
    match case {
        "singleton" => (n, n, n),
        "empty" => (n, 0, 0),
        "huge" => (1, n, n),
        "chain" => (
            n.checked_mul(depth).unwrap(),
            n.checked_mul(depth).unwrap(),
            n,
        ),
        "nested" => (depth, n.checked_mul(depth).unwrap(), n),
        "product" => {
            let n2 = n.checked_mul(n).unwrap();
            let n3 = n2.checked_mul(n).unwrap();
            (
                n.checked_add(n2).unwrap().checked_add(n3).unwrap(),
                n3.checked_mul(3).unwrap(),
                n3,
            )
        }
        _ => panic!("unknown case: {case}"),
    }
}

fn check_item(value: &Value, expected: usize, payload: &str) {
    match (payload, value) {
        ("int", Value::Int(actual)) => assert_eq!(actual, &int(expected as i64)),
        ("string", Value::Str(actual)) => {
            assert_eq!(
                actual,
                &format!("{expected}:{}", "x".repeat(STRING_PADDING))
            );
        }
        _ => panic!("unexpected payload value: {value:?}"),
    }
}

fn verify(value: &Value, case: &str, n: usize, payload: &str, m: usize) {
    let Value::List(list) = value else {
        panic!("expected List result")
    };
    assert_eq!(list.len(), m);
    for (i, value) in list.iter().enumerate() {
        if case == "product" {
            let Value::Tuple(fields) = value else {
                panic!("expected product Tuple")
            };
            assert_eq!(fields.len(), 3);
            check_item(&fields[0], i / (n * n) + 1, payload);
            check_item(&fields[1], i / n % n + 1, payload);
            check_item(&fields[2], i % n + 1, payload);
        } else {
            check_item(&value, i + 1, payload);
        }
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        (4..=5).contains(&args.len()),
        "usage: generic_do case n payload [depth]"
    );
    let case = &args[1];
    let n: usize = args[2].parse().unwrap();
    let payload = &args[3];
    let depth: usize = args.get(4).map(|s| s.parse().unwrap()).unwrap_or(8);
    assert!(n > 0 && n <= i64::MAX as usize && depth > 0);
    let (b, e, m) = cardinalities(case, n, depth);
    let input = source(case, n, payload, depth);
    let source_hash = sindr::ir::stable_hash_hex(&input);
    let ((snapshot, sources), setup) = measured(|| {
        let modules = xldr::cached_default_script_module_sources(xldr::StdlibVariant::Default)
            .expect("standard module sources");
        let sources =
            xldr::compose_script_compile_sources(FILE_NAME, &input, modules.as_ref().clone());
        let snapshot = xldr::default_stdlib_semantic_snapshot().expect("standard semantic prefix");
        (snapshot, sources)
    });
    let cache_state = format!(
        "{:?}",
        xldr::stdlib_semantic_snapshot_cache_state(xldr::StdlibVariant::Default)
    );
    let (bytecode, compiled) = measured(|| compile(&sources, &snapshot));
    let opcodes = bytecode.opcodes.len();
    let functions = bytecode.functions.len();
    // Drop local compile inputs/handles outside all measured VM intervals.
    // xldr's global cache retains the shared standard-library snapshot;
    // its allocation remains part of the process RSS baseline.
    drop(sources);
    drop(snapshot);
    let (mut vm, prepared) = measured(|| eldr::VM::new(bytecode));
    let (result, ran) = measured(|| vm.run());
    result.unwrap_or_else(|e| panic!("VM run: {e:?}"));
    let Value::List(list) = vm.last_value().expect("result value") else {
        panic!("expected List result")
    };
    assert_eq!(list.len(), m); // O(1), does not clone or traverse payload.
    if std::env::var_os("PROBE_VERIFY").is_some() {
        verify(vm.last_value().unwrap(), case, n, payload, m);
    }
    let (_, dropped) = measured(|| drop(vm));
    // Delay JSON output until all intervals finish to avoid retaining its
    // formatting allocations across VM measurement boundaries.
    for (phase, metrics) in [
        ("setup_stdlib", setup),
        ("compile", compiled),
        ("prepare_vm", prepared),
        ("run_vm", ran),
        ("drop_vm", dropped),
    ] {
        emit(
            case,
            phase,
            n,
            payload,
            metrics,
            json!({
                "entry": "compiler_generic_do", "depth": depth,
                "expected_B": b, "expected_E": e, "expected_M": m,
                "callback_observation": "unobserved", "observer_enabled": false,
                "input_builder": "recursive_cons", "string_padding_bytes": STRING_PADDING,
                "source_hash": source_hash, "stdlib_cache_state": cache_state,
                "bytecode_opcodes": opcodes, "bytecode_functions": functions,
            }),
        );
    }
}
