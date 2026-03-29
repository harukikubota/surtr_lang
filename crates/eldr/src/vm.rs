use forge::bytecode::{Bytecode, BytecodeChunk};
use forge::opcode::Opcode;
use forge::registry::TypeRegistry;

use crate::error::RuntimeError;
use crate::value::{Callable, CallableTarget, Value};

#[derive(Debug, Clone)]
struct CallFrame {
    return_pc: usize,
    stack_base: usize,
    call_site: Option<(u32, u32)>,
    locals: Vec<Value>,
}

/// The Surtr virtual machine — executes bytecode produced by Forge.
pub struct VM {
    bytecode: Bytecode,
    /// Operand stack
    stack: Vec<Value>,
    /// Call stack / locals frames
    frames: Vec<CallFrame>,
    /// Program counter (used by full-program `run`)
    pc: usize,
    /// Source code (for eprint / ariadne)
    source: Option<String>,
    /// Source file name
    source_file: Option<String>,
    /// Captured stdout (for testing). `None` = print to real stdout.
    pub output: Option<Vec<String>>,
    /// Captured stderr (for testing). `None` = print to real stderr.
    pub error_output: Option<Vec<String>>,
}

impl VM {
    pub fn new(bytecode: Bytecode) -> Self {
        let num_locals = bytecode.num_locals;
        Self {
            bytecode,
            stack: Vec::new(),
            frames: vec![CallFrame {
                return_pc: 0,
                stack_base: 0,
                call_site: None,
                locals: vec![Value::Unit; num_locals],
            }],
            pc: 0,
            source: None,
            source_file: None,
            output: None,
            error_output: None,
        }
    }

    /// Create an empty VM intended for REPL/incremental execution.
    pub fn new_interactive(type_registry: TypeRegistry) -> Self {
        Self::new(Bytecode {
            opcodes: Vec::new(),
            constants: Vec::new(),
            num_locals: 0,
            type_registry,
            error_templates: Vec::new(),
            functions: Vec::new(),
        })
    }

    /// Set source code for error reporting.
    pub fn with_source(mut self, source: String, file: String) -> Self {
        self.source = Some(source);
        self.source_file = Some(file);
        self
    }

    /// Access source text if attached.
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// Access source file name if attached.
    pub fn source_file(&self) -> Option<&str> {
        self.source_file.as_deref()
    }

    /// Enable stdout capture (for testing).
    pub fn with_output_capture(mut self) -> Self {
        self.output = Some(Vec::new());
        self
    }

    /// Enable stderr capture (for testing).
    pub fn with_error_capture(mut self) -> Self {
        self.error_output = Some(Vec::new());
        self
    }

    /// Access the type registry (used by builtins). Returns a clone to avoid borrow issues.
    pub fn type_registry(&self) -> TypeRegistry {
        self.bytecode.type_registry.clone()
    }

    /// Read a local slot value (used by REPL display logic).
    pub fn get_local(&self, slot: u32) -> Option<Value> {
        self.frames
            .last()
            .and_then(|frame| frame.locals.get(slot as usize).cloned())
    }

    /// Execute the loaded bytecode (`run` mode expects `Halt`).
    pub fn run(&mut self) -> Result<(), RuntimeError> {
        loop {
            if self.pc >= self.bytecode.opcodes.len() {
                return Err(RuntimeError {
                    message: "PC out of bounds".into(),
                });
            }
            let op = self.bytecode.opcodes[self.pc].clone();
            let mut next_pc = self.pc + 1;
            let halted = self.execute_opcode(op, &mut next_pc)?;
            self.pc = next_pc;

            if halted {
                return Ok(());
            }
        }
    }

