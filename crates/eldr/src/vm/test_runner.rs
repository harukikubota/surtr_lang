use super::{
    RichError, RuntimeError, VmCapturedIo, VmTestDiagnostic, VmTestEvent, VmTestEventKind, VM,
};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmTestScopeKind {
    Test,
    Describe,
}
impl VmTestScopeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Describe => "describe",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmTestScope {
    pub kind: VmTestScopeKind,
    pub name: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmTestDeclaration {
    It,
    Xit,
    Pend,
}
impl VmTestDeclaration {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::It => "it",
            Self::Xit => "xit",
            Self::Pend => "pend",
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmTestPolicy {
    pub test_filter: Option<String>,
    pub describe_filter: Option<String>,
    pub it_filter: Option<String>,
    pub list: bool,
    pub include_xit: bool,
    pub timings: bool,
}
impl VmTestPolicy {
    pub fn selects(&self, scopes: &[VmTestScope], name: &str) -> bool {
        let matches_scope = |filter: &Option<String>, kind| {
            filter.as_ref().is_none_or(|text| {
                scopes
                    .iter()
                    .any(|scope| scope.kind == kind && scope.name.contains(text))
            })
        };
        matches_scope(&self.test_filter, VmTestScopeKind::Test)
            && matches_scope(&self.describe_filter, VmTestScopeKind::Describe)
            && self
                .it_filter
                .as_ref()
                .is_none_or(|text| name.contains(text))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmTestCase {
    pub case_index: usize,
    pub scopes: Vec<VmTestScope>,
    pub name: String,
    pub declaration: VmTestDeclaration,
    pub selected: bool,
    pub reason: Option<String>,
    pub duration_ns: Option<u128>,
}
#[derive(Debug, Clone)]
struct ActiveCase {
    case: VmTestCase,
    started: Option<Instant>,
    stdout: Option<Vec<String>>,
    stderr: Option<Vec<String>>,
    stdin: Option<String>,
    stdin_cursor: usize,
    stdout_cursor: usize,
    stderr_cursor: usize,
}
#[derive(Debug, Clone, Default)]
pub(super) struct VmTestRunner {
    pub policy: VmTestPolicy,
    scopes: Vec<VmTestScope>,
    next_index: usize,
    active: Option<ActiveCase>,
}
impl VmTestRunner {
    pub(super) fn reset(&mut self) {
        *self = Self {
            policy: self.policy.clone(),
            ..Self::default()
        };
    }
}
impl VM {
    pub fn with_test_policy(mut self, policy: VmTestPolicy) -> Self {
        self.test_runner.policy = policy;
        self
    }
    pub(crate) fn push_test_scope(&mut self, kind: &str, name: String) -> Result<(), RuntimeError> {
        let kind = match kind {
            "test" => VmTestScopeKind::Test,
            "describe" => VmTestScopeKind::Describe,
            _ => return Err(RuntimeError::new("invalid test scope kind")),
        };
        self.test_runner.scopes.push(VmTestScope { kind, name });
        Ok(())
    }
    pub(crate) fn pop_test_scope(&mut self) -> Result<(), RuntimeError> {
        self.test_runner
            .scopes
            .pop()
            .map(|_| ())
            .ok_or_else(|| RuntimeError::new("test scope stack underflow"))
    }
    pub(crate) fn begin_test_case(
        &mut self,
        declaration: &str,
        name: String,
        reason: String,
    ) -> Result<bool, RuntimeError> {
        if self.test_runner.active.is_some() {
            return Err(RuntimeError::new(
                "test cases cannot be declared inside a test case",
            ));
        }
        let declaration = match declaration {
            "it" => VmTestDeclaration::It,
            "xit" => VmTestDeclaration::Xit,
            "pend" => VmTestDeclaration::Pend,
            _ => return Err(RuntimeError::new("invalid test declaration kind")),
        };
        let reason = match declaration {
            VmTestDeclaration::It if reason.is_empty() => None,
            VmTestDeclaration::It => {
                return Err(RuntimeError::new("it cannot have a stopping reason"))
            }
            _ if reason.trim().is_empty() => {
                return Err(RuntimeError::new(
                    "test declaration reason must not be empty or whitespace",
                ))
            }
            _ => Some(reason),
        };
        let selected = self
            .test_runner
            .policy
            .selects(&self.test_runner.scopes, &name);
        let case = VmTestCase {
            case_index: self.test_runner.next_index,
            scopes: self.test_runner.scopes.clone(),
            name,
            declaration,
            selected,
            reason,
            duration_ns: None,
        };
        self.test_runner.next_index += 1;
        let kind = if !selected {
            Some(VmTestEventKind::Filtered)
        } else if declaration == VmTestDeclaration::Pend {
            Some(VmTestEventKind::Pending)
        } else if declaration == VmTestDeclaration::Xit && !self.test_runner.policy.include_xit {
            Some(VmTestEventKind::Skipped)
        } else if self.test_runner.policy.list {
            Some(VmTestEventKind::Runnable)
        } else {
            None
        };
        if let Some(kind) = kind {
            self.test_events
                .push(case_event(case, kind, None, None, None));
            return Ok(false);
        }
        let active = ActiveCase {
            case,
            started: None,
            stdout: self.output.as_mut().map(std::mem::take),
            stderr: self.error_output.as_mut().map(std::mem::take),
            stdin: self.stdin_input.replace(String::new()),
            stdin_cursor: self.stdin_input_cursor,
            stdout_cursor: self.test_stdout_cursor,
            stderr_cursor: self.test_stderr_cursor,
        };
        self.stdin_input_cursor = 0;
        self.test_stdout_cursor = 0;
        self.test_stderr_cursor = 0;
        self.test_runner.active = Some(ActiveCase {
            started: self.test_runner.policy.timings.then(Instant::now),
            ..active
        });
        Ok(true)
    }
    fn finish_test_case(
        &mut self,
        name: &str,
        kind: VmTestEventKind,
        detail: Option<String>,
        error: Option<&RichError>,
    ) -> Result<(), RuntimeError> {
        let duration = self
            .test_runner
            .active
            .as_ref()
            .and_then(|active| active.started.map(|time| time.elapsed().as_nanos()));
        let Some(active) = self.test_runner.active.as_ref() else {
            return Err(RuntimeError::new(
                "test case completion without an active case",
            ));
        };
        if active.case.name != name {
            return Err(RuntimeError::new(
                "test case completion name does not match active case",
            ));
        }
        let mut active = self
            .test_runner
            .active
            .take()
            .expect("validated active case");
        active.case.duration_ns = duration;
        let io = self.next_test_event_io();
        self.output = active.stdout;
        self.error_output = active.stderr;
        self.stdin_input = active.stdin;
        self.stdin_input_cursor = active.stdin_cursor;
        self.test_stdout_cursor = active.stdout_cursor;
        self.test_stderr_cursor = active.stderr_cursor;
        let detail = detail.or_else(|| error.map(RichError::to_display_string));
        let diagnostic =
            error.map(|error| VmTestDiagnostic::from_rich_error(error, &self.bytecode));
        self.test_events
            .push(case_event(active.case, kind, detail, io, diagnostic));
        Ok(())
    }
    pub(crate) fn record_test_pass(&mut self, name: String) -> Result<(), RuntimeError> {
        self.finish_test_case(&name, VmTestEventKind::Passed, None, None)
    }
    pub(crate) fn record_test_fail(
        &mut self,
        name: String,
        detail: String,
    ) -> Result<(), RuntimeError> {
        self.finish_test_case(&name, VmTestEventKind::Failed, Some(detail), None)
    }
    pub(crate) fn record_test_fail_error(
        &mut self,
        name: String,
        error: &RichError,
    ) -> Result<(), RuntimeError> {
        self.finish_test_case(&name, VmTestEventKind::Failed, None, Some(error))
    }
    pub(super) fn abort_test_case(&mut self, error: &RuntimeError) {
        if let Some(active) = self.test_runner.active.as_ref() {
            let name = active.case.name.clone();
            self.record_test_fail(name, error.to_string())
                .expect("active case must finish exactly once");
        }
    }
    pub(crate) fn record_current_scope_fail(&mut self, detail: String) {
        self.test_events.push(VmTestEvent {
            path: self
                .test_runner
                .scopes
                .iter()
                .map(|scope| scope.name.clone())
                .collect(),
            detail: Some(detail),
            kind: VmTestEventKind::ScopeFailed,
            io: None,
            diagnostic: None,
            case: None,
        });
    }
}
fn case_event(
    case: VmTestCase,
    kind: VmTestEventKind,
    detail: Option<String>,
    io: Option<VmCapturedIo>,
    diagnostic: Option<VmTestDiagnostic>,
) -> VmTestEvent {
    let mut path: Vec<_> = case.scopes.iter().map(|scope| scope.name.clone()).collect();
    path.push(case.name.clone());
    VmTestEvent {
        path,
        detail,
        kind,
        io,
        diagnostic,
        case: Some(case),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sindr::ir::Bytecode;

    #[test]
    fn typed_filters_cover_all_eight_sets_and_ancestor_or() {
        let scopes = vec![
            VmTestScope {
                kind: VmTestScopeKind::Test,
                name: "outer".into(),
            },
            VmTestScope {
                kind: VmTestScopeKind::Describe,
                name: "outer group".into(),
            },
            VmTestScope {
                kind: VmTestScopeKind::Test,
                name: "chosen suite".into(),
            },
            VmTestScope {
                kind: VmTestScopeKind::Describe,
                name: "chosen group".into(),
            },
        ];
        for bits in 0..8 {
            let policy = VmTestPolicy {
                test_filter: (bits & 1 != 0).then(|| "chosen suite".into()),
                describe_filter: (bits & 2 != 0).then(|| "chosen group".into()),
                it_filter: (bits & 4 != 0).then(|| "chosen case".into()),
                ..VmTestPolicy::default()
            };
            assert!(policy.selects(&scopes, "chosen case"));
            assert_eq!(policy.selects(&scopes, "other case"), bits & 4 == 0);
            assert_eq!(policy.selects(&[], "chosen case"), bits & 3 == 0);
        }
        assert!(!VmTestPolicy {
            test_filter: Some("chosen group".into()),
            ..VmTestPolicy::default()
        }
        .selects(&scopes, "chosen case"));
        assert!(!VmTestPolicy {
            it_filter: Some("Chosen".into()),
            ..VmTestPolicy::default()
        }
        .selects(&scopes, "chosen case"));
    }

    #[test]
    fn declaration_validation_precedes_selection_and_nested_cases_fail() {
        let mut vm = VM::new(Bytecode::default()).with_test_policy(VmTestPolicy {
            it_filter: Some("selected".into()),
            ..VmTestPolicy::default()
        });
        for kind in ["xit", "pend"] {
            assert!(vm
                .begin_test_case(kind, "filtered".into(), " \t\n".into())
                .is_err());
        }
        assert!(vm
            .begin_test_case("unknown", "selected".into(), "why".into())
            .is_err());
        assert!(vm.push_test_scope("unknown", "scope".into()).is_err());
        assert!(vm
            .begin_test_case("it", "selected".into(), "".into())
            .unwrap());
        for kind in ["it", "xit", "pend"] {
            assert!(vm
                .begin_test_case(kind, "filtered".into(), "why".into())
                .is_err());
        }
        assert!(vm.record_test_pass("wrong".into()).is_err());
        vm.record_test_pass("selected".into()).unwrap();
        assert!(vm.record_test_pass("selected".into()).is_err());
        assert_eq!(vm.test_events().len(), 1);
    }

    #[test]
    fn nonexecuted_cases_preserve_io_and_have_no_timing() {
        let mut vm = VM::new(Bytecode::default())
            .with_output_capture()
            .with_error_capture()
            .with_test_policy(VmTestPolicy {
                timings: true,
                ..VmTestPolicy::default()
            });
        vm.output.as_mut().unwrap().push("walk before".into());
        vm.push_stdin_input("walk input");
        assert_eq!(vm.read_injected_char().as_deref(), Some("w"));
        assert!(!vm
            .begin_test_case("xit", "paused".into(), "repair".into())
            .unwrap());
        assert!(!vm
            .begin_test_case("pend", "future".into(), "later".into())
            .unwrap());
        assert!(vm.begin_test_case("it", "run".into(), "".into()).unwrap());
        assert!(vm.output.as_ref().unwrap().is_empty());
        assert_eq!(vm.stdin_input.as_deref(), Some(""));
        assert!(vm.has_injected_stdin());
        assert!(vm.read_injected_char().is_none());
        assert!(vm.read_injected_line().is_none());
        vm.output.as_mut().unwrap().push("case output".into());
        vm.record_test_pass("run".into()).unwrap();
        assert_eq!(vm.take_stdout(), vec!["walk before"]);
        assert_eq!(vm.stdin_input.as_deref(), Some("walk input"));
        assert_eq!(vm.read_injected_line().as_deref(), Some("alk input"));
        for event in &vm.test_events()[..2] {
            assert!(event.io.is_none());
            assert!(event.case.as_ref().unwrap().duration_ns.is_none());
        }
        assert_eq!(
            vm.test_events()[2].io.as_ref().unwrap().stdout,
            vec!["case output"]
        );
        assert!(vm.test_events()[2]
            .case
            .as_ref()
            .unwrap()
            .duration_ns
            .is_some());
    }

    #[test]
    fn list_include_and_filter_policy_share_one_state_transition() {
        for list in [false, true] {
            for include_xit in [false, true] {
                let mut vm = VM::new(Bytecode::default()).with_test_policy(VmTestPolicy {
                    list,
                    include_xit,
                    it_filter: Some("keep".into()),
                    timings: true,
                    ..VmTestPolicy::default()
                });
                assert!(!vm
                    .begin_test_case("pend", "drop".into(), "later".into())
                    .unwrap());
                assert_eq!(vm.test_events()[0].kind, VmTestEventKind::Filtered);
                let executes = vm
                    .begin_test_case("xit", "keep".into(), "repair".into())
                    .unwrap();
                assert_eq!(executes, !list && include_xit);
                if executes {
                    vm.record_test_pass("keep".into()).unwrap();
                }
                assert_eq!(
                    vm.test_events()[1].kind,
                    if !include_xit {
                        VmTestEventKind::Skipped
                    } else if list {
                        VmTestEventKind::Runnable
                    } else {
                        VmTestEventKind::Passed
                    }
                );
                assert_eq!(vm.test_events()[1].case.as_ref().unwrap().case_index, 1);
            }
        }
    }
}
