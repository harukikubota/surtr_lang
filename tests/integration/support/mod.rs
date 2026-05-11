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
    compile_project_script, compile_project_sources, compile_script, compile_script_sources,
};
#[allow(unused_imports)]
pub use phase::{check_project_phase, check_script_phase, check_script_sources_phase};
#[allow(unused_imports)]
pub use run::{
    run_project_script, run_project_script_with_input, run_project_script_with_stderr, run_script,
    run_script_with_stderr,
};
#[allow(unused_imports)]
pub use sources::{
    collect_default_module_sources, collect_module_sources, collect_script_compile_sources,
    compose_script_sources, parse_module_stages,
};
#[allow(unused_imports)]
pub use timing::{format_timing_report, CacheStatsSnapshot, SlowFixtureTiming, TimingReportInput};
