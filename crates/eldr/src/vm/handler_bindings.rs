use super::*;
use std::cell::Cell;
use std::io::{self, Write};

pub(super) type BindingContextId = u64;

#[derive(Debug, Default)]
pub(super) struct HandlerIds {
    binding: Cell<u64>,
    context: Cell<u64>,
    request: Cell<u64>,
}

impl HandlerIds {
    fn next(counter: &Cell<u64>) -> Result<u64, RuntimeError> {
        let next = counter
            .get()
            .checked_add(1)
            .ok_or_else(|| RuntimeError::new("handler identity counter overflow"))?;
        counter.set(next);
        Ok(next)
    }
    fn binding(&self) -> Result<u64, RuntimeError> {
        Self::next(&self.binding)
    }
    fn context(&self) -> Result<u64, RuntimeError> {
        Self::next(&self.context)
    }
    pub(super) fn request(&self) -> Result<u64, RuntimeError> {
        Self::next(&self.request)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum HandlerTarget {
    StdOut,
    StdErr,
    StdIn,
    Null,
    File(PathBuf),
}

impl HandlerTarget {
    fn name(&self) -> String {
        match self {
            Self::StdOut => "StdOut".into(),
            Self::StdErr => "StdErr".into(),
            Self::StdIn => "StdIn".into(),
            Self::Null => "NullOutHandler".into(),
            Self::File(path) => format!("FileOutHandler(path={})", path.display()),
        }
    }
}

#[derive(Debug)]
pub(super) enum AcquisitionError {
    Metadata(RuntimeError),
    Open { path: String, detail: String },
}

impl AcquisitionError {
    fn cleanup(&mut self, error: RuntimeError) {
        match self {
            Self::Metadata(original) => {
                original.retain_secondary_failure(&error, "handler cleanup")
            }
            Self::Open { detail, .. } => detail.push_str(&format!("; handler cleanup: {error}")),
        }
    }
    pub(super) fn detail(&self) -> String {
        match self {
            Self::Metadata(error) => error.to_string(),
            Self::Open { path, detail } => format!("FileOutHandlerOpenFailed({path}): {detail}"),
        }
    }
}

fn resolve_target(
    target: &RuntimeHandlerTarget,
    cwd: &Path,
) -> Result<HandlerTarget, AcquisitionError> {
    if target.name != "FileOutHandler" {
        if !target.named_args.is_empty() {
            return Err(AcquisitionError::Metadata(RuntimeError::new(
                "standard handler metadata takes no arguments",
            )));
        }
        return match target.name.as_str() {
            "StdOut" => Ok(HandlerTarget::StdOut),
            "StdErr" => Ok(HandlerTarget::StdErr),
            "StdIn" => Ok(HandlerTarget::StdIn),
            "NullOutHandler" => Ok(HandlerTarget::Null),
            other => Err(AcquisitionError::Metadata(RuntimeError::new(format!(
                "unknown handler metadata target {other}"
            )))),
        };
    }
    let [arg] = target.named_args.as_slice() else {
        return Err(AcquisitionError::Metadata(RuntimeError::new(
            "FileOutHandler metadata requires exactly the named argument path",
        )));
    };
    if arg.name != "path" {
        return Err(AcquisitionError::Metadata(RuntimeError::new(
            "FileOutHandler metadata requires the named argument path",
        )));
    }
    let requested = Path::new(&arg.value);
    let absolute = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        cwd.join(requested)
    };
    let canonical = match std::fs::symlink_metadata(&absolute) {
        Ok(_) => std::fs::canonicalize(&absolute),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = absolute.parent().ok_or_else(|| AcquisitionError::Open {
                path: arg.value.clone(),
                detail: "file path has no parent".into(),
            })?;
            let leaf = absolute.file_name().ok_or_else(|| AcquisitionError::Open {
                path: arg.value.clone(),
                detail: "file path has no leaf".into(),
            })?;
            std::fs::canonicalize(parent).map(|parent| parent.join(leaf))
        }
        Err(error) => Err(error),
    }
    .map_err(|error| AcquisitionError::Open {
        path: arg.value.clone(),
        detail: error.to_string(),
    })?;
    Ok(HandlerTarget::File(canonical))
}

#[derive(Debug)]
pub(super) struct FileSink {
    pub(super) path: PathBuf,
    file: RefCell<File>,
    active_vms: Cell<usize>,
    #[cfg(test)]
    pub(super) fail_write: Cell<bool>,
    #[cfg(test)]
    pub(super) fail_flush: Cell<bool>,
    #[cfg(test)]
    writes: Cell<usize>,
    #[cfg(test)]
    flushes: Cell<usize>,
}

impl FileSink {
    fn open(path: PathBuf) -> io::Result<Rc<Self>> {
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Rc::new(Self {
            path,
            file: RefCell::new(file),
            active_vms: Cell::new(0),
            #[cfg(test)]
            fail_write: Cell::new(false),
            #[cfg(test)]
            fail_flush: Cell::new(false),
            #[cfg(test)]
            writes: Cell::new(0),
            #[cfg(test)]
            flushes: Cell::new(0),
        }))
    }
    pub(super) fn write(&self, text: &str) -> io::Result<()> {
        #[cfg(test)]
        {
            self.writes.set(self.writes.get() + 1);
            if self.fail_write.get() {
                return Err(io::Error::other("injected FileOut write failure"));
            }
        }
        self.file.borrow_mut().write_all(text.as_bytes())
    }
    fn flush(&self) -> io::Result<()> {
        #[cfg(test)]
        {
            self.flushes.set(self.flushes.get() + 1);
            if self.fail_flush.get() {
                return Err(io::Error::other("injected FileOut flush failure"));
            }
        }
        self.file.borrow_mut().flush()
    }
}

#[derive(Debug)]
pub(super) struct ActiveFileLease {
    pub(super) sink: Rc<FileSink>,
    active: bool,
}

impl ActiveFileLease {
    fn acquire(sink: Rc<FileSink>) -> Self {
        sink.active_vms.set(
            sink.active_vms
                .get()
                .checked_add(1)
                .expect("handler VM lease count overflow"),
        );
        Self { sink, active: true }
    }
    fn release(mut self) -> Result<(), RuntimeError> {
        let count = self
            .sink
            .active_vms
            .get()
            .checked_sub(1)
            .expect("handler VM lease count underflow");
        let result = if count == 0 {
            self.sink.flush()
        } else {
            Ok(())
        };
        self.sink.active_vms.set(count);
        self.active = false;
        result.map_err(|error| {
            let mut failure = RuntimeError::process_lifecycle_failed(format!(
                "FileOutHandler flush failed for {}: {error}",
                self.sink.path.display()
            ));
            failure.context.details.push(format!(
                "file_out_cleanup_path={}",
                self.sink.path.display()
            ));
            failure
        })
    }
}

