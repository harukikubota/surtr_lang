use super::{
    decode_callable_arg, decode_hash_map_arg, BuiltinContinuation, BuiltinOutcome, RuntimeError,
    Value, VM,
};
use sindr::runtime::{Callable, HashMapHandle};

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
        self.output = self.output.insert(self.pending_key, result?);
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

pub(super) fn builtin_map_values(
    _vm: &mut VM,
    args: Vec<Value>,
) -> Result<BuiltinOutcome, RuntimeError> {
    let map = decode_hash_map_arg(&args[0], "map_values", "map")?;
    let mapper = decode_callable_arg(&args[1], "map_values", "f")?;
    let mut remaining = map.sorted_entries().into_iter();
    match remaining.next() {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sindr::ir::Bytecode;
    use sindr::primitives::int;
    use sindr::runtime::{CallableMetadata, CallableTarget};

    fn start(entries: Vec<(String, Value)>) -> BuiltinOutcome {
        let mut vm = VM::new(Bytecode::default());
        builtin_map_values(
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
        .expect("valid map and callback")
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
}
