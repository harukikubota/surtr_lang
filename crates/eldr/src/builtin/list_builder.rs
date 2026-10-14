use crate::error::RuntimeError;
use sindr::runtime::{ListHandle, Value, LIST_CHUNK_CAPACITY};
use std::{fmt, rc::Rc};

/// Append-only segments keep checkpoint prefixes immutable.
#[derive(Default)]
pub(super) struct ListBuilder {
    head: Option<Rc<Segment>>,
    len: usize,
}

struct Segment {
    prefix: Option<Rc<Segment>>,
    items: Vec<Value>,
}

impl fmt::Debug for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Segment")
            .field("items_len", &self.items.len())
            .field("has_prefix", &self.prefix.is_some())
            .finish()
    }
}

impl Drop for Segment {
    fn drop(&mut self) {
        let mut prefix = self.prefix.take();
        while let Some(segment) = prefix {
            let Ok(mut segment) = Rc::try_unwrap(segment) else {
                break;
            };
            prefix = segment.prefix.take();
        }
    }
}

impl fmt::Debug for ListBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListBuilder")
            .field("len", &self.len)
            .field("head", &self.head)
            .finish()
    }
}

impl Clone for ListBuilder {
    fn clone(&self) -> Self {
        #[cfg(test)]
        record(|metrics| metrics.clones += 1);
        // An empty checkpoint has no values to share. Keep any reserved buffer
        // unique to the original builder so its next push can reuse it.
        if self.len == 0 {
            return Self::default();
        }
        Self {
            head: self.head.clone(),
            len: self.len,
        }
    }
}

impl ListBuilder {
    pub(super) fn with_capacity(capacity: usize) -> Result<Self, RuntimeError> {
        if capacity == 0 {
            return Ok(Self::default());
        }
        if capacity > isize::MAX as usize / std::mem::size_of::<Value>() {
            return Err(RuntimeError::new("List builder capacity overflow"));
        }
        let mut items = Vec::new();
        items
            .try_reserve(capacity.min(LIST_CHUNK_CAPACITY))
            .map_err(|_| RuntimeError::new("List builder capacity overflow"))?;
        Ok(Self {
            head: Some(Rc::new(Segment {
                prefix: None,
                items,
            })),
            len: 0,
        })
    }

    #[inline]
    pub(super) fn push(&mut self, value: Value) -> Result<(), RuntimeError> {
        let len = self
            .len
            .checked_add(1)
            .ok_or_else(|| RuntimeError::new("List builder length overflow"))?;
        let capacity_growth;
        if let Some(segment) = self
            .head
            .as_mut()
            .and_then(Rc::get_mut)
            .filter(|segment| segment.items.len() < LIST_CHUNK_CAPACITY)
        {
            let capacity = segment.items.capacity();
            if segment.items.len() == capacity {
                segment
                    .items
                    .try_reserve(1)
                    .map_err(|_| RuntimeError::new("List builder capacity overflow"))?;
            }
            segment.items.push(value);
            capacity_growth = segment.items.capacity() != capacity;
        } else {
            let mut items = Vec::new();
            items
                .try_reserve(1)
                .map_err(|_| RuntimeError::new("List builder capacity overflow"))?;
            items.push(value);
            // A reserved empty buffer does not become an empty prefix segment.
            let prefix = if self.len == 0 {
                None
            } else {
                self.head.take()
            };
            self.head = Some(Rc::new(Segment { prefix, items }));
            capacity_growth = true;
        }
        self.len = len;
        #[cfg(test)]
        record(|metrics| {
            metrics.pushes += 1;
            metrics.capacity_growths += usize::from(capacity_growth);
        });
        #[cfg(not(test))]
        let _ = capacity_growth;
        Ok(())
    }

    pub(super) fn finish(self) -> ListBuilderFinish {
        let cursor = self.head.as_ref().map_or(0, |segment| segment.items.len());
        ListBuilderFinish {
            current: self.head,
            cursor,
            output: ListHandle::empty(),
            len: self.len,
        }
    }
}

/// Immutable output and segment cursors also support checkpoints during finish.
#[derive(Clone)]
pub(super) struct ListBuilderFinish {
    current: Option<Rc<Segment>>,
    cursor: usize,
    output: ListHandle,
    len: usize,
}

