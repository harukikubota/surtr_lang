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

const LANGUAGE_FEATURE_BUCKETS: usize = 8;

fn stable_bucket(key: &str, bucket_count: usize) -> usize {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    (hash as usize) % bucket_count
}

fn run_bucket_cases(
    module: &str,
    cases: &[(&str, fn())],
    bucket: usize,
    bucket_count: usize,
) -> usize {
    assert!(bucket_count > 0, "bucket_count must be positive");
    assert!(
        bucket < bucket_count,
        "bucket {} out of range {}",
        bucket,
        bucket_count
    );

    let mut ran = 0usize;
    for (name, case) in cases.iter() {
        if stable_bucket(&format!("{module}::{name}"), bucket_count) != bucket {
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

    ran
}

fn run_language_features_bucket(bucket: usize, bucket_count: usize) {
    let ran = core_language::run_bucket(bucket, bucket_count)
        + pipelines_and_usecases::run_bucket(bucket, bucket_count)
        + runtime_observation::run_bucket(bucket, bucket_count)
        + safebind_and_errors::run_bucket(bucket, bucket_count);
    assert!(
        ran > 0,
        "no cases assigned to language_features bucket {} of {}",
        bucket,
        bucket_count
    );
}

macro_rules! language_feature_bucket_test {
    ($name:ident, $bucket:expr) => {
        #[test]
        fn $name() {
            run_language_features_bucket($bucket, LANGUAGE_FEATURE_BUCKETS);
        }
    };
}

language_feature_bucket_test!(language_features_bucket_0, 0);
language_feature_bucket_test!(language_features_bucket_1, 1);
language_feature_bucket_test!(language_features_bucket_2, 2);
language_feature_bucket_test!(language_features_bucket_3, 3);
language_feature_bucket_test!(language_features_bucket_4, 4);
language_feature_bucket_test!(language_features_bucket_5, 5);
language_feature_bucket_test!(language_features_bucket_6, 6);
language_feature_bucket_test!(language_features_bucket_7, 7);
