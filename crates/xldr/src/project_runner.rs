use std::fmt;

use eldr::VM;
use sindr::policy::{CompileUnitKind, SourceKind};
use surtr_analysis::{ProjectRunnerResult, ProjectRunnerSourceInput};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRunnerVmError {
    phase: &'static str,
    message: String,
}

impl ProjectRunnerVmError {
    fn new(phase: &'static str, message: impl Into<String>) -> Self {
        Self {
            phase,
            message: message.into(),
        }
    }

    pub fn phase(&self) -> &'static str {
        self.phase
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ProjectRunnerVmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.phase, self.message)
    }
}

impl std::error::Error for ProjectRunnerVmError {}

pub fn execute_project_runner_source(
    input: ProjectRunnerSourceInput,
) -> Result<ProjectRunnerResult, ProjectRunnerVmError> {
    let std_snapshot = crate::default_stdlib_semantic_snapshot()
        .map_err(|error| ProjectRunnerVmError::new("load", error.to_string()))?;
    let user_ast = surtr_analysis::parse_document(
        &input.source,
        0,
        SourceKind::ProjectConfigSource,
        CompileUnitKind::Project,
        None,
    )
    .map_err(|error| ProjectRunnerVmError::new("parse", error.message()))?;

    let resolved = sigil::resolve_staged_program_from_state(
        &std_snapshot.module_stages,
        user_ast,
        &std_snapshot.declaration_index,
        Some(crate::script_pseudo_module_path(
            &input.project_file.to_string_lossy(),
        )),
        std_snapshot.default_stage_count,
        std_snapshot.resolve_state,
    )
    .map_err(|error| ProjectRunnerVmError::new("resolve", error.message))?;

    let mut scar_session = scar::ScarSession::new();
    scar_session.rollback(std_snapshot.scar_checkpoint.clone());
    let next_fun_idx = std_snapshot
        .bytecode
        .functions
        .iter()
        .map(|entry| entry.fun_idx.saturating_add(1))
        .max()
        .unwrap_or(0);
    scar_session.ensure_next_fun_idx_at_least(next_fun_idx);
    let typed = scar_session
        .typecheck_staged_program_with_context(
            resolved,
            scar::TypecheckContext {
                runtime_policy: crate::derive_runtime_policy(
                    CompileUnitKind::Project,
                    SourceKind::ProjectConfigSource,
                    None,
                ),
                enforce_builtin_type_contracts: false,
                allow_error_function_params: false,
            },
        )
        .map_err(|error| ProjectRunnerVmError::new("typecheck", error.message))?;

    let mut forge_session = forge::ForgeSession::from_bytecode(&std_snapshot.bytecode);
    let (chunk, _) = forge_session
        .codegen_chunk_typed_program(typed)
        .map_err(|error| ProjectRunnerVmError::new("codegen", error.message))?;
    let bytecode = forge::compose_bytecode_with_chunk(std_snapshot.bytecode.clone(), chunk)
        .map_err(|error| ProjectRunnerVmError::new("codegen", error.message))?;

    let runner_args = input
        .normalized_args
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>();
    let mut vm = VM::new(bytecode)
        .with_source(
            input.source.clone(),
            input.project_file.to_string_lossy().into_owned(),
        )
        .with_cli_args(runner_args);
    vm.run()
        .map_err(|error| ProjectRunnerVmError::new("runtime", error.message))?;
    let value = vm
        .last_value()
        .ok_or_else(|| ProjectRunnerVmError::new("runtime", "project runner produced no value"))?;
    surtr_analysis::decode_project_runner_value(&input.project_file, value, vm.type_registry())
        .map_err(|error| ProjectRunnerVmError::new("decode", error.message().to_string()))
}
