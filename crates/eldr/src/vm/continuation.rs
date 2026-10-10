use super::*;
use crate::builtin::{BuiltinContinuation, BuiltinOutcome};

#[derive(Debug, Clone)]
pub(super) enum Invocation {
    Callable(Callable, Vec<Value>),
    Builtin(BuiltinOutcome),
}

#[derive(Debug, Clone)]
enum ReturnDestination {
    Stack {
        tail: bool,
        previous_site: Option<(u32, u32)>,
        previous_trace: Option<RuntimeStackFrame>,
    },
    Builtin(BuiltinContinuation),
}

#[derive(Debug, Clone)]
pub(super) struct ContinuationFrame {
    destination: ReturnDestination,
    frame_depth: usize,
    stack_base: usize,
    resume_pc: usize,
    origin_pc: Option<usize>,
    breadcrumb_base: usize,
    waiting_function: bool,
    waiting_future: Option<FutureId>,
}

impl VM {
    /// A scheduler-origin job has an explicit root, independent of any suspended caller.
    pub(super) fn standalone_builtin_context(outcome: BuiltinOutcome) -> ExecutionContext {
        ExecutionContext {
            stack: Vec::new(),
            frames: vec![CallFrame {
                return_pc: 0,
                stack_base: 0,
                call_site: None,
                trace_frame: None,
                tail_call_breadcrumb_base_len: 0,
                locals: Vec::new(),
            }],
            tail_call_breadcrumbs: VecDeque::new(),
            continuations: vec![ContinuationFrame {
                destination: ReturnDestination::Stack {
                    tail: false,
                    previous_site: None,
                    previous_trace: None,
                },
                frame_depth: 1,
                stack_base: 0,
                resume_pc: 0,
                origin_pc: None,
                breadcrumb_base: 0,
                waiting_function: false,
                waiting_future: None,
            }],
            pending_invocation: Some(Ok(Invocation::Builtin(outcome))),
            cancellation: None,
            pc: 0,
            target: ExecutionTarget::FrameDepth(1),
        }
    }

    pub(super) fn cancel_execution_context(context: &mut ExecutionContext, error: RuntimeError) {
        // A just-started builtin can already own a resource before its callback is dispatched.
        // Attach that pending cleanup before replacing the work with error delivery.
        if let Some(Ok(Invocation::Builtin(outcome))) = context.pending_invocation.take() {
            let continuation = match outcome {
                BuiltinOutcome::Call { continuation, .. }
                | BuiltinOutcome::Wait { continuation, .. }
                | BuiltinOutcome::Resume(continuation) => Some(continuation),
                BuiltinOutcome::Complete(_) => None,
            };
            if let Some(continuation) = continuation {
                context.continuations.push(ContinuationFrame {
                    destination: ReturnDestination::Builtin(continuation),
                    frame_depth: context.frames.len(),
                    stack_base: context.stack.len(),
                    resume_pc: context.pc,
                    origin_pc: context.continuations.last().and_then(|c| c.origin_pc),
                    breadcrumb_base: context.tail_call_breadcrumbs.len(),
                    waiting_function: false,
                    waiting_future: None,
                });
            }
        }
        context.cancellation = Some(error.clone());
        context.pending_invocation = Some(Err(error));
    }

    pub(super) fn schedule_callable(
        &mut self,
        callable: Callable,
        args: Vec<Value>,
        resume_pc: usize,
        tail: bool,
        site: Option<(u32, u32)>,
        trace: Option<RuntimeStackFrame>,
    ) -> Result<(), RuntimeError> {
        self.schedule_invocation(
            Invocation::Callable(callable, args),
            resume_pc,
            tail,
            site,
            trace,
        )
    }

    pub(super) fn schedule_invocation(
        &mut self,
        invocation: Invocation,
        resume_pc: usize,
        tail: bool,
        site: Option<(u32, u32)>,
        trace: Option<RuntimeStackFrame>,
    ) -> Result<(), RuntimeError> {
        let frame = self.current_frame_mut()?;
        let previous_site = frame.call_site;
        let previous_trace = frame.trace_frame.clone();
        frame.call_site = site;
        frame.trace_frame = trace;
        self.continuations.push(ContinuationFrame {
            destination: ReturnDestination::Stack {
                tail,
                previous_site,
                previous_trace,
            },
            frame_depth: self.frames.len(),
            stack_base: self.stack.len(),
            resume_pc,
            origin_pc: Some(self.pc),
            breadcrumb_base: self.tail_call_breadcrumbs.len(),
            waiting_function: false,
            waiting_future: None,
        });
        self.pending_invocation = Some(Ok(invocation));
        Ok(())
    }

