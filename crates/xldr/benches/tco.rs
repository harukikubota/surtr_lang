use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use eldr::vm::{VmObservation, VmObservationOptions};
use eldr::VM;
use forge::bytecode::{populate_error_template_lines, Bytecode};
use sindr::policy::CompileUnitKind;

const FIB_TAIL_50: &str = r#"
def fib_tail(n: Int, a: Int, b: Int) -> Int {
  if(n == 0, a, fib_tail(n - 1, b, a + b))
}

fib_tail(50, 0, 1)
"#;

const REDUCE_WITH_FIB: &str = r#"
def fib_tail(n: Int, a: Int, b: Int) -> Int {
  if(n == 0, a, fib_tail(n - 1, b, a + b))
}

def fib(n: Int) -> Int {
  fib_tail(n, 0, 1)
}

values = [44, 45, 46, 47, 48, 49, 50]
List::reduce(values, 0, {|acc, n| acc + fib(n) })
"#;

const SUM_NON_TAIL_10000: &str = r#"
def sum_non_tail(n: Int) -> Int {
  if(n == 0, 0, 1 + sum_non_tail(n - 1))
}

sum_non_tail(10000)
"#;

#[derive(Clone, Copy)]
struct BenchCase {
    id: &'static str,
    source_name: &'static str,
    source: &'static str,
}

const CASES: &[BenchCase] = &[
    BenchCase {
        id: "fib_tail_50",
        source_name: "fib_tail_50.srt",
        source: FIB_TAIL_50,
    },
    BenchCase {
        id: "reduce_with_fib_tail_inputs",
        source_name: "reduce_with_fib_tail_inputs.srt",
        source: REDUCE_WITH_FIB,
    },
    BenchCase {
        id: "sum_non_tail_10000",
        source_name: "sum_non_tail_10000.srt",
        source: SUM_NON_TAIL_10000,
    },
];

fn collect_script_compile_sources(
    file_name: &str,
    source: &str,
) -> Result<xldr::CompileSources, String> {
    let module_inputs = xldr::collect_additional_default_std_module_inputs()
        .map_err(|e| format!("phase=load; message={}", e))?;
    let module_sources = xldr::collect_module_sources_with_module_stages(&[module_inputs])
        .map_err(|e| format!("phase=load; message={}", e))?;
    Ok(xldr::compose_script_compile_sources(
        file_name,
        source,
        module_sources,
    ))
}

fn compile_script(source_name: &str, source: &str) -> Result<Bytecode, String> {
    let compile_sources = collect_script_compile_sources(source_name, source)?;
    let sources = &compile_sources.sources;
    let user_source_id = compile_sources.user_source_id;
    let module_stages =
        xldr::parse_module_stages_from_compile_sources(&compile_sources, CompileUnitKind::Script)
            .map_err(|e| {
            let file_name = sources.file_name(e.source_id).unwrap_or("<unknown>");
            format!("phase=parse; file={}; message={}", file_name, e.message())
        })?;

    let user_source = sources.source(user_source_id).unwrap_or("");
    let user_ast = spire::parse_with_context(
        user_source,
        spire::ParserContext::script(user_source_id.0)
            .with_rules(xldr::derive_parse_rules(xldr::SourceKind::Script)),
    )
    .map_err(|e| {
        let file_name = sources.file_name(user_source_id).unwrap_or("<unknown>");
        format!("phase=parse; file={}; message={}", file_name, e.message())
    })?;

    let docs = xldr::collect_doc_entries(
        &module_stages,
        &user_ast,
        Some(compile_sources.user_module_path.as_str()),
    );
    let declaration_index = sigil::precollect_declaration_index(&module_stages)
        .map_err(|e| format!("phase=resolve; message={}", e))?;
    let resolved = sigil::resolve_staged_program(
        &module_stages,
        user_ast,
        &declaration_index,
        Some(compile_sources.user_module_path.clone()),
    )
    .map_err(|e| format!("phase=resolve; message={}", e))?;
    let typed = scar::typecheck_with_context(
        resolved,
        scar::TypecheckContext {
            runtime_policy: xldr::derive_runtime_policy(
                CompileUnitKind::Script,
                xldr::SourceKind::Script,
                None,
            ),
            enforce_builtin_type_contracts: true,
            allow_error_function_params: false,
            allow_private_facet_inspection: false,
        },
    )
    .map_err(|e| format!("phase=typecheck; message={}", e))?;
    let mut bytecode =
        forge::codegen(typed).map_err(|e| format!("phase=codegen; message={}", e))?;
    populate_error_template_lines(&mut bytecode.error_templates, user_source);
    bytecode.docs = docs;
    Ok(bytecode)
}

fn observe_case(case: BenchCase) -> Result<VmObservation, String> {
    let bytecode = compile_script(case.source_name, case.source)?;
    let mut vm = VM::new(bytecode);
    vm.enable_observation(VmObservationOptions::default());
    vm.run()
        .map_err(|e| format!("phase=runtime; message={}", e))?;
    vm.observation()
        .ok_or_else(|| "observation was not captured".to_string())
}

fn bench_case(case: BenchCase) {
    let bytecode = compile_script(case.source_name, case.source).expect("compile should succeed");
    let mut vm = VM::new(bytecode);
    vm.run().expect("run should succeed");
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("tco");
    group.sample_size(10);

    for case in CASES {
        let observation = observe_case(*case).expect("observation should succeed");
        eprintln!(
            "observation {}: max_frame_depth={} function_calls={} return_count={} tail_calls_optimized={}",
            case.id,
            observation.stats.max_frame_depth,
            observation.stats.function_calls,
            observation.stats.return_count,
            observation.stats.tail_calls_optimized
        );

        group.bench_with_input(BenchmarkId::from_parameter(case.id), case, |b, case| {
            b.iter(|| bench_case(*case));
        });
    }

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