    /// Execute an incremental bytecode chunk and return the final stack top.
    /// If the stack is empty at the end, returns `Unit`.
    pub fn push(&mut self, chunk: BytecodeChunk) -> Result<Value, RuntimeError> {
        let code_base = self.bytecode.opcodes.len();
        self.bytecode.constants.extend(chunk.constants);
        self.bytecode.type_registry.entries.extend(chunk.type_entries);
        self.bytecode.error_templates.extend(chunk.error_templates);
        self.bytecode.opcodes.extend(chunk.opcodes);
        for mut entry in chunk.functions {
            entry.entry_pc += code_base as u32;
            self.bytecode.functions.push(entry);
        }
        if let Some(frame) = self.frames.first_mut() {
            frame
                .locals
                .extend(std::iter::repeat_n(Value::Unit, chunk.new_locals));
        }

        let mut pc = code_base;
        while pc < self.bytecode.opcodes.len() {
            let op = self.bytecode.opcodes[pc].clone();
            pc += 1;
            let halted = self.execute_opcode(op, &mut pc)?;
            if halted {
                break;
            }
        }

        let result = self.stack.pop().unwrap_or(Value::Unit);
        self.stack.clear();
        Ok(result)
    }

    fn execute_opcode(&mut self, op: Opcode, pc: &mut usize) -> Result<bool, RuntimeError> {
        use crate::builtin::BUILTINS;
        use forge::bytecode::Constant;

        match op {
            Opcode::Halt => return Ok(true),

            Opcode::LoadConst(idx) => {
                let c = &self.bytecode.constants[idx as usize];
                let val = match c {
                    Constant::Int(n) => Value::Int(*n),
                    Constant::Float(f) => Value::Float(*f),
                    Constant::Str(s) => Value::Str(s.clone()),
                    Constant::Bool(b) => Value::Bool(*b),
                    Constant::Unit => Value::Unit,
                };
                self.stack.push(val);
            }

            Opcode::LoadBuiltinRef(builtin_id) => {
                self.stack.push(Value::Callable(Callable {
                    target: CallableTarget::Builtin(builtin_id),
                    captured: Vec::new(),
                }));
            }

            Opcode::LoadFunctionRef(fun_idx) => {
                self.stack.push(Value::Callable(Callable {
                    target: CallableTarget::Function(fun_idx),
                    captured: Vec::new(),
                }));
            }

            Opcode::LoadLocal(slot) => {
                let val = self
                    .current_frame()
                    .locals
                    .get(slot as usize)
                    .cloned()
                    .unwrap_or(Value::Unit);
                self.stack.push(val);
            }

            Opcode::StoreLocal(slot) => {
                let val = self.pop_stack()?;
                // Grow locals if needed
                let frame = self.current_frame_mut();
                while frame.locals.len() <= slot as usize {
                    frame.locals.push(Value::Unit);
                }
                frame.locals[slot as usize] = val;
            }

            Opcode::Pop => {
                self.pop_stack()?;
            }

            // ── Arithmetic (Int) ──
            Opcode::AddInt => self.int_binop(|a, b| Ok(Value::Int(a + b)))?,
            Opcode::SubInt => self.int_binop(|a, b| Ok(Value::Int(a - b)))?,
            Opcode::MulInt => self.int_binop(|a, b| Ok(Value::Int(a * b)))?,
            Opcode::DivInt => self.int_binop(|a, b| {
                if b == 0 {
                    Err(RuntimeError {
                        message: "Division by zero".into(),
                    })
                } else {
                    Ok(Value::Int(a / b))
                }
            })?,
            Opcode::ModInt => self.int_binop(|a, b| {
                if b == 0 {
                    Err(RuntimeError {
                        message: "Modulo by zero".into(),
                    })
                } else {
                    Ok(Value::Int(a % b))
                }
            })?,

            // ── Arithmetic (Float) ──
            Opcode::AddFloat => self.float_binop(|a, b| Value::Float(a + b))?,
            Opcode::SubFloat => self.float_binop(|a, b| Value::Float(a - b))?,
            Opcode::MulFloat => self.float_binop(|a, b| Value::Float(a * b))?,
            Opcode::DivFloat => self.float_binop(|a, b| Value::Float(a / b))?,

            // ── Comparison (Int) ──
            Opcode::EqInt => self.int_binop(|a, b| Ok(Value::Bool(a == b)))?,
            Opcode::NeqInt => self.int_binop(|a, b| Ok(Value::Bool(a != b)))?,
            Opcode::LtInt => self.int_binop(|a, b| Ok(Value::Bool(a < b)))?,
            Opcode::GtInt => self.int_binop(|a, b| Ok(Value::Bool(a > b)))?,
            Opcode::LteInt => self.int_binop(|a, b| Ok(Value::Bool(a <= b)))?,
            Opcode::GteInt => self.int_binop(|a, b| Ok(Value::Bool(a >= b)))?,

            // ── Comparison (Float) ──
            Opcode::EqFloat => self.float_binop(|a, b| Value::Bool(a == b))?,
            Opcode::NeqFloat => self.float_binop(|a, b| Value::Bool(a != b))?,
            Opcode::LtFloat => self.float_binop(|a, b| Value::Bool(a < b))?,
            Opcode::GtFloat => self.float_binop(|a, b| Value::Bool(a > b))?,
            Opcode::LteFloat => self.float_binop(|a, b| Value::Bool(a <= b))?,
            Opcode::GteFloat => self.float_binop(|a, b| Value::Bool(a >= b))?,

            // ── Comparison (String) ──
            Opcode::EqStr => {
                let b = self.pop_str()?;
                let a = self.pop_str()?;
                self.stack.push(Value::Bool(a == b));
            }
            Opcode::NeqStr => {
                let b = self.pop_str()?;
                let a = self.pop_str()?;
                self.stack.push(Value::Bool(a != b));
            }

            // ── Comparison (Bool) ──
            Opcode::EqBool => {
                let b = self.pop_bool()?;
                let a = self.pop_bool()?;
                self.stack.push(Value::Bool(a == b));
            }
            Opcode::NeqBool => {
                let b = self.pop_bool()?;
                let a = self.pop_bool()?;
                self.stack.push(Value::Bool(a != b));
            }

            // ── String ──
            Opcode::ConcatStr => {
                let b = self.pop_str()?;
                let a = self.pop_str()?;
                self.stack.push(Value::Str(a + &b));
            }

            // ── Unary ──
            Opcode::NegInt => {
                let a = self.pop_int()?;
                self.stack.push(Value::Int(-a));
            }
            Opcode::NegFloat => {
                let a = self.pop_float()?;
                self.stack.push(Value::Float(-a));
            }
            Opcode::NotBool => {
                let a = self.pop_bool()?;
                self.stack.push(Value::Bool(!a));
            }

            // ── List ──
            Opcode::ListNew(n) => {
                let mut elems = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    elems.push(self.pop_stack()?);
                }
                elems.reverse();
                self.stack.push(Value::List(elems));
            }
            Opcode::ListEmpty => {
                self.stack.push(Value::List(Vec::new()));
            }

            // ── Struct / Tagged ──
            Opcode::StructNew(num_fields) => {
                let mut fields = Vec::with_capacity(num_fields as usize);
                for _ in 0..num_fields {
                    fields.push(self.pop_stack()?);
                }
                fields.reverse();
                let tag_val = self.pop_stack()?;
                let tag = match tag_val {
                    Value::Int(t) => t as u32,
                    _ => {
                        return Err(RuntimeError {
                            message: "StructNew: expected Int tag".into(),
                        });
                    }
                };
                self.stack.push(Value::Tagged { tag, fields });
            }
            Opcode::GetField(idx) => {
                let val = self.pop_stack()?;
                match val {
                    Value::Tagged { fields, .. } => {
                        if (idx as usize) < fields.len() {
                            self.stack.push(fields[idx as usize].clone());
                        } else {
                            return Err(RuntimeError {
                                message: format!("Field index {} out of bounds", idx),
                            });
                        }
                    }
                    _ => {
                        return Err(RuntimeError {
                            message: "GetField on non-tagged value".into(),
                        });
                    }
                }
            }
            Opcode::GetTag => {
                let val = self.pop_stack()?;
                match val {
                    Value::Tagged { tag, .. } => {
                        self.stack.push(Value::Int(tag as i64));
                    }
                    _ => {
                        return Err(RuntimeError {
                            message: "GetTag on non-tagged value".into(),
                        });
                    }
                }
            }

            // ── Built-in function call ──
            Opcode::CallBuiltin(builtin_id, arity) => {
                let mut args = Vec::with_capacity(arity as usize);
                for _ in 0..arity {
                    args.push(self.pop_stack()?);
                }
                args.reverse();
                let builtin = &BUILTINS[builtin_id as usize];
                let result = (builtin.func)(self, args)?;
                self.stack.push(result);
            }

            Opcode::Call(fun_idx, arity, span_start, span_end) => {
                let entry = self
                    .bytecode
                    .functions
                    .get(fun_idx as usize)
                    .ok_or_else(|| RuntimeError {
                        message: format!("Unknown function index: {}", fun_idx),
                    })?;
                if entry.arity != arity {
                    return Err(RuntimeError {
                        message: format!(
                            "Call arity mismatch for function {}: expected {}, got {}",
                            fun_idx, entry.arity, arity
                        ),
                    });
                }
                if self.stack.len() < arity as usize {
                    return Err(RuntimeError {
                        message: "Stack underflow".into(),
                    });
                }
                let stack_base = self.stack.len() - arity as usize;
                let return_pc = *pc;
                self.frames.push(CallFrame {
                    return_pc,
                    stack_base,
                    call_site: Some((span_start, span_end)),
                    locals: Vec::new(),
                });
                *pc = entry.entry_pc as usize;
            }

            Opcode::MakeError(template_id) => {
                let message = match self.pop_stack()? {
                    Value::Str(s) => s,
                    other => {
                        return Err(RuntimeError {
                            message: format!("MakeError expects String, got {:?}", other),
                        });
                    }
                };
                let template = self
                    .bytecode
                    .error_templates
                    .get(template_id as usize)
                    .ok_or_else(|| RuntimeError {
                        message: format!("Unknown error template: {}", template_id),
                    })?;
                let call_site = self.current_frame().call_site.clone();
                let (span_start, span_end) = call_site
                    .map(|(start, end)| (start, end))
                    .unwrap_or((template.span_start, template.span_end));
                let location = crate::value::Location {
                    file: self
                        .source_file()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "<repl>".to_string()),
                    func: template.kind.clone(),
                    line: template.line,
                    column: template.column,
                    span_start,
                    span_end,
                };
                self.stack.push(Value::Error(Box::new(crate::value::RichError {
                    kind: template.kind.clone(),
                    message,
                    location,
                })));
            }

