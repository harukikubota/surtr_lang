//! Strict, process-shared cache for Scar's test-only standard prefix.
use std::fs;
use std::path::Path;

use bincode::Options;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Envelope {
    schema: u32,
    key: String,
    digest: [u8; 32],
    payload: Vec<u8>,
}

pub(crate) fn encode<T: Serialize>(key: &str, payload: &T) -> Vec<u8> {
    let payload = bincode::serialize(payload).expect("Scar test prefix should serialize");
    bincode::serialize(&Envelope {
        schema: SCHEMA,
        key: key.to_owned(),
        digest: Sha256::digest(&payload).into(),
        payload,
    })
    .expect("Scar test prefix envelope should serialize")
}

pub(crate) fn decode<T: DeserializeOwned>(key: &str, bytes: &[u8]) -> Result<T, String> {
    let envelope: Envelope = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(bytes.len() as u64)
        .reject_trailing_bytes()
        .deserialize(bytes)
        .map_err(|err| format!("invalid Scar test prefix envelope: {err}"))?;
    if envelope.schema != SCHEMA || envelope.key != key {
        return Err("Scar test prefix schema or compiler/source key mismatch".into());
    }
    if envelope.digest != <[u8; 32]>::from(Sha256::digest(&envelope.payload)) {
        return Err("Scar test prefix payload checksum mismatch".into());
    }
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(envelope.payload.len() as u64)
        .reject_trailing_bytes()
        .deserialize(&envelope.payload)
        .map_err(|err| format!("invalid Scar test prefix payload: {err}"))
}

pub(crate) fn load_or_build<T: Serialize + DeserializeOwned>(
    path: &Path,
    key: &str,
    build: impl FnOnce() -> T,
) -> T {
    let read = || match fs::read(path) {
        Ok(bytes) => Some(decode(key, &bytes).unwrap_or_else(|err| {
            panic!(
                "{}: {err}; remove this invalid test cache before retrying",
                path.display()
            )
        })),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => panic!("cannot read Scar test prefix {}: {err}", path.display()),
    };
    if let Some(payload) = read() {
        return payload;
    }
    fs::create_dir_all(path.parent().expect("cache should have parent"))
        .expect("Scar test cache directory should be writable");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))
        .expect("Scar test cache lock should open");
    lock.lock().expect("Scar test cache lock should succeed");
    if let Some(payload) = read() {
        return payload;
    }
    let payload = build();
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, encode(key, &payload)).expect("Scar test prefix should be writable");
    fs::rename(&temporary, path).expect("Scar test prefix should publish atomically");
    payload
}

/// A setup-nominated prefix is mandatory: a missing file is a setup failure.
pub(crate) fn load_prepared<T: DeserializeOwned>(path: &Path, key: &str) -> T {
    let bytes = fs::read(path).unwrap_or_else(|err| {
        panic!(
            "cannot read prepared Scar test prefix {}: {err}",
            path.display()
        )
    });
    decode(key, &bytes).unwrap_or_else(|err| {
        panic!(
            "invalid prepared Scar test prefix {}: {err}",
            path.display()
        )
    })
}
