use crate::error::RuntimeError;
use sindr::runtime::{ListHandle, Value};

/// Mutable construction state stays inside its owning VM continuation.
#[derive(Debug, Default)]
pub(super) struct ListBuilder {
    items: Vec<Value>,
}

impl Clone for ListBuilder {
    fn clone(&self) -> Self {
        // Cloning is used by transaction checkpoints, never by ordinary switching.
        #[cfg(test)]
        record(|metrics| metrics.clones += 1);
        Self {
            items: self.items.clone(),
        }
    }
}

impl ListBuilder {
    pub(super) fn with_capacity(capacity: usize) -> Result<Self, RuntimeError> {
        let mut items = Vec::new();
        items
            .try_reserve(capacity)
            .map_err(|_| RuntimeError::new("List builder capacity overflow"))?;
        Ok(Self { items })
    }

    pub(super) fn push(&mut self, value: Value) -> Result<(), RuntimeError> {
        self.items
            .len()
            .checked_add(1)
            .ok_or_else(|| RuntimeError::new("List builder length overflow"))?;
        self.items
            .try_reserve(1)
            .map_err(|_| RuntimeError::new("List builder capacity overflow"))?;
        self.items.push(value);
        #[cfg(test)]
        record(|metrics| metrics.pushes += 1);
        Ok(())
    }

    pub(super) fn finish(self) -> ListHandle {
        #[cfg(test)]
        record(|metrics| metrics.finishes += 1);
        ListHandle::from_items(self.items)
    }
}

#[cfg(test)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ListBuilderMetrics {
    pub pushes: usize,
    pub finishes: usize,
    pub clones: usize,
}
#[cfg(test)]
thread_local! {
    static METRICS: std::cell::Cell<ListBuilderMetrics> = std::cell::Cell::new(ListBuilderMetrics {pushes:0,finishes:0,clones:0});
}
#[cfg(test)]
fn record(update: impl FnOnce(&mut ListBuilderMetrics)) {
    METRICS.with(|cell| {
        let mut value = cell.get();
        update(&mut value);
        cell.set(value);
    });
}
#[cfg(test)]
pub(crate) fn list_builder_metrics() -> ListBuilderMetrics {
    METRICS.with(std::cell::Cell::get)
}
#[cfg(test)]
pub(crate) fn reset_list_builder_metrics() {
    METRICS.with(|cell| cell.set(ListBuilderMetrics::default()));
}