            Opcode::MakeClosure(num_captured) => {
                let mut captured = Vec::with_capacity(num_captured as usize);
                for _ in 0..num_captured {
                    captured.push(self.pop_stack()?);
                }
                captured.reverse();
                let target = self.pop_stack()?;
                let callable = match target {
                    Value::Callable(mut callable) => {
                        let mut merged = callable.captured;
                        merged.extend(captured);
                        callable.captured = merged;
                        callable
                    }
                    _ => {
                        return Err(RuntimeError {
                            message: "MakeClosure expects a callable target".into(),
                        });
                    }
                };
                self.stack.push(Value::Callable(callable));
            }

            Opcode::CallClosure(arity, span_start, span_end) => {
                let mut args = Vec::with_capacity(arity as usize);
                for _ in 0..arity {
                    args.push(self.pop_stack()?);
                }
                args.reverse();
                let callable = match self.pop_stack()? {
                    Value::Callable(callable) => callable,
                    _ => {
                        return Err(RuntimeError {
                            message: "CallClosure expects a callable value".into(),
                        });
                    }
                };

                let mut full_args = callable.captured.clone();
                full_args.extend(args);

                match callable.target {
                    CallableTarget::Builtin(builtin_id) => {
                        use crate::builtin::BUILTINS;
                        let builtin = &BUILTINS[builtin_id as usize];
                        if usize::from(builtin.arity) != full_args.len() {
                            return Err(RuntimeError {
                                message: format!(
                                    "builtin {} arity mismatch: expected {}, got {}",
                                    builtin.name,
                                    builtin.arity,
                                    full_args.len()
                                ),
                            });
                        }
                        let result = (builtin.func)(self, full_args)?;
                        self.stack.push(result);
                    }
                    CallableTarget::Function(fun_idx) => {
                        let entry = self
                            .bytecode
                            .functions
                            .get(fun_idx as usize)
                            .ok_or_else(|| RuntimeError {
                                message: format!("Unknown function index: {}", fun_idx),
                            })?;
                        if entry.arity as usize != full_args.len() {
                            return Err(RuntimeError {
                                message: format!(
                                    "Call arity mismatch for function {}: expected {}, got {}",
                                    fun_idx,
                                    entry.arity,
                                    full_args.len()
                                ),
                            });
                        }
                let stack_base = self.stack.len();
                self.stack.extend(full_args);
                let return_pc = *pc;
                self.frames.push(CallFrame {
                    return_pc,
                    stack_base,
                    call_site: Some((span_start, span_end)),
                    locals: Vec::new(),
                });
                *pc = entry.entry_pc as usize;
            }
                }
            }

