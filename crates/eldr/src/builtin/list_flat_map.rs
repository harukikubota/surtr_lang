use super::{decode_callable_arg, BuiltinContinuation, BuiltinOutcome, RuntimeError, Value, VM};
use sindr::runtime::{Callable, ListHandle};

/// Mutable construction state stays inside its owning VM continuation.
#[derive(Debug, Default)]
struct ListBuilder {
    items: Vec<Value>,
}

impl Clone for ListBuilder {
    fn clone(&self) -> Self {
        // Cloning is used by transaction checkpoints, never by ordinary switching.
        #[cfg(test)]
        record(|metrics| metrics.builder_clones += 1);
        Self {
            items: self.items.clone(),
        }
    }
}

impl ListBuilder {
    fn push(&mut self, value: Value) -> Result<(), RuntimeError> {
        self.items
            .len()
            .checked_add(1)
            .ok_or_else(|| RuntimeError::new("List builder length overflow"))?;
        self.items.push(value);
        #[cfg(test)]
        record(|metrics| metrics.builder_pushes += 1);
        Ok(())
    }

    fn finish(self) -> ListHandle {
        #[cfg(test)]
        record(|metrics| metrics.builder_finishes += 1);
        ListHandle::from_items(self.items)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FlatMapContinuation {
    input: ListHandle,
    mapper: Callable,
    output: ListHandle,
    builder: ListBuilder,
    awaiting_mapper: bool,
}

impl FlatMapContinuation {
    fn runnable(self) -> BuiltinOutcome {
        BuiltinOutcome::Resume(BuiltinContinuation::FlatMap(self))
    }

    pub(super) fn resume(
        mut self,
        result: Result<Value, RuntimeError>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        let value = result?;
        if self.awaiting_mapper {
            let Value::List(output) = value else {
                return Err(RuntimeError::new(format!(
                    "list_flat_map mapper must return List, got {value:?}"
                )));
            };
            self.output = output;
            self.awaiting_mapper = false;
            return Ok(self.runnable());
        }

        // Each resume emits at most one element, including when the mapper returns
        // one very large Packed List. The engine charges this transition to its budget.
        if let Some(value) = self.output.head_value() {
            self.output = self
                .output
                .tail_handle()
                .ok_or_else(|| RuntimeError::new("list_flat_map output cursor has no tail"))?;
            self.builder.push(value)?;
            return Ok(self.runnable());
        }

        if let Some(value) = self.input.head_value() {
            self.input = self
                .input
                .tail_handle()
                .ok_or_else(|| RuntimeError::new("list_flat_map input cursor has no tail"))?;
            self.awaiting_mapper = true;
            #[cfg(test)]
            record(|metrics| {
                metrics.input_visits += 1;
                metrics.callback_calls += 1;
            });
            return Ok(BuiltinOutcome::Call {
                callable: self.mapper.clone(),
                args: vec![value],
                continuation: BuiltinContinuation::FlatMap(self),
            });
        }

        Ok(BuiltinOutcome::Complete(Value::List(self.builder.finish())))
    }
}

pub(super) fn builtin_list_flat_map(
    _vm: &mut VM,
    args: Vec<Value>,
) -> Result<BuiltinOutcome, RuntimeError> {
    let Value::List(input) = &args[0] else {
        return Err(RuntimeError::new("list_flat_map expects List as values"));
    };
    let mapper = decode_callable_arg(&args[1], "list_flat_map", "f")?;
    Ok(FlatMapContinuation {
        input: input.clone(),
        mapper,
        output: ListHandle::empty(),
        builder: ListBuilder::default(),
        awaiting_mapper: false,
    }
    .runnable())
}

#[cfg(test)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FlatMapMetrics {
    pub input_visits: usize,
    pub callback_calls: usize,
    pub builder_pushes: usize,
    pub builder_finishes: usize,
    pub builder_clones: usize,
}

#[cfg(test)]
thread_local! {
    static METRICS: std::cell::Cell<FlatMapMetrics> = std::cell::Cell::new(FlatMapMetrics {
        input_visits:0,callback_calls:0,builder_pushes:0,builder_finishes:0,builder_clones:0,
    });
}

#[cfg(test)]
fn record(update: impl FnOnce(&mut FlatMapMetrics)) {
    METRICS.with(|cell| {
        let mut metrics = cell.get();
        update(&mut metrics);
        cell.set(metrics);
    });
}

#[cfg(test)]
pub(crate) fn flat_map_metrics() -> FlatMapMetrics {
    METRICS.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(crate) fn reset_flat_map_metrics() {
    METRICS.with(|cell| cell.set(FlatMapMetrics::default()));
}
