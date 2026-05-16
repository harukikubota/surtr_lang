mod cache;
mod compile;
mod phase;
mod run;
mod sources;
mod timing;
mod types;

#[allow(unused_imports)]
pub(crate) use cache::cache_stats_snapshot;
#[allow(unused_imports)]
pub use compile::{
    compile_module_fixture_case, compile_project_script, compile_project_sources, compile_script,
    compile_script_sources, compile_sources_for_module_fixture,
};
#[allow(unused_imports)]
pub use phase::{check_project_phase, check_script_phase, check_script_sources_phase};
#[allow(unused_imports)]
pub use run::{
    run_module_fixture_case, run_project_script, run_project_script_with_input,
    run_project_script_with_stderr, run_script, run_script_with_stderr,
};
#[allow(unused_imports)]
pub use sources::{
    collect_default_module_sources, collect_module_sources, collect_script_compile_sources,
    compose_script_sources, parse_module_stages,
};
#[allow(unused_imports)]
pub use timing::{env_flag_enabled, stable_bucket, test_timing_enabled, timing_report_lock};
#[allow(unused_imports)]
pub use timing::{
    format_timing_report, print_timing_report, CacheStatsSnapshot, SlowFixtureTiming,
    TimingReportInput,
};
