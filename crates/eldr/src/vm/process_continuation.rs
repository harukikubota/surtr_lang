use super::*;
use crate::builtin::{BuiltinContinuation, BuiltinOutcome};

#[derive(Debug, Clone)]
pub(super) struct DetachedTask {
    pub(super) owner_pid: Option<u64>,
    pub(super) state: DetachedTaskState,
    pub(super) completion: DetachedCompletion,
    started: bool,
    cancelling: bool,
    init_context: Option<(String, &'static str, &'static str)>,
}

#[derive(Debug, Clone)]
pub(super) enum DetachedTaskState {
    Runnable(ExecutionContext),
    Waiting {
        future_id: FutureId,
        context: Option<ExecutionContext>,
    },
}

#[derive(Debug, Clone)]
pub(super) enum DetachedCompletion {
    Future(Option<FutureId>),
    ErrorFuture(FutureId),
    Reply {
        correlation_id: CorrelationId,
        future_id: FutureId,
    },
}

#[derive(Debug, Clone)]
pub(super) struct SingletonFlight {
    pub(super) future_id: FutureId,
    waiters: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum RuntimeContinuation {
    ErrorConstruction {
        kind: String,
        location: Location,
        stack_trace: Vec<RuntimeStackFrame>,
        wrapper: Option<u32>,
    },
    Identity,
    TaskCall {
        completion: FutureId,
        previous_trace: Option<RuntimeStackFrame>,
    },
    StartedTask {
        task_id: u64,
        value: Value,
    },
    Singleton(Box<SingletonInit>),
    SingletonAwait {
        process_name: String,
    },
    Spawn {
        process_name: String,
        supervisor: Option<String>,
        after: Option<Box<RuntimeContinuation>>,
    },
    Broadcast {
        members: VecDeque<PidHandle>,
        message: Callable,
        timeout_ms: Option<u64>,
        results: Vec<Value>,
        receiving: bool,
    },
    Workers {
        supervisor: String,
        process_name: String,
        init: Callable,
        strategy: WorkerStrategyState,
        members: Vec<u64>,
        receiving: bool,
    },
    Refill {
        workers_id: u64,
        receiving: bool,
    },
    AwaitTask {
        trace: Option<RuntimeStackFrame>,
        unit: bool,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct SingletonInit {
    process_name: String,
    spec: RuntimeProcessSpec,
    deadline: u64,
    timeout_ms: u64,
    retry_ms: u64,
    trigger: &'static str,
    receiving: bool,
    callback_future: Option<FutureId>,
}

impl SingletonInit {
    fn context(&self, error: RuntimeError) -> RuntimeError {
        VM::with_vm_init_process_context(
            error,
            &self.process_name,
            if runtime_spec_is_standby(&self.spec) {
                "Standby"
            } else {
                "Eager"
            },
            self.trigger,
        )
    }

    fn timeout(&self, vm: &mut VM) -> RuntimeError {
        let detail = format!("init timed out after {}ms", self.timeout_ms);
        vm.process_runtime
            .root_supervisor
            .boot_failures
            .insert(self.process_name.clone(), detail.clone());
        self.context(RuntimeError::process_init_timeout(format!(
            "process `{}` failed to boot: {detail}",
            self.process_name
        )))
    }

    fn resume(
        mut self,
        vm: &mut VM,
        result: Result<Value, RuntimeError>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        if let Some(id) = self.callback_future.take() {
            vm.forget_internal_future(id);
        }
        if vm.process_runtime.current_tick_ms >= self.deadline {
            return Err(self.timeout(vm));
        }
        if !self.receiving {
            self.receiving = true;
            let callable = vm.callable_for_function(self.spec.init.callable.fun_idx);
            let remaining = self.deadline - vm.process_runtime.current_tick_ms;
            return vm.start_timed_callback(
                callable,
                Vec::new(),
                remaining,
                RuntimeContinuation::Singleton(Box::new(self)),
            );
        }
        let value = result.map_err(|error| self.context(error))?;
        let state = match decode_vm_result(value, "__root_boot", "init")
            .map_err(|error| self.context(error))?
        {
            Ok(value) if runtime_spec_is_standby(&self.spec) => {
                match decode_standby_init(value).map_err(|error| self.context(error))? {
                    StandbyInitOutcome::Ready(state) => Some(state),
                    pending => {
                        let max_retry = vm
                            .bytecode
                            .runtime_boot_plan
                            .runtime_limits
                            .pending_max_retry_ms;
                        let delay = match pending {
                            StandbyInitOutcome::Pending => {
                                let delay = self.retry_ms.min(max_retry);
                                self.retry_ms = self.retry_ms.saturating_mul(2).min(max_retry);
                                delay
                            }
                            StandbyInitOutcome::PendingAfter(duration) => vm
                                .duration_millis(&duration, "StandbyInit::PendingAfter")
                                .map_err(|error| self.context(error))?,
                            StandbyInitOutcome::Ready(_) => unreachable!(),
                        }
                        .min(self.deadline - vm.process_runtime.current_tick_ms);
                        self.receiving = false;
                        self.trigger = "standby_retry";
                        if delay == 0 {
                            return Ok(RuntimeContinuation::Singleton(Box::new(self)).pending());
                        }
                        let Value::PendingFuture(future_id) = vm.process_sleep(delay)? else {
                            unreachable!()
                        };
                        return Ok(BuiltinOutcome::Wait {
                            future_id,
                            continuation: BuiltinContinuation::Runtime(
                                RuntimeContinuation::Singleton(Box::new(self)),
                            ),
                        });
                    }
                }
            }
            Ok(state) => Some(state),
            Err(error) => {
                let detail = error.visible_message().to_string();
                vm.process_runtime
                    .root_supervisor
                    .boot_failures
                    .insert(self.process_name.clone(), detail.clone());
                return Err(self.context(RuntimeError::process_init_failed(format!(
                    "process `{}` failed to boot: {detail}",
                    self.process_name
                ))));
            }
        };
        let pid = vm.allocate_process_state(self.process_name.clone(), state)?;
        vm.process_runtime
            .singleton_by_name
            .insert(self.process_name.clone(), pid);
        Ok(BuiltinOutcome::Complete(Value::Pid(PidHandle {
            id: pid,
            process_name: self.process_name,
        })))
    }
}

impl RuntimeContinuation {
    fn callback(self, callable: Callable, args: Vec<Value>) -> BuiltinOutcome {
        BuiltinOutcome::Call {
            callable,
            args,
            continuation: BuiltinContinuation::Runtime(self),
        }
    }

    fn pending(self) -> BuiltinOutcome {
        BuiltinOutcome::Resume(BuiltinContinuation::Runtime(self))
    }

    pub(crate) fn resume(
        self,
        vm: &mut VM,
        result: Result<Value, RuntimeError>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        if let Self::ErrorConstruction {
            kind,
            location,
            stack_trace,
            wrapper,
        } = self
        {
            let Value::Error(mut error) = result? else {
                return Err(RuntimeError::new("Error constructor did not return Error"));
            };
            if error.kind != kind {
                return Err(RuntimeError::new(
                    "Error constructor returned a different declaration",
                ));
            }
            error.location = location;
            error.stack_trace = stack_trace;
            let value = Value::Error(error);
            let value = if let Some(tag) = wrapper {
                let fields = if tag == 1 {
                    vec![value]
                } else {
                    let entry =
                        vm.bytecode.type_registry.lookup(tag).ok_or_else(|| {
                            RuntimeError::new("missing MatchResult wrapper metadata")
                        })?;
                    if sindr::builtin::match_result_variant_meta(&entry.name)
                        != Some(sindr::builtin::MATCH_RESULT_ERR_VARIANT)
                    {
                        return Err(RuntimeError::new("noncanonical Error wrapper"));
                    }
                    vec![
                        Value::Int(sindr::builtin::MATCH_RESULT_ERR_VARIANT.discriminant.into()),
                        value,
                    ]
                };
                Value::Tagged { tag, fields }
            } else {
                value
            };
            return Ok(BuiltinOutcome::Complete(value));
        }
        if let Self::TaskCall {
            completion,
            previous_trace,
        } = self
        {
            vm.current_frame_mut()?.trace_frame = previous_trace;
            let value = result?;
            vm.process_runtime.resolve_future(completion, value.clone());
            return Ok(BuiltinOutcome::Complete(value));
        }
        if let Self::Singleton(init) = self {
            return init.resume(vm, result);
        }
        if let Self::SingletonAwait { process_name } = self {
            vm.release_singleton_waiter(&process_name)?;
            return Ok(BuiltinOutcome::Complete(result?));
        }
        if let Err(error) = result {
            match self {
                Self::Refill { workers_id, .. } => {
                    vm.process_runtime.refilling_worker_sets.remove(&workers_id);
                }
                Self::Spawn {
                    after: Some(after), ..
                } => return after.resume(vm, Err(error)),
                _ => {}
            }
            return Err(error);
        }
        let value = result?;
        match self {
            Self::ErrorConstruction { .. } | Self::TaskCall { .. } => unreachable!(),
            Self::Singleton(_) => unreachable!(),
            Self::SingletonAwait { .. } => unreachable!(),
            Self::Identity => Ok(BuiltinOutcome::Complete(value)),
            Self::StartedTask { task_id, value } => {
                if !vm.task_reached_startup_boundary(task_id) {
                    Ok(Self::StartedTask { task_id, value }.pending())
                } else {
                    Ok(BuiltinOutcome::Complete(value))
                }
            }
            Self::Spawn {
                process_name,
                supervisor,
                after,
            } => {
                let result = match decode_vm_result(value, "__process_spawn", "init")? {
                    Ok(state) => {
                        let pid = match supervisor {
                            Some(supervisor) => vm.allocate_supervised_worker(
                                process_name.clone(),
                                Some(state),
                                supervisor,
                            )?,
                            None => vm.allocate_process_instance(
                                process_name.clone(),
                                Some(state),
                                None,
                                None,
                            )?,
                        };
                        ok_vm_result(Value::Pid(PidHandle {
                            id: pid,
                            process_name,
                        }))
                    }
                    Err(error) => err_vm_result(error),
                };
                match after {
                    Some(after) => after.resume(vm, Ok(result)),
                    None => Ok(BuiltinOutcome::Complete(result)),
                }
            }
            Self::Broadcast {
                mut members,
                message,
                timeout_ms,
                mut results,
                receiving,
            } => {
                if receiving {
                    results.push(value);
                }
                let Some(pid) = members.pop_front() else {
                    return Ok(BuiltinOutcome::Complete(Value::List(
                        ListHandle::from_items(results),
                    )));
                };
                let next = Self::Broadcast {
                    members,
                    message: message.clone(),
                    timeout_ms,
                    results,
                    receiving: true,
                };
                match timeout_ms {
                    Some(timeout) => {
                        vm.start_timed_callback(message, vec![Value::Pid(pid)], timeout, next)
                    }
                    None => Ok(next.callback(message, vec![Value::Pid(pid)])),
                }
            }
            Self::Workers {
                supervisor,
                process_name,
                init,
                strategy,
                mut members,
                receiving,
            } => {
                if receiving {
                    match vm.pid_handle_like_from_result(value, "WorkerCreationExpectedPid")? {
                        Ok(pid) => members.push(pid.id),
                        Err(error) => return Ok(error),
                    }
                }
                let target = strategy.target();
                if members.len() < target as usize {
                    let next = Self::Workers {
                        supervisor: supervisor.clone(),
                        process_name: process_name.clone(),
                        init: init.clone(),
                        strategy,
                        members,
                        receiving: true,
                    };
                    return Ok(Self::Spawn {
                        process_name,
                        supervisor: Some(supervisor),
                        after: Some(Box::new(next)),
                    }
                    .callback(init, Vec::new()));
                }
                let id = vm.process_runtime.next_workers_id;
                vm.process_runtime.next_workers_id += 1;
                vm.process_runtime.worker_sets.insert(
                    id,
                    WorkerSetState {
                        supervisor_name: supervisor,
                        worker_process: process_name.clone(),
                        init_callable: init,
                        strategy,
                        target,
                        members,
                        next_index: 0,
                    },
                );
                Ok(BuiltinOutcome::Complete(ok_vm_result(Value::Workers(
                    WorkersHandle { id, process_name },
                ))))
            }
            Self::Refill {
                workers_id,
                receiving,
            } => {
                if receiving {
                    let pid =
                        match vm.pid_handle_like_from_result(value, "WorkerRefillExpectedPid")? {
                            Ok(pid) => pid,
                            Err(error) => {
                                vm.process_runtime.refilling_worker_sets.remove(&workers_id);
                                return Ok(error);
                            }
                        };
                    let state = vm
                        .process_runtime
                        .worker_sets
                        .get_mut(&workers_id)
                        .ok_or_else(|| RuntimeError::new("worker set disappeared during refill"))?;
                    state.members.push(pid.id);
                }
                let state = vm
                    .process_runtime
                    .worker_sets
                    .get(&workers_id)
                    .ok_or_else(|| RuntimeError::new("unknown worker set during refill"))?;
                if state.members.len() >= state.target as usize {
                    vm.process_runtime.refilling_worker_sets.remove(&workers_id);
                    return Ok(BuiltinOutcome::Complete(ok_vm_result(Value::Unit)));
                }
                Ok(Self::Spawn {
                    process_name: state.worker_process.clone(),
                    supervisor: Some(state.supervisor_name.clone()),
                    after: Some(Box::new(Self::Refill {
                        workers_id,
                        receiving: true,
                    })),
                }
                .callback(state.init_callable.clone(), Vec::new()))
            }
            Self::AwaitTask { trace, unit } => {
                if unit {
                    return Ok(BuiltinOutcome::Complete(ok_vm_result(Value::Unit)));
                }
                let value = match (trace, value) {
                    (Some(trace), Value::Tagged { tag: 1, fields }) => match fields.as_slice() {
                        [Value::Error(error)] => {
                            let mut error = (**error).clone();
                            error.stack_trace.insert(0, trace);
                            err_vm_result(error)
                        }
                        _ => Value::Tagged { tag: 1, fields },
                    },
                    (_, value) => value,
                };
                Ok(BuiltinOutcome::Complete(value))
            }
        }
    }
}

impl VM {
    pub(super) fn schedule_future_error(
        &mut self,
        future_id: FutureId,
        kind: &str,
        args: Vec<Value>,
    ) {
        let Some(future) = self.process_runtime.futures.get_mut(&future_id) else {
            return;
        };
        if !matches!(future.state, super::FutureState::Running) || future.error_generation_pending {
            return;
        }
        future.error_generation_pending = true;
        future.deadline_tick = None;
        future.cancel_on_timeout = false;
        if let Some(correlation) = future.correlation_id.take() {
            self.process_runtime.reply_table.remove(&correlation);
        }
        self.process_runtime
            .deadline_queue
            .retain(|entry| entry.future_id != future_id);
        let outcome = future
            .creation_context
            .clone()
            .ok_or_else(|| RuntimeError::new("future Error generation has no creation context"))
            .and_then(|(location, trace)| {
                self.language_error_outcome_at(kind, args, Some(1), location, trace)
            });
        let mut context = Self::standalone_builtin_context(BuiltinOutcome::Complete(Value::Unit));
        context.pending_invocation = Some(outcome.map(continuation::Invocation::Builtin));
        self.register_runtime_task(
            None,
            DetachedTaskState::Runnable(context),
            DetachedCompletion::ErrorFuture(future_id),
        );
    }

    pub(super) fn poll_runtime_deadlines(&mut self) {
        let elapsed = self
            .runtime_clock_anchor
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        if elapsed > 0 {
            self.runtime_clock_anchor += Duration::from_millis(elapsed);
            self.process_runtime.current_tick_ms =
                self.process_runtime.current_tick_ms.saturating_add(elapsed);
            self.expire_process_deadlines(self.process_runtime.current_tick_ms);
        }
    }

    fn register_runtime_task(
        &mut self,
        owner_pid: Option<u64>,
        state: DetachedTaskState,
        completion: DetachedCompletion,
    ) -> u64 {
        let id = self.process_runtime.next_detached_task_id;
        self.process_runtime.next_detached_task_id += 1;
        self.process_runtime.detached_tasks.insert(
            id,
            DetachedTask {
                owner_pid,
                state,
                completion,
                started: false,
                cancelling: false,
                init_context: None,
            },
        );
        id
    }

    fn schedule_callback(
        &mut self,
        callable: Callable,
        args: Vec<Value>,
        owner_pid: Option<u64>,
        completion: DetachedCompletion,
    ) -> Result<u64, RuntimeError> {
        let context = self.prepare_callable_context(callable, args)?;
        Ok(self.register_runtime_task(owner_pid, DetachedTaskState::Runnable(context), completion))
    }

    fn start_timed_callback(
        &mut self,
        callable: Callable,
        args: Vec<Value>,
        timeout_ms: u64,
        mut continuation: RuntimeContinuation,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        self.poll_runtime_deadlines();
        let completion = self
            .process_runtime
            .allocate_future_after(None, timeout_ms, true);
        if matches!(continuation, RuntimeContinuation::Singleton(_)) {
            self.process_runtime
                .futures
                .get_mut(&completion)
                .expect("new internal init future")
                .init_deadline_timeout = true;
        } else {
            self.stamp_future_context(completion)?;
        }
        let id = self.schedule_callback(
            callable,
            args,
            None,
            DetachedCompletion::Future(Some(completion)),
        )?;
        if let RuntimeContinuation::Singleton(init) = &mut continuation {
            init.callback_future = Some(completion);
            self.process_runtime
                .detached_tasks
                .get_mut(&id)
                .ok_or_else(|| RuntimeError::new("scheduled init callback disappeared"))?
                .init_context = Some((
                init.process_name.clone(),
                if runtime_spec_is_standby(&init.spec) {
                    "Standby"
                } else {
                    "Eager"
                },
                init.trigger,
            ));
        }
        Ok(BuiltinOutcome::Wait {
            future_id: completion,
            continuation: BuiltinContinuation::Runtime(continuation),
        })
    }

    pub(crate) fn start_task(
        &mut self,
        callable: Callable,
        mode: TaskMode,
        timeout_ms: Option<u64>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        self.poll_runtime_deadlines();
        let completion = if matches!(mode, TaskMode::Call | TaskMode::Async) || timeout_ms.is_some()
        {
            Some(match timeout_ms {
                Some(ms) => self.process_runtime.allocate_future_after(None, ms, true),
                None => self.process_runtime.allocate_future(None, None, false),
            })
        } else {
            None
        };
        if let Some(id) = completion {
            self.stamp_future_context(id)?;
        }
        let previous = self.current_frame()?.trace_frame.clone();
        if let Some((start, end)) = self.current_frame()?.call_site {
            let trace = self.trace_frame_for_task(Self::task_mode_trace_name(mode), start, end);
            self.current_frame_mut()?.trace_frame = Some(trace);
        }
        if mode == TaskMode::Call && timeout_ms.is_none() {
            return Ok(RuntimeContinuation::TaskCall {
                completion: completion
                    .ok_or_else(|| RuntimeError::new("task completion future is missing"))?,
                previous_trace: previous,
            }
            .callback(callable, Vec::new()));
        }
        let scheduled = self.schedule_callback(
            callable,
            Vec::new(),
            None,
            DetachedCompletion::Future(completion),
        );
        self.current_frame_mut()?.trace_frame = previous;
        let task_id = scheduled?;
        match mode {
            TaskMode::Async => Ok(RuntimeContinuation::StartedTask {
                task_id,
                value: Value::TaskHandle(
                    completion
                        .ok_or_else(|| RuntimeError::new("task completion future is missing"))?,
                ),
            }
            .pending()),
            TaskMode::Call => Ok(BuiltinOutcome::Wait {
                future_id: completion
                    .ok_or_else(|| RuntimeError::new("task completion future is missing"))?,
                continuation: BuiltinContinuation::Runtime(RuntimeContinuation::Identity),
            }),
            TaskMode::Launch | TaskMode::Cast => match completion {
                Some(future_id) => Ok(BuiltinOutcome::Wait {
                    future_id,
                    continuation: BuiltinContinuation::Runtime(RuntimeContinuation::AwaitTask {
                        trace: None,
                        unit: true,
                    }),
                }),
                None => Ok(RuntimeContinuation::StartedTask {
                    task_id,
                    value: ok_vm_result(Value::Unit),
                }
                .pending()),
            },
        }
    }

    pub(crate) fn start_await_task(
        &mut self,
        value: &Value,
        timeout_ms: Option<u64>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        self.poll_runtime_deadlines();
        let future_id = match value {
            Value::TaskHandle(id) | Value::PendingFuture(id) => *id,
            other => return Ok(BuiltinOutcome::Complete(other.clone())),
        };
        let label = if timeout_ms.is_some() {
            "Task::await_timeout"
        } else {
            "Task::await"
        };
        let trace = self
            .current_frame()?
            .call_site
            .map(|(start, end)| self.trace_frame_for_task(label, start, end));
        let completion = match timeout_ms {
            Some(ms) => self.process_runtime.allocate_future_after(None, ms, true),
            None => self.process_runtime.allocate_future(None, None, false),
        };
        self.stamp_future_context(completion)?;
        self.register_runtime_task(
            None,
            DetachedTaskState::Waiting {
                future_id,
                context: None,
            },
            DetachedCompletion::Future(Some(completion)),
        );
        Ok(BuiltinOutcome::Wait {
            future_id: completion,
            continuation: BuiltinContinuation::Runtime(RuntimeContinuation::AwaitTask {
                trace,
                unit: false,
            }),
        })
    }

    pub(super) fn has_runnable_detached_tasks(&self) -> bool {
        self.process_runtime.detached_tasks.values().any(|task| {
            task.cancelling
                || self.task_completion_ready(task)
                || match &task.state {
                    DetachedTaskState::Runnable(_) => true,
                    DetachedTaskState::Waiting { future_id, .. } => {
                        self.ready_future_value(*future_id).is_some()
                    }
                }
        })
    }

    pub(super) fn drive_ready_detached_tasks(&mut self) -> Result<(), RuntimeError> {
        self.poll_runtime_deadlines();
        let ids = self
            .process_runtime
            .detached_tasks
            .iter()
            .filter_map(|(id, task)| {
                let ready = task.cancelling
                    || self.task_completion_ready(task)
                    || match &task.state {
                        DetachedTaskState::Runnable(_) => true,
                        DetachedTaskState::Waiting { future_id, .. } => {
                            self.ready_future_value(*future_id).is_some()
                        }
                    };
                ready.then_some(*id)
            })
            .collect::<Vec<_>>();
        let saved_pc = self.pc;
        let saved = self.take_execution_context(saved_pc, ExecutionTarget::TopLevel);
        let result = (|| {
            for id in ids {
                let Some(mut task) = self.process_runtime.detached_tasks.take(&id) else {
                    continue;
                };
                if self.task_completion_ready(&task) && !task.cancelling {
                    match &mut task.state {
                        DetachedTaskState::Runnable(context)
                        | DetachedTaskState::Waiting {
                            context: Some(context),
                            ..
                        } => {
                            Self::cancel_execution_context(
                                context,
                                RuntimeError::new("task cancelled after completion deadline"),
                            );
                            task.cancelling = true;
                        }
                        DetachedTaskState::Waiting { context: None, .. } => {
                            self.publish_completed_cleanup(&task.completion);
                            continue;
                        }
                    }
                }
                let mut context = match task.state {
                    DetachedTaskState::Runnable(context) => context,
                    DetachedTaskState::Waiting {
                        future_id,
                        context: Some(context),
                    } => {
                        if !task.cancelling && self.ready_future_value(future_id).is_none() {
                            return Err(RuntimeError::new(
                                "scheduled waiting task has no ready future",
                            ));
                        }
                        context
                    }
                    DetachedTaskState::Waiting {
                        future_id,
                        context: None,
                    } => {
                        let value = self.ready_future_value(future_id).ok_or_else(|| {
                            RuntimeError::new("scheduled waiting task has no ready future")
                        })?;
                        self.complete_runtime_task(task.completion, value);
                        continue;
                    }
                };
                let outcome = self.run_quantum(&mut context, &mut Budget::new(256));
                task.started |= match &outcome {
                    ProcessRunOutcome::QuantumExpired => true,
                    ProcessRunOutcome::Pending(id)
                    | ProcessRunOutcome::Halted(Value::PendingFuture(id)) => {
                        self.future_reached_startup_boundary(*id)
                    }
                    _ => false,
                };
                self.poll_runtime_deadlines();
                task.state = match outcome {
                    ProcessRunOutcome::QuantumExpired => DetachedTaskState::Runnable(context),
                    ProcessRunOutcome::Pending(future_id) => {
                        if let DetachedCompletion::Future(Some(completion)) = task.completion {
                            if self
                                .process_runtime
                                .futures
                                .get(&future_id)
                                .is_some_and(|future| future.correlation_id.is_some())
                            {
                                if let Some(deadline) = self
                                    .process_runtime
                                    .futures
                                    .get(&completion)
                                    .and_then(|future| future.deadline_tick)
                                {
                                    self.process_runtime.attach_future_deadline(
                                        future_id,
                                        self.process_runtime.current_tick_ms,
                                        deadline
                                            .saturating_sub(self.process_runtime.current_tick_ms),
                                        true,
                                    );
                                }
                            }
                        }
                        DetachedTaskState::Waiting {
                            future_id,
                            context: Some(context),
                        }
                    }
                    ProcessRunOutcome::Halted(Value::PendingFuture(future_id)) => {
                        DetachedTaskState::Waiting {
                            future_id,
                            context: None,
                        }
                    }
                    ProcessRunOutcome::Halted(value) => {
                        self.complete_runtime_task(task.completion, value);
                        continue;
                    }
                    ProcessRunOutcome::Failed(_) if task.cancelling => {
                        self.publish_completed_cleanup(&task.completion);
                        continue;
                    }
                    ProcessRunOutcome::Failed(error) => {
                        return Err(match task.init_context {
                            Some((name, policy, trigger)) => {
                                Self::with_vm_init_process_context(error, &name, policy, trigger)
                            }
                            None => error,
                        })
                    }
                };
                self.process_runtime.detached_tasks.insert(id, task);
            }
            Ok(())
        })();
        self.restore_execution_context(saved);
        result
    }

    pub(super) fn drive_ready_runtime_quantum(&mut self) -> Result<(), RuntimeError> {
        self.drive_ready_detached_tasks()?;
        let saved = self.take_execution_context(self.pc, ExecutionTarget::TopLevel);
        let outcome = self.scheduler_tick(128);
        self.restore_execution_context(saved);
        if let Some(ProcessRunOutcome::Failed(error)) = outcome? {
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn has_runnable_background_work(&self) -> bool {
        !self.process_runtime.run_queue.is_empty() || self.has_runnable_detached_tasks()
    }

    fn task_completion_ready(&self, task: &DetachedTask) -> bool {
        match task.completion {
            DetachedCompletion::ErrorFuture(id) => self.raw_ready_future_value(id).is_some(),
            DetachedCompletion::Future(Some(id)) => {
                self.raw_ready_future_value(id).is_some()
                    || self
                        .process_runtime
                        .futures
                        .get(&id)
                        .is_some_and(|future| future.error_generation_pending)
            }
            DetachedCompletion::Reply { correlation_id, .. } => !self
                .process_runtime
                .reply_table
                .contains_key(&correlation_id),
            DetachedCompletion::Future(None) => false,
        }
    }

    fn publish_completed_cleanup(&mut self, completion: &DetachedCompletion) {
        let id = match completion {
            DetachedCompletion::Future(Some(id))
            | DetachedCompletion::ErrorFuture(id)
            | DetachedCompletion::Reply { future_id: id, .. } => *id,
            DetachedCompletion::Future(None) => return,
        };
        if self.ready_future_value(id).is_some() {
            self.process_runtime.wake_future_waiters(id);
        }
    }

    fn future_producer(&self, future_id: FutureId) -> Option<u64> {
        self.process_runtime
            .detached_tasks
            .iter()
            .find_map(|(id, task)| {
                let produces = match task.completion {
                    DetachedCompletion::Future(Some(future))
                    | DetachedCompletion::ErrorFuture(future)
                    | DetachedCompletion::Reply {
                        future_id: future, ..
                    } => future == future_id,
                    DetachedCompletion::Future(None) => false,
                };
                produces.then_some(*id)
            })
    }

    fn future_reached_startup_boundary(&self, future_id: FutureId) -> bool {
        if self.ready_future_value(future_id).is_some() {
            return false;
        }
        self.future_producer(future_id)
            .is_none_or(|id| self.task_reached_startup_boundary(id))
    }

    // An internal completion future does not itself end Task::async's initial prefix.
    // Follow its producer until an actual wait/quantum, or let the caller consume a
    // ready result before deciding that its initial prefix has finished.
    fn task_reached_startup_boundary(&self, mut id: u64) -> bool {
        let mut seen = BTreeSet::new();
        loop {
            if !seen.insert(id) {
                return true;
            }
            let Some(task) = self.process_runtime.detached_tasks.get(&id) else {
                return true;
            };
            if task.started {
                return true;
            }
            match task.state {
                DetachedTaskState::Runnable(_) => return false,
                DetachedTaskState::Waiting { future_id, .. } => {
                    if self.ready_future_value(future_id).is_some() {
                        return false;
                    }
                    match self.future_producer(future_id) {
                        Some(producer) => id = producer,
                        None => return true,
                    }
                }
            }
        }
    }

    pub(super) fn ready_future_value(&self, future_id: FutureId) -> Option<Value> {
        let value = self.raw_ready_future_value(future_id)?;
        let cleanup_pending =
            self.process_runtime
                .detached_tasks
                .values()
                .any(|task| match task.completion {
                    DetachedCompletion::Future(Some(id))
                    | DetachedCompletion::ErrorFuture(id)
                    | DetachedCompletion::Reply { future_id: id, .. } => id == future_id,
                    DetachedCompletion::Future(None) => false,
                });
        (!cleanup_pending).then_some(value)
    }

    pub(super) fn remove_process_detached_tasks(&mut self, pid: u64) {
        let owned_ids = self
            .process_runtime
            .detached_tasks
            .iter()
            .filter_map(|(id, task)| (task.owner_pid == Some(pid)).then_some(*id))
            .collect::<Vec<_>>();
        for id in owned_ids {
            let mut task = self
                .process_runtime
                .detached_tasks
                .take(&id)
                .expect("selected task remains registered during cancellation");
            match &mut task.state {
                DetachedTaskState::Runnable(context)
                | DetachedTaskState::Waiting {
                    context: Some(context),
                    ..
                } => {
                    Self::cancel_execution_context(context, RuntimeError::new("process stopped"));
                    task.cancelling = true;
                    self.process_runtime.detached_tasks.insert(id, task);
                }
                DetachedTaskState::Waiting { context: None, .. } => {}
            }
        }
    }

    fn complete_runtime_task(&mut self, completion: DetachedCompletion, value: Value) {
        match completion {
            DetachedCompletion::ErrorFuture(id) => {
                if let Some(future) = self.process_runtime.futures.get_mut(&id) {
                    future.error_generation_pending = false;
                }
                self.complete_runtime_task(DetachedCompletion::Future(Some(id)), value);
            }
            DetachedCompletion::Future(Some(id)) => {
                self.process_runtime.resolve_future(id, value);
                let abandoned =
                    self.process_runtime
                        .singleton_inits
                        .iter()
                        .find_map(|(name, flight)| {
                            (flight.future_id == id && flight.waiters == 0).then_some(name.clone())
                        });
                if let Some(name) = abandoned {
                    self.process_runtime.singleton_inits.remove(&name);
                    self.forget_internal_future(id);
                }
            }
            DetachedCompletion::Future(None) => {}
            DetachedCompletion::Reply { correlation_id, .. } => {
                self.process_runtime.resolve_reply(correlation_id, value);
            }
        }
    }

    pub(crate) fn genserver_call_reply_later(
        &mut self,
        pid: &PidHandle,
        next_state: Value,
        callback: Callable,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        if let Err(error) = decode_vm_result(
            match self.process_store(pid, next_state)? {
                BuiltinOutcome::Complete(value) => value,
                error => return Ok(error),
            },
            "__genserver_call_reply_later",
            "state store",
        )? {
            return Ok(BuiltinOutcome::Complete(err_vm_result(error)));
        }
        let future_id = self
            .process_runtime
            .allocate_future(Some(pid.id), None, false);
        self.stamp_future_context(future_id)?;
        let correlation_id = self.process_runtime.allocate_correlation_id();
        self.process_runtime
            .register_reply_waiter(correlation_id, future_id);
        self.schedule_callback(
            callback,
            Vec::new(),
            Some(pid.id),
            DetachedCompletion::Reply {
                correlation_id,
                future_id,
            },
        )?;
        Ok(BuiltinOutcome::Complete(Value::PendingFuture(future_id)))
    }

    pub(crate) fn start_singleton(
        &mut self,
        process_name: String,
        timeout_ms: Option<u64>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        self.poll_runtime_deadlines();
        let process_name = self
            .process_runtime
            .canonical_process_name(&process_name)
            .unwrap_or(&process_name)
            .to_string();
        if let Some(id) = self
            .process_runtime
            .singleton_pid_by_process_name(&process_name)
        {
            return Ok(BuiltinOutcome::Complete(Value::Pid(PidHandle {
                id,
                process_name,
            })));
        }
        if let Some(detail) = self
            .process_runtime
            .root_supervisor
            .boot_failures
            .get(&process_name)
        {
            return Err(self.boot_failure_error(&process_name, detail));
        }
        if let Some(flight) = self.process_runtime.singleton_inits.get_mut(&process_name) {
            flight.waiters += 1;
            return Ok(BuiltinOutcome::Wait {
                future_id: flight.future_id,
                continuation: BuiltinContinuation::Runtime(RuntimeContinuation::SingletonAwait {
                    process_name,
                }),
            });
        }
        let spec = self
            .process_runtime
            .spec_by_process_name(&process_name)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::new(format!("unknown singleton process `{process_name}`"))
            })?;
        if spec.instance != RuntimeProcessInstance::Singleton {
            return Err(RuntimeError::new(format!(
                "process `{process_name}` is not a singleton"
            )));
        }
        let limits = &self.bytecode.runtime_boot_plan.runtime_limits;
        let timeout_ms = timeout_ms.unwrap_or(limits.default_init_timeout_ms);
        let retry_ms = limits.pending_initial_retry_ms;
        let outcome = RuntimeContinuation::Singleton(Box::new(SingletonInit {
            process_name: process_name.clone(),
            spec,
            deadline: self
                .process_runtime
                .current_tick_ms
                .saturating_add(timeout_ms),
            timeout_ms,
            retry_ms,
            trigger: "boot",
            receiving: false,
            callback_future: None,
        }))
        .pending();
        let context = self.prepare_builtin_context(outcome)?;
        let future_id = self.process_runtime.allocate_future(None, None, false);
        self.process_runtime.singleton_inits.insert(
            process_name.clone(),
            SingletonFlight {
                future_id,
                waiters: 1,
            },
        );
        self.register_runtime_task(
            None,
            DetachedTaskState::Runnable(context),
            DetachedCompletion::Future(Some(future_id)),
        );
        Ok(BuiltinOutcome::Wait {
            future_id,
            continuation: BuiltinContinuation::Runtime(RuntimeContinuation::SingletonAwait {
                process_name,
            }),
        })
    }

    fn forget_internal_future(&mut self, id: FutureId) {
        self.process_runtime.futures.remove(&id);
        self.process_runtime
            .deadline_queue
            .retain(|entry| entry.future_id != id);
    }

    fn release_singleton_waiter(&mut self, name: &str) -> Result<(), RuntimeError> {
        let flight = self
            .process_runtime
            .singleton_inits
            .get_mut(name)
            .ok_or_else(|| {
                RuntimeError::new(format!(
                    "singleton init waiter has no initialization: {name}"
                ))
            })?;
        flight.waiters = flight.waiters.checked_sub(1).ok_or_else(|| {
            RuntimeError::new(format!("singleton init waiter count underflow: {name}"))
        })?;
        let id = flight.future_id;
        if flight.waiters == 0 && self.ready_future_value(id).is_some() {
            self.process_runtime.singleton_inits.remove(name);
            self.forget_internal_future(id);
        }
        Ok(())
    }

    pub(super) fn refill_worker_set(&mut self, workers_id: u64) {
        if !self
            .process_runtime
            .refilling_worker_sets
            .insert(workers_id)
        {
            return;
        }
        let outcome = RuntimeContinuation::Refill {
            workers_id,
            receiving: false,
        }
        .pending();
        let context = Self::standalone_builtin_context(outcome);
        self.register_runtime_task(
            None,
            DetachedTaskState::Runnable(context),
            DetachedCompletion::Future(None),
        );
    }

    pub(crate) fn start_process_spawn(
        &mut self,
        process_name: String,
        init: Callable,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        let supervisor = self
            .process_runtime
            .specs_by_name
            .get(&process_name)
            .filter(|spec| spec.instance == RuntimeProcessInstance::Worker)
            .map(|_| "DynamicSupervisor".to_string());
        Ok(RuntimeContinuation::Spawn {
            process_name,
            supervisor,
            after: None,
        }
        .callback(init, Vec::new()))
    }

    pub(crate) fn start_supervisor_spawn(
        &mut self,
        supervisor: String,
        worker_name: Option<String>,
        init: Callable,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        let process_name = worker_name
            .or_else(|| self.infer_worker_process_name_from_callable(&init))
            .ok_or_else(|| {
                RuntimeError::new(
                    "__supervisor_spawn could not infer worker process from init callable",
                )
            })?;
        Ok(RuntimeContinuation::Spawn {
            process_name,
            supervisor: Some(supervisor),
            after: None,
        }
        .callback(init, Vec::new()))
    }

    pub(crate) fn start_supervisor_workers(
        &mut self,
        supervisor: String,
        process_name: String,
        init: Callable,
        strategy_value: Value,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        let strategy = match self.decode_worker_strategy(&strategy_value) {
            Ok(strategy) => strategy,
            Err(WorkerStrategyDecodeError::Internal(message)) => {
                return Err(RuntimeError::new(message))
            }
            Err(WorkerStrategyDecodeError::OutOfRange { field, value }) => {
                return self.language_error_outcome(
                    "WorkerStrategyFieldOutOfRange",
                    vec![Value::Str(field), Value::Int(value)],
                    Some(1),
                )
            }
        };
        let target = strategy.target();
        if strategy.init != target {
            return self.language_error_outcome(
                "WorkerStrategyInitTargetMismatch",
                vec![Value::Int(int(strategy.init)), Value::Int(int(target))],
                Some(1),
            );
        }
        if strategy.min < 0 || strategy.min > target || target > strategy.max {
            return self.language_error_outcome(
                "WorkerStrategyBoundsInvalid",
                vec![
                    Value::Int(int(strategy.min)),
                    Value::Int(int(target)),
                    Value::Int(int(strategy.max)),
                ],
                Some(1),
            );
        }
        Ok(RuntimeContinuation::Workers {
            supervisor,
            process_name,
            init,
            strategy,
            members: Vec::new(),
            receiving: false,
        }
        .pending())
    }

    pub(crate) fn start_workers_submit(
        &mut self,
        handle: &WorkersHandle,
        message: Callable,
        timeout_ms: Option<u64>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        let pid = self.next_workers_pid(handle)?;
        match timeout_ms {
            Some(timeout) => self.start_timed_callback(
                message,
                vec![Value::Pid(pid)],
                timeout,
                RuntimeContinuation::Identity,
            ),
            None => Ok(RuntimeContinuation::Identity.callback(message, vec![Value::Pid(pid)])),
        }
    }

    pub(crate) fn start_workers_broadcast(
        &mut self,
        handle: &WorkersHandle,
        message: Callable,
        timeout_ms: Option<u64>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        let state = self
            .process_runtime
            .worker_sets
            .get(&handle.id)
            .ok_or_else(|| {
                RuntimeError::new(format!(
                    "unknown workers handle {} for {}",
                    handle.id, handle.process_name
                ))
            })?;
        let members = state
            .members
            .iter()
            .map(|id| {
                let process = self.process_runtime.processes.get(id).ok_or_else(|| {
                    RuntimeError::new(format!("worker {id} has no process state"))
                })?;
                let spec = self
                    .process_runtime
                    .spec_for_id(process.spec_id)
                    .ok_or_else(|| RuntimeError::new(format!("worker {id} has no process spec")))?;
                Ok(PidHandle {
                    id: *id,
                    process_name: spec.type_name.clone(),
                })
            })
            .collect::<Result<VecDeque<_>, RuntimeError>>()?;
        Ok(RuntimeContinuation::Broadcast {
            members,
            message,
            timeout_ms,
            results: Vec::new(),
            receiving: false,
        }
        .pending())
    }
}

// Rust API tests drive the same continuation entry points as bytecode dispatch.
#[cfg(test)]
impl VM {
    pub(super) fn process_spawn(
        &mut self,
        name: String,
        init: Callable,
    ) -> Result<Value, RuntimeError> {
        let outcome = self.start_process_spawn(name, init)?;
        self.drive_builtin_outcome(outcome)
    }
    pub(super) fn invoke_task(
        &mut self,
        callable: Callable,
        mode: TaskMode,
    ) -> Result<Value, RuntimeError> {
        self.invoke_task_with_timeout(callable, mode, None)
    }
    pub(super) fn invoke_task_with_timeout(
        &mut self,
        callable: Callable,
        mode: TaskMode,
        timeout_ms: Option<u64>,
    ) -> Result<Value, RuntimeError> {
        let outcome = self.start_task(callable, mode, timeout_ms)?;
        self.drive_builtin_outcome(outcome)
    }
    pub(super) fn await_task_handle(
        &mut self,
        value: &Value,
        timeout_ms: Option<u64>,
    ) -> Result<Value, RuntimeError> {
        let outcome = self.start_await_task(value, timeout_ms)?;
        self.drive_builtin_outcome(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::tests::{
        base_bytecode, function_entry, install_process_error_definitions, singleton_boot_bytecode,
        test_runtime_process_spec,
    };
    use sindr::ir::RuntimeProcessKind;

    #[test]
    fn foreground_future_wait_drives_runnable_process_and_propagates_failure() {
        for driver in ["wait", "pump", "drain"] {
            for fail in [false, true] {
                let mut bytecode = base_bytecode(vec![Opcode::Halt]);
                bytecode.runtime_process_specs.entries = vec![test_runtime_process_spec(
                    0,
                    "Worker",
                    RuntimeProcessKind::Agent,
                    RuntimeProcessInstance::Worker,
                    false,
                    0,
                    0,
                    None,
                )];
                let mut vm = VM::new(bytecode);
                let future = vm.process_runtime.allocate_future(None, None, false);
                let pid = vm
                    .allocate_process_instance("Worker".into(), Some(Value::Unit), None, None)
                    .unwrap();
                let mut context = VM::standalone_builtin_context(BuiltinOutcome::Resume(
                    BuiltinContinuation::Runtime(RuntimeContinuation::TaskCall {
                        completion: future,
                        previous_trace: None,
                    }),
                ));
                if fail {
                    VM::cancel_execution_context(
                        &mut context,
                        RuntimeError::new("process failure during wait"),
                    );
                }
                vm.process_runtime
                    .processes
                    .get_mut(&pid)
                    .unwrap()
                    .execution_context = Some(context);
                vm.process_runtime.enqueue_runnable(pid);
                let result = match driver {
                    "wait" => vm.wait_for_any_future(&[future]),
                    "pump" => vm.pump_background_ready(),
                    "drain" => vm.drain_background_tasks(),
                    _ => unreachable!(),
                };
                if fail {
                    assert!(result
                        .unwrap_err()
                        .message
                        .contains("process failure during wait"));
                } else {
                    result.unwrap();
                    assert_eq!(vm.ready_future_value(future), Some(Value::Unit));
                }
                assert_eq!(
                    vm.frames.len(),
                    1,
                    "foreground frame must survive process dispatch"
                );
            }
        }
    }

    #[test]
    fn cleanup_visibility_rewakes_process_that_rewaited_after_timeout() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt, Opcode::LoadLocal(0), Opcode::Return]);
        bytecode.functions = vec![function_entry(0, 1, 1, 1, Some("Worker::await"))];
        bytecode.runtime_process_specs.entries = vec![test_runtime_process_spec(
            0,
            "Worker",
            RuntimeProcessKind::Agent,
            RuntimeProcessInstance::Worker,
            false,
            0,
            0,
            None,
        )];
        bytecode.type_registry.register(sindr::runtime::TypeEntry {
            tag: 2,
            name: "Duration".into(),
            kind: sindr::runtime::TypeKind::Struct,
            field_names: vec!["millis".into()],
            private_flags: vec![true],
        });
        let mut vm = VM::new(bytecode);
        install_process_error_definitions(&mut vm);
        let mut body = Callable {
            target: CallableTarget::Builtin(
                sindr::builtin::builtin_id_by_name("__process_sleep").unwrap(),
            ),
            lexical_captures: vec![Value::Tagged {
                tag: 2,
                fields: vec![Value::Int(int(5000))],
            }],
            metadata: CallableMetadata::default(),
        };
        for _ in 0..300 {
            body = Callable {
                target: CallableTarget::Builtin(
                    sindr::builtin::builtin_id_by_name("__task_call").unwrap(),
                ),
                lexical_captures: vec![Value::Callable(body)],
                metadata: CallableMetadata::default(),
            };
        }
        let completion = vm.process_runtime.allocate_future(None, None, true);
        vm.stamp_future_context(completion).unwrap();
        let task_id = vm
            .schedule_callback(
                body,
                Vec::new(),
                None,
                DetachedCompletion::Future(Some(completion)),
            )
            .unwrap();
        for _ in 0..20 {
            vm.drive_ready_detached_tasks().unwrap();
            if matches!(
                vm.process_runtime.detached_tasks[&task_id].state,
                DetachedTaskState::Waiting { .. }
            ) {
                break;
            }
        }
        assert!(matches!(
            vm.process_runtime.detached_tasks[&task_id].state,
            DetachedTaskState::Waiting { .. }
        ));
        let pid = vm
            .allocate_process_instance("Worker".into(), Some(Value::Unit), None, None)
            .unwrap();
        let context = vm
            .prepare_callable_context(
                vm.callable_for_function(0),
                vec![Value::PendingFuture(completion)],
            )
            .unwrap();
        vm.process_runtime
            .processes
            .get_mut(&pid)
            .unwrap()
            .execution_context = Some(context);
        vm.process_runtime
            .mark_process_waiting(pid, ProcessWaitReason::Future(completion));
        vm.process_runtime.attach_future_deadline(
            completion,
            vm.process_runtime.current_tick_ms,
            0,
            true,
        );
        vm.expire_process_deadlines(vm.process_runtime.current_tick_ms);
        vm.drive_ready_detached_tasks().unwrap();
        assert!(vm.raw_ready_future_value(completion).is_some());
        assert!(
            vm.ready_future_value(completion).is_none(),
            "cleanup must still own the completion"
        );
        assert!(
            matches!(vm.scheduler_tick(256).unwrap(), Some(ProcessRunOutcome::Pending(id)) if id == completion)
        );
        for _ in 0..20 {
            vm.drive_ready_detached_tasks().unwrap();
            if vm.ready_future_value(completion).is_some() {
                break;
            }
        }
        assert!(vm.ready_future_value(completion).is_some());
        assert_eq!(
            vm.process_runtime
                .run_queue
                .iter()
                .filter(|queued| **queued == pid)
                .count(),
            1
        );
        assert!(matches!(
            vm.scheduler_tick(256).unwrap(),
            Some(ProcessRunOutcome::Halted(Value::Tagged { tag: 1, .. }))
        ));
    }

    #[test]
    fn async_nested_call_runs_prefix_before_returning_handle() {
        let builtin = |name: &str, lexical_captures| Callable {
            target: CallableTarget::Builtin(sindr::builtin::builtin_id_by_name(name).unwrap()),
            lexical_captures,
            metadata: CallableMetadata::default(),
        };
        for timed in [false, true] {
            let print = builtin("print", vec![Value::Str("before handle".into())]);
            let nested_call = if timed {
                builtin(
                    "__task_call_timeout",
                    vec![
                        Value::Tagged {
                            tag: 2,
                            fields: vec![Value::Int(int(1000))],
                        },
                        Value::Callable(print),
                    ],
                )
            } else {
                builtin("__task_call", vec![Value::Callable(print)])
            };
            let mut bytecode = base_bytecode(vec![Opcode::Halt]);
            bytecode.type_registry.register(sindr::runtime::TypeEntry {
                tag: 2,
                name: "Duration".into(),
                kind: sindr::runtime::TypeKind::Struct,
                field_names: vec!["millis".into()],
                private_flags: vec![true],
            });
            let mut vm = VM::new(bytecode).with_output_capture();
            install_process_error_definitions(&mut vm);
            let start = vm.start_task(nested_call, TaskMode::Async, None).unwrap();
            assert!(matches!(
                vm.drive_builtin_outcome(start).unwrap(),
                Value::TaskHandle(_)
            ));
            assert_eq!(
                vm.output,
                Some(vec!["before handle".to_string()]),
                "timed={timed}"
            );
        }
    }

    #[test]
    fn concurrent_singleton_requests_share_one_init_and_release_internal_futures() {
        let bytecode = singleton_boot_bytecode(
            "Counter",
            RuntimeProcessKind::Agent,
            false,
            true,
            vec![
                Opcode::LoadConst(0),
                Opcode::LoadConst(1),
                Opcode::StructNew { field_count: 1 },
                Opcode::Return,
            ],
            vec![Constant::Tag(0), Constant::Int(int(42))],
        );
        let mut vm = VM::new(bytecode);
        install_process_error_definitions(&mut vm);
        let first = vm.start_singleton("Counter".into(), None).unwrap();
        let second = vm.start_singleton("Counter".into(), None).unwrap();
        assert_eq!(vm.process_runtime.singleton_inits.len(), 1);
        let first_pid = vm.drive_builtin_outcome(first).unwrap();
        let second_pid = vm.drive_builtin_outcome(second).unwrap();
        assert_eq!(first_pid, second_pid);
        assert_eq!(vm.process_runtime.processes.len(), 1);
        assert!(vm.process_runtime.singleton_inits.is_empty());
        assert!(vm.process_runtime.futures.is_empty());
    }

    #[test]
    fn simultaneous_worker_refills_keep_exact_target() {
        let mut bytecode = base_bytecode(vec![
            Opcode::Halt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::StructNew { field_count: 1 },
            Opcode::Return,
        ]);
        bytecode.constants = vec![Constant::Tag(0), Constant::Int(int(0))];
        bytecode.functions = vec![function_entry(0, 1, 0, 0, Some("Worker::init"))];
        bytecode.runtime_process_specs.entries = vec![test_runtime_process_spec(
            0,
            "Worker",
            RuntimeProcessKind::Agent,
            RuntimeProcessInstance::Worker,
            false,
            0,
            0,
            None,
        )];
        let mut vm = VM::new(bytecode);
        vm.process_runtime.worker_sets.insert(
            1,
            WorkerSetState {
                supervisor_name: "DynamicSupervisor".into(),
                worker_process: "Worker".into(),
                init_callable: vm.callable_for_function(0),
                strategy: WorkerStrategyState {
                    init: 2,
                    min: 0,
                    max: 2,
                    scale: WorkerScaleState::Fix(2),
                },
                target: 2,
                members: Vec::new(),
                next_index: 0,
            },
        );
        let caller = vm.take_execution_context(vm.pc, ExecutionTarget::TopLevel);
        vm.refill_worker_set(1);
        vm.refill_worker_set(1);
        assert_eq!(vm.process_runtime.detached_tasks.len(), 1);
        vm.restore_execution_context(caller);
        vm.drain_background_tasks().unwrap();
        assert_eq!(vm.process_runtime.worker_sets[&1].members.len(), 2);
        assert_eq!(vm.process_runtime.processes.len(), 2);
        assert!(vm.process_runtime.refilling_worker_sets.is_empty());
    }

    #[test]
    fn cpu_task_timeout_cleans_before_future_delivery() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt, Opcode::Jump(1)]);
        bytecode.functions = vec![function_entry(0, 1, 0, 0, Some("Task::spin"))];
        let mut vm = VM::new(bytecode);
        install_process_error_definitions(&mut vm);
        let call = vm
            .start_task(vm.callable_for_function(0), TaskMode::Async, Some(100))
            .unwrap();
        let Value::TaskHandle(future) = vm.drive_builtin_outcome(call).unwrap() else {
            panic!("task handle")
        };
        assert!(vm.has_runnable_detached_tasks());
        vm.runtime_clock_anchor = Instant::now() - Duration::from_millis(200);
        vm.drive_ready_detached_tasks().unwrap();
        assert!(matches!(
            vm.ready_future_value(future),
            Some(Value::Tagged { tag: 1, .. })
        ));
        assert!(vm.process_runtime.detached_tasks.is_empty());
    }

    #[test]
    fn new_timeout_does_not_include_idle_time_before_start() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        vm.runtime_clock_anchor = Instant::now() - Duration::from_millis(1000);
        let id = vm.process_sleep(100).unwrap();
        let Value::PendingFuture(id) = id else {
            panic!("sleep future")
        };
        let now = vm.process_runtime.current_tick_ms;
        assert!(now >= 1000);
        assert_eq!(
            vm.process_runtime.futures[&id].deadline_tick,
            Some(now + 100)
        );
        assert!(vm.ready_future_value(id).is_none());
    }
}
