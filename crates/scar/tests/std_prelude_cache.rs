#[path = "support/cache.rs"]
mod cache;

fn test_directory(name: &str) -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("scar-prefix-{name}-{}-{nonce}", std::process::id()))
}

#[test]
fn prefix_roundtrip_preserves_payload() {
    let payload = vec!["bootstrap".to_owned(), "types".to_owned()];
    let bytes = cache::encode("compiler-and-source", &payload);
    assert_eq!(
        cache::decode::<Vec<String>>("compiler-and-source", &bytes).unwrap(),
        payload
    );
}

#[test]
fn prefix_rejects_changed_compiler_or_source_key() {
    let bytes = cache::encode("before", &vec![1_u32]);
    assert!(cache::decode::<Vec<u32>>("after", &bytes)
        .unwrap_err()
        .contains("key mismatch"));
}

#[test]
fn prefix_rejects_corrupt_payload() {
    let mut bytes = cache::encode("same", &vec![1_u32]);
    *bytes.last_mut().unwrap() ^= 1;
    assert!(cache::decode::<Vec<u32>>("same", &bytes)
        .unwrap_err()
        .contains("checksum mismatch"));
}

#[test]
fn prefix_rejects_truncated_envelope() {
    assert!(cache::decode::<Vec<u32>>("same", b"invalid").is_err());
}

#[test]
fn prefix_reuses_published_payload_without_building() {
    let directory = test_directory("contract");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("prefix");
    let first = cache::load_or_build(&path, "same", || vec![42_u32]);
    let second: Vec<u32> = cache::load_or_build(&path, "same", || panic!("hit must not build"));
    assert_eq!(first, second);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn concurrent_prefix_miss_builds_once() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };
    let directory = test_directory("concurrent");
    std::fs::create_dir_all(&directory).unwrap();
    let path = Arc::new(directory.join("prefix"));
    let calls = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(4));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let path = path.clone();
            let calls = calls.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                cache::load_or_build(&path, "same", || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    vec![42_u32]
                })
            })
        })
        .collect();
    for thread in threads {
        assert_eq!(thread.join().unwrap(), vec![42_u32]);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn prepared_prefix_requires_existing_file() {
    let directory = test_directory("missing");
    let path = directory.join("missing");
    let result = std::panic::catch_unwind(|| cache::load_prepared::<Vec<u32>>(&path, "same"));
    assert!(result.is_err());
}

#[test]
fn prepared_prefix_requires_matching_key() {
    let directory = test_directory("prepared-key");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("prefix");
    std::fs::write(&path, cache::encode("before", &vec![42_u32])).unwrap();
    let result = std::panic::catch_unwind(|| cache::load_prepared::<Vec<u32>>(&path, "after"));
    assert!(result.is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn prefix_rejects_changed_schema() {
    let mut bytes = cache::encode("same", &vec![1_u32]);
    bytes[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(cache::decode::<Vec<u32>>("same", &bytes)
        .unwrap_err()
        .contains("schema"));
}

#[test]
fn prefix_rejects_trailing_envelope_bytes() {
    let mut bytes = cache::encode("same", &vec![1_u32]);
    bytes.extend_from_slice(b"trailing");
    assert!(cache::decode::<Vec<u32>>("same", &bytes)
        .unwrap_err()
        .contains("envelope"));
}