impl fmt::Debug for ListBuilderFinish {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListBuilderFinish")
            .field("cursor", &self.cursor)
            .field("output_len", &self.output.len())
            .field("len", &self.len)
            .finish()
    }
}

pub(super) enum ListFinishStep {
    Complete(ListHandle),
    Pending(ListBuilderFinish),
}

impl ListBuilderFinish {
    fn complete(self) -> Result<ListFinishStep, RuntimeError> {
        if self.output.len() != self.len {
            return Err(RuntimeError::new("List builder finished length mismatch"));
        }
        #[cfg(test)]
        record(|metrics| metrics.finishes += 1);
        Ok(ListFinishStep::Complete(self.output))
    }

    /// A transition moves one whole owned suffix buffer, one Value, or one boundary.
    pub(super) fn resume(mut self) -> Result<ListFinishStep, RuntimeError> {
        #[cfg(test)]
        record(|metrics| metrics.finish_steps += 1);
        let Some(current) = self.current.as_mut() else {
            return self.complete();
        };
        if self.cursor > current.items.len() {
            return Err(RuntimeError::new(
                "List builder finish cursor exceeds segment length",
            ));
        }
        // Every segment is bounded; moving its buffer costs one transition.
        if !current.items.is_empty()
            && self.cursor == current.items.len()
            && Rc::strong_count(current) == 1
        {
            let mut segment = Rc::try_unwrap(self.current.take().unwrap())
                .expect("uniquely owned List builder segment");
            self.current = segment.prefix.take();
            self.cursor = self
                .current
                .as_ref()
                .map_or(0, |segment| segment.items.len());
            self.output =
                ListHandle::prepend_items(std::mem::take(&mut segment.items), &self.output);
            return if self.current.is_none() {
                self.complete()
            } else {
                Ok(ListFinishStep::Pending(self))
            };
        }
        // After a finish checkpoint is released, discard already emitted Values
        // individually before moving the remaining uniquely owned Values.
        if let Some(segment) = Rc::get_mut(current) {
            if segment.items.len() > self.cursor {
                drop(segment.items.pop().expect("processed List builder suffix"));
                return Ok(ListFinishStep::Pending(self));
            }
        }
        if self.cursor == 0 {
            self.current = if let Some(segment) = Rc::get_mut(current) {
                segment.prefix.take()
            } else {
                current.prefix.clone()
            };
            self.cursor = self
                .current
                .as_ref()
                .map_or(0, |segment| segment.items.len());
            return if self.current.is_none() {
                self.complete()
            } else {
                Ok(ListFinishStep::Pending(self))
            };
        }
        self.cursor -= 1;
        let value = if let Some(segment) = Rc::get_mut(current) {
            segment.items.pop().expect("remaining List builder item")
        } else {
            #[cfg(test)]
            record(|metrics| metrics.finish_copied_items += 1);
            current.items[self.cursor].clone()
        };
        self.output = ListHandle::cons(value, &self.output);
        Ok(ListFinishStep::Pending(self))
    }
}

