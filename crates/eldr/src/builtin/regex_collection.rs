use super::list_builder::ListBuilder;
use super::list_finish::finish_list;
use super::{compile_cached_regex, decode_regex_arg, BuiltinContinuation, BuiltinOutcome};
use super::{RegexMatchHandle, RuntimeError, Value, VM};
use std::rc::Rc;

#[derive(Debug, Clone)]
enum Action {
    FindAll(ListBuilder),
    Split {
        output: ListBuilder,
        end: usize,
    },
    ReplaceAll {
        output: String,
        replacement: String,
        needs_captures: bool,
        end: usize,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct RegexCollectionContinuation {
    regex: regex::Regex,
    input: Rc<String>,
    start: usize,
    last_match_end: Option<usize>,
    action: Action,
}

impl RegexCollectionContinuation {
    fn pending(self) -> BuiltinOutcome {
        BuiltinOutcome::Resume(BuiltinContinuation::RegexCollection(self))
    }

    pub(super) fn resume(
        self,
        result: Result<Value, RuntimeError>,
    ) -> Result<BuiltinOutcome, RuntimeError> {
        result?;
        self.resume_steps(1).0
    }

    pub(crate) fn resume_steps(
        mut self,
        max_steps: u64,
    ) -> (Result<BuiltinOutcome, RuntimeError>, u64) {
        let mut used = 0;
        while used < max_steps {
            used += 1;
            match self.step() {
                Ok(true) => return (self.complete(), used),
                Ok(false) => {}
                Err(error) => return (Err(error), used),
            }
        }
        (Ok(self.pending()), used)
    }

    /// At most one search. An overlapping empty match consumes its own step.
    fn step(&mut self) -> Result<bool, RuntimeError> {
        if self.start > self.input.len() {
            return Ok(true);
        }
        let captures = matches!(
            self.action,
            Action::ReplaceAll {
                needs_captures: true,
                ..
            }
        )
        .then(|| self.regex.captures_at(&self.input, self.start));
        let matched = match captures.as_ref() {
            Some(Some(captures)) => captures.get(0),
            Some(None) => None,
            None => self.regex.find_at(&self.input, self.start),
        };
        let Some(matched) = matched else {
            return Ok(true);
        };
        let (start, end) = (matched.start(), matched.end());
        if start == end && self.last_match_end == Some(end) {
            self.start = self
                .start
                .checked_add(1)
                .ok_or_else(|| RuntimeError::new("Regex search cursor overflow"))?;
            return Ok(false);
        }
        self.start = end;
        self.last_match_end = Some(end);
        match &mut self.action {
            Action::FindAll(output) => output.push(Value::RegexMatch(RegexMatchHandle {
                input: Rc::clone(&self.input),
                start,
                end,
            }))?,
            Action::Split {
                output,
                end: previous,
            } => {
                output.push(Value::Str(self.input[*previous..start].to_owned()))?;
                *previous = end;
            }
            Action::ReplaceAll {
                output,
                replacement,
                needs_captures,
                end: previous,
            } => {
                output.push_str(&self.input[*previous..start]);
                if *needs_captures {
                    captures
                        .as_ref()
                        .expect("replacement search captures")
                        .as_ref()
                        .expect("matched replacement captures")
                        .expand(replacement, output);
                } else {
                    output.push_str(replacement);
                }
                *previous = end;
            }
        }
        Ok(false)
    }

    fn complete(self) -> Result<BuiltinOutcome, RuntimeError> {
        match self.action {
            Action::FindAll(output) => finish_list(output, None),
            Action::Split { mut output, end } => {
                output.push(Value::Str(self.input[end..].to_owned()))?;
                finish_list(output, None)
            }
            Action::ReplaceAll {
                mut output, end, ..
            } => {
                output.push_str(&self.input[end..]);
                Ok(BuiltinOutcome::Complete(Value::Str(output)))
            }
        }
    }
}

fn start(args: Vec<Value>, name: &str) -> Result<BuiltinOutcome, RuntimeError> {
    let mut args = args.into_iter();
    let regex = args.next().expect("builtin arity checked");
    let pattern = decode_regex_arg(&regex, name, "re")?;
    let input = args.next().expect("builtin arity checked");
    let Value::Str(input) = input else {
        return Err(RuntimeError::new(format!("{name} expects String as input")));
    };
    let action = match name {
        "find_all" => Action::FindAll(ListBuilder::default()),
        "split" => Action::Split {
            output: ListBuilder::default(),
            end: 0,
        },
        "replace_all" => {
            let replacement = args.next().expect("builtin arity checked");
            let Value::Str(replacement) = replacement else {
                return Err(RuntimeError::new(
                    "replace_all expects String as replacement",
                ));
            };
            Action::ReplaceAll {
                output: String::with_capacity(input.len()),
                needs_captures: replacement.contains('$'),
                replacement,
                end: 0,
            }
        }
        _ => unreachable!("registered Regex collection builtin"),
    };
    Ok(RegexCollectionContinuation {
        regex: compile_cached_regex(pattern, name)?,
        input: Rc::new(input),
        start: 0,
        last_match_end: None,
        action,
    }
    .pending())
}

pub(super) fn builtin_regex_find_all(
    _vm: &mut VM,
    args: Vec<Value>,
) -> Result<BuiltinOutcome, RuntimeError> {
    start(args, "find_all")
}

pub(super) fn builtin_regex_split(
    _vm: &mut VM,
    args: Vec<Value>,
) -> Result<BuiltinOutcome, RuntimeError> {
    start(args, "split")
}

pub(super) fn builtin_regex_replace_all(
    _vm: &mut VM,
    args: Vec<Value>,
) -> Result<BuiltinOutcome, RuntimeError> {
    start(args, "replace_all")
}
