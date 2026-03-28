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
    gene.finish()
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
    constants: Vec<Constant>,
    slot_map: HashMap<u32, u32>, // unique_id → local slot
    next_slot: u32,
    type_registry: TypeRegistry,
    error_templates: Vec<ErrTemplate>,
    next_label: u32,
    label_positions: HashMap<Label, usize>, // label → IR index it points to
}

impl Codegen {
    fn new() -> Self {
        Self {
            ir: Vec::new(),
            constants: Vec::new(),
            slot_map: HashMap::new(),
            next_slot: 0,
            type_registry: TypeRegistry::new(),
            error_templates: Vec::new(),
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
        if let Some(&slot) = self.slot_map.get(&unique_id) {
            return slot;
        }
        let slot = self.next_slot;
        self.next_slot += 1;
        self.slot_map.insert(unique_id, slot);
        slot
    }

    fn add_constant(&mut self, c: Constant) -> u32 {
        // Check for existing identical constant
        for (i, existing) in self.constants.iter().enumerate() {
            if existing == &c {
                return i as u32;
            }
        }
        let idx = self.constants.len() as u32;
        self.constants.push(c);
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
        for (i, stmt) in stmts.iter().enumerate() {
            self.emit_node(stmt)?;
            // Top-level expressions that aren't the last: pop unused values
            // Binds already produce Unit on stack, so pop it too
            let is_last = i == stmts.len() - 1;
            // Always pop top-level statement results
            self.emit(Opcode::Pop);
        }
        self.emit(Opcode::Halt);
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

            TypedInner::DeferrorDef(_tag, _show_expr) => {
                // Error type definitions don't produce runtime code in phase 1
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::StructDef(tag, name, field_names) => {
                self.type_registry.register(TypeEntry {
                    tag: *tag,
                    name: name.clone(),
                    kind: TypeKind::Struct,
                    field_names: field_names.clone(),
                });
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::RecordDef(tag, name, field_names) => {
                self.type_registry.register(TypeEntry {
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
            _ => {
                return Err(CodegenError {
                    message: "Non-builtin function calls not supported in phase 1".into(),
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

    // ── Match ──

    fn emit_match(
        &mut self,
        scrutinee: &TypedNode,
        arms: &[(TypedMatchPattern, TypedNode)],
    ) -> Result<(), CodegenError> {
        self.emit_node(scrutinee)?;

        let scrut_slot = self.next_slot;
        self.next_slot += 1;
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
                TypedMatchPattern::BoolLit(b) => {
                    self.emit(Opcode::LoadLocal(scrut_slot));
                    let bool_const = self.add_constant(Constant::Bool(*b));
                    self.emit(Opcode::LoadConst(bool_const));
                    self.emit(Opcode::EqBool);
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

    fn finish(self) -> Result<Bytecode, CodegenError> {
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

        Ok(Bytecode {
            opcodes,
            constants: self.constants,
            num_locals: self.next_slot as usize,
            type_registry: self.type_registry,
            error_templates: self.error_templates,
        })
    }
}
