use sindr::ir::{Bytecode, BytecodeChunk, Constant, FunctionEntry, Opcode};
use sindr::runtime::{Callable, CallableTarget, Location, RichError, TypeRegistry, Value};

use crate::builtin::call_builtin;
use crate::error::RuntimeError;

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
            source_map: None,
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

    /// Access the type registry (used by builtins).
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
        self.bytecode
            .type_registry
            .entries
            .extend(chunk.type_entries);
        self.bytecode.error_templates.extend(chunk.error_templates);
        self.bytecode.opcodes.extend(chunk.opcodes);
        for mut entry in chunk.functions {
            entry.entry_pc += code_base as u32;
            let idx = entry.fun_idx as usize;
            if idx == self.bytecode.functions.len() {
                self.bytecode.functions.push(entry);
            } else if idx < self.bytecode.functions.len() {
                self.bytecode.functions[idx] = entry;
            } else {
                return Err(RuntimeError {
                    message: format!(
                        "Function table invariant violated in chunk: fun_idx {} > len {}",
                        idx,
                        self.bytecode.functions.len()
                    ),
                });
            }
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
        match op {
            Opcode::Halt => return Ok(true),

            Opcode::LoadConst(idx) => {
                let c = self
                    .bytecode
                    .constants
                    .get(idx as usize)
                    .ok_or_else(|| RuntimeError {
                        message: format!("LoadConst index out of bounds: {}", idx),
                    })?;
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
                    .current_frame()?
                    .locals
                    .get(slot as usize)
                    .cloned()
                    .ok_or_else(|| RuntimeError {
                        message: format!("LoadLocal out of bounds: {}", slot),
                    })?;
                self.stack.push(val);
            }

            Opcode::StoreLocal(slot) => {
                let val = self.pop_stack()?;
                let target = self
                    .current_frame_mut()?
                    .locals
                    .get_mut(slot as usize)
                    .ok_or_else(|| RuntimeError {
                        message: format!("StoreLocal out of bounds: {}", slot),
                    })?;
                *target = val;
            }

            Opcode::Pop => {
                self.pop_stack()?;
            }

            // Arithmetic (Int)
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

            // Arithmetic (Float)
            Opcode::AddFloat => self.float_binop(|a, b| Value::Float(a + b))?,
            Opcode::SubFloat => self.float_binop(|a, b| Value::Float(a - b))?,
            Opcode::MulFloat => self.float_binop(|a, b| Value::Float(a * b))?,
            Opcode::DivFloat => self.float_binop(|a, b| Value::Float(a / b))?,

            // Comparison (Int)
            Opcode::EqInt => self.int_binop(|a, b| Ok(Value::Bool(a == b)))?,
            Opcode::NeqInt => self.int_binop(|a, b| Ok(Value::Bool(a != b)))?,
            Opcode::LtInt => self.int_binop(|a, b| Ok(Value::Bool(a < b)))?,
            Opcode::GtInt => self.int_binop(|a, b| Ok(Value::Bool(a > b)))?,
            Opcode::LteInt => self.int_binop(|a, b| Ok(Value::Bool(a <= b)))?,
            Opcode::GteInt => self.int_binop(|a, b| Ok(Value::Bool(a >= b)))?,

            // Comparison (Float)
            Opcode::EqFloat => self.float_binop(|a, b| Value::Bool(a == b))?,
            Opcode::NeqFloat => self.float_binop(|a, b| Value::Bool(a != b))?,
            Opcode::LtFloat => self.float_binop(|a, b| Value::Bool(a < b))?,
            Opcode::GtFloat => self.float_binop(|a, b| Value::Bool(a > b))?,
            Opcode::LteFloat => self.float_binop(|a, b| Value::Bool(a <= b))?,
            Opcode::GteFloat => self.float_binop(|a, b| Value::Bool(a >= b))?,

            // Comparison (String)
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

            // Comparison (Bool)
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

            // String
            Opcode::ConcatStr => {
                let b = self.pop_str()?;
                let a = self.pop_str()?;
                self.stack.push(Value::Str(a + &b));
            }

            // Unary
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

            // List
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

            // Struct / Tagged
            Opcode::StructNew(num_fields) => {
                let mut fields = Vec::with_capacity(num_fields as usize);
                for _ in 0..num_fields {
                    fields.push(self.pop_stack()?);
                }
                fields.reverse();
                let tag_val = self.pop_stack()?;
                let tag = match tag_val {
                    Value::Int(tag) => u32::try_from(tag).map_err(|_| RuntimeError {
                        message: format!("StructNew: invalid tag value {}", tag),
                    })?,
                    other => {
                        return Err(RuntimeError {
                            message: format!("StructNew: expected Int tag, got {:?}", other),
                        });
                    }
                };
                self.stack.push(Value::Tagged { tag, fields });
            }
            Opcode::GetField(idx) => {
                let val = self.pop_stack()?;
                match val {
                    Value::Tagged { fields, .. } => {
                        let field =
                            fields
                                .get(idx as usize)
                                .cloned()
                                .ok_or_else(|| RuntimeError {
                                    message: format!("Field index {} out of bounds", idx),
                                })?;
                        self.stack.push(field);
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

            // Built-in function call
            Opcode::CallBuiltin(builtin_id, arity) => {
                let mut args = Vec::with_capacity(arity as usize);
                for _ in 0..arity {
                    args.push(self.pop_stack()?);
                }
                args.reverse();
                let result = call_builtin(self, builtin_id, args)?;
                self.stack.push(result);
            }

            Opcode::Call(fun_idx, arity, span_start, span_end) => {
                let entry = self.function_entry(fun_idx)?.clone();
                if entry.arity != arity {
                    return Err(RuntimeError {
                        message: format!(
                            "Call arity mismatch for function {}: expected {}, got {}",
                            fun_idx, entry.arity, arity
                        ),
                    });
                }

                let mut args = Vec::with_capacity(arity as usize);
                for _ in 0..arity {
                    args.push(self.pop_stack()?);
                }
                args.reverse();

                if entry.entry_pc as usize >= self.bytecode.opcodes.len() {
                    return Err(RuntimeError {
                        message: format!(
                            "Function {} entry_pc out of bounds: {}",
                            fun_idx, entry.entry_pc
                        ),
                    });
                }

                let locals = Self::build_locals_for_call(&entry, args)?;
                let return_pc = *pc;
                let stack_base = self.stack.len();
                self.frames.push(CallFrame {
                    return_pc,
                    stack_base,
                    call_site: Some((span_start, span_end)),
                    locals,
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
                let call_site = self.current_frame()?.call_site;
                let (span_start, span_end) = call_site
                    .map(|(start, end)| (start, end))
                    .unwrap_or((template.span_start, template.span_end));
                let location = Location {
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
                self.stack.push(Value::Error(Box::new(RichError {
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

                let mut full_args = callable.captured;
                full_args.extend(args);

                match callable.target {
                    CallableTarget::Builtin(builtin_id) => {
                        let result = call_builtin(self, builtin_id, full_args)?;
                        self.stack.push(result);
                    }
                    CallableTarget::Function(fun_idx) => {
                        let entry = self.function_entry(fun_idx)?.clone();
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
                        if entry.entry_pc as usize >= self.bytecode.opcodes.len() {
                            return Err(RuntimeError {
                                message: format!(
                                    "Function {} entry_pc out of bounds: {}",
                                    fun_idx, entry.entry_pc
                                ),
                            });
                        }

                        let locals = Self::build_locals_for_call(&entry, full_args)?;
                        let return_pc = *pc;
                        let stack_base = self.stack.len();
                        self.frames.push(CallFrame {
                            return_pc,
                            stack_base,
                            call_site: Some((span_start, span_end)),
                            locals,
                        });
                        *pc = entry.entry_pc as usize;
                    }
                }
            }

            // Control flow
            Opcode::Jump(addr) => {
                *pc = self.validate_jump_target(addr)?;
            }
            Opcode::JumpIfFalse(addr) => {
                let val = self.pop_stack()?;
                match val {
                    Value::Bool(false) => {
                        *pc = self.validate_jump_target(addr)?;
                    }
                    Value::Bool(true) => {}
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
                    Value::Bool(true) => {
                        *pc = self.validate_jump_target(addr)?;
                    }
                    Value::Bool(false) => {}
                    _ => {
                        return Err(RuntimeError {
                            message: "JumpIfTrue: expected Bool".into(),
                        });
                    }
                }
            }

            // Deprecated frame management opcodes are no-ops under the new calling convention.
            Opcode::MakeFrame(_) | Opcode::PopFrame => {}

            // Return
            Opcode::Return => {
                if self.frames.len() == 1 {
                    return Err(RuntimeError {
                        message: "Return at top-level".into(),
                    });
                }

                let ret = self.pop_stack()?;
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

    fn build_locals_for_call(
        entry: &FunctionEntry,
        args: Vec<Value>,
    ) -> Result<Vec<Value>, RuntimeError> {
        let num_locals = entry.num_locals as usize;
        if num_locals < args.len() {
            return Err(RuntimeError {
                message: format!(
                    "Function {} requires at least {} local slots, got {}",
                    entry.fun_idx,
                    args.len(),
                    num_locals
                ),
            });
        }

        let mut locals = vec![Value::Unit; num_locals];
        for (idx, arg) in args.into_iter().enumerate() {
            locals[idx] = arg;
        }
        Ok(locals)
    }

    fn function_entry(&self, fun_idx: u32) -> Result<&FunctionEntry, RuntimeError> {
        let idx = fun_idx as usize;
        let entry = self
            .bytecode
            .functions
            .get(idx)
            .ok_or_else(|| RuntimeError {
                message: format!("Unknown function index: {}", fun_idx),
            })?;

        if entry.fun_idx != fun_idx {
            return Err(RuntimeError {
                message: format!(
                    "Function table invariant violated: functions[{}].fun_idx = {}",
                    idx, entry.fun_idx
                ),
            });
        }

        Ok(entry)
    }

    fn validate_jump_target(&self, addr: u32) -> Result<usize, RuntimeError> {
        let target = addr as usize;
        if target >= self.bytecode.opcodes.len() {
            return Err(RuntimeError {
                message: format!("Invalid jump target: {}", addr),
            });
        }
        Ok(target)
    }

    // Stack helpers

    fn pop_stack(&mut self) -> Result<Value, RuntimeError> {
        self.stack.pop().ok_or_else(|| RuntimeError {
            message: "Stack underflow".into(),
        })
    }

    fn current_frame(&self) -> Result<&CallFrame, RuntimeError> {
        self.frames.last().ok_or_else(|| RuntimeError {
            message: "Frame stack underflow".into(),
        })
    }

    fn current_frame_mut(&mut self) -> Result<&mut CallFrame, RuntimeError> {
        self.frames.last_mut().ok_or_else(|| RuntimeError {
            message: "Frame stack underflow".into(),
        })
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

#[cfg(test)]
mod tests {
    use super::VM;
    use sindr::ir::{Bytecode, Constant, FunctionEntry, Opcode};
    use sindr::runtime::TypeRegistry;

    fn base_bytecode(opcodes: Vec<Opcode>) -> Bytecode {
        Bytecode {
            opcodes,
            constants: Vec::new(),
            num_locals: 0,
            type_registry: TypeRegistry::new(),
            error_templates: Vec::new(),
            functions: Vec::new(),
            source_map: None,
        }
    }

    #[test]
    fn top_level_return_is_runtime_error() {
        let bytecode = base_bytecode(vec![Opcode::Return]);
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("top-level"));
    }

    #[test]
    fn load_local_out_of_bounds_is_runtime_error() {
        let bytecode = base_bytecode(vec![Opcode::LoadLocal(0), Opcode::Halt]);
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("LoadLocal out of bounds"));
    }

    #[test]
    fn invalid_jump_is_runtime_error() {
        let bytecode = base_bytecode(vec![Opcode::Jump(42), Opcode::Halt]);
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("Invalid jump target"));
    }

    #[test]
    fn unknown_function_index_is_runtime_error() {
        let bytecode = base_bytecode(vec![Opcode::Call(1, 0, 0, 0), Opcode::Halt]);
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("Unknown function index"));
    }

    #[test]
    fn call_initializes_locals_without_makeframe() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::Call(0, 1, 0, 0),
            Opcode::Halt,
            Opcode::LoadLocal(0),
            Opcode::Return,
        ]);
        bytecode.constants = vec![Constant::Int(5)];
        bytecode.functions = vec![FunctionEntry {
            fun_idx: 0,
            entry_pc: 3,
            num_locals: 1,
            arity: 1,
        }];

        VM::new(bytecode).run().expect("run should succeed");
    }

    #[test]
    fn frame_stack_underflow_is_runtime_error() {
        let bytecode = base_bytecode(vec![Opcode::LoadLocal(0), Opcode::Halt]);
        let mut vm = VM::new(bytecode);
        vm.frames.clear();
        let err = vm.run().expect_err("must fail");
        assert!(err.message.contains("Frame stack underflow"));
    }

    #[test]
    fn function_table_mismatch_is_runtime_error() {
        let mut bytecode = base_bytecode(vec![Opcode::Call(0, 0, 0, 0), Opcode::Halt]);
        bytecode.functions = vec![FunctionEntry {
            fun_idx: 1,
            entry_pc: 1,
            num_locals: 0,
            arity: 0,
        }];

        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("Function table invariant violated"));
    }
}
