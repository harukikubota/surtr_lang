use super::list_builder::ListBuilder;
use super::list_finish::finish_list;
use super::{BuiltinContinuation, BuiltinOutcome, RuntimeError, Value, VM};
use sindr::runtime::{Callable, HashMapHandle, ListHandle};

#[derive(Debug, Clone)]
pub(crate) struct MapValuesContinuation {
    remaining: std::vec::IntoIter<(String, Value)>,
    mapper: Callable,
    output: HashMapHandle,
    pending_key: String,
}

impl MapValuesContinuation {
    pub(super) fn resume(
        mut self,
        result: Result<Value, RuntimeError>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        self.output.entries.insert(self.pending_key, result?);
        match self.remaining.next() {
            Some((key, value)) => {
                self.pending_key = key;
                Ok(BuiltinOutcome::Call {
                    callable: self.mapper.clone(),
                    args: vec![value],
                    continuation: BuiltinContinuation::MapValues(self),
                })
            }
            None => Ok(BuiltinOutcome::Complete(Value::HashMap(self.output))),
        }
    }
}

#[derive(Debug, Clone)]
enum SortedAction {
    Callback(Callable),
    Keys,
    Values,
}

#[derive(Debug, Clone)]
pub(crate) struct SortedMapContinuation {
    entries: Vec<(String, Value)>,
    heapify_next: usize,
    heap_end: usize,
    sift_root: Option<usize>,
    action: SortedAction,
}

impl SortedMapContinuation {
    fn start(map: HashMapHandle, action: SortedAction) -> Result<BuiltinOutcome, RuntimeError> {
        // Only move entries here; sorting and collection use VM reductions.
        let entries: Vec<_> = map.entries.into_iter().collect();
        let len = entries.len();
        let continuation = Self {
            entries,
            heapify_next: len / 2,
            heap_end: len,
            sift_root: None,
            action,
        };
        if len < 2 {
            continuation.sorted()
        } else {
            Ok(continuation.pending())
        }
    }

    fn pending(self) -> BuiltinOutcome {
        BuiltinOutcome::Resume(BuiltinContinuation::SortedMap(self))
    }

    pub(super) fn resume(
        self,
        result: Result<Value, RuntimeError>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        result?;
        self.resume_steps(1).0
    }

    /// Each heap boundary/level consumes one, even in the grouped pure path.
    pub(crate) fn resume_steps(
        mut self,
        max_steps: u64,
    ) -> (Result<BuiltinOutcome, RuntimeError>, u64) {
        let mut used = 0;
        while used < max_steps {
            used += 1;
            match self.step() {
                Ok(true) => return (self.sorted(), used),
                Ok(false) => {}
                Err(error) => return (Err(error), used),
            }
        }
        (Ok(self.pending()), used)
    }

    fn step(&mut self) -> Result<bool, RuntimeError> {
        if let Some(root) = self.sift_root {
            let left = root
                .checked_mul(2)
                .and_then(|n| n.checked_add(1))
                .ok_or_else(|| RuntimeError::new("HashMap sort cursor overflow"))?;
            if left >= self.heap_end {
                self.sift_root = None;
            } else {
                let right = left + 1;
                let child = if right < self.heap_end && self.entries[left].0 < self.entries[right].0
                {
                    right
                } else {
                    left
                };
                if self.entries[root].0 < self.entries[child].0 {
                    self.entries.swap(root, child);
                    self.sift_root = Some(child);
                } else {
                    self.sift_root = None;
                }
            }
        } else if self.heapify_next > 0 {
            self.heapify_next -= 1;
            self.sift_root = Some(self.heapify_next);
        } else if self.heap_end > 1 {
            self.heap_end -= 1;
            self.entries.swap(0, self.heap_end);
            self.sift_root = Some(0);
        } else {
            return Ok(true);
        }
        Ok(false)
    }

