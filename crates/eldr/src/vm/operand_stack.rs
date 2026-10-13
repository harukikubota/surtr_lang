use sindr::runtime::{Callable, Value};
use std::collections::HashMap;
use std::ops::Deref;

/// Owns both the values and the number of top-level PendingFuture slots.
/// Mutable access cannot change a Value variant outside this type.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct OperandStack {
    values: Vec<Value>,
    pending_futures: usize,
}

impl OperandStack {
    pub(super) fn has_pending_futures(&self) -> bool {
        self.pending_futures != 0
    }

    pub(super) fn push(&mut self, value: Value) {
        if matches!(value, Value::PendingFuture(_)) {
            self.pending_futures += 1;
        }
        self.values.push(value);
    }

    pub(super) fn pop(&mut self) -> Option<Value> {
        let value = self.values.pop()?;
        if matches!(value, Value::PendingFuture(_)) {
            self.pending_futures = self
                .pending_futures
                .checked_sub(1)
                .expect("PendingFuture stack count underflow");
        }
        Some(value)
    }

    pub(super) fn clear(&mut self) {
        self.values.clear();
        self.pending_futures = 0;
    }

    pub(super) fn truncate(&mut self, len: usize) {
        if len >= self.values.len() {
            return;
        }
        if self.pending_futures != 0 {
            let removed = self.values[len..]
                .iter()
                .filter(|v| matches!(v, Value::PendingFuture(_)))
                .count();
            self.pending_futures = self
                .pending_futures
                .checked_sub(removed)
                .expect("PendingFuture stack count underflow");
        }
        self.values.truncate(len);
    }

    pub(super) fn last_callable_mut(&mut self) -> Option<&mut Callable> {
        match self.values.last_mut() {
            Some(Value::Callable(callable)) => Some(callable),
            _ => None,
        }
    }

    pub(super) fn replace_ready_futures(&mut self, ready: &HashMap<u64, Value>) {
        for value in &mut self.values {
            if let Value::PendingFuture(id) = value {
                if let Some(resolved) = ready.get(id) {
                    let resolved = resolved.clone();
                    if !matches!(resolved, Value::PendingFuture(_)) {
                        self.pending_futures = self
                            .pending_futures
                            .checked_sub(1)
                            .expect("PendingFuture stack count underflow");
                    }
                    *value = resolved;
                }
            }
        }
    }
}

impl Deref for OperandStack {
    type Target = [Value];
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl From<Vec<Value>> for OperandStack {
    fn from(values: Vec<Value>) -> Self {
        let pending_futures = values
            .iter()
            .filter(|v| matches!(v, Value::PendingFuture(_)))
            .count();
        Self {
            values,
            pending_futures,
        }
    }
}

impl FromIterator<Value> for OperandStack {
    fn from_iter<T: IntoIterator<Item = Value>>(values: T) -> Self {
        Vec::from_iter(values).into()
    }
}

#[cfg(test)]
impl PartialEq<Vec<Value>> for OperandStack {
    fn eq(&self, other: &Vec<Value>) -> bool {
        self.values == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn count(stack: &OperandStack) {
        assert_eq!(
            stack.pending_futures,
            stack
                .values
                .iter()
                .filter(|v| matches!(v, Value::PendingFuture(_)))
                .count()
        );
    }

    #[test]
    fn mutations_and_clones_keep_the_exact_pending_slot_count() {
        let mut stack = OperandStack::default();
        for value in [
            Value::PendingFuture(1),
            Value::Unit,
            Value::PendingFuture(2),
        ] {
            stack.push(value);
            count(&stack);
        }
        let saved = stack.clone();
        stack.pop();
        count(&stack);
        count(&saved);
        assert!(stack.has_pending_futures());
        stack.truncate(1);
        count(&stack);
        stack.truncate(9);
        count(&stack);
        stack.truncate(0);
        count(&stack);
        assert!(!stack.has_pending_futures());
        stack.push(Value::PendingFuture(3));
        stack.clear();
        count(&stack);
        assert_eq!(stack.pop(), None);
        assert_eq!(saved.pending_futures, 2);
    }

    #[test]
    fn only_top_level_future_variants_contribute_to_the_count() {
        let stack: OperandStack = [
            Value::Tuple(vec![Value::PendingFuture(1)]),
            Value::PendingFuture(1),
            Value::PendingFuture(1),
        ]
        .into_iter()
        .collect();
        count(&stack);
        assert_eq!(stack.pending_futures, 2);
    }

    #[test]
    fn resolving_duplicates_and_future_valued_results_keeps_counts_exact() {
        let mut stack: OperandStack = vec![
            Value::PendingFuture(1),
            Value::PendingFuture(1),
            Value::PendingFuture(2),
            Value::Unit,
        ]
        .into();
        stack.replace_ready_futures(&[(1, Value::PendingFuture(3)), (2, Value::Unit)].into());
        count(&stack);
        assert_eq!(stack.pending_futures, 2);
        stack.replace_ready_futures(&[(3, Value::Unit)].into());
        count(&stack);
        assert!(!stack.has_pending_futures());
    }
}