            // ── Control flow ──
            Opcode::Jump(addr) => {
                *pc = addr as usize;
            }
            Opcode::JumpIfFalse(addr) => {
                let val = self.pop_stack()?;
                match val {
                    Value::Bool(false) => *pc = addr as usize,
                    Value::Bool(true) => {} // fall through
                    _ => {
                        return Err(RuntimeError {
                            message: "JumpIfFalse: expected Bool".into(),
                        });
                    }
                }
            }
            Opcode::JumpIfTrue(addr) => {
                let val = self.pop_stack()?;
                match val {
                    Value::Bool(true) => *pc = addr as usize,
                    Value::Bool(false) => {} // fall through
                    _ => {
                        return Err(RuntimeError {
                            message: "JumpIfTrue: expected Bool".into(),
                        });
                    }
                }
            }

            // ── Frame management ──
            Opcode::MakeFrame(num_locals) => {
                self.current_frame_mut().locals = vec![Value::Unit; num_locals as usize];
            }
            Opcode::PopFrame => {
                self.current_frame_mut().locals.clear();
            }

            // ── Return ──
            Opcode::Return => {
                let ret = self.stack.pop().unwrap_or(Value::Unit);
                let frame = self.frames.pop().ok_or_else(|| RuntimeError {
                    message: "Return with empty frame stack".into(),
                })?;
                self.stack.truncate(frame.stack_base);
                self.stack.push(ret);
                *pc = frame.return_pc;
            }
        }

        Ok(false)
    }

    // ── Stack helpers ──

    fn pop_stack(&mut self) -> Result<Value, RuntimeError> {
        self.stack.pop().ok_or_else(|| RuntimeError {
            message: "Stack underflow".into(),
        })
    }

    fn current_frame(&self) -> &CallFrame {
        self.frames
            .last()
            .expect("VM always has at least one frame")
    }

    fn current_frame_mut(&mut self) -> &mut CallFrame {
        self.frames
            .last_mut()
            .expect("VM always has at least one frame")
    }

    fn pop_int(&mut self) -> Result<i64, RuntimeError> {
        match self.pop_stack()? {
            Value::Int(n) => Ok(n),
            other => Err(RuntimeError {
                message: format!("Expected Int, got {:?}", other),
            }),
        }
    }

    fn pop_float(&mut self) -> Result<f64, RuntimeError> {
        match self.pop_stack()? {
            Value::Float(f) => Ok(f),
            other => Err(RuntimeError {
                message: format!("Expected Float, got {:?}", other),
            }),
        }
    }

    fn pop_str(&mut self) -> Result<String, RuntimeError> {
        match self.pop_stack()? {
            Value::Str(s) => Ok(s),
            other => Err(RuntimeError {
                message: format!("Expected Str, got {:?}", other),
            }),
        }
    }

    fn pop_bool(&mut self) -> Result<bool, RuntimeError> {
        match self.pop_stack()? {
            Value::Bool(b) => Ok(b),
            other => Err(RuntimeError {
                message: format!("Expected Bool, got {:?}", other),
            }),
        }
    }

    fn int_binop<F>(&mut self, f: F) -> Result<(), RuntimeError>
    where
        F: FnOnce(i64, i64) -> Result<Value, RuntimeError>,
    {
        let b = self.pop_int()?;
        let a = self.pop_int()?;
        let result = f(a, b)?;
        self.stack.push(result);
        Ok(())
    }

    fn float_binop<F>(&mut self, f: F) -> Result<(), RuntimeError>
    where
        F: FnOnce(f64, f64) -> Value,
    {
        let b = self.pop_float()?;
        let a = self.pop_float()?;
        self.stack.push(f(a, b));
        Ok(())
    }
}
