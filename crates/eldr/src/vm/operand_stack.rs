use sindr::runtime::{Callable, Value};
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

/// Owns the values and the sorted positions of top-level PendingFuture slots.
/// Mutable access cannot change a Value variant outside this type.
#[derive(Debug, Default)]
pub(super) struct OperandStack {
    values: Vec<Value>,
    pending_slots: Option<Rc<Vec<usize>>>,
}

impl Clone for OperandStack {
    fn clone(&self) -> Self {
        Self {
            values: self.values.clone(),
            pending_slots: self
                .pending_slots
                .as_ref()
                .filter(|slots| !slots.is_empty())
                .cloned(),
        }
    }
}

impl PartialEq for OperandStack {
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values && self.pending_positions() == other.pending_positions()
    }
}

impl OperandStack {
    pub(super) fn has_pending_futures(&self) -> bool {
        !self.pending_positions().is_empty()
    }

    fn pending_positions(&self) -> &[usize] {
        match &self.pending_slots {
            Some(slots) => slots,
            None => &[],
        }
    }

    pub(super) fn pending_future_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.pending_positions()
            .iter()
            .map(|&slot| match &self.values[slot] {
                Value::PendingFuture(id) => *id,
                _ => unreachable!("PendingFuture stack position mismatch"),
            })
    }

    #[cfg(test)]
    pub(super) fn pending_slot_count(&self) -> usize {
        self.pending_positions().len()
    }

    #[inline(always)]
    pub(super) fn push(&mut self, value: Value) {
        if matches!(value, Value::PendingFuture(_)) {
            self.push_pending_position();
        }
        self.values.push(value);
    }

    #[cold]
    fn push_pending_position(&mut self) {
        Rc::make_mut(
            self.pending_slots
                .get_or_insert_with(|| Rc::new(Vec::new())),
        )
        .push(self.values.len());
    }

    #[inline(always)]
    pub(super) fn pop(&mut self) -> Option<Value> {
        let slot = self.values.len().checked_sub(1)?;
        if let Some(slots) = &mut self.pending_slots {
            if slots.last() == Some(&slot) {
                if slots.len() == 1 {
                    self.clear_pending_positions();
                } else {
                    Rc::make_mut(slots).pop();
                }
            }
        }
        self.values.pop()
    }

    fn clear_pending_positions(&mut self) {
        if let Some(slots) = &mut self.pending_slots {
            if let Some(owned) = Rc::get_mut(slots) {
                owned.clear();
            } else {
                self.pending_slots = None;
            }
        }
    }

    pub(super) fn clear(&mut self) {
        self.values.clear();
        self.clear_pending_positions();
    }

    pub(super) fn truncate(&mut self, len: usize) {
        if len >= self.values.len() {
            return;
        }
        if let Some(slots) = &mut self.pending_slots {
            let kept = slots.partition_point(|&slot| slot < len);
            if kept == 0 {
                self.clear_pending_positions();
            } else if kept < slots.len() {
                Rc::make_mut(slots).truncate(kept);
            }
        }
        self.values.truncate(len);
    }

    pub(super) fn last_callable_mut(&mut self) -> Option<&mut Callable> {
        match self.values.last_mut() {
            Some(Value::Callable(callable)) => Some(callable),
            _ => None,
        }
    }

    pub(super) fn replace_ready_futures(&mut self, ready: &HashMap<u64, &Value>) {
        let values = &mut self.values;
        let Some(slots) = &mut self.pending_slots else {
            return;
        };
        let mut remaining = 0;
        for &slot in slots.iter() {
            let Value::PendingFuture(id) = &values[slot] else {
                unreachable!("PendingFuture stack position mismatch");
            };
            if let Some(resolved) = ready.get(id) {
                #[cfg(test)]
                super::future_stack_tests::record_slot_value_clone();
                values[slot] = (**resolved).clone();
            }
            remaining += usize::from(matches!(values[slot], Value::PendingFuture(_)));
        }
        if remaining == 0 {
            self.clear_pending_positions();
        } else if remaining < slots.len() {
            Rc::make_mut(slots).retain(|&slot| matches!(values[slot], Value::PendingFuture(_)));
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
        let pending_slots: Vec<_> = values
            .iter()
            .enumerate()
            .filter_map(|(slot, value)| matches!(value, Value::PendingFuture(_)).then_some(slot))
            .collect();
        Self {
            values,
            pending_slots: (!pending_slots.is_empty()).then(|| Rc::new(pending_slots)),
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
            stack.pending_positions(),
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
        assert_eq!(saved.pending_positions(), &[0, 2]);
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
        assert_eq!(stack.pending_positions().len(), 2);
    }

    #[test]
    fn position_metadata_fits_in_one_pointer_width() {
        assert!(
            std::mem::size_of::<OperandStack>()
                <= std::mem::size_of::<Vec<Value>>() + std::mem::size_of::<usize>()
        );
    }

    #[test]
    fn ordinary_values_do_not_allocate_a_pending_slot_buffer() {
        let mut stack = OperandStack::default();
        for _ in 0..1024 {
            stack.push(Value::Unit);
        }
        assert!(stack.pending_slots.is_none());
        stack.truncate(512);
        stack.pop();
        assert!(stack.pending_slots.is_none());
        stack.clear();
        assert!(stack.pending_slots.is_none());
        let stack: OperandStack = vec![Value::Unit; 1024].into();
        assert!(stack.pending_slots.is_none());
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
        stack.replace_ready_futures(&[(1, &Value::PendingFuture(3)), (2, &Value::Unit)].into());
        count(&stack);
        assert_eq!(stack.pending_positions().len(), 2);
        stack.replace_ready_futures(&[(3, &Value::Unit)].into());
        count(&stack);
        assert!(!stack.has_pending_futures());
    }

    #[test]
    fn clones_omit_empty_position_buffers_and_preserve_original_reservations() {
        for operation in ["pop", "truncate", "clear", "resolve"] {
            let mut original: OperandStack = vec![Value::Unit, Value::PendingFuture(1)].into();
            match operation {
                "pop" => {
                    original.pop();
                }
                "truncate" => original.truncate(1),
                "clear" => original.clear(),
                "resolve" => original.replace_ready_futures(&[(1, &Value::Unit)].into()),
                _ => unreachable!(),
            }
            let original_slots = original.pending_slots.as_ref().unwrap();
            let original_box = &**original_slots as *const Vec<usize>;
            let original_buffer = original_slots.as_ptr();
            let capacity = original_slots.capacity();
            let mut saved = original.clone();
            assert!(
                saved.pending_slots.is_none(),
                "{operation}: empty clone must not allocate metadata"
            );
            assert_eq!(saved, original);
            count(&saved);
            let saved_values = saved.values.clone();
            original.push(Value::PendingFuture(2));
            assert_eq!(
                original.pending_slots.as_ref().unwrap().capacity(),
                capacity
            );
            assert_eq!(
                &**original.pending_slots.as_ref().unwrap() as *const Vec<usize>,
                original_box
            );
            assert_eq!(
                original.pending_slots.as_ref().unwrap().as_ptr(),
                original_buffer
            );
            assert_eq!(saved.values, saved_values);
            assert!(!saved.has_pending_futures());
            saved.push(Value::PendingFuture(3));
            saved.replace_ready_futures(&[(3, &Value::Unit)].into());
            assert_eq!(original.pending_future_ids().collect::<Vec<_>>(), vec![2]);
            count(&saved);
            count(&original);
        }
    }

    #[test]
    fn pending_positions_are_shared_until_an_actual_partial_mutation() {
        let mut original: OperandStack = vec![
            Value::PendingFuture(1),
            Value::PendingFuture(1),
            Value::PendingFuture(2),
        ]
        .into();
        let saved = original.clone();
        let shared = &**saved.pending_slots.as_ref().unwrap() as *const Vec<usize>;
        assert_eq!(
            &**original.pending_slots.as_ref().unwrap() as *const Vec<usize>,
            shared
        );
        original.push(Value::Unit);
        original.pop();
        original.truncate(3);
        original.replace_ready_futures(&HashMap::new());
        original.replace_ready_futures(&[(99, &Value::Unit)].into());
        original.replace_ready_futures(&[(1, &Value::PendingFuture(4))].into());
        assert_eq!(
            &**original.pending_slots.as_ref().unwrap() as *const Vec<usize>,
            shared
        );
        assert_eq!(
            saved.values,
            vec![
                Value::PendingFuture(1),
                Value::PendingFuture(1),
                Value::PendingFuture(2)
            ]
        );
        original.replace_ready_futures(&[(2, &Value::Unit)].into());
        assert_ne!(
            &**original.pending_slots.as_ref().unwrap() as *const Vec<usize>,
            shared
        );
        assert_eq!(original.pending_positions(), &[0, 1]);
        assert_eq!(saved.pending_positions(), &[0, 1, 2]);
        count(&original);
        count(&saved);
        let mut appended = saved.clone();
        appended.push(Value::PendingFuture(3));
        assert_eq!(saved.pending_positions(), &[0, 1, 2]);
        assert_eq!(appended.pending_positions(), &[0, 1, 2, 3]);
    }

    #[test]
    fn retiring_all_shared_positions_discards_the_view_without_copying() {
        for operation in ["pop", "truncate", "clear", "resolve"] {
            let mut original: OperandStack = vec![Value::Unit, Value::PendingFuture(1)].into();
            let saved = original.clone();
            match operation {
                "pop" => {
                    original.pop();
                }
                "truncate" => original.truncate(1),
                "clear" => original.clear(),
                "resolve" => original.replace_ready_futures(&[(1, &Value::Unit)].into()),
                _ => unreachable!(),
            }
            assert!(
                original.pending_slots.is_none(),
                "{operation}: do not copy a shared table only to empty it"
            );
            assert_eq!(saved.pending_positions(), &[1]);
            assert_eq!(saved.values, vec![Value::Unit, Value::PendingFuture(1)]);
            count(&original);
            count(&saved);
        }
    }
}
