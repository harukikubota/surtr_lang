use super::list_builder::ListBuilder;
use super::{
    decode_callable_arg, option_none, option_some, BuiltinContinuation, BuiltinOutcome,
    RuntimeError, Value, VM,
};
use sindr::builtin::{builtin_id_by_name, OPTION_NONE_VARIANT, OPTION_SOME_VARIANT};
use sindr::primitives::{int, SurtrInt, ToPrimitive, Zero};
use sindr::runtime::{
    Callable, CallableTarget, GeneratorHandle, GeneratorProducer, InfiniteGeneratorHandle,
    InfiniteGeneratorProducer,
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) enum GeneratorContinuation {
    Next { source: Value },
    Materialize(MaterializeContinuation),
}
#[derive(Debug, Clone)]
pub(crate) struct MaterializeContinuation {
    source: Value,
    mode: MaterializeMode,
    builder: ListBuilder,
    phase: MaterializePhase,
}
#[derive(Debug, Clone)]
enum MaterializeMode {
    Count(SurtrInt),
    While(Callable),
    All,
}
#[derive(Debug, Clone)]
enum MaterializePhase {
    Start,
    Step,
    Predicate { item: Value, next: Value },
    Append { item: Value, next: Value },
}

fn pair(value: Value) -> Result<(Value, Value), RuntimeError> {
    let Value::Tuple(mut fields) = value else {
        return Err(RuntimeError::new(
            "generator step must return an (item, state) pair",
        ));
    };
    if fields.len() != 2 {
        return Err(RuntimeError::new(
            "generator step pair must contain exactly two fields",
        ));
    }
    let state = fields.pop().unwrap();
    Ok((fields.pop().unwrap(), state))
}
fn option(vm: &VM, value: Value) -> Result<Option<Value>, RuntimeError> {
    let Value::Tagged { tag, mut fields } = value else {
        return Err(RuntimeError::new(
            "finite generator step must return Option",
        ));
    };
    if vm
        .type_registry()
        .tag_by_name(OPTION_NONE_VARIANT.qualified_name)
        == Some(tag)
        && fields == [Value::Int(int(OPTION_NONE_VARIANT.discriminant))]
    {
        return Ok(None);
    }
    if vm
        .type_registry()
        .tag_by_name(OPTION_SOME_VARIANT.qualified_name)
        == Some(tag)
        && fields.len() == 2
        && fields[0] == Value::Int(int(OPTION_SOME_VARIANT.discriminant))
    {
        return Ok(fields.pop());
    }
    Err(RuntimeError::new(
        "finite generator step returned an invalid Option",
    ))
}
fn terminal() -> Value {
    Value::Generator(GeneratorHandle(Arc::new(GeneratorProducer::Terminal)))
}
fn finite_unfold(state: Value, step: Callable) -> Value {
    Value::Generator(GeneratorHandle(Arc::new(GeneratorProducer::Unfold {
        state,
        step,
    })))
}
fn infinite_unfold(state: Value, step: Callable) -> Value {
    Value::InfiniteGenerator(InfiniteGeneratorHandle(Arc::new(
        InfiniteGeneratorProducer::Unfold { state, step },
    )))
}
fn check_kind(value: &Value, infinite: bool) -> Result<(), RuntimeError> {
    if matches!(
        (value, infinite),
        (Value::Generator(_), false) | (Value::InfiniteGenerator(_), true)
    ) {
        Ok(())
    } else {
        Err(RuntimeError::new(if infinite {
            "expects InfiniteGenerator as gen"
        } else {
            "expects Generator as gen"
        }))
    }
}
impl GeneratorContinuation {
    pub(super) fn resume(
        self,
        vm: &mut VM,
        result: Result<Value, RuntimeError>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        let result = result?;
        match self {
            Self::Next { source } => {
                let value = match source {
                    Value::Generator(handle) => {
                        let GeneratorProducer::Unfold { step, .. } = handle.0.as_ref() else {
                            return Err(RuntimeError::new(
                                "invalid finite generator continuation state",
                            ));
                        };
                        match option(vm, result)? {
                            Some(value) => {
                                let (item, state) = pair(value)?;
                                option_some(
                                    vm,
                                    Value::Tuple(vec![item, finite_unfold(state, step.clone())]),
                                )?
                            }
                            None => option_none(vm)?,
                        }
                    }
                    Value::InfiniteGenerator(handle) => {
                        let InfiniteGeneratorProducer::Unfold { step, .. } = handle.0.as_ref();
                        let (item, state) = pair(result)?;
                        Value::Tuple(vec![item, infinite_unfold(state, step.clone())])
                    }
                    _ => return Err(RuntimeError::new("invalid generator continuation kind")),
                };
                Ok(BuiltinOutcome::Complete(value))
            }
            Self::Materialize(continuation) => continuation.resume(vm, result),
        }
    }
}
impl MaterializeContinuation {
    fn runnable(self) -> BuiltinOutcome {
        BuiltinOutcome::Resume(BuiltinContinuation::Generator(
            GeneratorContinuation::Materialize(self),
        ))
    }
    fn finish(self) -> BuiltinOutcome {
        let list = Value::List(self.builder.finish());
        BuiltinOutcome::Complete(match self.mode {
            MaterializeMode::All => list,
            _ => Value::Tuple(vec![list, self.source]),
        })
    }
    fn resume(mut self, vm: &mut VM, result: Value) -> Result<BuiltinOutcome, RuntimeError> {
        match std::mem::replace(&mut self.phase, MaterializePhase::Start) {
            MaterializePhase::Step => {
                let value = if matches!(self.source, Value::Generator(_)) {
                    option(vm, result)?
                } else {
                    Some(result)
                };
                let Some(value) = value else {
                    self.source = terminal();
                    return Ok(self.finish());
                };
                let (item, next) = pair(value)?;
                check_kind(&next, matches!(self.source, Value::InfiniteGenerator(_)))?;
                if let MaterializeMode::While(predicate) = &self.mode {
                    let callable = predicate.clone();
                    self.phase = MaterializePhase::Predicate {
                        item: item.clone(),
                        next,
                    };
                    return Ok(BuiltinOutcome::Call {
                        callable,
                        args: vec![item],
                        continuation: BuiltinContinuation::Generator(
                            GeneratorContinuation::Materialize(self),
                        ),
                    });
                }
                self.phase = MaterializePhase::Append { item, next };
                Ok(self.runnable())
            }
            MaterializePhase::Predicate { item, next } => {
                let Value::Bool(keep) = result else {
                    return Err(RuntimeError::new(
                        "generator take_while predicate must return Boolean",
                    ));
                };
                if !keep {
                    return Ok(self.finish());
                }
                self.phase = MaterializePhase::Append { item, next };
                Ok(self.runnable())
            }
            MaterializePhase::Append { item, next } => {
                self.builder.push(item)?;
                self.source = next;
                if let MaterializeMode::Count(remaining) = &mut self.mode {
                    *remaining -= 1;
                }
                Ok(self.runnable())
            }
            MaterializePhase::Start => {
                if matches!(&self.mode,MaterializeMode::Count(n) if n<=&int(0)) {
                    return Ok(self.finish());
                }
                let name = if matches!(self.source, Value::Generator(_)) {
                    "gen_step"
                } else {
                    "inf_gen_next"
                };
                let id = builtin_id_by_name(name)
                    .ok_or_else(|| RuntimeError::new("missing generator step builtin"))?;
                let args = vec![self.source.clone()];
                self.phase = MaterializePhase::Step;
                Ok(BuiltinOutcome::Call {
                    callable: Callable {
                        target: CallableTarget::Builtin(id),
                        lexical_captures: vec![],
                        metadata: Default::default(),
                    },
                    args,
                    continuation: BuiltinContinuation::Generator(
                        GeneratorContinuation::Materialize(self),
                    ),
                })
            }
        }
    }
}
pub(super) fn unfold(
    _vm: &mut VM,
    args: Vec<Value>,
    infinite: bool,
) -> Result<BuiltinOutcome, RuntimeError> {
    let step = decode_callable_arg(&args[1], "generator unfold", "step")?;
    Ok(BuiltinOutcome::Complete(if infinite {
        infinite_unfold(args[0].clone(), step)
    } else {
        finite_unfold(args[0].clone(), step)
    }))
}
pub(super) fn step(
    vm: &mut VM,
    args: Vec<Value>,
    infinite: bool,
) -> Result<BuiltinOutcome, RuntimeError> {
    let source = &args[0];
    check_kind(source, infinite)?;
    let (state, step) = match source {
        Value::Generator(handle) => match handle.0.as_ref() {
            GeneratorProducer::Terminal => return Ok(BuiltinOutcome::Complete(option_none(vm)?)),
            GeneratorProducer::Unfold { state, step } => (state.clone(), step.clone()),
            GeneratorProducer::Range {
                current,
                stop,
                step,
            }
            | GeneratorProducer::RangeChar {
                current,
                stop,
                step,
            } => {
                if step.is_zero() {
                    return Err(RuntimeError::new("generator range has zero step"));
                }
                if (step > &int(0) && current > stop) || (step < &int(0) && current < stop) {
                    return Ok(BuiltinOutcome::Complete(option_none(vm)?));
                }
                let is_char = matches!(handle.0.as_ref(), GeneratorProducer::RangeChar { .. });
                let item = if is_char {
                    let scalar = current
                        .to_u8()
                        .filter(|n| n.is_ascii())
                        .ok_or_else(|| RuntimeError::new("invalid character generator state"))?;
                    Value::Str(char::from(scalar).to_string())
                } else {
                    Value::Int(current.clone())
                };
                let producer = if is_char {
                    GeneratorProducer::RangeChar {
                        current: current + step,
                        stop: stop.clone(),
                        step: step.clone(),
                    }
                } else {
                    GeneratorProducer::Range {
                        current: current + step,
                        stop: stop.clone(),
                        step: step.clone(),
                    }
                };
                let rest = Value::Generator(GeneratorHandle(Arc::new(producer)));
                return Ok(BuiltinOutcome::Complete(option_some(
                    vm,
                    Value::Tuple(vec![item, rest]),
                )?));
            }
        },
        Value::InfiniteGenerator(handle) => {
            let InfiniteGeneratorProducer::Unfold { state, step } = handle.0.as_ref();
            (state.clone(), step.clone())
        }
        _ => unreachable!(),
    };
    Ok(BuiltinOutcome::Call {
        callable: step,
        args: vec![state],
        continuation: BuiltinContinuation::Generator(GeneratorContinuation::Next {
            source: source.clone(),
        }),
    })
}
pub(super) fn materialize(
    _vm: &mut VM,
    args: Vec<Value>,
    infinite: bool,
    operation: &str,
) -> Result<BuiltinOutcome, RuntimeError> {
    check_kind(&args[0], infinite)?;
    let mode = match operation {
        "take" => {
            let Value::Int(count) = &args[1] else {
                return Err(RuntimeError::new("generator take expects Int as count"));
            };
            MaterializeMode::Count(count.clone())
        }
        "take_while" => MaterializeMode::While(decode_callable_arg(
            &args[1],
            "generator take_while",
            "predicate",
        )?),
        "to_list" => MaterializeMode::All,
        _ => return Err(RuntimeError::new("invalid generator materialize operation")),
    };
    // This only caps the reservation, never the BigInt request or resulting length.
    let capacity = match &mode {
        MaterializeMode::Count(n) if n > &int(0) => n.to_usize().unwrap_or(1024).min(1024),
        _ => 0,
    };
    Ok(MaterializeContinuation {
        source: args[0].clone(),
        mode,
        builder: ListBuilder::with_capacity(capacity)?,
        phase: MaterializePhase::Start,
    }
    .runnable())
}
pub(super) fn range(
    _vm: &mut VM,
    args: Vec<Value>,
    chars: bool,
) -> Result<BuiltinOutcome, RuntimeError> {
    let (Value::Int(current), Value::Int(stop)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new("generator range expects Int endpoints"));
    };
    let step = if chars {
        let Value::Int(step) = &args[2] else {
            return Err(RuntimeError::new(
                "character generator range expects Int step",
            ));
        };
        step.clone()
    } else {
        int(1)
    };
    if chars
        && (step.is_zero()
            || current.to_u8().is_none_or(|n| !n.is_ascii())
            || stop.to_u8().is_none_or(|n| !n.is_ascii()))
    {
        return Err(RuntimeError::new("invalid validated character range"));
    }
    let producer = if chars {
        GeneratorProducer::RangeChar {
            current: current.clone(),
            stop: stop.clone(),
            step,
        }
    } else {
        GeneratorProducer::Range {
            current: current.clone(),
            stop: stop.clone(),
            step,
        }
    };
    Ok(BuiltinOutcome::Complete(Value::Generator(GeneratorHandle(
        Arc::new(producer),
    ))))
}
