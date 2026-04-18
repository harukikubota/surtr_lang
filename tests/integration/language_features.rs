#[path = "language_features/core_language.rs"]
mod core_language;
#[path = "language_features/harness.rs"]
mod harness;
#[path = "language_features/pipelines_and_usecases.rs"]
mod pipelines_and_usecases;
#[path = "language_features/runtime_observation.rs"]
mod runtime_observation;
#[path = "language_features/safebind_and_errors.rs"]
mod safebind_and_errors;

fn run_bucket_cases(module: &str, cases: &[(&str, fn())], bucket: usize, bucket_count: usize) {
    assert!(bucket_count > 0, "bucket_count must be positive");
    assert!(
        bucket < bucket_count,
        "bucket {} out of range {}",
        bucket,
        bucket_count
    );

    let mut ran = 0usize;
    for (index, (name, case)) in cases.iter().enumerate() {
        if index % bucket_count != bucket {
            continue;
        }
        ran += 1;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (case)();
        }));
        if let Err(payload) = result {
            if let Some(msg) = payload.downcast_ref::<&str>() {
                panic!(
                    "language_features bucket {}/{} failed at {}::{}: {}",
                    bucket, bucket_count, module, name, msg
                );
            }
            if let Some(msg) = payload.downcast_ref::<String>() {
                panic!(
                    "language_features bucket {}/{} failed at {}::{}: {}",
                    bucket, bucket_count, module, name, msg
                );
            }
            panic!(
                "language_features bucket {}/{} failed at {}::{} with non-string panic payload",
                bucket, bucket_count, module, name
            );
        }
    }

    assert!(
        ran > 0,
        "no cases assigned to language_features bucket {} of {} for module {}",
        bucket,
        bucket_count,
        module
    );
}

fn run_language_features_bucket(bucket: usize, bucket_count: usize) {
    core_language::run_bucket(bucket, bucket_count);
    pipelines_and_usecases::run_bucket(bucket, bucket_count);
    runtime_observation::run_bucket(bucket, bucket_count);
    safebind_and_errors::run_bucket(bucket, bucket_count);
}

#[test]
fn language_features_bucket_0() {
    run_language_features_bucket(0, 4);
}

#[test]
fn language_features_bucket_1() {
    run_language_features_bucket(1, 4);
}

#[test]
fn language_features_bucket_2() {
    run_language_features_bucket(2, 4);
}

#[test]
fn language_features_bucket_3() {
    run_language_features_bucket(3, 4);
}
