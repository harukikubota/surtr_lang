use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use xldr::ReplEngine;

#[derive(Clone, Copy)]
struct CompletionCase {
    id: &'static str,
    input: &'static str,
}

fn bench_case(case: CompletionCase) {
    let engine = ReplEngine::new().expect("REPL engine should bootstrap");
    let completion = engine.completions(case.input, case.input.len());
    assert!(
        !completion.candidates.is_empty(),
        "completion bench case should produce candidates"
    );
}

fn criterion_benchmark(c: &mut Criterion) {
    let cases = [
        CompletionCase {
            id: "qualified_type_path",
            input: "String::re",
        },
        CompletionCase {
            id: "global_binding_prefix",
            input: "pri",
        },
    ];

    let mut group = c.benchmark_group("repl_completion");
    for case in cases {
        group.bench_with_input(BenchmarkId::from_parameter(case.id), &case, |b, case| {
            b.iter(|| bench_case(*case));
        });
    }
    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
