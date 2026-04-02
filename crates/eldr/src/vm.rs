use sindr::ir::{
    line_column_for_offset, Bytecode, BytecodeChunk, Constant, FunctionEntry, Opcode, SourceMap,
};
use sindr::runtime::{Callable, CallableTarget, Location, RichError, TypeRegistry, Value};
use std::collections::BTreeSet;

use crate::builtin::call_builtin;
use crate::error::{RuntimeError, RuntimeErrorContext};

#[derive(Debug, Clone)]
struct CallFrame {
    return_pc: usize,
    stack_base: usize,
    call_site: Option<(u32, u32)>,
    locals: Vec<Value>,
}

/// The Surtr virtual machine — executes bytecode produced by Forge.
#[derive(Clone)]
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

    /// Replace source context for later runtime diagnostics.
    pub fn set_source(&mut self, source: String, file: String) {
        self.source = Some(source);
        self.source_file = Some(file);
    }

    /// Access source text if attached.
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// Access source file name if attached.
    pub fn source_file(&self) -> Option<&str> {
        self.source_file.as_deref()
    }

    pub fn runtime_error_location(&self) -> Option<Location> {
        let (span_start, span_end) = self.current_frame().ok()?.call_site?;
        let file = self
            .source_file()
            .map(str::to_string)
            .unwrap_or_else(|| "<runtime>".to_string());
        let (line, column) = self
            .source()
            .map(|source| line_column_for_offset(source, span_start as usize))
            .unwrap_or((0, 0));
        Some(Location {
            file,
            func: "<runtime>".into(),
            line,
            column,
            span_start,
            span_end,
        })
    }

    fn runtime_error_context(&self, pc: usize, opcode: &Opcode) -> RuntimeErrorContext {
        let current_function = self
            .bytecode
            .functions
            .iter()
            .filter(|entry| entry.entry_pc as usize <= pc)
            .max_by_key(|entry| entry.entry_pc)
            .map(|entry| format!("fun#{}", entry.fun_idx));

        let mut details = vec![
            format!("stack_depth={}", self.stack.len()),
            format!("frame_depth={}", self.frames.len()),
        ];

        if let Some(frame) = self.frames.last() {
            details.push(format!("locals_len={}", frame.locals.len()));
            details.push(format!("stack_base={}", frame.stack_base));
        }
        if let Some(top) = self.stack.last() {
            details.push(format!("stack_top={:?}", top));
        }

        RuntimeErrorContext {
            pc: Some(pc),
            opcode: Some(format!("{:?}", opcode)),
            function: current_function,
            call_site: self.runtime_error_location(),
            details,
        }
    }

    fn enrich_runtime_error(&self, err: RuntimeError, pc: usize, opcode: &Opcode) -> RuntimeError {
        let RuntimeError {
            message,
            context: err_context,
        } = err;
        let mut context = self.runtime_error_context(pc, opcode);
        if err_context.pc.is_some() {
            context.pc = err_context.pc;
        }
        if err_context.opcode.is_some() {
            context.opcode = err_context.opcode;
        }
        if err_context.function.is_some() {
            context.function = err_context.function;
        }
        if err_context.call_site.is_some() {
            context.call_site = err_context.call_site;
        }
        context.details.extend(err_context.details);
        RuntimeError { message, context }
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
        self.verify_loaded_bytecode()?;
        loop {
            if self.pc >= self.bytecode.opcodes.len() {
                return Err(RuntimeError::new("PC out of bounds"));
            }
            let op = self.bytecode.opcodes[self.pc].clone();
            let mut next_pc = self.pc + 1;
            let halted = self
                .execute_opcode(op.clone(), &mut next_pc)
                .map_err(|err| self.enrich_runtime_error(err, self.pc, &op))?;
            self.pc = next_pc;

            if halted {
                return Ok(());
            }
        }
    }

    /// Execute an incremental bytecode chunk and return the final stack top.
    /// If the stack is empty at the end, returns `Unit`.
    ///
    /// Contract with Forge `codegen_chunk`:
    /// - chunk opcode indices are chunk-local (`LoadConst` / `MakeError`)
    /// - chunk `source_map` indices are chunk-local
    /// - this method relocates them using `const_base` / `error_template_base`
    /// - top-level execution starts at appended `code_base` and stops at first `Halt`
    pub fn push(&mut self, chunk: BytecodeChunk) -> Result<Value, RuntimeError> {
        let BytecodeChunk {
            opcodes,
            source_map,
            const_base: chunk_const_base,
            constants,
            new_locals,
            type_entries,
            error_template_base: chunk_error_template_base,
            error_templates,
            functions,
        } = chunk;
        let code_base = self.bytecode.opcodes.len();
        let const_base = self.bytecode.constants.len();
        let error_template_base = self.bytecode.error_templates.len();
        if chunk_const_base as usize != const_base {
            return Err(RuntimeError::new(format!(
                "Chunk constant base mismatch: chunk={}, vm={}",
                chunk_const_base, const_base
            )));
        }
        if chunk_error_template_base as usize != error_template_base {
            return Err(RuntimeError::new(format!(
                "Chunk error template base mismatch: chunk={}, vm={}",
                chunk_error_template_base, error_template_base
            )));
        }
        let mut chunk_opcodes = opcodes;
        Self::relocate_chunk_indices(
            &mut chunk_opcodes,
            code_base,
            const_base,
            error_template_base,
        )?;
        self.bytecode.constants.extend(constants);
        self.bytecode.type_registry.entries.extend(type_entries);
        self.bytecode.error_templates.extend(error_templates);
        self.bytecode.opcodes.extend(chunk_opcodes);
        self.relocate_and_extend_source_map(source_map, code_base)?;
        // Invariant: runtime uses O(1) lookup `functions[fun_idx as usize]`.
        // push() may append a new slot or replace an existing slot, but never create holes.
        for mut entry in functions {
            entry.entry_pc += code_base as u32;
            let idx = entry.fun_idx as usize;
            if idx == self.bytecode.functions.len() {
                self.bytecode.functions.push(entry);
            } else if idx < self.bytecode.functions.len() {
                self.bytecode.functions[idx] = entry;
            } else {
                return Err(RuntimeError::new(format!(
                    "Function table invariant violated in chunk: fun_idx {} > len {}",
                    idx,
                    self.bytecode.functions.len()
                )));
            }
        }
        if let Some(frame) = self.frames.first_mut() {
            frame
                .locals
                .extend(std::iter::repeat_n(Value::Unit, new_locals));
        }

        let mut pc = code_base;
        while pc < self.bytecode.opcodes.len() {
            let current_pc = pc;
            let op = self.bytecode.opcodes[pc].clone();
            pc += 1;
            let halted = self
                .execute_opcode(op.clone(), &mut pc)
                .map_err(|err| self.enrich_runtime_error(err, current_pc, &op))?;
            if halted {
                break;
            }
        }

        let result = self.stack.pop().unwrap_or(Value::Unit);
        self.stack.clear();
        Ok(result)
    }

    /// Execute a chunk atomically, preserving the existing VM state on failure.
    pub fn push_atomic(&mut self, chunk: BytecodeChunk) -> Result<Value, RuntimeError> {
        self.verify_chunk(&chunk)?;
        let mut next = self.clone();
        let result = next.push(chunk)?;
        next.verify_loaded_bytecode()?;
        *self = next;
        Ok(result)
    }

    fn verify_loaded_bytecode(&self) -> Result<(), RuntimeError> {
        Self::verify_program(&self.bytecode)
    }

    fn verify_program(bytecode: &Bytecode) -> Result<(), RuntimeError> {
        Self::verify_type_registry_entries(&bytecode.type_registry.entries, None)?;
        Self::verify_source_map_entries(bytecode.source_map.as_ref(), bytecode.opcodes.len(), "")?;

        let halt_pos = if let Some(pos) = bytecode
            .opcodes
            .iter()
            .position(|op| matches!(op, Opcode::Halt))
        {
            pos
        } else if bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::Return))
        {
            return Err(RuntimeError::new("Return at top-level"));
        } else {
            return Err(RuntimeError::new("Bytecode verifier: missing Halt"));
        };

        for (idx, op) in bytecode.opcodes.iter().enumerate() {
            match op {
                Opcode::Jump(addr) | Opcode::JumpIfFalse(addr) | Opcode::JumpIfTrue(addr) => {
                    if *addr as usize >= bytecode.opcodes.len() {
                        return Err(RuntimeError::new(format!("Invalid jump target: {}", addr)));
                    }
                }
                Opcode::LoadConst(idx) => {
                    if *idx as usize >= bytecode.constants.len() {
                        return Err(RuntimeError::new(format!(
                            "LoadConst index out of bounds: {}",
                            idx
                        )));
                    }
                }
                Opcode::MakeError(template_id) => {
                    if *template_id as usize >= bytecode.error_templates.len() {
                        return Err(RuntimeError::new(format!(
                            "Unknown error template: {}",
                            template_id
                        )));
                    }
                }
                Opcode::Return if idx <= halt_pos => {
                    return Err(RuntimeError::new("Return at top-level"));
                }
                _ => {}
            }
        }

        for (idx, entry) in bytecode.functions.iter().enumerate() {
            if entry.fun_idx as usize != idx {
                return Err(RuntimeError::new(format!(
                    "Function table invariant violated: functions[{}].fun_idx = {}",
                    idx, entry.fun_idx
                )));
            }
            if entry.entry_pc as usize >= bytecode.opcodes.len() {
                return Err(RuntimeError::new(format!(
                    "Function {} entry_pc out of bounds: {}",
                    entry.fun_idx, entry.entry_pc
                )));
            }
            if entry.entry_pc as usize <= halt_pos {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: function {} entry_pc {} must be after top-level Halt {}",
                    entry.fun_idx, entry.entry_pc, halt_pos
                )));
            }
        }

        Ok(())
    }

    fn verify_chunk(&self, chunk: &BytecodeChunk) -> Result<(), RuntimeError> {
        Self::verify_type_registry_entries(
            &chunk.type_entries,
            Some(&self.bytecode.type_registry.entries),
        )?;
        Self::verify_source_map_entries(chunk.source_map.as_ref(), chunk.opcodes.len(), "chunk")?;

        let const_base = self.bytecode.constants.len();
        if chunk.const_base as usize != const_base {
            return Err(RuntimeError::new(format!(
                "Chunk constant base mismatch: chunk={}, vm={}",
                chunk.const_base, const_base
            )));
        }

        let error_template_base = self.bytecode.error_templates.len();
        if chunk.error_template_base as usize != error_template_base {
            return Err(RuntimeError::new(format!(
                "Chunk error template base mismatch: chunk={}, vm={}",
                chunk.error_template_base, error_template_base
            )));
        }

        let halt_pos = chunk
            .opcodes
            .iter()
            .position(|op| matches!(op, Opcode::Halt))
            .ok_or_else(|| RuntimeError::new("Bytecode verifier: chunk missing Halt"))?;

        for (idx, op) in chunk.opcodes.iter().enumerate() {
            match op {
                Opcode::Jump(addr) | Opcode::JumpIfFalse(addr) | Opcode::JumpIfTrue(addr) => {
                    if *addr as usize >= chunk.opcodes.len() {
                        return Err(RuntimeError::new(format!(
                            "Bytecode verifier: chunk jump target out of bounds: {}",
                            addr
                        )));
                    }
                }
                Opcode::LoadConst(idx) => {
                    if *idx as usize >= chunk.constants.len() {
                        return Err(RuntimeError::new(format!(
                            "Bytecode verifier: chunk LoadConst index out of bounds: {}",
                            idx
                        )));
                    }
                }
                Opcode::MakeError(template_id) => {
                    if *template_id as usize >= chunk.error_templates.len() {
                        return Err(RuntimeError::new(format!(
                            "Bytecode verifier: chunk error template out of bounds: {}",
                            template_id
                        )));
                    }
                }
                Opcode::Return if idx <= halt_pos => {
                    return Err(RuntimeError::new("Return at top-level"));
                }
                _ => {}
            }
        }

        let mut seen_fun_idxs = BTreeSet::new();
        let mut next_append_idx = self.bytecode.functions.len();
        let mut sorted_entries = chunk.functions.iter().collect::<Vec<_>>();
        sorted_entries.sort_by_key(|entry| entry.fun_idx);

        for entry in sorted_entries {
            let idx = entry.fun_idx as usize;
            if !seen_fun_idxs.insert(entry.fun_idx) {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: duplicate function entry for fun_idx {}",
                    entry.fun_idx
                )));
            }
            if idx > next_append_idx {
                return Err(RuntimeError::new(format!(
                    "Function table invariant violated in chunk: fun_idx {} > len {}",
                    idx, next_append_idx
                )));
            }
            if idx == next_append_idx {
                next_append_idx += 1;
            }
            if entry.entry_pc as usize >= chunk.opcodes.len() {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: function {} entry_pc out of chunk bounds: {}",
                    entry.fun_idx, entry.entry_pc
                )));
            }
            if entry.entry_pc as usize <= halt_pos {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: function {} entry_pc {} must be after top-level Halt {}",
                    entry.fun_idx, entry.entry_pc, halt_pos
                )));
            }
        }

        Ok(())
    }

    fn verify_type_registry_entries(
        entries: &[sindr::runtime::TypeEntry],
        existing: Option<&[sindr::runtime::TypeEntry]>,
    ) -> Result<(), RuntimeError> {
        let mut seen_tags = BTreeSet::new();
        if let Some(existing) = existing {
            for entry in existing {
                seen_tags.insert(entry.tag);
            }
        }

        for entry in entries {
            if matches!(entry.tag, 0 | 1) {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: reserved result tag reused in TypeRegistry: {}",
                    entry.tag
                )));
            }

            if !seen_tags.insert(entry.tag) {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: duplicate type tag in TypeRegistry: {}",
                    entry.tag
                )));
            }
        }

        Ok(())
    }

    fn verify_source_map_entries(
        source_map: Option<&SourceMap>,
        opcode_len: usize,
        context: &str,
    ) -> Result<(), RuntimeError> {
        let Some(source_map) = source_map else {
            return Ok(());
        };
        let mut seen_opcode_indices = BTreeSet::new();
        for entry in &source_map.entries {
            if entry.opcode_index as usize >= opcode_len {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: {}source_map opcode_index out of bounds: {}",
                    if context.is_empty() { "" } else { "chunk " },
                    entry.opcode_index
                )));
            }

            if !seen_opcode_indices.insert(entry.opcode_index) {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: {}duplicate source_map entry for opcode_index {}",
                    if context.is_empty() { "" } else { "chunk " },
                    entry.opcode_index
                )));
            }
        }

        Ok(())
    }

    fn relocate_and_extend_source_map(
        &mut self,
        source_map: Option<SourceMap>,
        code_base: usize,
    ) -> Result<(), RuntimeError> {
        let Some(mut source_map) = source_map else {
            return Ok(());
        };

        let code_base =
            u32::try_from(code_base).map_err(|_| RuntimeError::new("opcode count exceeds u32"))?;
        for entry in &mut source_map.entries {
            entry.opcode_index = entry
                .opcode_index
                .checked_add(code_base)
                .ok_or_else(|| RuntimeError::new("source_map opcode index overflow"))?;
        }

        match self.bytecode.source_map.as_mut() {
            Some(existing) => existing.entries.extend(source_map.entries),
            None => self.bytecode.source_map = Some(source_map),
        }

        Ok(())
    }

    fn relocate_chunk_indices(
        opcodes: &mut [Opcode],
        code_base: usize,
        const_base: usize,
        error_template_base: usize,
    ) -> Result<(), RuntimeError> {
        let code_base = u32::try_from(code_base).map_err(|_| {
            RuntimeError::new(format!(
                "Code base too large for jump relocation: {}",
                code_base
            ))
        })?;
        let const_base = u32::try_from(const_base).map_err(|_| {
            RuntimeError::new(format!(
                "Constant base too large for relocation: {}",
                const_base
            ))
        })?;
        let error_template_base = u32::try_from(error_template_base).map_err(|_| {
            RuntimeError::new(format!(
                "Error template base too large for relocation: {}",
                error_template_base
            ))
        })?;

        for op in opcodes.iter_mut() {
            match op {
                Opcode::Jump(addr) | Opcode::JumpIfFalse(addr) | Opcode::JumpIfTrue(addr) => {
                    *addr = addr.checked_add(code_base).ok_or_else(|| {
                        RuntimeError::new(format!(
                            "Jump relocation overflow: target {} + base {}",
                            *addr, code_base
                        ))
                    })?;
                }
                Opcode::LoadConst(idx) => {
                    *idx = idx.checked_add(const_base).ok_or_else(|| {
                        RuntimeError::new(format!(
                            "Const relocation overflow: index {} + base {}",
                            *idx, const_base
                        ))
                    })?;
                }
                Opcode::MakeError(template_id) => {
                    *template_id =
                        template_id
                            .checked_add(error_template_base)
                            .ok_or_else(|| {
                                RuntimeError::new(format!(
                                    "Error template relocation overflow: id {} + base {}",
                                    *template_id, error_template_base
                                ))
                            })?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn execute_opcode(&mut self, op: Opcode, pc: &mut usize) -> Result<bool, RuntimeError> {
        match op {
            Opcode::Halt => return Ok(true),

            Opcode::LoadConst(idx) => {
                let c = self.bytecode.constants.get(idx as usize).ok_or_else(|| {
                    RuntimeError::new(format!("LoadConst index out of bounds: {}", idx))
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
                    .ok_or_else(|| {
                        RuntimeError::new(format!("LoadLocal out of bounds: {}", slot))
                    })?;
                self.stack.push(val);
            }

            Opcode::StoreLocal(slot) => {
                let val = self.pop_stack()?;
                let target = self
                    .current_frame_mut()?
                    .locals
                    .get_mut(slot as usize)
                    .ok_or_else(|| {
                        RuntimeError::new(format!("StoreLocal out of bounds: {}", slot))
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

            // Arithmetic (Float)
            Opcode::AddFloat => self.float_binop(|a, b| Value::Float(a + b))?,
            Opcode::SubFloat => self.float_binop(|a, b| Value::Float(a - b))?,
            Opcode::MulFloat => self.float_binop(|a, b| Value::Float(a * b))?,

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
                    Value::Int(tag) => u32::try_from(tag).map_err(|_| {
                        RuntimeError::new(format!("StructNew: invalid tag value {}", tag))
                    })?,
                    other => {
                        return Err(RuntimeError::new(format!(
                            "StructNew: expected Int tag, got {:?}",
                            other
                        )));
                    }
                };
                self.stack.push(Value::Tagged { tag, fields });
            }
            Opcode::GetField(idx) => {
                let val = self.pop_stack()?;
                match val {
                    Value::Tagged { fields, .. } => {
                        let field = fields.get(idx as usize).cloned().ok_or_else(|| {
                            RuntimeError::new(format!("Field index {} out of bounds", idx))
                        })?;
                        self.stack.push(field);
                    }
                    _ => {
                        return Err(RuntimeError::new("GetField on non-tagged value"));
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
                        return Err(RuntimeError::new("GetTag on non-tagged value"));
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
                    return Err(RuntimeError::new(format!(
                        "Call arity mismatch for function {}: expected {}, got {}",
                        fun_idx, entry.arity, arity
                    )));
                }

                let mut args = Vec::with_capacity(arity as usize);
                for _ in 0..arity {
                    args.push(self.pop_stack()?);
                }
                args.reverse();

                if entry.entry_pc as usize >= self.bytecode.opcodes.len() {
                    return Err(RuntimeError::new(format!(
                        "Function {} entry_pc out of bounds: {}",
                        fun_idx, entry.entry_pc
                    )));
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
                        return Err(RuntimeError::new(format!(
                            "MakeError expects String, got {:?}",
                            other
                        )));
                    }
                };
                let template = self
                    .bytecode
                    .error_templates
                    .get(template_id as usize)
                    .ok_or_else(|| {
                        RuntimeError::new(format!("Unknown error template: {}", template_id))
                    })?;
                let call_site = self.current_frame()?.call_site;
                let (span_start, span_end) = call_site
                    .map(|(start, end)| (start, end))
                    .unwrap_or((template.span_start, template.span_end));
                let (line, column) = match call_site {
                    Some((span_start, _)) => self
                        .source()
                        .map(|source| line_column_for_offset(source, span_start as usize))
                        .unwrap_or((template.line, template.column)),
                    None => (template.line, template.column),
                };
                let location = Location {
                    file: self
                        .source_file()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "<repl>".to_string()),
                    func: template.kind.clone(),
                    line,
                    column,
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
                // NOTE: `captured` currently co-locates lexical captures and partial-application args.
                // CallClosure prepends them to runtime args before `build_locals_for_call`.
                let target = self.pop_stack()?;
                let callable = match target {
                    Value::Callable(mut callable) => {
                        let mut merged = callable.captured;
                        merged.extend(captured);
                        callable.captured = merged;
                        callable
                    }
                    _ => {
                        return Err(RuntimeError::new("MakeClosure expects a callable target"));
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
                        return Err(RuntimeError::new("CallClosure expects a callable value"));
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
                            return Err(RuntimeError::new(format!(
                                "Call arity mismatch for function {}: expected {}, got {}",
                                fun_idx,
                                entry.arity,
                                full_args.len()
                            )));
                        }
                        if entry.entry_pc as usize >= self.bytecode.opcodes.len() {
                            return Err(RuntimeError::new(format!(
                                "Function {} entry_pc out of bounds: {}",
                                fun_idx, entry.entry_pc
                            )));
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
                        return Err(RuntimeError::new("JumpIfFalse: expected Bool"));
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
                        return Err(RuntimeError::new("JumpIfTrue: expected Bool"));
                    }
                }
            }

            // Deprecated frame management opcodes are no-ops under the new calling convention.
            Opcode::MakeFrame(_) | Opcode::PopFrame => {}

            // Return
            Opcode::Return => {
                if self.frames.len() == 1 {
                    return Err(RuntimeError::new("Return at top-level"));
                }

                let ret = self.pop_stack()?;
                let frame = self
                    .frames
                    .pop()
                    .ok_or_else(|| RuntimeError::new("Return with empty frame stack"))?;
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
            return Err(RuntimeError::new(format!(
                "Function {} requires at least {} local slots, got {}",
                entry.fun_idx,
                args.len(),
                num_locals
            )));
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
            .ok_or_else(|| RuntimeError::new(format!("Unknown function index: {}", fun_idx)))?;

        if entry.fun_idx != fun_idx {
            return Err(RuntimeError::new(format!(
                "Function table invariant violated: functions[{}].fun_idx = {}",
                idx, entry.fun_idx
            )));
        }

        Ok(entry)
    }

    fn validate_jump_target(&self, addr: u32) -> Result<usize, RuntimeError> {
        let target = addr as usize;
        if target >= self.bytecode.opcodes.len() {
            return Err(RuntimeError::new(format!("Invalid jump target: {}", addr)));
        }
        Ok(target)
    }

    // Stack helpers

    fn pop_stack(&mut self) -> Result<Value, RuntimeError> {
        self.stack
            .pop()
            .ok_or_else(|| RuntimeError::new("Stack underflow"))
    }

    fn current_frame(&self) -> Result<&CallFrame, RuntimeError> {
        self.frames
            .last()
            .ok_or_else(|| RuntimeError::new("Frame stack underflow"))
    }

    fn current_frame_mut(&mut self) -> Result<&mut CallFrame, RuntimeError> {
        self.frames
            .last_mut()
            .ok_or_else(|| RuntimeError::new("Frame stack underflow"))
    }

    fn pop_int(&mut self) -> Result<i64, RuntimeError> {
        match self.pop_stack()? {
            Value::Int(n) => Ok(n),
            other => Err(RuntimeError::new(format!("Expected Int, got {:?}", other))),
        }
    }

    fn pop_float(&mut self) -> Result<f64, RuntimeError> {
        match self.pop_stack()? {
            Value::Float(f) => Ok(f),
            other => Err(RuntimeError::new(format!(
                "Expected Float, got {:?}",
                other
            ))),
        }
    }

    fn pop_str(&mut self) -> Result<String, RuntimeError> {
        match self.pop_stack()? {
            Value::Str(s) => Ok(s),
            other => Err(RuntimeError::new(format!("Expected Str, got {:?}", other))),
        }
    }

    fn pop_bool(&mut self) -> Result<bool, RuntimeError> {
        match self.pop_stack()? {
            Value::Bool(b) => Ok(b),
            other => Err(RuntimeError::new(format!("Expected Bool, got {:?}", other))),
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
    use sindr::ir::{
        Bytecode, BytecodeChunk, Constant, ErrTemplate, FunctionEntry, Opcode, OpcodeSource,
        SourceMap,
    };
    use sindr::runtime::{TypeEntry, TypeKind, TypeRegistry, Value};

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
    fn runtime_error_captures_vm_context() {
        let bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::JumpIfFalse(0),
            Opcode::Halt,
        ]);
        let mut vm = VM::new(bytecode);
        vm.bytecode.constants = vec![Constant::Int(1)];

        let err = vm.run().expect_err("must fail");
        assert_eq!(err.context.pc, Some(1));
        assert_eq!(err.context.opcode.as_deref(), Some("JumpIfFalse(0)"));
        assert!(err
            .context
            .details
            .iter()
            .any(|detail| detail.starts_with("stack_depth=")));
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

    #[test]
    fn push_relocates_jump_targets() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            // `Jump(2)` must be relocated to absolute pc=3 because code_base=1.
            opcodes: vec![Opcode::Jump(2), Opcode::LoadConst(0), Opcode::LoadConst(1)],
            source_map: None,
            const_base: 0,
            constants: vec![Constant::Int(99), Constant::Int(7)],
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
        };

        let result = vm.push(chunk).expect("push should succeed");
        assert_eq!(result, Value::Int(7));
    }

    #[test]
    fn push_relocates_const_indices() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.constants = vec![Constant::Int(10)];
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::LoadConst(0), Opcode::Halt],
            source_map: None,
            const_base: 1,
            constants: vec![Constant::Int(42)],
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
        };

        let result = vm.push(chunk).expect("push should succeed");
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn push_relocates_make_error_template_indices() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.error_templates = vec![ErrTemplate {
            id: 0,
            kind: "Old".into(),
            span_start: 0,
            span_end: 0,
            line: 1,
            column: 1,
            format: "{}".into(),
            num_params: 1,
        }];
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::LoadConst(0), Opcode::MakeError(0), Opcode::Halt],
            source_map: None,
            const_base: 0,
            constants: vec![Constant::Str("new message".into())],
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 1,
            error_templates: vec![ErrTemplate {
                id: 0,
                kind: "NewKind".into(),
                span_start: 10,
                span_end: 20,
                line: 2,
                column: 3,
                format: "{}".into(),
                num_params: 1,
            }],
            functions: Vec::new(),
        };

        let result = vm.push(chunk).expect("push should succeed");
        match result {
            Value::Error(rich) => {
                assert_eq!(rich.kind, "NewKind");
                assert_eq!(rich.message, "new message");
                assert_eq!(rich.location.line, 2);
                assert_eq!(rich.location.column, 3);
            }
            other => panic!("expected Value::Error, got {:?}", other),
        }
    }

    #[test]
    fn make_error_prefers_call_site_line_and_column_when_source_is_available() {
        let source = "deferror Boom {\n  \"boom\"\n}\n\nBoom()\n".to_string();
        let mut vm =
            VM::new(base_bytecode(vec![Opcode::Halt])).with_source(source, "sample.srt".into());
        vm.frames[0].call_site = Some((30, 36));
        let result = vm
            .push(BytecodeChunk {
                opcodes: vec![Opcode::LoadConst(0), Opcode::MakeError(0), Opcode::Halt],
                source_map: None,
                const_base: 0,
                constants: vec![Constant::Str("boom".into())],
                new_locals: 0,
                type_entries: Vec::new(),
                error_template_base: 0,
                error_templates: vec![ErrTemplate {
                    id: 0,
                    kind: "Boom".into(),
                    span_start: 0,
                    span_end: 5,
                    line: 1,
                    column: 1,
                    format: "{}".into(),
                    num_params: 1,
                }],
                functions: Vec::new(),
            })
            .expect("push should succeed");
        match result {
            Value::Error(rich) => {
                assert_eq!(rich.location.line, 5);
                assert_eq!(rich.location.column, 3);
                assert_eq!(rich.location.span_start, 30);
                assert_eq!(rich.location.span_end, 36);
            }
            other => panic!("expected Value::Error, got {:?}", other),
        }
    }

    #[test]
    fn push_fails_when_constant_base_mismatches() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode);
        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::Halt],
            source_map: None,
            const_base: 1,
            constants: Vec::new(),
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
        };

        let err = vm.push(chunk).expect_err("must fail");
        assert!(err.message.contains("Chunk constant base mismatch"));
    }

    #[test]
    fn push_atomic_preserves_vm_on_failure() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode);
        let before = vm.clone();

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::Halt],
            source_map: None,
            const_base: 1,
            constants: Vec::new(),
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
        };

        let err = vm.push_atomic(chunk).expect_err("must fail");
        assert!(err.message.contains("Chunk constant base mismatch"));
        assert_eq!(vm.bytecode, before.bytecode);
        assert_eq!(vm.stack, before.stack);
        assert_eq!(vm.frames.len(), before.frames.len());
        assert_eq!(vm.pc, before.pc);
    }

    #[test]
    fn push_atomic_rejects_chunk_without_halt() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode);
        let before = vm.clone();

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::LoadConst(0)],
            source_map: None,
            const_base: 0,
            constants: vec![Constant::Int(1)],
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
        };

        let err = vm.push_atomic(chunk).expect_err("must fail");
        assert!(err.message.contains("chunk missing Halt"));
        assert_eq!(vm.bytecode, before.bytecode);
    }

    #[test]
    fn push_relocates_chunk_source_map_indices() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::Halt],
            source_map: Some(SourceMap {
                entries: vec![OpcodeSource {
                    opcode_index: 0,
                    span_start: 2,
                    span_end: 5,
                    line: 1,
                    column: 3,
                    source_name: Some("repl".into()),
                }],
            }),
            const_base: 0,
            constants: Vec::new(),
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
        };

        vm.push(chunk).expect("push should succeed");
        let source_map = vm
            .bytecode
            .source_map
            .expect("source map should be present");
        assert_eq!(source_map.entries.len(), 1);
        assert_eq!(source_map.entries[0].opcode_index, 1);
        assert_eq!(source_map.entries[0].line, 1);
        assert_eq!(source_map.entries[0].column, 3);
        assert_eq!(source_map.entries[0].source_name.as_deref(), Some("repl"));
    }

    #[test]
    fn push_atomic_rejects_chunk_out_of_bounds_source_map_entry() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::Halt],
            source_map: Some(SourceMap {
                entries: vec![OpcodeSource {
                    opcode_index: 1,
                    span_start: 0,
                    span_end: 1,
                    line: 1,
                    column: 1,
                    source_name: Some("repl".into()),
                }],
            }),
            const_base: 0,
            constants: Vec::new(),
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
        };

        let err = vm.push_atomic(chunk).expect_err("must fail");
        assert!(err
            .message
            .contains("chunk source_map opcode_index out of bounds"));
    }

    #[test]
    fn run_rejects_function_entry_before_top_level_halt() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt, Opcode::Return]);
        bytecode.functions = vec![FunctionEntry {
            fun_idx: 0,
            entry_pc: 0,
            num_locals: 0,
            arity: 0,
        }];

        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("must be after top-level Halt"));
    }

    #[test]
    fn run_rejects_reserved_type_tag_in_registry() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.type_registry.register(TypeEntry {
            tag: 0,
            name: "Bad".into(),
            kind: TypeKind::Struct,
            field_names: vec![],
        });

        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("reserved result tag"));
    }

    #[test]
    fn run_rejects_duplicate_type_tag_in_registry() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.type_registry.register(TypeEntry {
            tag: 10,
            name: "A".into(),
            kind: TypeKind::Struct,
            field_names: vec![],
        });
        bytecode.type_registry.register(TypeEntry {
            tag: 10,
            name: "B".into(),
            kind: TypeKind::Record,
            field_names: vec![],
        });

        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("duplicate type tag"));
    }

    #[test]
    fn run_rejects_out_of_bounds_source_map_entry() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.source_map = Some(SourceMap {
            entries: vec![OpcodeSource {
                opcode_index: 1,
                span_start: 0,
                span_end: 1,
                line: 1,
                column: 1,
                source_name: Some("main.srt".into()),
            }],
        });

        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err
            .message
            .contains("source_map opcode_index out of bounds"));
    }

    #[test]
    fn run_rejects_duplicate_source_map_opcode_index() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt, Opcode::Halt]);
        bytecode.source_map = Some(SourceMap {
            entries: vec![
                OpcodeSource {
                    opcode_index: 0,
                    span_start: 0,
                    span_end: 1,
                    line: 1,
                    column: 1,
                    source_name: Some("main.srt".into()),
                },
                OpcodeSource {
                    opcode_index: 0,
                    span_start: 2,
                    span_end: 3,
                    line: 1,
                    column: 3,
                    source_name: Some("main.srt".into()),
                },
            ],
        });

        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("duplicate source_map entry"));
    }

    #[test]
    fn push_atomic_rejects_chunk_reserved_type_tag() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::Halt],
            source_map: None,
            const_base: 0,
            constants: Vec::new(),
            new_locals: 0,
            type_entries: vec![TypeEntry {
                tag: 1,
                name: "Bad".into(),
                kind: TypeKind::Struct,
                field_names: vec![],
            }],
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
        };

        let err = vm.push_atomic(chunk).expect_err("must fail");
        assert!(err.message.contains("reserved result tag"));
    }

    #[test]
    fn push_atomic_rejects_chunk_duplicate_type_tag_against_vm() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.type_registry.register(TypeEntry {
            tag: 10,
            name: "Existing".into(),
            kind: TypeKind::Struct,
            field_names: vec![],
        });
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::Halt],
            source_map: None,
            const_base: 0,
            constants: Vec::new(),
            new_locals: 0,
            type_entries: vec![TypeEntry {
                tag: 10,
                name: "Duplicate".into(),
                kind: TypeKind::Record,
                field_names: vec![],
            }],
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
        };

        let err = vm.push_atomic(chunk).expect_err("must fail");
        assert!(err.message.contains("duplicate type tag"));
    }
}
