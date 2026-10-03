use std::env;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub fn env_flag_enabled(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true" | "TRUE" | "yes" | "YES"))
}

pub fn test_timing_enabled() -> bool {
    env_flag_enabled(env::var("SURTR_TEST_TIMING").ok().as_deref())
}

pub fn timing_report_lock() -> &'static Mutex<()> {
    static TIMING_REPORT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    TIMING_REPORT_LOCK.get_or_init(|| Mutex::new(()))
}

pub fn stable_bucket(key: &str, bucket_count: usize) -> usize {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    assert!(bucket_count > 0, "bucket_count must be positive");

    let mut hash = FNV_OFFSET;
    for byte in key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    (hash as usize) % bucket_count
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStatsSnapshot {
    pub semantic_prefix_hits: u64,
    pub semantic_prefix_misses: u64,
    pub semantic_prefix_writes: u64,
    pub semantic_prefix_corrupt: u64,
    pub final_bytecode_hits: u64,
    pub final_bytecode_misses: u64,
    pub final_bytecode_writes: u64,
    pub final_bytecode_corrupt: u64,
}

impl CacheStatsSnapshot {
    pub fn saturating_delta_since(&self, earlier: &Self) -> Self {
        Self {
            semantic_prefix_hits: self
                .semantic_prefix_hits
                .saturating_sub(earlier.semantic_prefix_hits),
            semantic_prefix_misses: self
                .semantic_prefix_misses
                .saturating_sub(earlier.semantic_prefix_misses),
            semantic_prefix_writes: self
                .semantic_prefix_writes
                .saturating_sub(earlier.semantic_prefix_writes),
            semantic_prefix_corrupt: self
                .semantic_prefix_corrupt
                .saturating_sub(earlier.semantic_prefix_corrupt),
            final_bytecode_hits: self
                .final_bytecode_hits
                .saturating_sub(earlier.final_bytecode_hits),
            final_bytecode_misses: self
                .final_bytecode_misses
                .saturating_sub(earlier.final_bytecode_misses),
            final_bytecode_writes: self
                .final_bytecode_writes
                .saturating_sub(earlier.final_bytecode_writes),
            final_bytecode_corrupt: self
                .final_bytecode_corrupt
                .saturating_sub(earlier.final_bytecode_corrupt),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlowFixtureTiming {
    pub path: PathBuf,
    pub phase: String,
    pub duration: Duration,
}

#[derive(Debug, Clone)]
pub struct TimingReportInput<'a> {
    pub group: &'a str,
    pub fixture_count: usize,
    pub total: Duration,
    pub cache: CacheStatsSnapshot,
    pub slowest: &'a [SlowFixtureTiming],
}

pub fn format_timing_report(input: &TimingReportInput<'_>) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "surtr test timing group={} fixtures={} total={:.3}s\n",
        input.group,
        input.fixture_count,
        input.total.as_secs_f64()
    ));
    output.push_str(&format!(
        "cache prefix hit={} miss={} write={} corrupt={} final hit={} miss={} write={} corrupt={}\n",
        input.cache.semantic_prefix_hits,
        input.cache.semantic_prefix_misses,
        input.cache.semantic_prefix_writes,
        input.cache.semantic_prefix_corrupt,
        input.cache.final_bytecode_hits,
        input.cache.final_bytecode_misses,
        input.cache.final_bytecode_writes,
        input.cache.final_bytecode_corrupt
    ));

    for fixture in input.slowest.iter().take(10) {
        output.push_str(&format!(
            "slow fixture {:.3}s [{}] {}\n",
            fixture.duration.as_secs_f64(),
            fixture.phase,
            fixture.path.display()
        ));
    }

    output.trim_end().to_string()
}

pub fn print_timing_report(
    group: &str,
    fixture_count: usize,
    total: Duration,
    cache: CacheStatsSnapshot,
    slowest: &[SlowFixtureTiming],
) {
    eprintln!(
        "{}",
        format_timing_report(&TimingReportInput {
            group,
            fixture_count,
            total,
            cache,
            slowest,
        })
    );
}