    pub(super) fn error_construction_source(&self) -> Option<(u32, u32)> {
        self.continuations
            .iter()
            .rev()
            .find_map(|frame| match &frame.destination {
                ReturnDestination::Builtin(BuiltinContinuation::Runtime(
                    RuntimeContinuation::ErrorConstruction { location, .. },
                )) => Some((location.span_start, location.span_end)),
                _ => None,
            })
    }

    pub(super) fn callable_invocation(
        &mut self,
        callable: Callable,
        args: Vec<Value>,
    ) -> Result<(), RuntimeError> {
        let Callable {
            target,
            lexical_captures,
            ..
        } = callable;
        match target {
            CallableTarget::Builtin(id) => {
                let mut full = lexical_captures;
                full.extend(args);
                self.pending_invocation =
                    Some(call_builtin(self, id, full).map(Invocation::Builtin));
            }
            CallableTarget::Function(id) => {
                let entry = self.function_entry(id)?.clone();
                let mut full = lexical_captures;
                full.extend(args);
                if full.len() != entry.arity as usize {
                    return Err(RuntimeError::new(format!(
                        "Call arity mismatch for function {}: expected {}, got {}",
                        id,
                        entry.arity,
                        full.len()
                    )));
                }
                if entry.entry_pc as usize >= self.bytecode.opcodes.len() {
                    return Err(RuntimeError::new(format!(
                        "Function {} entry_pc out of bounds: {}",
                        id, entry.entry_pc
                    )));
                }
                let locals = Self::build_locals_for_call(&entry, full)?;
                let call_site = self.current_frame()?.call_site;
                let trace_frame = call_site.map(|(a, b)| {
                    self.trace_frame_for_call(&entry, RuntimeCallKind::ClosureFunction, a, b, false)
                });
                let continuation = self
                    .continuations
                    .last_mut()
                    .ok_or_else(|| RuntimeError::new("callable has no return destination"))?;
                continuation.waiting_function = true;
                self.frames.push(CallFrame {
                    return_pc: continuation.resume_pc,
                    stack_base: self.stack.len(),
                    call_site,
                    trace_frame,
                    tail_call_breadcrumb_base_len: self.tail_call_breadcrumbs.len(),
                    locals,
                });
                self.pc = entry.entry_pc as usize;
            }
            CallableTarget::Template(id) => {
                let template = self.callable_template(id)?.clone();
                let action = match template.kind {
                    CallableTemplateKind::PartialDirectCall {
                        target,
                        arg_sources,
                    } => {
                        let full = arg_sources
                            .into_iter()
                            .map(|source| match source {
                                CallableTemplateArg::Bound(i) => {
                                    lexical_captures.get(i as usize).cloned().ok_or_else(|| {
                                        RuntimeError::new(format!(
                                            "Callable template {} bound arg out of bounds: {}",
                                            id, i
                                        ))
                                    })
                                }
                                CallableTemplateArg::Runtime(i) => {
                                    args.get(i as usize).cloned().ok_or_else(|| {
                                        RuntimeError::new(format!(
                                            "Callable template {} runtime arg out of bounds: {}",
                                            id, i
                                        ))
                                    })
                                }
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        Invocation::Callable(self.direct_callable(target), full)
                    }
                    CallableTemplateKind::InjectDirectCall {
                        target,
                        bound_arg_count,
                    } => {
                        let (first, rest) = args.split_first().ok_or_else(|| {
                            RuntimeError::new(format!(
                                "Callable template {} requires at least one runtime argument",
                                id
                            ))
                        })?;
                        if lexical_captures.len() < bound_arg_count as usize {
                            return Err(RuntimeError::new(
                                "Callable template bound arg out of bounds",
                            ));
                        }
                        let mut full = vec![first.clone()];
                        full.extend(lexical_captures.into_iter().take(bound_arg_count as usize));
                        full.extend(rest.iter().cloned());
                        Invocation::Callable(self.direct_callable(target), full)
                    }
                    CallableTemplateKind::ComposeDirect {
                        flavor: CallableTemplateComposeFlavor::Plain,
                    } => {
                        if args.len() != 1 {
                            return Err(RuntimeError::new(format!(
                                "Callable template {} requires one runtime argument",
                                id
                            )));
                        }
                        let lhs = match lexical_captures.first() {
                            Some(Value::Callable(c)) => c.clone(),
                            _ => {
                                return Err(RuntimeError::new(format!(
                                    "Callable template {} expects lhs callable capture",
                                    id
                                )))
                            }
                        };
                        let rhs = match lexical_captures.get(1) {
                            Some(Value::Callable(c)) => c.clone(),
                            _ => {
                                return Err(RuntimeError::new(format!(
                                    "Callable template {} expects rhs callable capture",
                                    id
                                )))
                            }
                        };
                        Invocation::Builtin(BuiltinOutcome::Call {
                            callable: lhs,
                            args,
                            continuation: BuiltinContinuation::Compose { rhs },
                        })
                    }
                };
                self.pending_invocation = Some(Ok(action));
            }
        }
        Ok(())
    }

    pub(super) fn direct_callable(&self, target: CallableTemplateDirectTarget) -> Callable {
        match target {
            CallableTemplateDirectTarget::Function(id) => self.callable_for_function(id),
            CallableTemplateDirectTarget::Builtin(id) => Callable {
                target: CallableTarget::Builtin(id),
                lexical_captures: Vec::new(),
                metadata: Default::default(),
            },
        }
    }

    pub(super) fn continuation_ready(&self) -> bool {
        self.pending_invocation.is_some()
            || self.continuations.last().is_some_and(|c| {
                c.waiting_future.is_some()
                    || c.waiting_function && self.frames.len() == c.frame_depth
            })
    }

    pub(super) fn step_continuation(&mut self, target: ExecutionTarget) -> StepOutcome {
        if self.pending_invocation.is_none() {
            let Some(frame) = self.continuations.last_mut() else {
                return StepOutcome::RuntimeError(RuntimeError::new(
                    "missing continuation return destination",
                ));
            };
            if let Some(id) = frame.waiting_future {
                if let Some(value) = self.ready_future_value(id) {
                    if let Some(frame) = self.continuations.last_mut() {
                        frame.waiting_future = None;
                    } else {
                        return StepOutcome::RuntimeError(RuntimeError::new(
                            "missing future return destination",
                        ));
                    }
                    self.pending_invocation =
                        Some(Ok(Invocation::Builtin(BuiltinOutcome::Complete(value))));
                } else {
                    return StepOutcome::Pending {
                        future_id: id,
                        resume: self.take_execution_context(self.pc, target),
                    };
                }
            } else {
                frame.waiting_function = false;
                self.pending_invocation = Some(
                    self.pop_stack()
                        .map(|v| Invocation::Builtin(BuiltinOutcome::Complete(v))),
                );
            }
        }
        let Some(invocation) = self.pending_invocation.take() else {
            return StepOutcome::RuntimeError(RuntimeError::new("missing pending invocation"));
        };
        let result = match invocation {
            Ok(Invocation::Callable(callable, args)) => self.callable_invocation(callable, args),
            Ok(Invocation::Builtin(BuiltinOutcome::Call {
                callable,
                args,
                continuation,
            })) => {
                self.push_builtin_continuation(continuation);
                self.pending_invocation = Some(Ok(Invocation::Callable(callable, args)));
                Ok(())
            }
            Ok(Invocation::Builtin(BuiltinOutcome::Resume(continuation))) => {
                self.pending_invocation = Some(
                    continuation
                        .resume(self, Ok(Value::Unit))
                        .map(Invocation::Builtin),
                );
                Ok(())
            }
            Ok(Invocation::Builtin(BuiltinOutcome::Wait {
                future_id,
                continuation,
            })) => {
                self.push_builtin_continuation(continuation);
                let Some(frame) = self.continuations.last_mut() else {
                    return StepOutcome::RuntimeError(RuntimeError::new(
                        "missing future return destination",
                    ));
                };
                frame.waiting_future = Some(future_id);
                Ok(())
            }
            Ok(Invocation::Builtin(BuiltinOutcome::Complete(Value::PendingFuture(id)))) => {
                let Some(frame) = self.continuations.last_mut() else {
                    return StepOutcome::RuntimeError(RuntimeError::new(
                        "missing future return destination",
                    ));
                };
                frame.waiting_future = Some(id);
                Ok(())
            }
            Ok(Invocation::Builtin(BuiltinOutcome::Complete(value))) => {
                self.deliver_continuation(Ok(value))
            }
            Err(error) => {
                if self.continuations.is_empty() {
                    return StepOutcome::RuntimeError(error);
                }
                self.deliver_continuation(Err(error))
            }
        };
        if let Err(error) = result {
            if self.continuations.is_empty() {
                return StepOutcome::RuntimeError(error);
            }
            self.pending_invocation = Some(Err(self.enrich_continuation_error(error)));
        }
        // Callback completion must be delivered before the enclosing FrameDepth can halt.
        if self.continuations.is_empty() && self.pending_invocation.is_none() {
            match self.complete_execution_target(&target) {
                Ok(Some(value)) => return StepOutcome::Halt(value),
                Err(error) => return StepOutcome::RuntimeError(error),
                _ => {}
            }
        }
        StepOutcome::Continue
    }

    pub(super) fn push_builtin_continuation(&mut self, continuation: BuiltinContinuation) {
        self.continuations.push(ContinuationFrame {
            destination: ReturnDestination::Builtin(continuation),
            frame_depth: self.frames.len(),
            stack_base: self.stack.len(),
            resume_pc: self.pc,
            origin_pc: self.continuations.last().and_then(|c| c.origin_pc),
            breadcrumb_base: self.tail_call_breadcrumbs.len(),
            waiting_function: false,
            waiting_future: None,
        });
    }

    pub(super) fn enrich_continuation_error(&self, error: RuntimeError) -> RuntimeError {
        let origin = self
            .continuations
            .last()
            .and_then(|c| c.origin_pc)
            .and_then(|pc| self.bytecode.opcodes.get(pc).map(|op| (pc, op)));
        if let Some((pc, op)) = origin {
            self.enrich_runtime_error(error, pc, op)
        } else {
            let mut error = error;
            if error.context.call_site.is_none() {
                error.context.call_site = self.runtime_error_location();
            }
            if error.context.stack_trace.is_empty() {
                error.context.stack_trace = self.current_stack_trace_snapshot();
            }
            error
        }
    }

    pub(super) fn deliver_continuation(
        &mut self,
        result: Result<Value, RuntimeError>,
    ) -> Result<(), RuntimeError> {
        let result = match &self.cancellation {
            Some(error) => Err(error.clone()),
            None => result,
        }
        .map_err(|error| self.enrich_continuation_error(error));
        let frame = self
            .continuations
            .pop()
            .ok_or_else(|| RuntimeError::new("missing continuation return destination"))?;
        self.frames.truncate(frame.frame_depth);
        self.tail_call_breadcrumbs.truncate(frame.breadcrumb_base);
        self.stack.truncate(frame.stack_base);
        self.pc = frame.resume_pc;
        match frame.destination {
            ReturnDestination::Builtin(continuation) => {
                let resumed = continuation.resume(self, result).map(Invocation::Builtin);
                self.pending_invocation = Some(match &self.cancellation {
                    Some(error) => Err(error.clone()),
                    None => resumed,
                });
            }
            ReturnDestination::Stack {
                tail,
                previous_site,
                previous_trace,
            } => {
                let caller = self.current_frame_mut()?;
                caller.call_site = previous_site;
                caller.trace_frame = previous_trace;
                let value = result?;
                if tail {
                    let mut pc = self.pc;
                    self.return_from_current_frame(value, &mut pc)?;
                    self.pc = pc;
                } else {
                    self.stack.push(value);
                }
            }
        }
        Ok(())
    }
}
