use sindr::runtime::{Callable, Value};
use std::collections::HashMap;
use std::ops::Deref;

/// Owns the values and the sorted positions of top-level PendingFuture slots.
/// Mutable access cannot change a Value variant outside this type.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct OperandStack {
    values: Vec<Value>,
    pending_slots: Vec<usize>,
}

impl OperandStack {
    pub(super) fn has_pending_futures(&self) -> bool {
        !self.pending_slots.is_empty()
    }

    pub(super) fn pending_future_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.pending_slots
            .iter()
            .map(|&slot| match &self.values[slot] {
                Value::PendingFuture(id) => *id,
                _ => unreachable!("PendingFuture stack position mismatch"),
            })
    }

    #[cfg(test)]
    pub(super) fn pending_slot_count(&self) -> usize {
        self.pending_slots.len()
    }

    pub(super) fn push(&mut self, value: Value) {
        if matches!(value, Value::PendingFuture(_)) {
            self.pending_slots.push(self.values.len());
        }
        self.values.push(value);
    }

    pub(super) fn pop(&mut self) -> Option<Value> {
        let value = self.values.pop()?;
        if matches!(value, Value::PendingFuture(_)) {
            assert_eq!(
                self.pending_slots.pop(),
                Some(self.values.len()),
                "PendingFuture stack position mismatch"
            );
        }
        Some(value)
    }

    pub(super) fn clear(&mut self) {
        self.values.clear();
        self.pending_slots.clear();
    }

    pub(super) fn truncate(&mut self, len: usize) {
        if len >= self.values.len() {
            return;
        }
        let kept = self.pending_slots.partition_point(|&slot| slot < len);
        self.pending_slots.truncate(kept);
        self.values.truncate(len);
    }

    pub(super) fn last_callable_mut(&mut self) -> Option<&mut Callable> {
        match self.values.last_mut() {
            Some(Value::Callable(callable)) => Some(callable),
            _ => None,
        }
    }

    pub(super) fn replace_ready_futures(&mut self, ready: &HashMap<u64, Value>) {
        let values = &mut self.values;
        self.pending_slots.retain(|&slot| {
            let Value::PendingFuture(id) = &values[slot] else {
                unreachable!("PendingFuture stack position mismatch");
            };
            let Some(resolved) = ready.get(id) else {
                return true;
            };
            let resolved = resolved.clone();
            let still_pending = matches!(resolved, Value::PendingFuture(_));
            values[slot] = resolved;
            still_pending
        });
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
        let pending_slots = values
            .iter()
            .enumerate()
            .filter_map(|(slot, value)| matches!(value, Value::PendingFuture(_)).then_some(slot))
            .collect();
        Self {
            values,
            pending_slots,
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
            stack.pending_slots,
            stack.values.iter().enumerate()
                .filter_map(|(slot, value)| matches!(value, Value::PendingFuture(_)).then_some(slot))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn mutations_and_clones_keep_the_exact_pending_slot_positions() {
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
        assert_eq!(saved.pending_slots, vec![0, 2]);
    }

    #[test]
    fn only_top_level_future_variants_contribute_to_the_index() {
        let stack: OperandStack = [
            Value::Tuple(vec![Value::PendingFuture(1)]),
            Value::PendingFuture(1),
            Value::PendingFuture(1),
        ]
        .into_iter()
        .collect();
        count(&stack);
        assert_eq!(stack.pending_slots.len(), 2);
    }

    #[test]
    fn ordinary_values_do_not_allocate_a_pending_slot_buffer() {
        let mut stack = OperandStack::default();
        for _ in 0..1024 {
            stack.push(Value::Unit);
        }
        assert_eq!(stack.pending_slots.capacity(), 0);
        stack.truncate(512);
        stack.pop();
        assert_eq!(stack.pending_slots.capacity(), 0);
        stack.clear();
        assert_eq!(stack.pending_slots.capacity(), 0);
        let stack: OperandStack = vec![Value::Unit; 1024].into();
        assert_eq!(stack.pending_slots.capacity(), 0);
    }

    #[test]
    fn resolving_duplicates_and_future_valued_results_keeps_positions_exact() {
        let mut stack: OperandStack = vec![
            Value::PendingFuture(1),
            Value::PendingFuture(1),
            Value::PendingFuture(2),
            Value::Unit,
        ]
        .into();
        stack.replace_ready_futures(&[(1, Value::PendingFuture(3)), (2, Value::Unit)].into());
        count(&stack);
        assert_eq!(stack.pending_slots.len(), 2);
        stack.replace_ready_futures(&[(3, Value::Unit)].into());
        count(&stack);
        assert!(!stack.has_pending_futures());
    }
}
