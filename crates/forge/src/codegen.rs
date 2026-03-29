#![allow(unused_imports, unused_variables)]

use std::collections::HashMap;

use scar::typed::*;
use scar::types::Ty;
use spire::ast::{BinOp, Lit, Span};

use crate::bytecode::*;
use crate::error::CodegenError;
use crate::opcode::Opcode;
use crate::registry::{TypeEntry, TypeKind, TypeRegistry};

/// Lower the typed AST to bytecode.
pub fn codegen(typed: Vec<TypedNode>) -> Result<Bytecode, CodegenError> {
    let mut gene = Codegen::new();
    gene.emit_program(typed)?;
    let (opcodes, state) = gene.finalize();
    Ok(Bytecode {
        opcodes,
        constants: state.constants,
        num_locals: state.next_slot as usize,
        type_registry: state.type_registry,
        error_templates: state.error_templates,
        functions: state.functions,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplTypeKind {
    Struct,
    Record,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingInfo {
    pub name: String,
    pub ty: String,
    pub slot_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDefDisplay {
    pub name: String,
    pub kind: ReplTypeKind,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkMeta {
    pub bindings: Vec<BindingInfo>,
    pub type_defs: Vec<TypeDefDisplay>,
    pub function_defs: Vec<String>,
}

#[derive(Debug, Clone)]
struct CodegenState {
    constants: Vec<Constant>,
    slot_map: HashMap<u32, u32>, // unique_id → local slot
    next_slot: u32,
    type_registry: TypeRegistry,
    error_templates: Vec<ErrTemplate>,
    functions: Vec<FunctionEntry>,
}

impl CodegenState {
    fn new() -> Self {
        Self {
            constants: Vec::new(),
            slot_map: HashMap::new(),
            next_slot: 0,
            type_registry: TypeRegistry::new(),
            error_templates: Vec::new(),
            functions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ForgeCheckpoint {
    state: CodegenState,
}

#[derive(Debug, Clone)]
pub struct ForgeSession {
    state: CodegenState,
}

impl ForgeSession {
    pub fn new() -> Self {
        Self {
            state: CodegenState::new(),
        }
    }

    pub fn checkpoint(&self) -> ForgeCheckpoint {
        ForgeCheckpoint {
            state: self.state.clone(),
        }
    }

    pub fn rollback(&mut self, checkpoint: ForgeCheckpoint) {
        self.state = checkpoint.state;
    }

    pub fn type_registry(&self) -> TypeRegistry {
        self.state.type_registry.clone()
    }

    pub fn codegen_chunk(
        &mut self,
        typed: Vec<TypedNode>,
    ) -> Result<(BytecodeChunk, ChunkMeta), CodegenError> {
        let before = self.state.clone();
        let typed_for_meta = typed.clone();

        let mut gene = Codegen::from_state(before.clone());
        gene.emit_program_chunk(typed)?;
        let (opcodes, after) = gene.finalize();

        let new_constants = after.constants[before.constants.len()..].to_vec();
        let new_locals = after.next_slot.saturating_sub(before.next_slot) as usize;
        let type_entries = after.type_registry.entries[before.type_registry.entries.len()..].to_vec();
        let meta = collect_chunk_meta(&typed_for_meta, &after.slot_map);
        let functions = after.functions[before.functions.len()..].to_vec();

        self.state = after;

        Ok((
            BytecodeChunk {
                opcodes,
                constants: new_constants,
                new_locals,
                type_entries,
                functions,
            },
            meta,
        ))
    }
}

impl Default for ForgeSession {
    fn default() -> Self {
        Self::new()
    }
}

fn collect_chunk_meta(typed: &[TypedNode], slot_map: &HashMap<u32, u32>) -> ChunkMeta {
    let mut bindings = Vec::new();
    let mut type_defs = Vec::new();
    let mut function_defs = Vec::new();

    for stmt in typed {
        match &stmt.node {
            TypedInner::Bind(TypedPattern::Var(ty, id), _) => {
                if let Some(slot_id) = slot_map.get(&id.unique_id) {
                    bindings.push(BindingInfo {
                        name: id.name.clone(),
                        ty: ty_to_string(ty),
                        slot_id: *slot_id,
                    });
                }
            }
            TypedInner::StructDef(_, name, field_names) => {
                type_defs.push(TypeDefDisplay {
                    name: name.clone(),
                    kind: ReplTypeKind::Struct,
                    fields: field_names
                        .iter()
                        .map(|field| (field.clone(), String::new()))
                        .collect(),
                });
            }
            TypedInner::RecordDef(_, name, field_names) => {
                type_defs.push(TypeDefDisplay {
                    name: name.clone(),
                    kind: ReplTypeKind::Record,
                    fields: field_names
                        .iter()
                        .map(|field| (field.clone(), String::new()))
                        .collect(),
                });
            }
            TypedInner::DeferrorDef(_, id, _) => {
                type_defs.push(TypeDefDisplay {
                    name: id.name.clone(),
                    kind: ReplTypeKind::Error,
                    fields: Vec::new(),
                });
            }
            TypedInner::Def(_, id, _, _, _) => {
                function_defs.push(id.name.clone());
            }
            _ => {}
        }
    }

    ChunkMeta {
        bindings,
        type_defs,
        function_defs,
    }
}

fn ty_to_string(ty: &Ty) -> String {
    match ty {
        Ty::Int => "Int".into(),
        Ty::Float => "Float".into(),
        Ty::Str => "String".into(),
        Ty::Bool => "Boolean".into(),
        Ty::Unit => "Unit".into(),
        Ty::List(inner) => format!("[{}]", ty_to_string(inner)),
        Ty::Result(ok, err) => format!("Result<{}, {}>", ty_to_string(ok), ty_to_string(err)),
        Ty::Struct(name, _) | Ty::Record(name, _) => name.clone(),
        Ty::Error => "Error".into(),
        Ty::Var(id) => format!("${}", id),
        Ty::Func(params, ret) => format!(
            "({}) -> {}",
            params
                .iter()
                .map(ty_to_string)
                .collect::<Vec<_>>()
                .join(", "),
            ty_to_string(ret)
        ),
        Ty::BuiltinFunc { name, .. } => format!("Builtin({})", name),
        Ty::UserFunc { .. } => "UserFunc".into(),
    }
}

// ── IR with labels (resolved to absolute addresses at the end) ──

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum IrOp {
    Op(Opcode),
    /// Jump to label (resolved later)
    JumpLabel(Label),
    /// Jump-if-false to label
    JumpIfFalseLabel(Label),
    /// Jump-if-true to label
    JumpIfTrueLabel(Label),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Label(u32);

struct Codegen {
    ir: Vec<IrOp>,
    state: CodegenState,
    next_label: u32,
    label_positions: HashMap<Label, usize>, // label → IR index it points to
}

impl Codegen {
    fn new() -> Self {
        Self::from_state(CodegenState::new())
    }

    fn from_state(state: CodegenState) -> Self {
        Self {
            ir: Vec::new(),
            state,
            next_label: 0,
            label_positions: HashMap::new(),
        }
    }

    fn fresh_label(&mut self) -> Label {
        let l = Label(self.next_label);
        self.next_label += 1;
        l
    }

    fn alloc_slot(&mut self, unique_id: u32) -> u32 {
        if let Some(&slot) = self.state.slot_map.get(&unique_id) {
            return slot;
        }
        let slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.state.slot_map.insert(unique_id, slot);
        slot
    }

    fn add_constant(&mut self, c: Constant) -> u32 {
        // Check for existing identical constant
        for (i, existing) in self.state.constants.iter().enumerate() {
            if existing == &c {
                return i as u32;
            }
        }
        let idx = self.state.constants.len() as u32;
        self.state.constants.push(c);
        idx
    }

    fn emit(&mut self, op: Opcode) {
        self.ir.push(IrOp::Op(op));
    }

    fn emit_jump(&mut self, label: Label) {
        self.ir.push(IrOp::JumpLabel(label));
    }

    fn emit_jump_if_false(&mut self, label: Label) {
        self.ir.push(IrOp::JumpIfFalseLabel(label));
    }

    #[allow(dead_code)]
    fn emit_jump_if_true(&mut self, label: Label) {
        self.ir.push(IrOp::JumpIfTrueLabel(label));
    }

    #[allow(dead_code)]
    fn current_pos(&self) -> usize {
        self.ir.len()
    }

    // ── Program ──

    fn emit_program(&mut self, stmts: Vec<TypedNode>) -> Result<(), CodegenError> {
        self.emit_program_with_functions(stmts, true)
    }

    fn emit_program_chunk(&mut self, stmts: Vec<TypedNode>) -> Result<(), CodegenError> {
        self.emit_program_with_functions(stmts, false)
    }

    fn emit_program_with_functions(
        &mut self,
        stmts: Vec<TypedNode>,
        pop_last: bool,
    ) -> Result<(), CodegenError> {
        let mut defs = Vec::new();
        let mut main_stmts = Vec::new();

        for stmt in &stmts {
            match &stmt.node {
                TypedInner::Def(..) => defs.push(stmt),
                _ => main_stmts.push(stmt),
            }
        }

        for (i, stmt) in main_stmts.iter().enumerate() {
            self.emit_node(stmt)?;
            if pop_last || i + 1 < main_stmts.len() {
                self.emit(Opcode::Pop);
            }
        }

        self.emit(Opcode::Halt);

        for def in defs {
            self.emit_function_def(def)?;
        }

        Ok(())
    }

    fn emit_function_def(&mut self, node: &TypedNode) -> Result<(), CodegenError> {
        let (fun_idx, id, params, _ret_ty, body) = match &node.node {
            TypedInner::Def(fun_idx, id, params, ret_ty, body) => (fun_idx, id, params, ret_ty, body),
            _ => {
                return Err(CodegenError {
                    message: "expected function definition".into(),
                    span: node.span.clone(),
                });
            }
        };

        let saved_slot_map = self.state.slot_map.clone();
        let saved_next_slot = self.state.next_slot;

        self.state.slot_map = HashMap::new();
        self.state.next_slot = 0;

        for (slot, param) in params.iter().enumerate() {
            self.state.slot_map.insert(param.id.unique_id, slot as u32);
        }
        self.state.next_slot = params.len() as u32;

        let entry_pc = self.current_pos() as u32;
        self.emit(Opcode::MakeFrame(params.len() as u32));
        for slot in (0..params.len()).rev() {
            self.emit(Opcode::StoreLocal(slot as u32));
        }
        self.emit_node(body)?;
        self.emit(Opcode::PopFrame);
        self.emit(Opcode::Return);

        let num_locals = self.state.next_slot;
        self.state.functions.push(FunctionEntry {
            fun_idx: *fun_idx,
            entry_pc,
            num_locals,
            arity: params.len() as u8,
        });

        self.state.slot_map = saved_slot_map;
        self.state.next_slot = saved_next_slot;

        let _ = id;
        Ok(())
    }

    fn emit_node(&mut self, node: &TypedNode) -> Result<(), CodegenError> {
        match &node.node {
            TypedInner::Lit(lit) => {
                let c = self.lit_to_constant(lit);
                let idx = self.add_constant(c);
                self.emit(Opcode::LoadConst(idx));
            }

            TypedInner::Var(id) => {
                if matches!(node.ty, Ty::UserFunc { .. }) {
                    return Err(CodegenError {
                        message: "User-defined functions are not first-class values in phase 2 step 4".into(),
                        span: node.span.clone(),
                    });
                }
                let slot = self.alloc_slot(id.unique_id);
                self.emit(Opcode::LoadLocal(slot));
            }

            TypedInner::Bind(pat, rhs) => {
                self.emit_node(rhs)?;
                match pat {
                    TypedPattern::Var(_, id) => {
                        let slot = self.alloc_slot(id.unique_id);
                        self.emit(Opcode::StoreLocal(slot));
                    }
                    TypedPattern::Wildcard(_) => {
                        self.emit(Opcode::Pop);
                    }
                }
                // Bind produces Unit
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::App(func, args) => {
                self.emit_app(func, args)?;
            }

            TypedInner::BinOp(op, left, right) => {
                self.emit_node(left)?;
                self.emit_node(right)?;
                let opcode = self.binop_to_opcode(op, &left.ty)?;
                self.emit(opcode);
            }

            TypedInner::List(elems) => {
                if elems.is_empty() {
                    self.emit(Opcode::ListEmpty);
                } else {
                    for elem in elems {
                        self.emit_node(elem)?;
                    }
                    self.emit(Opcode::ListNew(elems.len() as u32));
                }
            }

            TypedInner::InterpolatedStr(parts) => {
                self.emit_interpolated_str(parts)?;
            }

            TypedInner::If(cond, then, else_opt) => {
                self.emit_if(cond, then, else_opt)?;
            }

            TypedInner::Match(scrutinee, arms) => {
                self.emit_match(scrutinee, arms)?;
            }

            TypedInner::FieldAccess(expr, idx) => {
                self.emit_node(expr)?;
                self.emit(Opcode::GetField(*idx));
            }

            TypedInner::StructLit(tag, fields) => {
                // Push tag first, then fields
                let tag_const = self.add_constant(Constant::Int(*tag as i64));
                self.emit(Opcode::LoadConst(tag_const));
                for field in fields {
                    self.emit_node(field)?;
                }
                // StructNew expects tag + n fields on stack
                self.emit(Opcode::StructNew(fields.len() as u32));
            }

            TypedInner::ConstructorCall(tag, fields) => {
                let tag_const = self.add_constant(Constant::Int(*tag as i64));
                self.emit(Opcode::LoadConst(tag_const));
                for field in fields {
                    self.emit_node(field)?;
                }
                self.emit(Opcode::StructNew(fields.len() as u32));
            }

            TypedInner::Block(stmts) => {
                for (i, s) in stmts.iter().enumerate() {
                    self.emit_node(s)?;
                    if i < stmts.len() - 1 {
                        self.emit(Opcode::Pop);
                    }
                }
            }

            TypedInner::Semi(inner) => {
                self.emit_node(inner)?;
                self.emit(Opcode::Pop);
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::DeferrorDef(_tag, id, show_expr) => {
                // Initialize no-arg deferror value from its show expression.
                // This keeps `Err(MyError)` from becoming Unit at runtime.
                self.emit_node(show_expr)?;
                let slot = self.alloc_slot(id.unique_id);
                self.emit(Opcode::StoreLocal(slot));
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::Def(_fun_idx, _id, _params, _ret_ty, _body) => {
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::StructDef(tag, name, field_names) => {
                self.state.type_registry.register(TypeEntry {
                    tag: *tag,
                    name: name.clone(),
                    kind: TypeKind::Struct,
                    field_names: field_names.clone(),
                });
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::RecordDef(tag, name, field_names) => {
                self.state.type_registry.register(TypeEntry {
                    tag: *tag,
                    name: name.clone(),
                    kind: TypeKind::Record,
                    field_names: field_names.clone(),
                });
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }
        }
        Ok(())
    }

    // ── Function application ──

    fn emit_app(&mut self, func: &TypedNode, args: &[TypedNode]) -> Result<(), CodegenError> {
        match &func.ty {
            Ty::BuiltinFunc { name, .. } => {
                let builtin_id = match name.as_str() {
                    "print" => 0u16,
                    "to_string" => 1,
                    "eprint" => 2,
                    _ => {
                        return Err(CodegenError {
                            message: format!("Unknown builtin: {}", name),
                            span: func.span.clone(),
                        });
                    }
                };
                for arg in args {
                    self.emit_node(arg)?;
                }
                self.emit(Opcode::CallBuiltin(builtin_id, args.len() as u8));
            }
            Ty::UserFunc { fun_idx, params, .. } => {
                if args.len() != params.len() {
                    return Err(CodegenError {
                        message: format!(
                            "function expects {} argument(s), got {}",
                            params.len(),
                            args.len()
                        ),
                        span: func.span.clone(),
                    });
                }
                for arg in args {
                    self.emit_node(arg)?;
                }
                self.emit(Opcode::Call(*fun_idx, args.len() as u8));
            }
            _ => {
                return Err(CodegenError {
                    message: "Non-function value in call position".into(),
                    span: func.span.clone(),
                });
            }
        }
        Ok(())
    }

    // ── If ──

    fn emit_if(
        &mut self,
        cond: &TypedNode,
        then: &TypedNode,
        else_opt: &Option<Box<TypedNode>>,
    ) -> Result<(), CodegenError> {
        self.emit_node(cond)?;

        match else_opt {
            Some(else_branch) => {
                let else_label = self.fresh_label();
                let end_label = self.fresh_label();

                self.emit_jump_if_false(else_label);
                self.emit_node(then)?;
                self.emit_jump(end_label);

                // Patch else label to current position
                self.patch_label(else_label);
                self.emit_node(else_branch)?;

                self.patch_label(end_label);
            }
            None => {
                let end_label = self.fresh_label();
                self.emit_jump_if_false(end_label);
                self.emit_node(then)?;
                self.emit(Opcode::Pop); // discard then result for 2-arg if
                self.patch_label(end_label);
                // Push Unit
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }
        }
        Ok(())
    }

    fn emit_interpolated_str(
        &mut self,
        parts: &[TypedInterpolatedPart],
    ) -> Result<(), CodegenError> {
        if parts.is_empty() {
            let empty = self.add_constant(Constant::Str(String::new()));
            self.emit(Opcode::LoadConst(empty));
            return Ok(());
        }

        let mut first = true;
        for part in parts {
            match part {
                TypedInterpolatedPart::Text(s) => {
                    let idx = self.add_constant(Constant::Str(s.clone()));
                    self.emit(Opcode::LoadConst(idx));
                }
                TypedInterpolatedPart::Expr(expr) => {
                    self.emit_node(expr)?;
                    // Reuse builtin_id=1 (to_string)
                    self.emit(Opcode::CallBuiltin(1, 1));
                }
            }

            if first {
                first = false;
            } else {
                self.emit(Opcode::ConcatStr);
            }
        }
        Ok(())
    }

    // ── Match ──

    fn emit_match(
        &mut self,
        scrutinee: &TypedNode,
        arms: &[(TypedMatchPattern, TypedNode)],
    ) -> Result<(), CodegenError> {
        self.emit_node(scrutinee)?;

        let scrut_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(scrut_slot));

        let end_label = self.fresh_label();
        let mut arm_labels: Vec<Label> = Vec::new();

        for _ in arms {
            arm_labels.push(self.fresh_label());
        }

        for (i, (pat, body)) in arms.iter().enumerate() {
            let next_arm = if i + 1 < arms.len() {
                arm_labels[i + 1]
            } else {
                end_label
            };

            match pat {
                TypedMatchPattern::Wildcard => {}
                TypedMatchPattern::BoolLit(b) => {
                    self.emit(Opcode::LoadLocal(scrut_slot));
                    let bool_const = self.add_constant(Constant::Bool(*b));
                    self.emit(Opcode::LoadConst(bool_const));
                    self.emit(Opcode::EqBool);
                    self.emit_jump_if_false(next_arm);
                }
                TypedMatchPattern::IntLit(n) => {
                    self.emit(Opcode::LoadLocal(scrut_slot));
                    let int_const = self.add_constant(Constant::Int(*n));
                    self.emit(Opcode::LoadConst(int_const));
                    self.emit(Opcode::EqInt);
                    self.emit_jump_if_false(next_arm);
                }
                TypedMatchPattern::StrLit(s) => {
                    self.emit(Opcode::LoadLocal(scrut_slot));
                    let str_const = self.add_constant(Constant::Str(s.clone()));
                    self.emit(Opcode::LoadConst(str_const));
                    self.emit(Opcode::EqStr);
                    self.emit_jump_if_false(next_arm);
                }
                TypedMatchPattern::Constructor(tag, inner_id) => {
                    self.emit(Opcode::LoadLocal(scrut_slot));
                    self.emit(Opcode::GetTag);
                    let tag_const = self.add_constant(Constant::Int(*tag as i64));
                    self.emit(Opcode::LoadConst(tag_const));
                    self.emit(Opcode::EqInt);
                    self.emit_jump_if_false(next_arm);

                    // Bind inner variable
                    if let Some(inner) = inner_id {
                        self.emit(Opcode::LoadLocal(scrut_slot));
                        self.emit(Opcode::GetField(0));
                        let inner_slot = self.alloc_slot(inner.unique_id);
                        self.emit(Opcode::StoreLocal(inner_slot));
                    }
                }
            }

            // Emit body
            self.emit_node(body)?;
            self.emit_jump(end_label);

            // Patch next arm label
            if i + 1 < arms.len() {
                self.patch_label(arm_labels[i + 1]);
            }
        }

        self.patch_label(end_label);
        Ok(())
    }

    // ── Label resolution ──

    /// Mark a label as pointing to the current IR position.
    fn patch_label(&mut self, label: Label) {
        self.label_positions.insert(label, self.ir.len());
    }

    // ── Helpers ──

    fn lit_to_constant(&self, lit: &Lit) -> Constant {
        match lit {
            Lit::Int(n) => Constant::Int(*n),
            Lit::Float(f) => Constant::Float(*f),
            Lit::Str(s) => Constant::Str(s.clone()),
            Lit::Bool(b) => Constant::Bool(*b),
            Lit::Unit => Constant::Unit,
        }
    }

    fn binop_to_opcode(&self, op: &BinOp, left_ty: &Ty) -> Result<Opcode, CodegenError> {
        let dummy_span = Span { start: 0, end: 0 };
        match (op, left_ty) {
            (BinOp::Add, Ty::Int) => Ok(Opcode::AddInt),
            (BinOp::Sub, Ty::Int) => Ok(Opcode::SubInt),
            (BinOp::Mul, Ty::Int) => Ok(Opcode::MulInt),
            (BinOp::Div, Ty::Int) => Ok(Opcode::DivInt),
            (BinOp::Mod, Ty::Int) => Ok(Opcode::ModInt),
            (BinOp::Add, Ty::Float) => Ok(Opcode::AddFloat),
            (BinOp::Sub, Ty::Float) => Ok(Opcode::SubFloat),
            (BinOp::Mul, Ty::Float) => Ok(Opcode::MulFloat),
            (BinOp::Div, Ty::Float) => Ok(Opcode::DivFloat),
            (BinOp::Eq, Ty::Int) => Ok(Opcode::EqInt),
            (BinOp::Neq, Ty::Int) => Ok(Opcode::NeqInt),
            (BinOp::Lt, Ty::Int) => Ok(Opcode::LtInt),
            (BinOp::Gt, Ty::Int) => Ok(Opcode::GtInt),
            (BinOp::Lte, Ty::Int) => Ok(Opcode::LteInt),
            (BinOp::Gte, Ty::Int) => Ok(Opcode::GteInt),
            (BinOp::Eq, Ty::Float) => Ok(Opcode::EqFloat),
            (BinOp::Neq, Ty::Float) => Ok(Opcode::NeqFloat),
            (BinOp::Lt, Ty::Float) => Ok(Opcode::LtFloat),
            (BinOp::Gt, Ty::Float) => Ok(Opcode::GtFloat),
            (BinOp::Lte, Ty::Float) => Ok(Opcode::LteFloat),
            (BinOp::Gte, Ty::Float) => Ok(Opcode::GteFloat),
            (BinOp::Eq, Ty::Str) => Ok(Opcode::EqStr),
            (BinOp::Neq, Ty::Str) => Ok(Opcode::NeqStr),
            (BinOp::Eq, Ty::Bool) => Ok(Opcode::EqBool),
            (BinOp::Neq, Ty::Bool) => Ok(Opcode::NeqBool),
            (BinOp::Concat, Ty::Str) => Ok(Opcode::ConcatStr),
            _ => Err(CodegenError {
                message: format!("Unsupported binop {:?} for type", op),
                span: dummy_span,
            }),
        }
    }

    // ── Finish: resolve labels → absolute addresses ──

    fn finalize(self) -> (Vec<Opcode>, CodegenState) {
        // Resolve labels to absolute IR indices → opcode positions.
        // IR ops map 1:1 to opcodes, so IR index == opcode index.
        let mut opcodes = Vec::new();
        for ir_op in &self.ir {
            match ir_op {
                IrOp::Op(op) => opcodes.push(op.clone()),
                IrOp::JumpLabel(label) => {
                    let pos = self.label_positions.get(label).copied().unwrap_or(0) as u32;
                    opcodes.push(Opcode::Jump(pos));
                }
                IrOp::JumpIfFalseLabel(label) => {
                    let pos = self.label_positions.get(label).copied().unwrap_or(0) as u32;
                    opcodes.push(Opcode::JumpIfFalse(pos));
                }
                IrOp::JumpIfTrueLabel(label) => {
                    let pos = self.label_positions.get(label).copied().unwrap_or(0) as u32;
                    opcodes.push(Opcode::JumpIfTrue(pos));
                }
            }
        }
        (opcodes, self.state)
    }
}
