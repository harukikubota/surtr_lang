//! Stable buckets for test targets that share a process-local standard prelude.

macro_rules! register_case_suite {
    ($source:literal, [$($case:ident),+ $(,)?]) => {
        crate::case_registry::register_case_suite!(@suite $source, [$($case),+], []);
    };
    ($source:literal, [$($case:ident),+ $(,)?], standalone = [$($standalone:ident),+ $(,)?]) => {
        crate::case_registry::register_case_suite!(@suite $source, [$($case),+], [$($standalone),+]);
    };
    (@suite $source:literal, [$($case:ident),+], [$($standalone:ident),*]) => {
        const CASES: &[(&str, fn())] = &[$((stringify!($case), $case as fn())),+];
        const STANDALONE_CASES: &[(&str, fn())] = &[$((stringify!($standalone), $standalone as fn())),*];
        const CASE_BUCKET_COUNT: usize = 8;

        #[test]
        fn case_inventory_has_unique_names_and_functions() {
            let source_without_whitespace = include_str!($source)
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>();
            assert_eq!(
                source_without_whitespace.matches("#[test]").count(),
                STANDALONE_CASES.len(),
                "only declared standalone cases may have test attributes in {}",
                $source,
            );
            for &(name, _) in STANDALONE_CASES {
                assert!(
                    source_without_whitespace.contains(&format!("#[test]fn{name}()")),
                    "declared standalone case must retain its test attribute: {name}",
                );
            }
            assert!(CASES.len() >= CASE_BUCKET_COUNT, "every bucket must contain a case");
            let mut names = std::collections::HashSet::new();
            let mut functions = std::collections::HashSet::new();
            for &(name, case) in CASES.iter().chain(STANDALONE_CASES) {
                assert!(names.insert(name), "duplicate case name: {name}");
                assert!(functions.insert(case as usize), "duplicate case function: {name}");
            }
        }

        fn run_case_bucket(bucket: usize) {
            assert!(bucket < CASE_BUCKET_COUNT, "invalid case bucket: {bucket}");
            for (index, &(name, case)) in CASES.iter().enumerate() {
                if index % CASE_BUCKET_COUNT == bucket {
                    eprintln!("Scar case {}::{name}", $source);
                    case();
                }
            }
        }

        crate::case_registry::register_case_suite!(@bucket case_bucket_0, 0);
        crate::case_registry::register_case_suite!(@bucket case_bucket_1, 1);
        crate::case_registry::register_case_suite!(@bucket case_bucket_2, 2);
        crate::case_registry::register_case_suite!(@bucket case_bucket_3, 3);
        crate::case_registry::register_case_suite!(@bucket case_bucket_4, 4);
        crate::case_registry::register_case_suite!(@bucket case_bucket_5, 5);
        crate::case_registry::register_case_suite!(@bucket case_bucket_6, 6);
        crate::case_registry::register_case_suite!(@bucket case_bucket_7, 7);
    };
    (@bucket $name:ident, $bucket:literal) => {
        #[test]
        fn $name() {
            run_case_bucket($bucket);
        }
    };
}

pub(crate) use register_case_suite;