impl Clone for ActiveFileLease {
    fn clone(&self) -> Self {
        assert!(self.active);
        Self::acquire(Rc::clone(&self.sink))
    }
}

impl Drop for ActiveFileLease {
    fn drop(&mut self) {
        if self.active {
            // Normal VM completion uses release(), which reports failure.
            // An abandoned host VM has no result receiver; retain its failure in stderr.
            let count = self
                .sink
                .active_vms
                .get()
                .checked_sub(1)
                .expect("handler VM lease count underflow");
            self.sink.active_vms.set(count);
            if count == 0 {
                if let Err(error) = self.sink.flush() {
                    eprintln!("FileOutHandler abandoned VM flush failed: {error}");
                }
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct ActiveFileSinks(pub(super) BTreeMap<u64, ActiveFileLease>);

#[derive(Debug, Clone, Default)]
pub(super) struct SavedFileSinks(pub(super) BTreeMap<u64, Rc<FileSink>>);

impl ActiveFileSinks {
    pub(super) fn checkpoint(&self) -> SavedFileSinks {
        SavedFileSinks(
            self.0
                .iter()
                .map(|(&id, lease)| (id, Rc::clone(&lease.sink)))
                .collect(),
        )
    }
    pub(super) fn restore(&mut self, saved: SavedFileSinks) -> Result<(), RuntimeError> {
        let removed: Vec<_> = self
            .0
            .keys()
            .filter(|id| !saved.0.contains_key(id))
            .copied()
            .collect();
        let mut error = None;
        for id in removed {
            merge_failure(&mut error, self.0.remove(&id).unwrap().release());
        }
        for (id, sink) in saved.0 {
            if let Some(existing) = self.0.get(&id) {
                if !Rc::ptr_eq(&existing.sink, &sink) {
                    return Err(RuntimeError::new(
                        "handler sink identity mismatch on rollback",
                    ));
                }
            } else {
                self.0.insert(id, ActiveFileLease::acquire(sink));
            }
        }
        error.map_or(Ok(()), Err)
    }
    pub(super) fn shutdown(&mut self) -> Result<(), RuntimeError> {
        let mut error = None;
        for (_, lease) in std::mem::take(&mut self.0) {
            merge_failure(&mut error, lease.release());
        }
        error.map_or(Ok(()), Err)
    }
}

fn merge_failure(first: &mut Option<RuntimeError>, result: Result<(), RuntimeError>) {
    if let Err(error) = result {
        if let Some(original) = first {
            original.retain_secondary_failure(&error, "handler cleanup");
        } else {
            *first = Some(error);
        }
    }
}

#[derive(Debug, Clone)]
struct HandlerBinding {
    identity: Weak<PidIdentity>,
    target: HandlerTarget,
}

#[derive(Debug, Clone)]
struct BindingContext {
    slots: Rc<BTreeMap<(String, String), PidHandle>>,
    sinks: Vec<u64>,
    references: usize,
}

#[derive(Debug, Clone)]
struct FileWriteRequest {
    sink: u64,
    context: Option<BindingContextId>,
    text: Rc<String>,
    origin: Option<(Location, Vec<RuntimeStackFrame>)>,
    started: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct HandlerState {
    pub(super) deferred_cleanup: RuntimeTable<RuntimeError>,
    requests: RuntimeTable<FileWriteRequest>,
    queues: CheckpointValue<BTreeMap<u64, VecDeque<u64>>>,
    contexts: RuntimeTable<BindingContext>,
    bindings: RuntimeTable<HandlerBinding>,
    by_target: CheckpointValue<BTreeMap<HandlerTarget, PidHandle>>,
    file_contexts: CheckpointValue<BTreeMap<u64, usize>>,
}

impl HandlerState {
    pub(super) fn acquire(
        &mut self,
        name: &str,
        targets: &BTreeMap<String, RuntimeHandlerTarget>,
        captured: &[PidHandle],
        cwd: &Path,
        ids: &HandlerIds,
        active: &mut ActiveFileSinks,
    ) -> Result<Option<BindingContextId>, AcquisitionError> {
        let context_id = ids.context().map_err(AcquisitionError::Metadata)?;
        let dead: Vec<_> = self
            .bindings
            .iter()
            .filter_map(|(id, binding)| (binding.identity.strong_count() == 0).then_some(*id))
            .collect();
        for id in dead {
            self.bindings.remove(&id);
        }
        let mut slots = BTreeMap::new();
        let mut sinks = BTreeSet::new();
        let mut created = Vec::new();
        let acquired = (|| {
            for (slot, target) in targets {
                let key = resolve_target(target, cwd)?;
                let pid = if let Some(pid) = self.by_target.get(&key) {
                    pid.clone()
                } else {
                    let id = ids.binding().map_err(AcquisitionError::Metadata)?;
                    if let HandlerTarget::File(path) = &key {
                        let sink = FileSink::open(path.clone()).map_err(|error| {
                            AcquisitionError::Open {
                                path: target.named_args[0].value.clone(),
                                detail: error.to_string(),
                            }
                        })?;
                        active.0.insert(id, ActiveFileLease::acquire(sink));
                    }
                    let pid = PidHandle::new(id, key.name(), PidKind::Handler);
                    self.bindings.insert(
                        id,
                        HandlerBinding {
                            identity: Rc::downgrade(pid.identity()),
                            target: key.clone(),
                        },
                    );
                    self.by_target.insert(key, pid.clone());
                    created.push(id);
                    pid
                };
                if matches!(
                    self.target(&pid).map_err(AcquisitionError::Metadata)?,
                    HandlerTarget::File(_)
                ) {
                    if !active.0.contains_key(&pid.id) {
                        return Err(AcquisitionError::Metadata(RuntimeError::new(
                            "registered FileOut sink has no active descriptor",
                        )));
                    }
                    sinks.insert(pid.id);
                }
                slots.insert((name.to_owned(), slot.clone()), pid);
            }
            for pid in captured {
                if matches!(
                    self.target(pid).map_err(AcquisitionError::Metadata)?,
                    HandlerTarget::File(_)
                ) {
                    if !active.0.contains_key(&pid.id) {
                        return Err(AcquisitionError::Metadata(RuntimeError::new(
                            "captured FileOut handler binding is closed",
                        )));
                    }
                    sinks.insert(pid.id);
                }
            }
            for sink in &sinks {
                self.file_contexts
                    .get(sink)
                    .copied()
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or_else(|| {
                        AcquisitionError::Metadata(RuntimeError::new(
                            "handler context count overflow",
                        ))
                    })?;
            }
            Ok(())
        })();
        if let Err(mut error) = acquired {
            drop(slots);
            for id in created {
                let binding = self.bindings.take(&id).expect("new handler binding");
                self.by_target.remove(&binding.target);
                if let Some(lease) = active.0.remove(&id) {
                    if let Err(cleanup) = lease.release() {
                        error.cleanup(cleanup);
                    }
                }
            }
            return Err(error);
        }
        if slots.is_empty() && sinks.is_empty() {
            return Ok(None);
        }
        let id = context_id;
        for sink in &sinks {
            *self.file_contexts.entry(*sink).or_default() += 1;
        }
        self.contexts.insert(
            id,
            BindingContext {
                slots: Rc::new(slots),
                sinks: sinks.into_iter().collect(),
                references: 1,
            },
        );
        Ok(Some(id))
    }

    fn remove_request(&mut self, id: u64) -> Result<FileWriteRequest, RuntimeError> {
        let request = self
            .requests
            .take(&id)
            .ok_or_else(|| RuntimeError::new("FileOut request disappeared"))?;
        let queue = self
            .queues
            .get_mut(&request.sink)
            .ok_or_else(|| RuntimeError::new("FileOut request has no FIFO"))?;
        if queue.front() == Some(&id) {
            queue.pop_front();
        } else {
            queue.retain(|queued| *queued != id);
        }
        if queue.is_empty() {
            self.queues.remove(&request.sink);
        }
        Ok(request)
    }

    pub(super) fn derive(
        &mut self,
        origin: Option<BindingContextId>,
        captured: &[PidHandle],
        ids: &HandlerIds,
        active: &ActiveFileSinks,
    ) -> Result<Option<BindingContextId>, RuntimeError> {
        let (slots, mut sinks) = match origin {
            Some(id) => {
                let context = self
                    .contexts
                    .get(&id)
                    .ok_or_else(|| RuntimeError::new("Task origin binding context disappeared"))?;
                (
                    Rc::clone(&context.slots),
                    context.sinks.iter().copied().collect::<BTreeSet<_>>(),
                )
            }
            None => (Rc::new(BTreeMap::new()), BTreeSet::new()),
        };
        for pid in captured {
            if matches!(self.target(pid)?, HandlerTarget::File(_)) {
                if !active.0.contains_key(&pid.id) {
                    return Err(RuntimeError::new(
                        "captured FileOut handler binding is closed",
                    ));
                }
                sinks.insert(pid.id);
            }
        }
        if origin.is_none() && sinks.is_empty() {
            return Ok(None);
        }
        if let Some(id) = origin {
            if self.contexts.get(&id).unwrap().sinks.len() == sinks.len() {
                self.retain(Some(id))?;
                return Ok(Some(id));
            }
        }
        let id = ids.context()?;
        for sink in &sinks {
            let count = self
                .file_contexts
                .get_mut(sink)
                .ok_or_else(|| RuntimeError::new("Task capture has no active handler owner"))?;
            *count = count
                .checked_add(1)
                .ok_or_else(|| RuntimeError::new("handler context count overflow"))?;
        }
        self.contexts.insert(
            id,
            BindingContext {
                slots,
                sinks: sinks.into_iter().collect(),
                references: 1,
            },
        );
        Ok(Some(id))
    }

    pub(super) fn target(&self, pid: &PidHandle) -> Result<&HandlerTarget, RuntimeError> {
        if pid.kind != PidKind::Handler {
            return Err(RuntimeError::new(
                "OutHandler requires a registered handler PID",
            ));
        }
        let binding = self
            .bindings
            .get(&pid.id)
            .ok_or_else(|| RuntimeError::new("unregistered handler PID"))?;
        if !binding.identity.ptr_eq(&Rc::downgrade(pid.identity())) {
            return Err(RuntimeError::new("handler PID identity mismatch"));
        }
        Ok(&binding.target)
    }

    pub(super) fn handler(
        &self,
        context: Option<BindingContextId>,
        name: &str,
        slot: &str,
    ) -> Result<PidHandle, RuntimeError> {
        let context = context
            .and_then(|id| self.contexts.get(&id))
            .ok_or_else(|| RuntimeError::new("ctx handler lookup has no live binding context"))?;
        context
            .slots
            .get(&(name.to_owned(), slot.to_owned()))
            .cloned()
            .ok_or_else(|| {
                RuntimeError::new(format!(
                    "handler slot `{slot}` is not bound for process `{name}`"
                ))
            })
    }

    pub(super) fn retain(&mut self, id: Option<BindingContextId>) -> Result<(), RuntimeError> {
        if let Some(id) = id {
            let context = self
                .contexts
                .get_mut(&id)
                .ok_or_else(|| RuntimeError::new("handler binding context disappeared"))?;
            context.references = context
                .references
                .checked_add(1)
                .ok_or_else(|| RuntimeError::new("handler binding owner count overflow"))?;
        }
        Ok(())
    }

    pub(super) fn release(
        &mut self,
        id: Option<BindingContextId>,
        active: &mut ActiveFileSinks,
    ) -> Result<(), RuntimeError> {
        let Some(id) = id else {
            return Ok(());
        };
        let context = self
            .contexts
            .get_mut(&id)
            .ok_or_else(|| RuntimeError::new("handler binding context disappeared"))?;
        context.references = context
            .references
            .checked_sub(1)
            .ok_or_else(|| RuntimeError::new("handler binding owner count underflow"))?;
        if context.references != 0 {
            return Ok(());
        }
        let context = self.contexts.take(&id).unwrap();
        let mut error = None;
        for sink in &context.sinks {
            let count = self
                .file_contexts
                .get_mut(sink)
                .ok_or_else(|| RuntimeError::new("handler sink owner count disappeared"))?;
            *count = count
                .checked_sub(1)
                .ok_or_else(|| RuntimeError::new("handler sink owner count underflow"))?;
            if *count == 0 {
                self.file_contexts.remove(sink);
                let binding = self
                    .bindings
                    .get(sink)
                    .ok_or_else(|| RuntimeError::new("handler sink binding disappeared"))?;
                self.by_target.remove(&binding.target);
                let lease = active
                    .0
                    .remove(sink)
                    .ok_or_else(|| RuntimeError::new("active handler descriptor disappeared"))?;
                merge_failure(&mut error, lease.release());
            }
        }
        drop(context);
        error.map_or(Ok(()), Err)
    }

    pub(super) fn has_file_bindings(&self) -> bool {
        self.bindings.values().any(|binding| {
            matches!(binding.target, HandlerTarget::File(_)) && binding.identity.strong_count() > 0
        })
    }
}

impl VM {
    pub(super) fn accept_file_write(
        &mut self,
        pid: &PidHandle,
        text: String,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        let ids = self
            .handler_ids
            .as_deref()
            .ok_or_else(|| RuntimeError::new("FileOut request has no identity allocator"))?;
        let request = ids.request()?;
        let origin = self
            .runtime_error_location()
            .map(|location| (location, self.current_stack_trace_snapshot()));
        let state = Rc::make_mut(
            self.process_runtime
                .handlers
                .as_mut()
                .ok_or_else(|| RuntimeError::new("FileOut request has no binding registry"))?,
        );
        let context = state.derive(
            None,
            std::slice::from_ref(pid),
            ids,
            &self.active_file_sinks,
        )?;
        state.requests.insert(
            request,
            FileWriteRequest {
                sink: pid.id,
                context,
                text: Rc::new(text),
                origin,
                started: false,
            },
        );
        state.queues.entry(pid.id).or_default().push_back(request);
        Ok(BuiltinOutcome::Resume(
            crate::builtin::BuiltinContinuation::Runtime(RuntimeContinuation::FileWrite {
                request,
            }),
        ))
    }

    pub(super) fn resume_file_write(
        &mut self,
        id: u64,
        incoming: Result<Value, RuntimeError>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        let state = self
            .process_runtime
            .handlers
            .as_ref()
            .ok_or_else(|| RuntimeError::new("FileOut continuation has no registry"))?;
        let request = state
            .requests
            .get(&id)
            .ok_or_else(|| RuntimeError::new("FileOut request resumed after completion"))?;
        if !request.started {
            if let Err(error) = incoming {
                let request = Rc::make_mut(self.process_runtime.handlers.as_mut().unwrap())
                    .remove_request(id)?;
                return Err(merge_handler_cleanup(
                    error,
                    self.release_handler_context(request.context),
                ));
            }
            if state
                .queues
                .get(&request.sink)
                .and_then(|queue| queue.front())
                == Some(&id)
            {
                Rc::make_mut(self.process_runtime.handlers.as_mut().unwrap())
                    .requests
                    .get_mut(&id)
                    .unwrap()
                    .started = true;
            }
            return Ok(BuiltinOutcome::Resume(
                crate::builtin::BuiltinContinuation::Runtime(RuntimeContinuation::FileWrite {
                    request: id,
                }),
            ));
        }
        // OS bytes are committed once. Remove logical queue state before invoking
        // a Surtr Error constructor, which may itself schedule runtime work.
        let request =
            Rc::make_mut(self.process_runtime.handlers.as_mut().unwrap()).remove_request(id)?;
        let sink = self
            .active_file_sinks
            .0
            .get(&request.sink)
            .ok_or_else(|| RuntimeError::new("accepted FileOut request lost descriptor"))?;
        let path = sink.sink.path.display().to_string();
        let written = sink.sink.write(&request.text);
        let cleanup = self.release_handler_context(request.context);
        if let Err(mut original) = incoming {
            if let Err(error) = written {
                original
                    .context
                    .details
                    .push(format!("file_out_cleanup_path={path}"));
                original
                    .context
                    .details
                    .push(format!("FileOutHandlerWriteFailed({path}): {error}"));
            }
            return Err(merge_handler_cleanup(original, cleanup));
        }
        if let Err(error) = written {
            let detail = match cleanup {
                Ok(()) => error.to_string(),
                Err(cleanup) => format!("{error}; handler cleanup: {cleanup}"),
            };
            let (location, trace) = request.origin.ok_or_else(|| {
                RuntimeError::new("FileOut Error generation has no source context")
            })?;
            return self.language_error_outcome_at(
                "FileOutHandlerWriteFailed",
                vec![Value::Str(path), Value::Str(detail)],
                Some(1),
                location,
                trace,
            );
        }
        cleanup?;
        Ok(BuiltinOutcome::Complete(ok_vm_result(Value::Unit)))
    }

    pub(super) fn derive_task_handlers(
        &mut self,
        callable: &Callable,
        args: &[Value],
    ) -> Result<Option<BindingContextId>, RuntimeError> {
        let origin = self.current_handler_bindings;
        let Some(state) = self.process_runtime.handlers.as_deref() else {
            return Ok(None);
        };
        let captured = if state.has_file_bindings() {
            captured_handlers(callable, args)
        } else {
            Vec::new()
        };
        if origin.is_none() && captured.is_empty() {
            return Ok(None);
        }
        let ids = self
            .handler_ids
            .as_deref()
            .ok_or_else(|| RuntimeError::new("handler registry has no identity allocator"))?;
        Rc::make_mut(self.process_runtime.handlers.as_mut().unwrap()).derive(
            origin,
            &captured,
            ids,
            &self.active_file_sinks,
        )
    }
}

fn captured_handlers(callable: &Callable, args: &[Value]) -> Vec<PidHandle> {
    use sindr::runtime::{GeneratorProducer, InfiniteGeneratorProducer};
    let mut pending: Vec<&Value> = callable.lexical_captures.iter().chain(args).collect();
    let mut seen = std::collections::HashSet::new();
    let mut handlers = BTreeMap::new();
    while let Some(value) = pending.pop() {
        if !seen.insert(value as *const Value) {
            continue;
        }
        match value {
            Value::Pid(pid) if pid.kind == PidKind::Handler => {
                handlers.insert(Rc::as_ptr(pid.identity()), pid.clone());
            }
            Value::Tuple(fields) | Value::Tagged { fields, .. } => pending.extend(fields),
            Value::List(list) => pending.extend(list.iter_refs()),
            Value::HashMap(map) => pending.extend(map.entries.values()),
            Value::Callable(callable) => pending.extend(&callable.lexical_captures),
            Value::Generator(generator) => {
                if let GeneratorProducer::Unfold { state, step } = generator.0.as_ref() {
                    pending.push(state);
                    pending.extend(&step.lexical_captures);
                }
            }
            Value::InfiniteGenerator(generator) => {
                let InfiniteGeneratorProducer::Unfold { state, step } = generator.0.as_ref();
                pending.push(state);
                pending.extend(&step.lexical_captures);
            }
            Value::Error(error) => {
                let mut error = error.as_ref();
                loop {
                    pending.extend(&error.payload);
                    if let Some(cause) = &error.cause {
                        error = cause;
                    } else {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    handlers.into_values().collect()
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub(in crate::vm) fn bound_file(vm: &mut VM, path: &Path) -> (BindingContextId, PidHandle) {
        let targets = BTreeMap::from([(
            "out".into(),
            RuntimeHandlerTarget {
                name: "FileOutHandler".into(),
                named_args: vec![sindr::ir::RuntimeHandlerArg {
                    name: "path".into(),
                    value: path.display().to_string(),
                }],
            },
        )]);
        let ids = vm
            .handler_ids
            .get_or_insert_with(|| Rc::new(HandlerIds::default()));
        let state = vm
            .process_runtime
            .handlers
            .get_or_insert_with(|| Rc::new(HandlerState::default()));
        let context = Rc::make_mut(state)
            .acquire(
                "Logger",
                &targets,
                &[],
                &vm.cwd,
                ids,
                &mut vm.active_file_sinks,
            )
            .unwrap()
            .unwrap();
        let pid = state.handler(Some(context), "Logger", "out").unwrap();
        (context, pid)
    }
    #[test]
    fn accepted_file_write_yields_before_external_io() {
        let path = std::env::temp_dir().join(format!(
            "surtr-file-write-budget-{}.log",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut vm = VM::new(super::super::tests::base_bytecode(vec![Opcode::Halt]));
        let (context, pid) = bound_file(&mut vm, &path);
        let outcome = vm.out_handler_write(&pid, "accepted".into()).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "",
            "acceptance must not perform OS write"
        );
        let mut execution = vm.prepare_builtin_context(outcome).unwrap();
        assert!(matches!(
            vm.run_quantum(&mut execution, &mut Budget::new(0)),
            ProcessRunOutcome::QuantumExpired
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        while matches!(
            vm.run_quantum(&mut execution, &mut Budget::new(1)),
            ProcessRunOutcome::QuantumExpired
        ) {}
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "accepted");
        vm.release_handler_context(Some(context)).unwrap();
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::tests::bound_file;
    use super::*;
    fn vm() -> VM {
        VM::new(super::super::tests::base_bytecode(vec![Opcode::Halt]))
    }
    fn path(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("surtr-handler-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("comma,equals=.log")
    }
    fn finish(vm: &mut VM, outcome: BuiltinOutcome) -> Value {
        vm.drive_builtin_outcome(outcome).unwrap()
    }

    #[test]
    fn canonical_aliases_share_and_rename_keeps_original_descriptor() {
        let path = path("aliases");
        let moved = path.with_extension("moved");
        let mut vm = vm();
        let (first, a) = bound_file(&mut vm, &path);
        let alias = path
            .parent()
            .unwrap()
            .join(".")
            .join(path.file_name().unwrap());
        let (second, b) = bound_file(&mut vm, &alias);
        assert!(a.same_identity(&b));
        let sink = Rc::clone(&vm.active_file_sinks.0[&a.id].sink);
        assert_eq!(vm.active_file_sinks.0.len(), 1);
        std::fs::rename(&path, &moved).unwrap();
        vm.release_handler_context(Some(first)).unwrap();
        assert_eq!(sink.flushes.get(), 0);
        let outcome = vm.out_handler_write(&b, "held".into()).unwrap();
        assert_eq!(finish(&mut vm, outcome), ok_vm_result(Value::Unit));
        vm.release_handler_context(Some(second)).unwrap();
        assert_eq!(sink.flushes.get(), 1);
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "held");
        assert!(!path.exists());
        assert!(vm.out_handler_write(&b, "closed".into()).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn saved_lease_reopens_no_path_and_does_not_flush_when_dropped() {
        let path = path("snapshot");
        let moved = path.with_extension("moved");
        let mut vm = vm();
        let (owner, pid) = bound_file(&mut vm, &path);
        let sink = Rc::clone(&vm.active_file_sinks.0[&pid.id].sink);
        let chunk = super::super::checkpoint_collections_tests::chunk(&vm, false);
        let checkpoint = vm.checkpoint_for_chunk(&chunk);
        let discarded = vm.checkpoint_for_chunk(&chunk);
        drop(discarded);
        assert_eq!(sink.flushes.get(), 0);
        let write = vm.out_handler_write(&pid, "before".into()).unwrap();
        finish(&mut vm, write);
        vm.release_handler_context(Some(owner)).unwrap();
        assert_eq!(sink.flushes.get(), 1);
        std::fs::rename(&path, &moved).unwrap();
        vm.rollback_to_checkpoint(checkpoint).unwrap();
        assert!(Rc::ptr_eq(&sink, &vm.active_file_sinks.0[&pid.id].sink));
        let write = vm.out_handler_write(&pid, "after".into()).unwrap();
        finish(&mut vm, write);
        vm.release_handler_context(Some(owner)).unwrap();
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "beforeafter");
        assert!(!path.exists());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn vm_clone_one_owner_end_does_not_flush_or_close_other() {
        let path = path("clone");
        let mut original = vm();
        let (owner, pid) = bound_file(&mut original, &path);
        let sink = Rc::clone(&original.active_file_sinks.0[&pid.id].sink);
        let mut branch = original.clone();
        original.release_handler_context(Some(owner)).unwrap();
        assert_eq!(sink.flushes.get(), 0);
        let outcome = branch.out_handler_write(&pid, "other".into()).unwrap();
        finish(&mut branch, outcome);
        branch.release_handler_context(Some(owner)).unwrap();
        assert_eq!(sink.flushes.get(), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "other");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn fifo_requests_own_sink_and_cancel_unstarted_without_write() {
        let path = path("fifo");
        let mut vm = vm();
        let (owner, pid) = bound_file(&mut vm, &path);
        let sink = Rc::clone(&vm.active_file_sinks.0[&pid.id].sink);
        let first = vm.out_handler_write(&pid, "first".into()).unwrap();
        let second = vm.out_handler_write(&pid, "second".into()).unwrap();
        vm.release_handler_context(Some(owner)).unwrap();
        assert_eq!(sink.flushes.get(), 0);
        let BuiltinOutcome::Resume(crate::builtin::BuiltinContinuation::Runtime(
            RuntimeContinuation::FileWrite { request: second_id },
        )) = second
        else {
            panic!()
        };
        let waiting = vm.resume_file_write(second_id, Ok(Value::Unit)).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        finish(&mut vm, first);
        finish(&mut vm, waiting);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "firstsecond");
        assert_eq!(sink.writes.get(), 2);
        assert_eq!(sink.flushes.get(), 1);
        let (owner, pid) = bound_file(&mut vm, &path);
        let pending = vm.out_handler_write(&pid, "cancelled".into()).unwrap();
        let BuiltinOutcome::Resume(crate::builtin::BuiltinContinuation::Runtime(
            RuntimeContinuation::FileWrite { request },
        )) = pending
        else {
            panic!()
        };
        let error = vm
            .resume_file_write(request, Err(RuntimeError::task_timeout("cancelled")))
            .unwrap_err();
        assert_eq!(error.kind(), crate::error::RuntimeErrorKind::TaskTimeout);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "firstsecond");
        vm.release_handler_context(Some(owner)).unwrap();
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn started_cancel_writes_once_and_keeps_original_failure() {
        let path = path("started");
        let mut vm = vm();
        let (owner, pid) = bound_file(&mut vm, &path);
        let sink = Rc::clone(&vm.active_file_sinks.0[&pid.id].sink);
        let pending = vm.out_handler_write(&pid, "started".into()).unwrap();
        let BuiltinOutcome::Resume(crate::builtin::BuiltinContinuation::Runtime(
            RuntimeContinuation::FileWrite { request },
        )) = pending
        else {
            panic!()
        };
        vm.resume_file_write(request, Ok(Value::Unit)).unwrap();
        vm.release_handler_context(Some(owner)).unwrap();
        sink.fail_flush.set(true);
        let error = vm
            .resume_file_write(request, Err(RuntimeError::call_timeout("caller timeout")))
            .unwrap_err();
        assert_eq!(error.kind(), crate::error::RuntimeErrorKind::CallTimeout);
        assert_eq!(error.message, "caller timeout");
        assert!(error
            .context
            .details
            .iter()
            .any(|detail| detail.contains("flush failed")));
        assert_eq!(sink.writes.get(), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "started");
        assert!(vm.resume_file_write(request, Ok(Value::Unit)).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn language_init_error_survives_last_owner_flush_failure_with_cause() {
        let path = path("errorcause");
        let mut vm = vm();
        super::super::tests::install_process_error_definitions(&mut vm);
        let (owner, pid) = bound_file(&mut vm, &path);
        vm.active_file_sinks.0[&pid.id].sink.fail_flush.set(true);
        let original = RichError::new(
            "UserInitError",
            "original init failed",
            Location {
                file: "input.srt".into(),
                func: "init".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 1,
            },
            None,
        );
        let original_value = err_vm_result(original.clone());
        let cleanup = vm.release_handler_context(Some(owner));
        let outcome = vm
            .finish_handler_result(original_value, cleanup, None)
            .unwrap();
        let Value::Tagged { tag: 1, fields } = finish(&mut vm, outcome) else {
            panic!()
        };
        let [Value::Error(result)] = fields.as_slice() else {
            panic!()
        };
        assert_eq!(result.kind, original.kind);
        assert_eq!(result.message, original.message);
        assert_eq!(result.location, original.location);
        assert!(result
            .cause
            .as_ref()
            .unwrap()
            .visible_message()
            .contains("write failed"));
        assert!(result
            .cause
            .as_ref()
            .unwrap()
            .payload
            .iter()
            .any(|value| matches!(value, Value::Str(detail) if detail.contains("flush failed"))));
        assert!(vm.active_file_sinks.0.is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[cfg(test)]
mod owner_boundary_tests {
    use super::tests::bound_file;
    use super::*;
    fn vm() -> VM {
        VM::new(super::super::tests::base_bytecode(vec![Opcode::Halt]))
    }
    fn path(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "surtr-handler-owner-{label}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("out.log")
    }
    #[test]
    fn partial_acquisition_failure_releases_only_new_sinks() {
        let path = path("partial");
        let new_path = path.with_extension("new");
        let bad_path = path.parent().unwrap().join("missing/out.log");
        let mut vm = vm();
        let (owner, pid) = bound_file(&mut vm, &path);
        let sink = Rc::clone(&vm.active_file_sinks.0[&pid.id].sink);
        let targets = BTreeMap::from([
            ("a".into(), file_target(&path)),
            ("b".into(), file_target(&new_path)),
            ("c".into(), file_target(&bad_path)),
        ]);
        vm.process_runtime
            .handler_contexts
            .insert("Second".into(), targets);
        let error = vm.acquire_process_handlers("Second").unwrap_err();
        assert!(error.message.contains("FileOutHandlerOpenFailed"));
        assert_eq!(vm.active_file_sinks.0.len(), 1);
        assert_eq!(sink.flushes.get(), 0);
        assert!(
            new_path.exists(),
            "created external file is not rolled back"
        );
        let outcome = vm.out_handler_write(&pid, "survives".into()).unwrap();
        vm.drive_builtin_outcome(outcome).unwrap();
        vm.release_handler_context(Some(owner)).unwrap();
        assert_eq!(sink.flushes.get(), 1);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    fn file_target(path: &Path) -> RuntimeHandlerTarget {
        RuntimeHandlerTarget {
            name: "FileOutHandler".into(),
            named_args: vec![sindr::ir::RuntimeHandlerArg {
                name: "path".into(),
                value: path.display().to_string(),
            }],
        }
    }
    #[test]
    fn nested_task_capture_holds_descriptor_and_closed_plain_pid_cannot_reopen() {
        use sindr::runtime::GeneratorHandle;
        let path = path("capture");
        let mut vm = vm();
        let (owner, pid) = bound_file(&mut vm, &path);
        let sink = Rc::clone(&vm.active_file_sinks.0[&pid.id].sink);
        let mut callable = vm.callable_for_function(0);
        let nested = Value::HashMap(sindr::runtime::HashMapHandle {
            entries: std::collections::HashMap::from([(
                "nested".into(),
                Value::List(ListHandle::from_items(vec![Value::Pid(pid.clone())])),
            )]),
        });
        callable.lexical_captures = vec![Value::Generator(GeneratorHandle(std::sync::Arc::new(
            sindr::runtime::GeneratorProducer::Unfold {
                state: nested,
                step: vm.callable_for_function(0),
            },
        )))];
        let task = vm.derive_task_handlers(&callable, &[]).unwrap();
        vm.release_handler_context(Some(owner)).unwrap();
        assert_eq!(sink.flushes.get(), 0);
        let outcome = vm.out_handler_write(&pid, "task".into()).unwrap();
        vm.drive_builtin_outcome(outcome).unwrap();
        vm.release_handler_context(task).unwrap();
        assert_eq!(sink.flushes.get(), 1);
        assert!(vm
            .derive_task_handlers(&callable, &[])
            .unwrap_err()
            .message
            .contains("closed"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "task");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn injected_write_failure_is_language_error_at_accepting_call_site() {
        let path = path("writefailure");
        let mut vm = vm().with_source(
            "OutHandler::write(out, text)".into(),
            "acceptance.srt".into(),
        );
        super::super::tests::install_process_error_definitions(&mut vm);
        let (owner, pid) = bound_file(&mut vm, &path);
        let expected = vm.runtime_error_location().unwrap();
        vm.active_file_sinks.0[&pid.id].sink.fail_write.set(true);
        let outcome = vm.out_handler_write(&pid, "failure".into()).unwrap();
        vm.pc = 0;
        let Value::Tagged { tag: 1, fields } = vm.drive_builtin_outcome(outcome).unwrap() else {
            panic!()
        };
        let [Value::Error(error)] = fields.as_slice() else {
            panic!()
        };
        assert_eq!(
            error.kind,
            compiler_global_error_kind("FileOutHandlerWriteFailed")
        );
        assert_eq!(error.location, expected);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        vm.release_handler_context(Some(owner)).unwrap();
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn rollback_flush_failure_restores_saved_state_before_reporting() {
        let path = path("rollbackfailure");
        let other = path.with_extension("other");
        let mut vm = vm();
        let (owner, pid) = bound_file(&mut vm, &path);
        let chunk = super::super::checkpoint_collections_tests::chunk(&vm, false);
        let checkpoint = vm.checkpoint_for_chunk(&chunk);
        let (_, branch_pid) = bound_file(&mut vm, &other);
        vm.active_file_sinks.0[&branch_pid.id]
            .sink
            .fail_flush
            .set(true);
        let error = vm.rollback_to_checkpoint(checkpoint).unwrap_err();
        assert_eq!(
            error.kind(),
            crate::error::RuntimeErrorKind::ProcessLifecycleFailed
        );
        assert!(vm.active_file_sinks.0.contains_key(&pid.id));
        assert!(!vm.active_file_sinks.0.contains_key(&branch_pid.id));
        let outcome = vm.out_handler_write(&pid, "restored".into()).unwrap();
        vm.drive_builtin_outcome(outcome).unwrap();
        vm.release_handler_context(Some(owner)).unwrap();
        let (new_owner, new_pid) = bound_file(&mut vm, &other);
        assert!(
            new_pid.id > branch_pid.id,
            "rollback must not reuse a failed branch identity"
        );
        vm.release_handler_context(Some(new_owner)).unwrap();
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;
    #[test]
    fn shutdown_reports_flush_failure_and_preserves_existing_vm_failure() {
        for failure in [false, true] {
            let path = std::env::temp_dir().join(format!(
                "surtr-file-shutdown-{}-{failure}.log",
                std::process::id()
            ));
            let mut vm = VM::new(super::super::tests::base_bytecode(if failure {
                vec![
                    Opcode::RuntimeContractViolation {
                        message: "original VM failure".into(),
                    },
                    Opcode::Halt,
                ]
            } else {
                vec![Opcode::Halt]
            }));
            let (_, pid) = super::tests::bound_file(&mut vm, &path);
            let sink = Rc::clone(&vm.active_file_sinks.0[&pid.id].sink);
            sink.fail_flush.set(true);
            let error = vm.run().unwrap_err();
            if failure {
                assert_eq!(error.message, "original VM failure");
                assert!(error
                    .context
                    .details
                    .iter()
                    .any(|detail| detail.contains("handler cleanup")
                        && detail.contains("flush failed")));
            } else {
                assert_eq!(
                    error.kind(),
                    crate::error::RuntimeErrorKind::ProcessLifecycleFailed
                );
            }
            assert_eq!(sink.flushes.get(), 1);
            assert!(vm.active_file_sinks.0.is_empty());
            std::fs::remove_file(path).unwrap();
        }
    }
}

#[cfg(test)]
mod multiple_cleanup_failure_tests {
    use super::*;
    #[test]
    fn existing_vm_failure_keeps_all_sink_flush_failures() {
        let mut vm = VM::new(crate::vm::tests::base_bytecode(vec![
            Opcode::RuntimeContractViolation {
                message: "original VM failure".into(),
            },
            Opcode::Halt,
        ]));
        let directory =
            std::env::temp_dir().join(format!("surtr-multiple-flush-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let first = directory.join("first.log");
        let second = directory.join("second.log");
        for path in [&first, &second] {
            let (_, pid) = super::tests::bound_file(&mut vm, path);
            vm.active_file_sinks.0[&pid.id].sink.fail_flush.set(true);
        }
        let error = vm.run().unwrap_err();
        assert_eq!(error.message, "original VM failure");
        assert_eq!(error.kind(), crate::error::RuntimeErrorKind::Generic);
        assert!(error
            .context
            .details
            .iter()
            .any(|detail| detail.contains("first.log")));
        assert!(
            error
                .context
                .details
                .iter()
                .any(|detail| detail.contains("second.log")),
            "all cleanup failures must survive the original failure"
        );
        assert!(vm.active_file_sinks.0.is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[cfg(test)]
mod canonical_process_binding_tests {
    use super::*;
    #[test]
    fn process_alias_acquisition_and_ctx_lookup_use_same_canonical_name() {
        let canonical = format!("{IMPLICIT_ROOT_NAMESPACE_PREFIX}Worker");
        let mut bytecode = crate::vm::tests::base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs.entries = vec![crate::vm::tests::test_runtime_process_spec(
            0,
            canonical.clone(),
            RuntimeProcessKind::Agent,
            RuntimeProcessInstance::Worker,
            false,
            0,
            0,
            None,
        )];
        let mut vm = VM::new(bytecode);
        vm.process_runtime.handler_contexts.insert(
            canonical.clone(),
            BTreeMap::from([(
                "out".into(),
                RuntimeHandlerTarget {
                    name: "StdOut".into(),
                    named_args: Vec::new(),
                },
            )]),
        );
        let owner = vm.acquire_process_handlers("Worker").unwrap();
        let pid = vm
            .with_handler_context(owner, |vm| {
                vm.process_context_handler("Worker".into(), "out".into())
            })
            .expect("the accepted process alias must use the canonical binding slots");
        let Value::Pid(pid) = pid else { panic!() };
        assert_eq!(pid.process_name, "StdOut");
        vm.release_handler_context(owner).unwrap();
    }
}

#[cfg(test)]
mod init_owner_tests {
    use super::*;
    use crate::vm::tests::{
        base_bytecode, function_entry, install_process_error_definitions, test_runtime_process_spec,
    };
    fn file_target(path: &Path) -> RuntimeHandlerTarget {
        RuntimeHandlerTarget {
            name: "FileOutHandler".into(),
            named_args: vec![sindr::ir::RuntimeHandlerArg {
                name: "path".into(),
                value: path.display().to_string(),
            }],
        }
    }
    fn directory(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("surtr-handler-init-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    #[test]
    fn standby_retry_keeps_same_descriptor_then_transfers_owner_to_process() {
        let mut bytecode = base_bytecode(vec![
            Opcode::Halt,
            Opcode::Jump(2),
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::StructNew { field_count: 0 },
            Opcode::StructNew { field_count: 1 },
            Opcode::Return,
            Opcode::LoadConst(0),
            Opcode::LoadConst(2),
            Opcode::LoadConst(3),
            Opcode::StructNew { field_count: 1 },
            Opcode::StructNew { field_count: 1 },
            Opcode::Return,
        ]);
        bytecode.constants = vec![
            Constant::Tag(0),
            Constant::Tag(0),
            Constant::Tag(2),
            Constant::Int(int(7)),
        ];
        bytecode.functions = vec![function_entry(0, 1, 0, 0, Some("Env::init"))];
        bytecode.runtime_process_specs.entries = vec![test_runtime_process_spec(
            0,
            "Env",
            RuntimeProcessKind::Agent,
            RuntimeProcessInstance::Singleton,
            true,
            0,
            0,
            None,
        )];
        bytecode
            .runtime_boot_plan
            .runtime_limits
            .pending_initial_retry_ms = 10_000;
        bytecode
            .runtime_boot_plan
            .runtime_limits
            .pending_max_retry_ms = 10_000;
        let mut vm = VM::new(bytecode);
        install_process_error_definitions(&mut vm);
        let dir = directory("standby");
        let path = dir.join("standby.log");
        let moved = dir.join("held.log");
        vm.process_runtime.handler_contexts.insert(
            "Env".into(),
            BTreeMap::from([("out".into(), file_target(&path))]),
        );
        let outcome = vm.start_singleton("Env".into(), Some(30_000)).unwrap();
        let BuiltinOutcome::Wait {
            future_id: root, ..
        } = &outcome
        else {
            panic!()
        };
        let root = *root;
        let retry = (0..8)
            .find_map(|_| {
                vm.drive_ready_detached_tasks().unwrap();
                if vm.process_runtime.detached_tasks.len() != 1 {
                    return None;
                }
                vm.process_runtime.futures.iter().find_map(|(id, record)| {
                    (*id != root && matches!(record.state, FutureState::Running)).then_some(*id)
                })
            })
            .expect("init must suspend in Standby retry");
        assert!(vm.process_runtime.processes.is_empty());
        assert_eq!(vm.active_file_sinks.0.len(), 1);
        let sink = Rc::clone(&vm.active_file_sinks.0.values().next().unwrap().sink);
        assert_eq!(sink.flushes.get(), 0);
        std::fs::rename(&path, &moved).unwrap();
        // Both branches were valid in the fixture image. Select Ready for the retry.
        vm.bytecode.opcodes[1] = Opcode::Jump(7);
        vm.process_runtime.resolve_future(retry, Value::Unit);
        let Value::Pid(pid) = vm.drive_builtin_outcome(outcome).unwrap() else {
            panic!()
        };
        let owner = vm
            .process_runtime
            .processes
            .get(&pid.id)
            .unwrap()
            .handler_bindings;
        let out = vm
            .process_runtime
            .handlers
            .as_ref()
            .unwrap()
            .handler(owner, "Env", "out")
            .unwrap();
        assert!(Rc::ptr_eq(&sink, &vm.active_file_sinks.0[&out.id].sink));
        assert_eq!(sink.flushes.get(), 0);
        let write = vm.out_handler_write(&out, "after retry".into()).unwrap();
        vm.drive_builtin_outcome(write).unwrap();
        vm.process_runtime
            .processes
            .get_mut(&pid.id)
            .unwrap()
            .acceptance = ProcessAcceptance::Stopping;
        vm.reclaim_stopped_process(pid.id).unwrap();
        assert_eq!(sink.flushes.get(), 1);
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "after retry");
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn workers_and_refill_share_active_sink_after_path_rename() {
        let mut bytecode = base_bytecode(vec![
            Opcode::Halt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::StructNew { field_count: 1 },
            Opcode::Return,
        ]);
        bytecode.constants = vec![Constant::Tag(0), Constant::Int(int(7))];
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
        let dir = directory("refill");
        let path = dir.join("workers.log");
        let moved = dir.join("held.log");
        vm.process_runtime.handler_contexts.insert(
            "Worker".into(),
            BTreeMap::from([("out".into(), file_target(&path))]),
        );
        let strategy = RuntimeContinuation::Workers {
            supervisor: "DynamicSupervisor".into(),
            process_name: "Worker".into(),
            init: vm.callable_for_function(0),
            strategy: WorkerStrategyState {
                init: 2,
                min: 0,
                max: 2,
                scale: WorkerScaleState::Fix(2),
            },
            members: Vec::new(),
            receiving: false,
        };
        let result = vm
            .drive_builtin_outcome(BuiltinOutcome::Resume(BuiltinContinuation::Runtime(
                strategy,
            )))
            .unwrap();
        let Value::Workers(workers) = decode_vm_result(result, "test", "workers")
            .unwrap()
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(vm.process_runtime.processes.len(), 2);
        assert_eq!(vm.active_file_sinks.0.len(), 1);
        let sink = Rc::clone(&vm.active_file_sinks.0.values().next().unwrap().sink);
        std::fs::rename(&path, &moved).unwrap();
        let removed = vm
            .process_runtime
            .worker_sets
            .get(&workers.id)
            .unwrap()
            .members[0];
        vm.process_runtime
            .processes
            .get_mut(&removed)
            .unwrap()
            .acceptance = ProcessAcceptance::Stopping;
        vm.reclaim_stopped_process(removed).unwrap();
        assert_eq!(sink.flushes.get(), 0);
        vm.drain_background_tasks().unwrap();
        assert_eq!(vm.process_runtime.processes.len(), 2);
        assert_eq!(vm.active_file_sinks.0.len(), 1);
        assert!(Rc::ptr_eq(
            &sink,
            &vm.active_file_sinks.0.values().next().unwrap().sink
        ));
        assert!(
            !path.exists(),
            "refill must use the active descriptor, not open the old pathname"
        );
        let members = vm
            .process_runtime
            .worker_sets
            .get(&workers.id)
            .unwrap()
            .members
            .clone();
        vm.process_runtime.worker_sets.remove(&workers.id);
        for pid in members {
            vm.process_runtime
                .processes
                .get_mut(&pid)
                .unwrap()
                .acceptance = ProcessAcceptance::Stopping;
            vm.reclaim_stopped_process(pid).unwrap();
        }
        assert_eq!(sink.flushes.get(), 1);
        assert!(vm.active_file_sinks.0.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod invalid_path_tests {
    use super::*;
    #[test]
    fn empty_string_path_is_an_open_failure_not_corrupt_metadata() {
        let dir =
            std::env::temp_dir().join(format!("surtr-empty-file-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut state = HandlerState::default();
        let mut active = ActiveFileSinks::default();
        let targets = BTreeMap::from([(
            "out".into(),
            RuntimeHandlerTarget {
                name: "FileOutHandler".into(),
                named_args: vec![sindr::ir::RuntimeHandlerArg {
                    name: "path".into(),
                    value: String::new(),
                }],
            },
        )]);
        let error = state
            .acquire(
                "Logger",
                &targets,
                &[],
                &dir,
                &HandlerIds::default(),
                &mut active,
            )
            .unwrap_err();
        assert!(
            matches!(error, AcquisitionError::Open { path, .. } if path.is_empty()),
            "valid String input must fail through the file open contract"
        );
        assert!(active.0.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