#[cfg(test)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ListBuilderMetrics {
    pub pushes: usize,
    pub finishes: usize,
    pub clones: usize,
    pub copied_items: usize,
    pub capacity_growths: usize,
    pub finish_copied_items: usize,
    pub finish_steps: usize,
}
#[cfg(test)]
thread_local! {
    static METRICS: std::cell::Cell<ListBuilderMetrics> = std::cell::Cell::new(ListBuilderMetrics {pushes:0,finishes:0,clones:0,copied_items:0,capacity_growths:0,finish_copied_items:0,finish_steps:0});
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

#[cfg(test)]
mod tests {
    use super::*;
    use sindr::primitives::int;

    fn numbers(builder: ListBuilder) -> Vec<Value> {
        finish(builder.finish()).iter().collect()
    }

    fn finish(mut continuation: ListBuilderFinish) -> ListHandle {
        loop {
            match continuation.resume().unwrap() {
                ListFinishStep::Complete(list) => return list,
                ListFinishStep::Pending(next) => continuation = next,
            }
        }
    }

    fn value(n: usize) -> Value {
        Value::Int(int(n))
    }

    #[test]
    fn checkpoint_shares_long_prefix_without_copying_values() {
        let mut builder = ListBuilder::default();
        for n in 0..1024 {
            builder
                .push(Value::Str(format!("{n}:{}", "x".repeat(1024))))
                .unwrap();
        }
        reset_list_builder_metrics();
        let saved = builder.clone();
        assert_eq!(list_builder_metrics().clones, 1);
        assert_eq!(list_builder_metrics().copied_items, 0);
        builder.push(Value::Unit).unwrap();
        assert_eq!(list_builder_metrics().copied_items, 0);
        assert_eq!(numbers(saved).len(), 1024);
        assert_eq!(numbers(builder).len(), 1025);
    }

    #[test]
    fn independent_branches_preserve_saved_prefix_and_order() {
        let mut builder = ListBuilder::default();
        builder.push(value(1)).unwrap();
        builder.push(value(2)).unwrap();
        let mut saved = builder.clone();
        builder.push(value(3)).unwrap();
        let mut second_saved = builder.clone();
        builder.push(value(4)).unwrap();
        saved.push(value(9)).unwrap();
        second_saved.push(value(8)).unwrap();
        assert_eq!(numbers(saved), vec![value(1), value(2), value(9)]);
        assert_eq!(
            numbers(second_saved),
            vec![value(1), value(2), value(3), value(8)]
        );
        assert_eq!(
            numbers(builder),
            vec![value(1), value(2), value(3), value(4)]
        );
    }

    #[test]
    fn empty_builder_and_reserved_empty_builder_finish_empty() {
        assert!(numbers(ListBuilder::default()).is_empty());
        assert!(numbers(ListBuilder::with_capacity(16).unwrap()).is_empty());
    }

    #[test]
    fn reserved_empty_checkpoint_does_not_create_an_empty_prefix() {
        let saved = ListBuilder::with_capacity(16).unwrap();
        reset_list_builder_metrics();
        let mut branch = saved.clone();
        branch.push(value(7)).unwrap();
        assert_eq!(list_builder_metrics().copied_items, 0);
        assert!(numbers(saved).is_empty());
        reset_list_builder_metrics();
        let ListFinishStep::Complete(list) = branch.finish().resume().unwrap() else {
            panic!("nonempty branch must finish without an empty prefix transition");
        };
        assert_eq!(list.iter().collect::<Vec<_>>(), vec![value(7)]);
        let metrics = list_builder_metrics();
        assert_eq!(metrics.copied_items, 0);
        assert_eq!(metrics.finish_copied_items, 0);
        assert_eq!(metrics.finish_steps, 1);
        assert_eq!(metrics.finishes, 1);
    }

    #[test]
    fn empty_checkpoint_preserves_the_original_reservation() {
        let mut original = ListBuilder::with_capacity(16).unwrap();
        let saved = original.clone();
        reset_list_builder_metrics();
        original.push(value(7)).unwrap();
        assert_eq!(list_builder_metrics().capacity_growths, 0);
        assert_eq!(numbers(original), vec![value(7)]);
        assert!(numbers(saved).is_empty());
    }

    #[test]
    fn released_checkpoint_reuses_the_unique_buffer() {
        let mut original = ListBuilder::with_capacity(16).unwrap();
        original.push(value(1)).unwrap();
        let saved = original.clone();
        drop(saved);
        reset_list_builder_metrics();
        original.push(value(2)).unwrap();
        assert_eq!(list_builder_metrics().capacity_growths, 0);
        let ListFinishStep::Complete(list) = original.finish().resume().unwrap() else {
            panic!("released checkpoint must keep the single-buffer transfer");
        };
        assert_eq!(list.iter().collect::<Vec<_>>(), vec![value(1), value(2)]);
        assert_eq!(list_builder_metrics().finish_copied_items, 0);
    }

    #[test]
    fn unique_chunk_finish_moves_without_copy_in_bounded_steps() {
        let mut builder = ListBuilder::with_capacity(1024).unwrap();
        for n in 0..1024 {
            builder.push(value(n)).unwrap();
        }
        reset_list_builder_metrics();
        let ListFinishStep::Pending(continuation) = builder.finish().resume().unwrap() else {
            panic!("1024 elements must not finish in one transition");
        };
        let list = finish(continuation);
        assert_eq!(list.len(), 1024);
        let metrics = list_builder_metrics();
        assert_eq!(metrics.finish_copied_items, 0);
        assert_eq!(metrics.finish_steps, 4);
        assert_eq!(metrics.finishes, 1);
    }

    #[test]
    fn shared_finish_copies_at_most_one_value_per_transition() {
        let mut builder = ListBuilder::default();
        for n in 0..1024 {
            builder.push(value(n)).unwrap();
        }
        let saved = builder.clone();
        let mut continuation = builder.finish();
        reset_list_builder_metrics();
        let result = loop {
            let before = list_builder_metrics();
            let step = continuation.resume().unwrap();
            let after = list_builder_metrics();
            assert!(after.finish_copied_items - before.finish_copied_items <= 1);
            assert_eq!(after.finish_steps - before.finish_steps, 1);
            match step {
                ListFinishStep::Complete(list) => break list,
                ListFinishStep::Pending(next) => continuation = next,
            }
        };
        assert_eq!(
            result.iter().collect::<Vec<_>>(),
            (0..1024).map(value).collect::<Vec<_>>()
        );
        assert_eq!(list_builder_metrics().finish_copied_items, 1024);
        assert_eq!(numbers(saved), result.iter().collect::<Vec<_>>());
    }

    #[test]
    fn finish_checkpoint_preserves_partial_output_and_remaining_prefix() {
        let mut builder = ListBuilder::default();
        for n in 0..32 {
            builder.push(value(n)).unwrap();
        }
        let saved = builder.clone();
        builder.push(value(32)).unwrap();
        let ListFinishStep::Pending(continuation) = builder.finish().resume().unwrap() else {
            panic!("saved prefix needs a budgeted finish");
        };
        let ListFinishStep::Pending(continuation) = continuation.resume().unwrap() else {
            panic!("prefix must advance one item at a time");
        };
        reset_list_builder_metrics();
        let snapshot = continuation.clone();
        assert_eq!(list_builder_metrics().finish_copied_items, 0);
        let expected: Vec<_> = (0..33).map(value).collect();
        assert_eq!(finish(continuation).iter().collect::<Vec<_>>(), expected);
        drop(saved);
        assert_eq!(finish(snapshot).iter().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn releasing_finish_checkpoint_cleans_processed_suffix_in_steps() {
        let mut builder = ListBuilder::default();
        for n in 0..16 {
            builder.push(value(n)).unwrap();
        }
        let saved = builder.clone();
        let mut continuation = builder.finish();
        for _ in 0..8 {
            let ListFinishStep::Pending(next) = continuation.resume().unwrap() else {
                panic!("shared finish must remain pending");
            };
            continuation = next;
        }
        drop(saved);
        reset_list_builder_metrics();
        let result = finish(continuation);
        assert_eq!(
            result.iter().collect::<Vec<_>>(),
            (0..16).map(value).collect::<Vec<_>>()
        );
        assert_eq!(list_builder_metrics().finish_copied_items, 0);
        // Eight emitted values are dropped individually, then the remaining
        // bounded owned buffer is attached to the already emitted output.
        assert_eq!(list_builder_metrics().finish_steps, 9);
    }

    #[test]
    fn length_and_capacity_overflow_are_errors() {
        let mut builder = ListBuilder {
            head: None,
            len: usize::MAX,
        };
        assert!(builder.push(Value::Unit).is_err());
        assert!(builder.head.is_none());
        assert_eq!(builder.len, usize::MAX);
        assert!(ListBuilder::with_capacity(usize::MAX).is_err());
    }

    #[test]
    fn repeated_checkpoints_debug_and_drop_do_not_recurse() {
        let mut builder = ListBuilder::default();
        for n in 0..10_000 {
            let saved = builder.clone();
            builder.push(value(n)).unwrap();
            drop(saved);
        }
        let debug = format!("{builder:?}");
        assert!(debug.len() < 200);
        let continuation = builder.clone().finish();
        let debug = format!("{continuation:?}");
        assert!(debug.len() < 200);
        drop(builder);
        drop(continuation);
    }
}