    fn sorted(self) -> Result<BuiltinOutcome, RuntimeError> {
        let mut remaining = self.entries.into_iter();
        match self.action {
            SortedAction::Callback(mapper) => match remaining.next() {
                Some((key, value)) => Ok(BuiltinOutcome::Call {
                    callable: mapper.clone(),
                    args: vec![value],
                    continuation: BuiltinContinuation::MapValues(MapValuesContinuation {
                        remaining,
                        mapper,
                        output: HashMapHandle::empty(),
                        pending_key: key,
                    }),
                }),
                None => Ok(BuiltinOutcome::Complete(Value::HashMap(
                    HashMapHandle::empty(),
                ))),
            },
            SortedAction::Keys | SortedAction::Values => MapListContinuation {
                remaining,
                keys: matches!(self.action, SortedAction::Keys),
                output: ListBuilder::default(),
            }
            .resume(Ok(Value::Unit)),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MapListContinuation {
    remaining: std::vec::IntoIter<(String, Value)>,
    keys: bool,
    output: ListBuilder,
}

impl MapListContinuation {
    pub(crate) fn resume_steps(
        mut self,
        max_steps: u64,
    ) -> (Result<BuiltinOutcome, RuntimeError>, u64) {
        let mut used = 0;
        while used < max_steps {
            used += 1;
            match self.resume(Ok(Value::Unit)) {
                Ok(BuiltinOutcome::Resume(BuiltinContinuation::MapList(next))) => self = next,
                outcome => return (outcome, used),
            }
        }
        (
            Ok(BuiltinOutcome::Resume(BuiltinContinuation::MapList(self))),
            used,
        )
    }

    pub(super) fn resume(
        mut self,
        result: Result<Value, RuntimeError>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        result?;
        let Some((key, value)) = self.remaining.next() else {
            return finish_list(self.output, None);
        };
        self.output
            .push(if self.keys { Value::Str(key) } else { value })?;
        Ok(BuiltinOutcome::Resume(BuiltinContinuation::MapList(self)))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MapFromEntriesContinuation {
    remaining: ListHandle,
    index: usize,
    output: HashMapHandle,
}

impl MapFromEntriesContinuation {
    pub(crate) fn resume_steps(
        mut self,
        max_steps: u64,
    ) -> (Result<BuiltinOutcome, RuntimeError>, u64) {
        let mut used = 0;
        while used < max_steps {
            used += 1;
            match self.resume(Ok(Value::Unit)) {
                Ok(BuiltinOutcome::Resume(BuiltinContinuation::MapFromEntries(next))) => {
                    self = next
                }
                outcome => return (outcome, used),
            }
        }
        (
            Ok(BuiltinOutcome::Resume(BuiltinContinuation::MapFromEntries(
                self,
            ))),
            used,
        )
    }

    pub(super) fn resume(
        mut self,
        result: Result<Value, RuntimeError>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        result?;
        let Some(entry) = self.remaining.head_value() else {
            return Ok(BuiltinOutcome::Complete(Value::HashMap(self.output)));
        };
        let Value::Tuple(items) = entry else {
            return Err(RuntimeError::new(format!(
                "map_from_entries expects tuple entries at index {}, got {:?}",
                self.index, entry
            )));
        };
        if items.len() != 2 {
            return Err(RuntimeError::new(format!(
                "map_from_entries expects (String, V) tuples at index {}, got arity {}",
                self.index,
                items.len()
            )));
        }
        let mut items = items.into_iter();
        let key = items.next().unwrap();
        let Value::Str(key) = key else {
            return Err(RuntimeError::new(format!(
                "map_from_entries expects String key at index {}, got {:?}",
                self.index, key
            )));
        };
        self.output.entries.insert(key, items.next().unwrap());
        self.remaining = self
            .remaining
            .tail_handle()
            .expect("nonempty entries List tail");
        self.index = self
            .index
            .checked_add(1)
            .ok_or_else(|| RuntimeError::new("HashMap entry index overflow"))?;
        Ok(BuiltinOutcome::Resume(BuiltinContinuation::MapFromEntries(
            self,
        )))
    }
}

fn owned_map(value: Value, name: &str) -> Result<HashMapHandle, RuntimeError> {
    match value {
        Value::HashMap(map) => Ok(map),
        other => Err(RuntimeError::new(format!(
            "{name} expects HashMap as map, got {other:?}"
        ))),
    }
}

pub(super) fn builtin_map_values(
    _vm: &mut VM,
    args: Vec<Value>,
) -> Result<BuiltinOutcome, RuntimeError> {
    let mut args = args.into_iter();
    let map = owned_map(args.next().expect("builtin arity checked"), "map_values")?;
    let mapper = args.next().expect("builtin arity checked");
    let Value::Callable(mapper) = mapper else {
        return Err(RuntimeError::new(format!(
            "map_values expects callable value as f, got {mapper:?}"
        )));
    };
    SortedMapContinuation::start(map, SortedAction::Callback(mapper))
}

pub(super) fn builtin_map_keys(
    _vm: &mut VM,
    args: Vec<Value>,
) -> Result<BuiltinOutcome, RuntimeError> {
    SortedMapContinuation::start(
        owned_map(
            args.into_iter().next().expect("builtin arity checked"),
            "map_keys",
        )?,
        SortedAction::Keys,
    )
}

pub(super) fn builtin_map_values_list(
    _vm: &mut VM,
    args: Vec<Value>,
) -> Result<BuiltinOutcome, RuntimeError> {
    SortedMapContinuation::start(
        owned_map(
            args.into_iter().next().expect("builtin arity checked"),
            "map_values_list",
        )?,
        SortedAction::Values,
    )
}

pub(super) fn builtin_map_from_entries(
    _vm: &mut VM,
    args: Vec<Value>,
) -> Result<BuiltinOutcome, RuntimeError> {
    let Value::List(remaining) = args.into_iter().next().expect("builtin arity checked") else {
        return Err(RuntimeError::new(
            "map_from_entries expects List<(String, V)>",
        ));
    };
    MapFromEntriesContinuation {
        remaining,
        index: 0,
        output: HashMapHandle::empty(),
    }
    .resume(Ok(Value::Unit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sindr::ir::Bytecode;
    use sindr::primitives::int;
    use sindr::runtime::{CallableMetadata, CallableTarget};

    fn start(entries: Vec<(String, Value)>) -> BuiltinOutcome {
        let mut vm = VM::new(Bytecode::default());
        let mut outcome = builtin_map_values(
            &mut vm,
            vec![
                Value::HashMap(HashMapHandle::from_entries(entries)),
                Value::Callable(Callable {
                    target: CallableTarget::Function(0),
                    lexical_captures: vec![],
                    metadata: CallableMetadata::default(),
                }),
            ],
        )
        .expect("valid map and callback");
        while let BuiltinOutcome::Resume(continuation) = outcome {
            outcome = continuation.resume(&mut vm, Ok(Value::Unit)).unwrap();
        }
        outcome
    }

    #[test]
    fn map_values_moves_owned_payloads_and_yields_before_sorting() {
        let payload = "owned payload".repeat(64);
        let pointer = payload.as_ptr();
        let mut vm = VM::new(Bytecode::default());
        let mut outcome = builtin_map_values(
            &mut vm,
            vec![
                Value::HashMap(HashMapHandle::from_entries(vec![
                    ("b".into(), Value::Unit),
                    ("a".into(), Value::Str(payload)),
                ])),
                Value::Callable(Callable {
                    target: CallableTarget::Function(0),
                    lexical_captures: vec![],
                    metadata: CallableMetadata::default(),
                }),
            ],
        )
        .unwrap();
        assert!(
            matches!(outcome, BuiltinOutcome::Resume(_)),
            "sort must yield before starting a callback"
        );
        while let BuiltinOutcome::Resume(next) = outcome {
            outcome = next.resume(&mut vm, Ok(Value::Unit)).unwrap();
        }
        let BuiltinOutcome::Call { args, .. } = outcome else {
            panic!("expected sorted first callback")
        };
        let Value::Str(text) = &args[0] else {
            panic!("a must be first")
        };
        assert_eq!(
            text.as_ptr(),
            pointer,
            "preparation must move owned payloads"
        );
    }

    #[test]
    fn map_values_keeps_callback_result_buffer_when_adding_the_next_value() {
        let BuiltinOutcome::Call { continuation, .. } =
            start(vec![("a".into(), Value::Unit), ("b".into(), Value::Unit)])
        else {
            panic!("expected first callback")
        };
        let value = "callback result".repeat(32);
        let buffer = value.as_ptr();
        let mut vm = VM::new(Bytecode::default());
        let BuiltinOutcome::Call { continuation, .. } =
            continuation.resume(&mut vm, Ok(Value::Str(value))).unwrap()
        else {
            panic!("expected second callback")
        };
        let BuiltinOutcome::Complete(Value::HashMap(output)) =
            continuation.resume(&mut vm, Ok(Value::Unit)).unwrap()
        else {
            panic!("expected complete map")
        };
        let Value::Str(value) = &output.entries["a"] else {
            panic!("expected String")
        };
        assert_eq!(
            value.as_ptr(),
            buffer,
            "later output must not clone earlier callback results"
        );
    }

    #[test]
    fn map_values_cloned_partial_output_resumes_independently() {
        let BuiltinOutcome::Call { continuation, .. } =
            start(vec![("a".into(), Value::Unit), ("b".into(), Value::Unit)])
        else {
            panic!("expected first callback")
        };
        let mut vm = VM::new(Bytecode::default());
        let BuiltinOutcome::Call { continuation, .. } = continuation
            .resume(&mut vm, Ok(Value::Str("first".into())))
            .unwrap()
        else {
            panic!("expected second callback")
        };
        let saved = continuation.clone();
        let BuiltinOutcome::Complete(Value::HashMap(live)) = continuation
            .resume(&mut vm, Ok(Value::Str("live".into())))
            .unwrap()
        else {
            panic!("expected live complete")
        };
        let BuiltinOutcome::Complete(Value::HashMap(saved)) = saved
            .resume(&mut vm, Ok(Value::Str("saved".into())))
            .unwrap()
        else {
            panic!("expected saved complete")
        };
        assert_eq!(live.get("a"), Some(Value::Str("first".into())));
        assert_eq!(saved.get("a"), Some(Value::Str("first".into())));
        assert_eq!(live.get("b"), Some(Value::Str("live".into())));
        assert_eq!(saved.get("b"), Some(Value::Str("saved".into())));
    }

    #[test]
    fn map_values_visits_each_key_once_in_sorted_order_and_preserves_result_values() {
        let mut outcome = start(vec![
            ("b".into(), Value::Int(int(2))),
            ("a".into(), Value::Int(int(1))),
        ]);
        let result_value =
            super::super::err_result_from_rich_error(sindr::runtime::RichError::new(
                "NoneError",
                "no value",
                sindr::runtime::Location {
                    file: "test.srt".into(),
                    func: "mapper".into(),
                    line: 1,
                    column: 1,
                    span_start: 0,
                    span_end: 0,
                },
                None,
            ));
        for expected in [1, 2] {
            let BuiltinOutcome::Call {
                args,
                continuation: BuiltinContinuation::MapValues(next),
                ..
            } = outcome
            else {
                panic!("expected next callback");
            };
            assert_eq!(args, vec![Value::Int(int(expected))]);
            outcome = next
                .resume(Ok(result_value.clone()))
                .expect("Result is ordinary value");
        }
        let BuiltinOutcome::Complete(Value::HashMap(map)) = outcome else {
            panic!("expected complete map");
        };
        assert_eq!(
            map.sorted_entries(),
            vec![
                ("a".into(), result_value.clone()),
                ("b".into(), result_value)
            ]
        );
    }

    #[test]
    fn map_values_empty_never_requests_callback() {
        assert!(
            matches!(start(vec![]), BuiltinOutcome::Complete(Value::HashMap(map)) if map.len() == 0)
        );
    }

    #[test]
    fn map_values_callback_runtime_error_is_propagated_without_partial_success() {
        let BuiltinOutcome::Call {
            continuation: BuiltinContinuation::MapValues(next),
            ..
        } = start(vec![("a".into(), Value::Unit), ("b".into(), Value::Unit)])
        else {
            panic!("expected callback");
        };
        let outcome = next
            .resume(Ok(Value::Unit))
            .expect("first callback succeeds");
        let BuiltinOutcome::Call {
            continuation: BuiltinContinuation::MapValues(next),
            ..
        } = outcome
        else {
            panic!("expected second callback");
        };
        let err = next
            .resume(Err(RuntimeError::new("callback failed")))
            .expect_err("must preserve runtime failure");
        assert_eq!(err.message, "callback failed");
    }
    fn finish_identity(mut outcome: BuiltinOutcome) -> Vec<(String, Value)> {
        let mut vm = VM::new(Bytecode::default());
        let mut observed = Vec::new();
        loop {
            outcome = match outcome {
                BuiltinOutcome::Resume(next) => next.resume(&mut vm, Ok(Value::Unit)).unwrap(),
                BuiltinOutcome::Call {
                    mut args,
                    continuation: BuiltinContinuation::MapValues(next),
                    ..
                } => {
                    let value = args.remove(0);
                    observed.push((next.pending_key.clone(), value.clone()));
                    next.resume(Ok(value)).unwrap()
                }
                BuiltinOutcome::Complete(Value::HashMap(map)) => {
                    assert_eq!(map.sorted_entries(), observed);
                    return observed;
                }
                _ => panic!("unexpected map outcome"),
            };
        }
    }

    #[test]
    fn map_sort_boundaries_and_mid_sort_checkpoint_preserve_order() {
        for n in [0, 1, 2, 3, 127, 256, 513] {
            let expected: Vec<_> = (0..n)
                .map(|i| (format!("k{i:04}"), Value::Int(int(i))))
                .collect();
            let mut vm = VM::new(Bytecode::default());
            let mut outcome = builtin_map_values(
                &mut vm,
                vec![
                    Value::HashMap(HashMapHandle::from_entries(
                        expected.iter().rev().cloned().collect(),
                    )),
                    Value::Callable(Callable {
                        target: CallableTarget::Function(0),
                        lexical_captures: vec![],
                        metadata: CallableMetadata::default(),
                    }),
                ],
            )
            .unwrap();
            if n > 3 {
                for _ in 0..3 {
                    let BuiltinOutcome::Resume(next) = outcome else {
                        panic!("sort finished too early")
                    };
                    outcome = next.resume(&mut vm, Ok(Value::Unit)).unwrap();
                }
            }
            let saved = outcome.clone();
            assert_eq!(finish_identity(outcome), expected);
            assert_eq!(finish_identity(saved), expected);
        }
    }

    #[test]
    fn map_sort_error_prevents_callback_and_result_delivery() {
        let mut vm = VM::new(Bytecode::default());
        let BuiltinOutcome::Resume(next) = builtin_map_values(
            &mut vm,
            vec![
                Value::HashMap(HashMapHandle::from_entries(vec![
                    ("b".into(), Value::Unit),
                    ("a".into(), Value::Unit),
                ])),
                Value::Callable(Callable {
                    target: CallableTarget::Function(0),
                    lexical_captures: vec![],
                    metadata: CallableMetadata::default(),
                }),
            ],
        )
        .unwrap() else {
            panic!("expected preparation")
        };
        let error = next
            .resume(&mut vm, Err(RuntimeError::new("cancelled sort")))
            .unwrap_err();
        assert_eq!(error.message, "cancelled sort");
    }
}
