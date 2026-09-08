use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MeasurementOutput {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhaseState {
    Executed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PhaseMeasurement {
    pub(crate) state: PhaseState,
    pub(crate) duration: Option<Duration>,
}

impl Default for PhaseMeasurement {
    fn default() -> Self {
        Self {
            state: PhaseState::Skipped,
            duration: None,
        }
    }
}

impl PhaseMeasurement {
    pub(crate) fn executed(duration: Duration) -> Self {
        Self {
            state: PhaseState::Executed,
            duration: Some(duration),
        }
    }

    fn json(self) -> Value {
        let mut value = Map::new();
        value.insert(
            "status".to_string(),
            Value::String(
                match self.state {
                    PhaseState::Executed => "executed",
                    PhaseState::Skipped => "skipped",
                }
                .to_string(),
            ),
        );
        if let Some(duration) = self.duration {
            value.insert("duration_us".to_string(), json!(duration.as_micros()));
            value.insert("duration_ms".to_string(), json!(duration.as_millis()));
        }
        Value::Object(value)
    }

    fn text(self) -> String {
        match (self.state, self.duration) {
            (PhaseState::Executed, Some(duration)) => format_duration(duration),
            _ => "skipped".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheState {
    NotUsed,
    Disabled,
    Cold,
    ProcessHit,
    DiskHit,
    Hit,
    Miss,
    Stored,
    StoreFailed,
}

impl CacheState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NotUsed => "not_used",
            Self::Disabled => "disabled",
            Self::Cold => "cold",
            Self::ProcessHit => "process_hit",
            Self::DiskHit => "disk_hit",
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Stored => "stored",
            Self::StoreFailed => "store_failed",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CompileMeasurement {
    pub(crate) input: String,
    pub(crate) source_read: PhaseMeasurement,
    pub(crate) compile_plan: PhaseMeasurement,
    pub(crate) stdlib_load: PhaseMeasurement,
    pub(crate) parse_modules: PhaseMeasurement,
    pub(crate) parse_user: PhaseMeasurement,
    pub(crate) resolve: PhaseMeasurement,
    pub(crate) typecheck: PhaseMeasurement,
    pub(crate) codegen: PhaseMeasurement,
    pub(crate) bytecode_encode: PhaseMeasurement,
    pub(crate) output_write: PhaseMeasurement,
    pub(crate) compile_total: PhaseMeasurement,
    pub(crate) decode: PhaseMeasurement,
    pub(crate) execute: PhaseMeasurement,
    pub(crate) total: PhaseMeasurement,
    pub(crate) stdlib_cache: CacheState,
    pub(crate) artifact_cache: CacheState,
    pub(crate) artifact_store: CacheState,
}

impl CompileMeasurement {
    pub(crate) fn new(input: impl Into<String>) -> Self {
        Self {
            input: input.into(),
            source_read: PhaseMeasurement::default(),
            compile_plan: PhaseMeasurement::default(),
            stdlib_load: PhaseMeasurement::default(),
            parse_modules: PhaseMeasurement::default(),
            parse_user: PhaseMeasurement::default(),
            resolve: PhaseMeasurement::default(),
            typecheck: PhaseMeasurement::default(),
            codegen: PhaseMeasurement::default(),
            bytecode_encode: PhaseMeasurement::default(),
            output_write: PhaseMeasurement::default(),
            compile_total: PhaseMeasurement::default(),
            decode: PhaseMeasurement::default(),
            execute: PhaseMeasurement::default(),
            total: PhaseMeasurement::default(),
            stdlib_cache: CacheState::NotUsed,
            artifact_cache: CacheState::NotUsed,
            artifact_store: CacheState::NotUsed,
        }
    }

    pub(crate) fn json(&self) -> Value {
        let mut phases = Map::new();
        for (name, phase) in self.phase_values() {
            phases.insert(name.to_string(), phase.json());
        }
        phases.insert("compile_total".to_string(), self.compile_total.json());
        json!({
            "schema_version": 1,
            "input": &self.input,
            "input_path": &self.input,
            "cache": {
                "stdlib": self.stdlib_cache.as_str(),
                "artifact": self.artifact_cache.as_str(),
                "artifact_store": self.artifact_store.as_str(),
            },
            "phases": phases,
            "compile_total": self.compile_total.json(),
            "total": self.total.json(),
            "compile_total_us": self.compile_total.duration.map(|duration| duration.as_micros()),
            "total_us": self.total.duration.map(|duration| duration.as_micros()),
        })
    }

    pub(crate) fn emit(&self, output: MeasurementOutput) {
        match output {
            MeasurementOutput::Text => self.emit_text(),
            MeasurementOutput::Json => eprintln!(
                "{}",
                serde_json::to_string(&self.json()).unwrap_or_else(|error| {
                    format!(r#"{{"schema_version":1,"error":"{}"}}"#, error)
                })
            ),
        }
    }

    fn emit_text(&self) {
        eprintln!("Phase times:");
        for (name, phase) in self.phase_values() {
            eprintln!("  {name}: {}", phase.text());
        }
        eprintln!(
            "  cache: stdlib={} artifact={} store={}",
            self.stdlib_cache.as_str(),
            self.artifact_cache.as_str(),
            self.artifact_store.as_str()
        );
    }

    fn phase_values(&self) -> [(&'static str, PhaseMeasurement); 15] {
        [
            ("source_read", self.source_read),
            ("compile_plan", self.compile_plan),
            ("stdlib_load", self.stdlib_load),
            ("parse_modules", self.parse_modules),
            ("parse_user", self.parse_user),
            ("parse", combine(self.parse_modules, self.parse_user)),
            ("resolve", self.resolve),
            ("typecheck", self.typecheck),
            ("codegen", self.codegen),
            ("bytecode_encode", self.bytecode_encode),
            ("output_write", self.output_write),
            ("compile", self.compile_total),
            ("decode", self.decode),
            ("execute", self.execute),
            ("total", self.total),
        ]
    }
}

fn combine(first: PhaseMeasurement, second: PhaseMeasurement) -> PhaseMeasurement {
    match (first.state, second.state) {
        (PhaseState::Skipped, PhaseState::Skipped) => PhaseMeasurement::default(),
        _ => PhaseMeasurement {
            state: PhaseState::Executed,
            duration: Some(
                first.duration.unwrap_or_default() + second.duration.unwrap_or_default(),
            ),
        },
    }
}

pub(crate) fn elapsed(start: Instant) -> PhaseMeasurement {
    PhaseMeasurement::executed(start.elapsed())
}

pub(crate) fn format_duration(duration: Duration) -> String {
    format!("{}us", duration.as_micros())
}
