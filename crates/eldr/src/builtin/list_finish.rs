use super::list_builder::{ListBuilder, ListBuilderFinish, ListFinishStep};
use super::{BuiltinContinuation, BuiltinOutcome, RuntimeError, Value};

/// The completed mapper/producer stays outside the budgeted prefix traversal.
#[derive(Debug, Clone)]
pub(crate) struct ListFinishContinuation {
    builder: ListBuilderFinish,
    rest: Option<Value>,
}

pub(super) fn finish_list(
    builder: ListBuilder,
    rest: Option<Value>,
) -> Result<BuiltinOutcome, RuntimeError> {
    ListFinishContinuation {
        builder: builder.finish(),
        rest,
    }
    .resume(Ok(Value::Unit))
}

impl ListFinishContinuation {
    pub(crate) fn resume(
        self,
        result: Result<Value, RuntimeError>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        result?;
        self.resume_steps(1).0
    }

    /// Keep the typed cursor local; every attempted transition still costs one.
    pub(crate) fn resume_steps(
        mut self,
        max_steps: u64,
    ) -> (Result<BuiltinOutcome, RuntimeError>, u64) {
        let mut used = 0;
        while used < max_steps {
            used += 1;
            match self.builder.resume() {
                Ok(ListFinishStep::Complete(list)) => {
                    let list = Value::List(list);
                    return (
                        Ok(BuiltinOutcome::Complete(match self.rest {
                            Some(rest) => Value::Tuple(vec![list, rest]),
                            None => list,
                        })),
                        used,
                    );
                }
                Ok(ListFinishStep::Pending(builder)) => self.builder = builder,
                Err(error) => return (Err(error), used),
            }
        }
        (
            Ok(BuiltinOutcome::Resume(BuiltinContinuation::ListFinish(
                self,
            ))),
            used,
        )
    }
}
