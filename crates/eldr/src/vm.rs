use forge::bytecode::Bytecode;
use forge::registry::TypeRegistry;

use crate::error::RuntimeError;
use crate::value::Value;

/// The Surtr virtual machine — executes bytecode produced by Forge.
pub struct VM {
    bytecode: Bytecode,
    /// Operand stack
    stack: Vec<Value>,
    /// Local variable slots
    locals: Vec<Value>,
    /// Program counter
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
            locals: vec![Value::Unit; num_locals],
            pc: 0,
            source: None,
            source_file: None,
            output: None,
            error_output: None,
        }
    }

    /// Set source code for error reporting.
    pub fn with_source(mut self, source: String, file: String) -> Self {
        self.source = Some(source);
        self.source_file = Some(file);
        self
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

    /// Execute the loaded bytecode.
    pub fn run(&mut self) -> Result<(), RuntimeError> {
        use crate::builtin::BUILTINS;
        use forge::bytecode::Constant;
        use forge::opcode::Opcode;

        loop {
            if self.pc >= self.bytecode.opcodes.len() {
                return Err(RuntimeError {
                    message: "PC out of bounds".into(),
                });
            }
            let op = self.bytecode.opcodes[self.pc].clone();
            self.pc += 1;

            match op {
                Opcode::Halt => return Ok(()),

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

                Opcode::LoadLocal(slot) => {
                    let val = self.locals[slot as usize].clone();
                    self.stack.push(val);
                }

                Opcode::StoreLocal(slot) => {
                    let val = self.pop_stack()?;
                    // Grow locals if needed
                    while self.locals.len() <= slot as usize {
                        self.locals.push(Value::Unit);
                    }
                    self.locals[slot as usize] = val;
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
                            })
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
                            })
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
                            })
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

                // ── Control flow ──
                Opcode::Jump(addr) => {
                    self.pc = addr as usize;
                }
                Opcode::JumpIfFalse(addr) => {
                    let val = self.pop_stack()?;
                    match val {
                        Value::Bool(false) => self.pc = addr as usize,
                        Value::Bool(true) => {} // fall through
                        _ => {
                            return Err(RuntimeError {
                                message: "JumpIfFalse: expected Bool".into(),
                            })
                        }
                    }
                }
                Opcode::JumpIfTrue(addr) => {
                    let val = self.pop_stack()?;
                    match val {
                        Value::Bool(true) => self.pc = addr as usize,
                        Value::Bool(false) => {} // fall through
                        _ => {
                            return Err(RuntimeError {
                                message: "JumpIfTrue: expected Bool".into(),
                            })
                        }
                    }
                }
            }
        }
    }

    // ── Stack helpers ──

    fn pop_stack(&mut self) -> Result<Value, RuntimeError> {
        self.stack.pop().ok_or_else(|| RuntimeError {
            message: "Stack underflow".into(),
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
