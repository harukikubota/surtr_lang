#![allow(unused_imports, unused_variables)]

use std::collections::HashMap;

use scar::typed::*;
use scar::types::Ty;
use sigil::resolved::ResolvedId;
use sindr::builtin::builtin_meta_by_name;
use sindr::ir::DocEntry;
use sindr::primitives::int;
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
        source_map: None,
        docs: Vec::new(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplTypeKind {
    Struct,
    Record,
    Error,
    Enum,
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
    pub docs: Vec<DocEntry>,
}

#[derive(Debug, Clone)]
struct CodegenState {
    constants: Vec<Constant>,
    slot_map: HashMap<u32, u32>, // unique_id → local slot
    next_slot: u32,
    next_fun_idx: u32,
    type_registry: TypeRegistry,
    error_templates: Vec<ErrTemplate>,
    functions: Vec<FunctionEntry>,
    error_ctor_funs: HashMap<String, (u32, u8)>, // error kind -> (fun_idx, arity)
}

impl CodegenState {
    fn new() -> Self {
        Self {
            constants: Vec::new(),
            slot_map: HashMap::new(),
            next_slot: 0,
            next_fun_idx: 0,
            type_registry: TypeRegistry::new(),
            error_templates: Vec::new(),
            functions: Vec::new(),
            error_ctor_funs: HashMap::new(),
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

    /// Restore a `ForgeSession` from an already-executed `Bytecode`.
    ///
    /// This is used when the TUI (or REPL) loads a pre-compiled `.eldr` file.
    /// The restored session can generate new REPL chunks that append on top of
    /// the loaded bytecode.  It cannot recover `slot_map` (let-binding
    /// unique_id → slot), so previous REPL local variables are not accessible
    /// by name in new input.  Function definitions and the type registry are
    /// fully restored.
    pub fn from_bytecode(bytecode: &sindr::ir::Bytecode) -> Self {
        let next_fun_idx = bytecode
            .functions
            .iter()
            .map(|f| f.fun_idx + 1)
            .max()
            .unwrap_or(0);

        // Reconstruct error_ctor_funs: ErrTemplate.kind → (fun_idx, num_params).
        let mut error_ctor_funs = HashMap::new();
        for template in &bytecode.error_templates {
            if let Some(fun_entry) = bytecode
                .functions
                .iter()
                .find(|f| f.qualified_name.as_deref() == Some(&template.kind))
            {
                error_ctor_funs.insert(
                    template.kind.clone(),
                    (fun_entry.fun_idx, template.num_params),
                );
            }
        }

        Self {
            state: CodegenState {
                constants: bytecode.constants.clone(),
                slot_map: HashMap::new(),
                next_slot: bytecode.num_locals as u32,
                next_fun_idx,
                type_registry: bytecode.type_registry.clone(),
                error_templates: bytecode.error_templates.clone(),
                functions: bytecode.functions.clone(),
                error_ctor_funs,
            },
        }
    }

    pub fn codegen_chunk(
        &mut self,
        typed: Vec<TypedNode>,
    ) -> Result<(BytecodeChunk, ChunkMeta), CodegenError> {
        self.codegen_chunk_with_options(typed, false)
    }

    pub fn codegen_chunk_repl_result(
        &mut self,
        typed: Vec<TypedNode>,
    ) -> Result<(BytecodeChunk, ChunkMeta), CodegenError> {
        self.codegen_chunk_with_options(typed, true)
    }

    fn codegen_chunk_with_options(
        &mut self,
        typed: Vec<TypedNode>,
        top_level_returns_result: bool,
    ) -> Result<(BytecodeChunk, ChunkMeta), CodegenError> {
        let before = self.state.clone();
        let typed_for_meta = typed.clone();
        let const_base = before.constants.len();
        let error_template_base = before.error_templates.len();

        let mut gene = Codegen::from_state(before.clone());
        gene.set_chunk_constant_dedup_start(const_base);
        gene.set_top_level_returns_result(top_level_returns_result);
        gene.emit_program_chunk(typed)?;
        let (mut opcodes, after) = gene.finalize();
        localize_chunk_indices(&mut opcodes, const_base, error_template_base)?;

        let new_constants = after.constants[before.constants.len()..].to_vec();
        let new_locals = after.next_slot.saturating_sub(before.next_slot) as usize;
        let type_entries =
            after.type_registry.entries[before.type_registry.entries.len()..].to_vec();
        let error_templates = after.error_templates[before.error_templates.len()..].to_vec();
        let meta = collect_chunk_meta(&typed_for_meta, &after.slot_map);
        let functions = after.functions[before.functions.len()..].to_vec();

        self.state = after;

        let const_base = u32::try_from(const_base).map_err(|_| CodegenError {
            message: "constant base exceeds u32".into(),
            span: Span { start: 0, end: 0 },
        })?;
        let error_template_base = u32::try_from(error_template_base).map_err(|_| CodegenError {
            message: "error template base exceeds u32".into(),
            span: Span { start: 0, end: 0 },
        })?;

        Ok((
            BytecodeChunk {
                opcodes,
                source_map: None,
                const_base,
                constants: new_constants,
                new_locals,
                type_entries,
                error_template_base,
                error_templates,
                functions,
            },
            meta,
        ))
    }
}

fn localize_chunk_indices(
    opcodes: &mut [Opcode],
    const_base: usize,
    error_template_base: usize,
) -> Result<(), CodegenError> {
    for op in opcodes.iter_mut() {
        match op {
            Opcode::LoadConst(idx) => {
                let idx_usize = *idx as usize;
                if idx_usize < const_base {
                    return Err(CodegenError {
                        message: format!(
                            "chunk constant index {} is below base {}",
                            idx_usize, const_base
                        ),
                        span: Span { start: 0, end: 0 },
                    });
                }
                *idx = (idx_usize - const_base) as u32;
            }
            Opcode::MakeError { template_id } => {
                let id_usize = *template_id as usize;
                if id_usize < error_template_base {
                    return Err(CodegenError {
                        message: format!(
                            "chunk error template index {} is below base {}",
                            id_usize, error_template_base
                        ),
                        span: Span { start: 0, end: 0 },
                    });
                }
                *template_id = (id_usize - error_template_base) as u32;
            }
            _ => {}
        }
    }
    Ok(())
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
        collect_stmt_meta(
            stmt,
            slot_map,
            &mut bindings,
            &mut type_defs,
            &mut function_defs,
        );
    }

    ChunkMeta {
        bindings,
        type_defs,
        function_defs,
        docs: Vec::new(),
    }
}

fn collect_stmt_meta(
    stmt: &TypedNode,
    slot_map: &HashMap<u32, u32>,
    bindings: &mut Vec<BindingInfo>,
    type_defs: &mut Vec<TypeDefDisplay>,
    function_defs: &mut Vec<String>,
) {
    match &stmt.node {
        TypedInner::Bind(pat, _) | TypedInner::SafeBind(pat, _) => {
            collect_pattern_binding_infos(pat, slot_map, bindings);
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
        TypedInner::DeferrorDef(_, _, id, _, _) => {
            type_defs.push(TypeDefDisplay {
                name: id.name.clone(),
                kind: ReplTypeKind::Error,
                fields: Vec::new(),
            });
            function_defs.push(id.name.clone());
        }
        TypedInner::EnumDef(name, variants) => {
            type_defs.push(TypeDefDisplay {
                name: name.clone(),
                kind: ReplTypeKind::Enum,
                fields: variants
                    .iter()
                    .map(|variant| (variant.constructor_name.clone(), String::new()))
                    .collect(),
            });
        }
        TypedInner::Def(_, id, _, _, _) => {
            function_defs.push(id.name.clone());
        }
        // `;` keeps Unit as expression result, but for REPL metadata we still
        // want to surface bindings/type defs introduced by the wrapped statement.
        TypedInner::Semi(inner) => {
            collect_stmt_meta(inner, slot_map, bindings, type_defs, function_defs);
        }
        _ => {}
    }
}

fn collect_pattern_binding_infos(
    pat: &TypedPattern,
    slot_map: &HashMap<u32, u32>,
    out: &mut Vec<BindingInfo>,
) {
    match pat {
        TypedPattern::Var(ty, id) => {
            if let Some(slot_id) = slot_map.get(&id.unique_id) {
                out.push(BindingInfo {
                    name: id.name.clone(),
                    ty: ty_to_string(ty),
                    slot_id: *slot_id,
                });
            }
        }
        TypedPattern::As(ty, inner, id) => {
            if let Some(slot_id) = slot_map.get(&id.unique_id) {
                out.push(BindingInfo {
                    name: id.name.clone(),
                    ty: ty_to_string(ty),
                    slot_id: *slot_id,
                });
            }
            collect_pattern_binding_infos(inner, slot_map, out);
        }
        TypedPattern::Wildcard(_)
        | TypedPattern::ListNil(_)
        | TypedPattern::IntLit(_, _)
        | TypedPattern::StrLit(_, _)
        | TypedPattern::BoolLit(_, _) => {}
        TypedPattern::ListCons(_, head, tail) => {
            collect_pattern_binding_infos(head, slot_map, out);
            collect_pattern_binding_infos(tail, slot_map, out);
        }
        TypedPattern::ResultOk(_, inner) => {
            collect_pattern_binding_infos(inner, slot_map, out);
        }
    }
}

fn ty_to_string(ty: &Ty) -> String {
    match ty {
        Ty::Int => "Int".into(),
        Ty::Float => "Float".into(),
        Ty::Str => "String".into(),
        Ty::Bool => "Boolean".into(),
        Ty::Unit => "Unit".into(),
        Ty::List(inner) => format!("List<{}>", ty_to_string(inner)),
        Ty::Result(ok, err) => format!("Result<{}, {}>", ty_to_string(ok), ty_to_string(err)),
        Ty::Struct(name, _) | Ty::Record(name, _) | Ty::Enum(name) => name.clone(),
        Ty::Error => "Error".into(),
        // Hide internal type-variable IDs from REPL output.
        Ty::Var(_id) => "_".into(),
        Ty::Func(params, ret) => {
            let param_str = params
                .iter()
                .map(ty_to_string)
                .collect::<Vec<_>>()
                .join(", ");
            if param_str.is_empty() {
                format!("(-> {})", ty_to_string(ret))
            } else {
                format!("({} -> {})", param_str, ty_to_string(ret))
            }
        }
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

#[derive(Debug, Clone)]
struct PendingClosure {
    fun_idx: u32,
    captures: Vec<ResolvedId>,
    params: Vec<TypedClosureParam>,
    body: Box<TypedNode>,
}

#[derive(Debug, Clone)]
struct PendingCompose {
    fun_idx: u32,
    flavor: ComposeFlavor,
    span: Span,
}

#[derive(Debug, Clone)]
struct PendingInjectCall {
    fun_idx: u32,
    extra_arg_count: usize,
    span: Span,
}

struct Codegen {
    ir: Vec<IrOp>,
    state: CodegenState,
    next_label: u32,
    label_positions: HashMap<Label, usize>, // label → IR index it points to
    pending_closures: Vec<PendingClosure>,
    pending_composes: Vec<PendingCompose>,
    pending_inject_calls: Vec<PendingInjectCall>,
    in_function: bool,
    top_level_returns_result: bool,
    constant_dedup_start: usize,
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
            pending_closures: Vec::new(),
            pending_composes: Vec::new(),
            pending_inject_calls: Vec::new(),
            in_function: false,
            top_level_returns_result: false,
            constant_dedup_start: 0,
        }
    }

    fn set_chunk_constant_dedup_start(&mut self, start: usize) {
        self.constant_dedup_start = start;
    }

    fn set_top_level_returns_result(&mut self, enabled: bool) {
        self.top_level_returns_result = enabled;
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

    fn reserve_fun_idx(&mut self) -> u32 {
        let fun_idx = self.state.next_fun_idx;
        self.state.next_fun_idx += 1;
        fun_idx
    }

    fn builtin_id(name: &str) -> Option<u16> {
        builtin_meta_by_name(name).map(|meta| meta.builtin_id)
    }

    fn emit_callable_ref(&mut self, node: &TypedNode) -> Result<(), CodegenError> {
        match &node.node {
            TypedInner::Var(id) => match &node.ty {
                Ty::BuiltinFunc { name, .. } => {
                    let builtin_id = Self::builtin_id(name).ok_or_else(|| CodegenError {
                        message: format!("Unknown builtin: {}", name),
                        span: node.span.clone(),
                    })?;
                    self.emit(Opcode::LoadBuiltinRef(builtin_id));
                }
                Ty::UserFunc { fun_idx, .. } => {
                    self.emit(Opcode::LoadFunctionRef(*fun_idx));
                }
                Ty::Func(_, _) => {
                    let slot = self.alloc_slot(id.unique_id);
                    self.emit(Opcode::LoadLocal(slot));
                }
                _ => {
                    return Err(CodegenError {
                        message: "Not a callable value".into(),
                        span: node.span.clone(),
                    });
                }
            },
            _ => {
                self.emit_node(node)?;
            }
        }
        Ok(())
    }

    fn emit_closure_function(
        &mut self,
        fun_idx: u32,
        params: &[TypedClosureParam],
        captures: &[ResolvedId],
        body: &TypedNode,
    ) -> Result<(), CodegenError> {
        let saved_slot_map = self.state.slot_map.clone();
        let saved_next_slot = self.state.next_slot;

        self.state.slot_map = HashMap::new();
        self.state.next_slot = 0;

        let mut slot = 0u32;
        for capture in captures {
            self.state.slot_map.insert(capture.unique_id, slot);
            slot += 1;
        }
        for param in params {
            self.state.slot_map.insert(param.id.unique_id, slot);
            slot += 1;
        }
        self.state.next_slot = slot;

        let entry_pc = self.current_pos() as u32;
        let total_arity = captures.len() + params.len();
        let prev_in_function = self.in_function;
        self.in_function = true;
        self.emit_node(body)?;
        self.in_function = prev_in_function;
        self.emit(Opcode::Return);

        self.state.functions.push(FunctionEntry {
            fun_idx,
            entry_pc,
            num_locals: self.state.next_slot,
            arity: total_arity as u8,
            qualified_name: None,
        });

        self.state.slot_map = saved_slot_map;
        self.state.next_slot = saved_next_slot;
        Ok(())
    }

    fn emit_pending_closures(&mut self) -> Result<(), CodegenError> {
        while !self.pending_closures.is_empty() {
            let pending = std::mem::take(&mut self.pending_closures);
            for closure in pending {
                self.emit_closure_function(
                    closure.fun_idx,
                    &closure.params,
                    &closure.captures,
                    &closure.body,
                )?;
            }
        }
        Ok(())
    }

    fn emit_compose_function(
        &mut self,
        fun_idx: u32,
        flavor: &ComposeFlavor,
        span: &Span,
    ) -> Result<(), CodegenError> {
        let saved_slot_map = self.state.slot_map.clone();
        let saved_next_slot = self.state.next_slot;

        self.state.slot_map = HashMap::new();
        self.state.next_slot = 3;

        let lhs_slot = 0u32;
        let rhs_slot = 1u32;
        let input_slot = 2u32;
        let entry_pc = self.current_pos() as u32;
        let prev_in_function = self.in_function;
        self.in_function = true;

        match flavor {
            ComposeFlavor::Plain => {
                self.emit(Opcode::LoadLocal(rhs_slot));
                self.emit(Opcode::LoadLocal(lhs_slot));
                self.emit(Opcode::LoadLocal(input_slot));
                self.emit(Opcode::CallClosure {
                    arity: 1,
                    span_start: span.start as u32,
                    span_end: span.end as u32,
                });
                self.emit(Opcode::CallClosure {
                    arity: 1,
                    span_start: span.start as u32,
                    span_end: span.end as u32,
                });
                self.emit(Opcode::Return);
            }
            ComposeFlavor::ResultMap | ComposeFlavor::ResultBind => {
                self.emit(Opcode::LoadLocal(lhs_slot));
                self.emit(Opcode::LoadLocal(input_slot));
                self.emit(Opcode::CallClosure {
                    arity: 1,
                    span_start: span.start as u32,
                    span_end: span.end as u32,
                });
                let result_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::StoreLocal(result_slot));

                self.emit(Opcode::LoadLocal(result_slot));
                self.emit(Opcode::GetTag);
                let err_tag = self.add_constant(Constant::Tag(1));
                self.emit(Opcode::LoadConst(err_tag));
                self.emit(Opcode::EqTag);

                let ok_path = self.fresh_label();
                self.emit_jump_if_false(ok_path);
                self.emit(Opcode::LoadLocal(result_slot));
                self.emit(Opcode::Return);

                self.patch_label(ok_path);
                match flavor {
                    ComposeFlavor::ResultMap => {
                        let ok_tag = self.add_constant(Constant::Tag(0));
                        self.emit(Opcode::LoadConst(ok_tag));
                        self.emit(Opcode::LoadLocal(rhs_slot));
                        self.emit(Opcode::LoadLocal(result_slot));
                        self.emit(Opcode::GetField { field_index: 0 });
                        self.emit(Opcode::CallClosure {
                            arity: 1,
                            span_start: span.start as u32,
                            span_end: span.end as u32,
                        });
                        self.emit(Opcode::StructNew { field_count: 1 });
                        self.emit(Opcode::Return);
                    }
                    ComposeFlavor::ResultBind => {
                        self.emit(Opcode::LoadLocal(rhs_slot));
                        self.emit(Opcode::LoadLocal(result_slot));
                        self.emit(Opcode::GetField { field_index: 0 });
                        self.emit(Opcode::CallClosure {
                            arity: 1,
                            span_start: span.start as u32,
                            span_end: span.end as u32,
                        });
                        self.emit(Opcode::Return);
                    }
                    _ => unreachable!(),
                }
            }
            ComposeFlavor::ListMap { helper } | ComposeFlavor::ListBind { helper } => {
                self.emit(Opcode::LoadLocal(lhs_slot));
                self.emit(Opcode::LoadLocal(input_slot));
                self.emit(Opcode::CallClosure {
                    arity: 1,
                    span_start: span.start as u32,
                    span_end: span.end as u32,
                });
                self.emit(Opcode::LoadLocal(rhs_slot));
                match helper {
                    ListHelperRef::Builtin(builtin_id) => self.emit(Opcode::CallBuiltin {
                        builtin_id: *builtin_id,
                        arity: 2,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    }),
                    ListHelperRef::User(fun_idx) => self.emit(Opcode::Call {
                        fun_idx: *fun_idx,
                        arity: 2,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    }),
                }
                self.emit(Opcode::Return);
            }
        }

        self.in_function = prev_in_function;
        self.state.functions.push(FunctionEntry {
            fun_idx,
            entry_pc,
            num_locals: self.state.next_slot,
            arity: 3,
            qualified_name: None,
        });

        self.state.slot_map = saved_slot_map;
        self.state.next_slot = saved_next_slot;
        Ok(())
    }

    fn emit_inject_call_function(
        &mut self,
        fun_idx: u32,
        extra_arg_count: usize,
        span: &Span,
    ) -> Result<(), CodegenError> {
        let saved_slot_map = self.state.slot_map.clone();
        let saved_next_slot = self.state.next_slot;

        self.state.slot_map = HashMap::new();
        let func_slot = 0u32;
        let input_slot = (extra_arg_count + 1) as u32;
        self.state.next_slot = input_slot + 1;

        let entry_pc = self.current_pos() as u32;
        let prev_in_function = self.in_function;
        self.in_function = true;

        self.emit(Opcode::LoadLocal(func_slot));
        self.emit(Opcode::LoadLocal(input_slot));
        for offset in 0..extra_arg_count {
            self.emit(Opcode::LoadLocal((offset + 1) as u32));
        }
        self.emit(Opcode::CallClosure {
            arity: (extra_arg_count + 1) as u8,
            span_start: span.start as u32,
            span_end: span.end as u32,
        });
        self.in_function = prev_in_function;
        self.emit(Opcode::Return);

        self.state.functions.push(FunctionEntry {
            fun_idx,
            entry_pc,
            num_locals: self.state.next_slot,
            arity: (extra_arg_count + 2) as u8,
            qualified_name: None,
        });

        self.state.slot_map = saved_slot_map;
        self.state.next_slot = saved_next_slot;
        Ok(())
    }

    fn emit_pending_callables(&mut self) -> Result<(), CodegenError> {
        while !self.pending_closures.is_empty()
            || !self.pending_composes.is_empty()
            || !self.pending_inject_calls.is_empty()
        {
            if !self.pending_closures.is_empty() {
                self.emit_pending_closures()?;
            }
            if !self.pending_composes.is_empty() {
                let pending = std::mem::take(&mut self.pending_composes);
                for compose in pending {
                    self.emit_compose_function(compose.fun_idx, &compose.flavor, &compose.span)?;
                }
            }
            if !self.pending_inject_calls.is_empty() {
                let pending = std::mem::take(&mut self.pending_inject_calls);
                for inject_call in pending {
                    self.emit_inject_call_function(
                        inject_call.fun_idx,
                        inject_call.extra_arg_count,
                        &inject_call.span,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn add_constant(&mut self, c: Constant) -> u32 {
        // Check for existing identical constant
        for (i, existing) in self
            .state
            .constants
            .iter()
            .enumerate()
            .skip(self.constant_dedup_start)
        {
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
        self.emit_program_with_functions(stmts, false)
    }

    fn emit_program_chunk(&mut self, stmts: Vec<TypedNode>) -> Result<(), CodegenError> {
        self.emit_program_with_functions(stmts, false)
    }

    fn emit_program_with_functions(
        &mut self,
        stmts: Vec<TypedNode>,
        pop_last: bool,
    ) -> Result<(), CodegenError> {
        // Contract with VM::push_atomic():
        // - Main/top-level statements are emitted first.
        // - A single Halt terminates top-level execution.
        // - Function bodies are emitted strictly after Halt and are entered only via Call/CallClosure.
        // - Top-level duplicate function names are rejected earlier in Sigil.
        let mut defs = Vec::new();
        let mut main_stmts = Vec::new();
        let max_def_fun_idx = stmts
            .iter()
            .filter_map(|stmt| match &stmt.node {
                TypedInner::Def(fun_idx, _, _, _, _) => Some(*fun_idx),
                TypedInner::DeferrorDef(_, fun_idx, _, _, _) => Some(*fun_idx),
                _ => None,
            })
            .max()
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let existing_fun_idx = self
            .state
            .functions
            .iter()
            .map(|entry| entry.fun_idx)
            .max()
            .map(|idx| idx + 1)
            .unwrap_or(0);
        self.state.next_fun_idx = self
            .state
            .next_fun_idx
            .max(max_def_fun_idx)
            .max(existing_fun_idx);

        for stmt in &stmts {
            if let TypedInner::DeferrorDef(_, fun_idx, id, params, _) = &stmt.node {
                self.state
                    .error_ctor_funs
                    .insert(id.name.clone(), (*fun_idx, params.len() as u8));
            }
            match &stmt.node {
                TypedInner::Def(..) | TypedInner::DeferrorDef(..) => defs.push(stmt),
                _ => main_stmts.push(stmt),
            }
        }
        defs.sort_by_key(|stmt| match &stmt.node {
            TypedInner::Def(fun_idx, _, _, _, _) => *fun_idx,
            TypedInner::DeferrorDef(_, fun_idx, _, _, _) => *fun_idx,
            _ => u32::MAX,
        });

        for (i, stmt) in main_stmts.iter().enumerate() {
            self.emit_node(stmt)?;
            if pop_last || i + 1 < main_stmts.len() {
                self.emit(Opcode::Pop);
            }
        }

        self.emit(Opcode::Halt);

        for def in defs {
            match &def.node {
                TypedInner::Def(..) => self.emit_function_def(def)?,
                TypedInner::DeferrorDef(..) => self.emit_error_def(def)?,
                _ => unreachable!(),
            }
        }

        self.emit_pending_callables()?;
        self.normalize_function_table()?;

        Ok(())
    }

    fn emit_function_def(&mut self, node: &TypedNode) -> Result<(), CodegenError> {
        let (fun_idx, id, params, _ret_ty, body) = match &node.node {
            TypedInner::Def(fun_idx, id, params, ret_ty, body) => {
                (fun_idx, id, params, ret_ty, body)
            }
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
        let prev_in_function = self.in_function;
        self.in_function = true;
        self.emit_node(body)?;
        self.in_function = prev_in_function;
        self.emit(Opcode::Return);

        let num_locals = self.state.next_slot;
        self.state.functions.push(FunctionEntry {
            fun_idx: *fun_idx,
            entry_pc,
            num_locals,
            arity: params.len() as u8,
            qualified_name: id.qualified_name.clone().or_else(|| Some(id.name.clone())),
        });
        self.state.next_fun_idx = self.state.next_fun_idx.max(*fun_idx + 1);

        self.state.slot_map = saved_slot_map;
        self.state.next_slot = saved_next_slot;

        let _ = id;
        Ok(())
    }

    fn emit_error_def(&mut self, node: &TypedNode) -> Result<(), CodegenError> {
        let (_tag, fun_idx, id, params, body) = match &node.node {
            TypedInner::DeferrorDef(tag, fun_idx, id, params, body) => {
                (tag, fun_idx, id, params, body)
            }
            _ => {
                return Err(CodegenError {
                    message: "expected error definition".into(),
                    span: node.span.clone(),
                });
            }
        };

        let template_id = self.state.error_templates.len() as u32;
        self.state.error_templates.push(ErrTemplate {
            id: template_id,
            kind: id.name.clone(),
            span_start: id.span.start as u32,
            span_end: id.span.end as u32,
            line: 0,
            column: 0,
            format: String::new(),
            num_params: params.len() as u8,
        });

        let saved_slot_map = self.state.slot_map.clone();
        let saved_next_slot = self.state.next_slot;

        self.state.slot_map = HashMap::new();
        self.state.next_slot = 0;

        for (slot, param) in params.iter().enumerate() {
            self.state.slot_map.insert(param.id.unique_id, slot as u32);
        }
        self.state.next_slot = params.len() as u32;

        let entry_pc = self.current_pos() as u32;
        let prev_in_function = self.in_function;
        self.in_function = true;
        self.emit_node(body)?;
        self.in_function = prev_in_function;
        self.emit(Opcode::MakeError { template_id });
        self.emit(Opcode::Return);

        let num_locals = self.state.next_slot;
        self.state.functions.push(FunctionEntry {
            fun_idx: *fun_idx,
            entry_pc,
            num_locals,
            arity: params.len() as u8,
            qualified_name: id.qualified_name.clone().or_else(|| Some(id.name.clone())),
        });
        self.state
            .error_ctor_funs
            .insert(id.name.clone(), (*fun_idx, params.len() as u8));

        self.state.slot_map = saved_slot_map;
        self.state.next_slot = saved_next_slot;
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
                if matches!(node.ty, Ty::BuiltinFunc { .. } | Ty::UserFunc { .. }) {
                    return Err(CodegenError {
                        message: "Function values must be captured explicitly".into(),
                        span: node.span.clone(),
                    });
                }
                let slot = self.alloc_slot(id.unique_id);
                self.emit(Opcode::LoadLocal(slot));
            }

            TypedInner::Bind(pat, rhs) => {
                self.emit_node(rhs)?;
                let payload_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::StoreLocal(payload_slot));

                let fail_label = self.fresh_label();
                self.emit_pattern_test_from_local_for_bind(
                    pat,
                    payload_slot,
                    fail_label,
                    &rhs.span,
                )?;
                self.emit_pattern_bind_from_local(pat, payload_slot)?;

                let success_label = self.fresh_label();
                self.emit_jump(success_label);

                self.patch_label(fail_label);
                self.emit_pattern_mismatch_failure(rhs.span.clone())?;

                self.patch_label(success_label);
                // Bind produces Unit
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::SafeBind(pat, rhs) => {
                self.emit_safebind(pat, rhs)?;
            }

            TypedInner::App(func, args) => {
                self.emit_app(node.span.clone(), func, args)?;
            }

            TypedInner::InjectCall(func, args) => {
                let fun_idx = self.reserve_fun_idx();
                self.pending_inject_calls.push(PendingInjectCall {
                    fun_idx,
                    extra_arg_count: args.len(),
                    span: node.span.clone(),
                });
                self.emit(Opcode::LoadFunctionRef(fun_idx));
                self.emit_callable_ref(func)?;
                for arg in args {
                    self.emit_node(arg)?;
                }
                self.emit(Opcode::CaptureClosure((args.len() + 1) as u8));
            }

            TypedInner::BinOp(op, left, right) => {
                if matches!(op, BinOp::Eq | BinOp::Neq) && matches!(left.ty, Ty::Enum(_)) {
                    self.emit_enum_eq(op, left, right)?;
                } else {
                    self.emit_node(left)?;
                    self.emit_node(right)?;
                    let opcode = self.binop_to_opcode(op, &left.ty)?;
                    self.emit(opcode);
                }
            }

            TypedInner::Pipe(left, right) => {
                self.emit_callable_ref(right)?;
                self.emit_node(left)?;
                self.emit(Opcode::CallClosure {
                    arity: 1,
                    span_start: node.span.start as u32,
                    span_end: node.span.end as u32,
                });
            }

            TypedInner::ResultMap(left, right) => {
                self.emit_node(left)?;
                let result_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::StoreLocal(result_slot));

                self.emit(Opcode::LoadLocal(result_slot));
                self.emit(Opcode::GetTag);
                let err_tag = self.add_constant(Constant::Tag(1));
                self.emit(Opcode::LoadConst(err_tag));
                self.emit(Opcode::EqTag);

                let ok_path = self.fresh_label();
                let end_label = self.fresh_label();
                self.emit_jump_if_false(ok_path);
                self.emit(Opcode::LoadLocal(result_slot));
                self.emit_jump(end_label);

                self.patch_label(ok_path);
                let ok_tag = self.add_constant(Constant::Tag(0));
                self.emit(Opcode::LoadConst(ok_tag));
                self.emit_callable_ref(right)?;
                self.emit(Opcode::LoadLocal(result_slot));
                self.emit(Opcode::GetField { field_index: 0 });
                self.emit(Opcode::CallClosure {
                    arity: 1,
                    span_start: node.span.start as u32,
                    span_end: node.span.end as u32,
                });
                self.emit(Opcode::StructNew { field_count: 1 });

                self.patch_label(end_label);
            }

            TypedInner::ResultBind(left, right) => {
                self.emit_node(left)?;
                let result_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::StoreLocal(result_slot));

                self.emit(Opcode::LoadLocal(result_slot));
                self.emit(Opcode::GetTag);
                let err_tag = self.add_constant(Constant::Tag(1));
                self.emit(Opcode::LoadConst(err_tag));
                self.emit(Opcode::EqTag);

                let ok_path = self.fresh_label();
                let end_label = self.fresh_label();
                self.emit_jump_if_false(ok_path);
                self.emit(Opcode::LoadLocal(result_slot));
                self.emit_jump(end_label);

                self.patch_label(ok_path);
                self.emit_callable_ref(right)?;
                self.emit(Opcode::LoadLocal(result_slot));
                self.emit(Opcode::GetField { field_index: 0 });
                self.emit(Opcode::CallClosure {
                    arity: 1,
                    span_start: node.span.start as u32,
                    span_end: node.span.end as u32,
                });

                self.patch_label(end_label);
            }

            TypedInner::Compose(flavor, left, right) => {
                let fun_idx = self.reserve_fun_idx();
                self.pending_composes.push(PendingCompose {
                    fun_idx,
                    flavor: flavor.clone(),
                    span: node.span.clone(),
                });
                self.emit(Opcode::LoadFunctionRef(fun_idx));
                self.emit_callable_ref(left)?;
                self.emit_callable_ref(right)?;
                self.emit(Opcode::CaptureClosure(2));
            }

            TypedInner::ListNil => self.emit(Opcode::ListNil),
            TypedInner::ListCons(head, tail) => {
                self.emit_node(head)?;
                self.emit_node(tail)?;
                self.emit(Opcode::ListCons);
            }
            TypedInner::ListLiteral(elems) => {
                for elem in elems {
                    self.emit_node(elem)?;
                }
                self.emit(Opcode::ListFromItems {
                    len: elems.len() as u32,
                });
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
                self.emit(Opcode::GetField { field_index: *idx });
            }

            TypedInner::StructLit(tag, fields) => {
                // Push tag first, then fields
                let tag_const = self.add_constant(Constant::Tag(*tag));
                self.emit(Opcode::LoadConst(tag_const));
                for field in fields {
                    self.emit_node(field)?;
                }
                // StructNew expects tag + n fields on stack
                self.emit(Opcode::StructNew {
                    field_count: fields.len() as u32,
                });
            }

            TypedInner::ConstructorCall(tag, fields) => {
                let tag_const = self.add_constant(Constant::Tag(*tag));
                self.emit(Opcode::LoadConst(tag_const));
                for field in fields {
                    self.emit_node(field)?;
                }
                self.emit(Opcode::StructNew {
                    field_count: fields.len() as u32,
                });
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

            TypedInner::DeferrorDef(_, _, _, _, _) => {
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::Def(_fun_idx, _id, _params, _ret_ty, _body) => {
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::Closure(params, captures, body) => {
                let filtered_captures: Vec<ResolvedId> = captures
                    .iter()
                    .filter(|id| self.state.slot_map.contains_key(&id.unique_id))
                    .cloned()
                    .collect();
                let fun_idx = self.reserve_fun_idx();
                self.pending_closures.push(PendingClosure {
                    fun_idx,
                    captures: filtered_captures.clone(),
                    params: params.clone(),
                    body: body.clone(),
                });
                self.emit(Opcode::LoadFunctionRef(fun_idx));
                for capture in &filtered_captures {
                    let slot = self.alloc_slot(capture.unique_id);
                    self.emit(Opcode::LoadLocal(slot));
                }
                self.emit(Opcode::CaptureClosure(filtered_captures.len() as u8));
            }

            TypedInner::Capture(target, args) => {
                self.emit_callable_ref(target)?;
                for arg in args {
                    self.emit_node(arg)?;
                }
                if !args.is_empty() {
                    self.emit(Opcode::CapturePartial(args.len() as u8));
                }
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

            TypedInner::EnumDef(_, variants) => {
                for variant in variants {
                    self.state.type_registry.register(TypeEntry {
                        tag: variant.tag,
                        name: variant.constructor_name.clone(),
                        kind: TypeKind::EnumVariant,
                        field_names: variant.field_names.clone(),
                    });
                }
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }
        }
        Ok(())
    }

    fn emit_safebind(&mut self, pat: &TypedPattern, rhs: &TypedNode) -> Result<(), CodegenError> {
        if !matches!(rhs.ty, Ty::Result(_, _)) {
            return self.emit_safebind_from_list(pat, rhs);
        }

        self.emit_node(rhs)?;

        // Preserve the Result value for tag check and payload extraction.
        let result_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(result_slot));

        self.emit(Opcode::LoadLocal(result_slot));
        self.emit(Opcode::GetTag);
        let err_tag = self.add_constant(Constant::Tag(1));
        self.emit(Opcode::LoadConst(err_tag));
        self.emit(Opcode::EqTag);

        let ok_path = self.fresh_label();
        self.emit_jump_if_false(ok_path);

        self.emit_propagate_result_from_local(result_slot, rhs.span.clone())?;

        self.patch_label(ok_path);

        self.emit(Opcode::LoadLocal(result_slot));
        self.emit(Opcode::GetField { field_index: 0 });
        let payload_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(payload_slot));

        if let Some(items) = Self::collect_exact_list_pattern_items(pat) {
            let lhs_len = items.len();
            let fail_shorts = (0..lhs_len).map(|_| self.fresh_label()).collect::<Vec<_>>();
            let fail_long = self.fresh_label();
            let fail_mismatch = self.fresh_label();
            let rest_slot = self.emit_exact_list_pattern_test_from_local(
                &items,
                payload_slot,
                &fail_shorts,
                fail_long,
                fail_mismatch,
                &rhs.span,
            )?;
            self.emit_pattern_bind_from_local(pat, payload_slot)?;
            let success_label = self.fresh_label();
            self.emit_jump(success_label);

            for (rhs_len, fail_short) in fail_shorts.into_iter().enumerate() {
                self.patch_label(fail_short);
                self.emit_list_len_mismatch_failure_concrete(
                    lhs_len,
                    rhs_len,
                    ">",
                    rhs.span.clone(),
                )?;
            }

            self.patch_label(fail_long);
            self.emit_list_len_mismatch_failure_rhs_long(lhs_len, rest_slot, rhs.span.clone())?;

            self.patch_label(fail_mismatch);
            self.emit_pattern_mismatch_failure(rhs.span.clone())?;

            self.patch_label(success_label);
            let unit_idx = self.add_constant(Constant::Unit);
            self.emit(Opcode::LoadConst(unit_idx));
            return Ok(());
        }

        let pattern_fail = self.fresh_label();
        self.emit_pattern_test_from_local(pat, payload_slot, pattern_fail, &rhs.span)?;
        self.emit_pattern_bind_from_local(pat, payload_slot)?;
        let success_label = self.fresh_label();
        self.emit_jump(success_label);

        self.patch_label(pattern_fail);
        self.emit_empty_list_failure(rhs.span.clone())?;

        self.patch_label(success_label);
        if matches!(pat, TypedPattern::Wildcard(_)) {
            // no-op
        }
        let unit_idx = self.add_constant(Constant::Unit);
        self.emit(Opcode::LoadConst(unit_idx));
        Ok(())
    }

    fn emit_safebind_from_list(
        &mut self,
        pat: &TypedPattern,
        rhs: &TypedNode,
    ) -> Result<(), CodegenError> {
        self.emit_node(rhs)?;

        let list_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(list_slot));

        if let Some(items) = Self::collect_exact_list_pattern_items(pat) {
            let lhs_len = items.len();
            let fail_shorts = (0..lhs_len).map(|_| self.fresh_label()).collect::<Vec<_>>();
            let fail_long = self.fresh_label();
            let fail_mismatch = self.fresh_label();
            let rest_slot = self.emit_exact_list_pattern_test_from_local(
                &items,
                list_slot,
                &fail_shorts,
                fail_long,
                fail_mismatch,
                &rhs.span,
            )?;
            self.emit_pattern_bind_from_local(pat, list_slot)?;
            let success_label = self.fresh_label();
            self.emit_jump(success_label);

            for (rhs_len, fail_short) in fail_shorts.into_iter().enumerate() {
                self.patch_label(fail_short);
                self.emit_list_len_mismatch_failure_concrete(
                    lhs_len,
                    rhs_len,
                    ">",
                    rhs.span.clone(),
                )?;
            }

            self.patch_label(fail_long);
            self.emit_list_len_mismatch_failure_rhs_long(lhs_len, rest_slot, rhs.span.clone())?;

            self.patch_label(fail_mismatch);
            self.emit_pattern_mismatch_failure(rhs.span.clone())?;

            self.patch_label(success_label);
            let unit_idx = self.add_constant(Constant::Unit);
            self.emit(Opcode::LoadConst(unit_idx));
            return Ok(());
        }

        let pattern_fail = self.fresh_label();
        self.emit_pattern_test_from_local(pat, list_slot, pattern_fail, &rhs.span)?;
        self.emit_pattern_bind_from_local(pat, list_slot)?;
        let success_label = self.fresh_label();
        self.emit_jump(success_label);

        self.patch_label(pattern_fail);
        self.emit_empty_list_failure(rhs.span.clone())?;

        self.patch_label(success_label);
        let unit_idx = self.add_constant(Constant::Unit);
        self.emit(Opcode::LoadConst(unit_idx));
        Ok(())
    }

    fn emit_empty_list_failure(&mut self, span: Span) -> Result<(), CodegenError> {
        self.emit_pattern_failure("EmptyList", "Empty List.", span)
    }

    fn emit_list_len_mismatch_failure_concrete(
        &mut self,
        lhs_len: usize,
        rhs_len: usize,
        op: &str,
        span: Span,
    ) -> Result<(), CodegenError> {
        self.emit_pattern_failure(
            "IndexOutOfBounds",
            &format!("LHS.len({}) {} RHS.len({})", lhs_len, op, rhs_len),
            span,
        )
    }

    fn emit_list_len_mismatch_failure_rhs_long(
        &mut self,
        lhs_len: usize,
        remainder_slot: u32,
        span: Span,
    ) -> Result<(), CodegenError> {
        let iter_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::LoadLocal(remainder_slot));
        self.emit(Opcode::StoreLocal(iter_slot));

        let rem_count_slot = self.state.next_slot;
        self.state.next_slot += 1;
        let zero_idx = self.add_constant(Constant::Int(int(0)));
        self.emit(Opcode::LoadConst(zero_idx));
        self.emit(Opcode::StoreLocal(rem_count_slot));

        let loop_head = self.fresh_label();
        let loop_done = self.fresh_label();
        self.patch_label(loop_head);
        self.emit(Opcode::LoadLocal(iter_slot));
        self.emit(Opcode::ListIsEmpty);
        self.emit_jump_if_true(loop_done);

        self.emit(Opcode::LoadLocal(rem_count_slot));
        let one_idx = self.add_constant(Constant::Int(int(1)));
        self.emit(Opcode::LoadConst(one_idx));
        self.emit(Opcode::AddInt);
        self.emit(Opcode::StoreLocal(rem_count_slot));

        self.emit(Opcode::LoadLocal(iter_slot));
        self.emit(Opcode::ListTail);
        self.emit(Opcode::StoreLocal(iter_slot));
        self.emit_jump(loop_head);
        self.patch_label(loop_done);

        let rhs_total_slot = self.state.next_slot;
        self.state.next_slot += 1;
        let lhs_idx = self.add_constant(Constant::Int(int(lhs_len as u64)));
        self.emit(Opcode::LoadConst(lhs_idx));
        self.emit(Opcode::LoadLocal(rem_count_slot));
        self.emit(Opcode::AddInt);
        self.emit(Opcode::StoreLocal(rhs_total_slot));

        let prefix_idx =
            self.add_constant(Constant::Str(format!("LHS.len({}) < RHS.len(", lhs_len)));
        self.emit(Opcode::LoadConst(prefix_idx));
        self.emit(Opcode::LoadLocal(rhs_total_slot));
        let to_string_id = Self::builtin_id("to_string").ok_or_else(|| CodegenError {
            message: "Unknown builtin: to_string".into(),
            span: span.clone(),
        })?;
        self.emit(Opcode::CallBuiltin {
            builtin_id: to_string_id,
            arity: 1,
            span_start: span.start as u32,
            span_end: span.end as u32,
        });
        self.emit(Opcode::ConcatStr);
        let suffix_idx = self.add_constant(Constant::Str(")".into()));
        self.emit(Opcode::LoadConst(suffix_idx));
        self.emit(Opcode::ConcatStr);

        self.emit_pattern_failure_from_message_stack("IndexOutOfBounds", span)
    }

    fn emit_pattern_mismatch_failure(&mut self, span: Span) -> Result<(), CodegenError> {
        self.emit_pattern_failure("PatternMismatch", "Pattern did not match.", span)
    }

    fn emit_pattern_failure(
        &mut self,
        kind: &str,
        message: &str,
        span: Span,
    ) -> Result<(), CodegenError> {
        if self.in_function {
            let tag_const = self.add_constant(Constant::Tag(1));
            self.emit(Opcode::LoadConst(tag_const));
            self.emit_error_value(kind, message, &span);
            self.emit(Opcode::StructNew { field_count: 1 });
            self.emit(Opcode::Return);
        } else if self.top_level_returns_result {
            let tag_const = self.add_constant(Constant::Tag(1));
            self.emit(Opcode::LoadConst(tag_const));
            self.emit_error_value(kind, message, &span);
            self.emit(Opcode::StructNew { field_count: 1 });
            self.emit(Opcode::Halt);
        } else {
            self.emit_error_value(kind, message, &span);
            let eprint_id = Self::builtin_id("eprint").ok_or_else(|| CodegenError {
                message: "Unknown builtin: eprint".into(),
                span: span.clone(),
            })?;
            self.emit(Opcode::CallBuiltin {
                builtin_id: eprint_id,
                arity: 1,
                span_start: span.start as u32,
                span_end: span.end as u32,
            });
            self.emit(Opcode::Halt);
        }
        Ok(())
    }

    fn emit_pattern_failure_from_message_stack(
        &mut self,
        kind: &str,
        span: Span,
    ) -> Result<(), CodegenError> {
        if self.in_function {
            let msg_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::StoreLocal(msg_slot));

            let tag_const = self.add_constant(Constant::Tag(1));
            self.emit(Opcode::LoadConst(tag_const));
            self.emit(Opcode::LoadLocal(msg_slot));
            self.emit_error_value_from_stack(kind, &span);
            self.emit(Opcode::StructNew { field_count: 1 });
            self.emit(Opcode::Return);
        } else if self.top_level_returns_result {
            let msg_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::StoreLocal(msg_slot));

            let tag_const = self.add_constant(Constant::Tag(1));
            self.emit(Opcode::LoadConst(tag_const));
            self.emit(Opcode::LoadLocal(msg_slot));
            self.emit_error_value_from_stack(kind, &span);
            self.emit(Opcode::StructNew { field_count: 1 });
            self.emit(Opcode::Halt);
        } else {
            self.emit_error_value_from_stack(kind, &span);
            let eprint_id = Self::builtin_id("eprint").ok_or_else(|| CodegenError {
                message: "Unknown builtin: eprint".into(),
                span: span.clone(),
            })?;
            self.emit(Opcode::CallBuiltin {
                builtin_id: eprint_id,
                arity: 1,
                span_start: span.start as u32,
                span_end: span.end as u32,
            });
            self.emit(Opcode::Halt);
        }
        Ok(())
    }

    fn emit_error_value(&mut self, kind: &str, message: &str, span: &Span) {
        if let Some((fun_idx, arity)) = self.state.error_ctor_funs.get(kind).copied() {
            match arity {
                0 => {
                    self.emit(Opcode::Call {
                        fun_idx,
                        arity: 0,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    });
                    return;
                }
                1 => {
                    let message_idx = self.add_constant(Constant::Str(message.into()));
                    self.emit(Opcode::LoadConst(message_idx));
                    self.emit(Opcode::Call {
                        fun_idx,
                        arity: 1,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    });
                    return;
                }
                _ => {
                    // fall back to literal when constructor arity is not supported here
                }
            }
        }

        let kind_idx = self.add_constant(Constant::Str(kind.into()));
        let message_idx = self.add_constant(Constant::Str(message.into()));
        self.emit(Opcode::MakeErrorLiteral {
            kind_const_idx: kind_idx,
            message_const_idx: message_idx,
        });
    }

    fn emit_error_value_from_stack(&mut self, kind: &str, span: &Span) {
        if let Some((fun_idx, arity)) = self.state.error_ctor_funs.get(kind).copied() {
            match arity {
                1 => {
                    self.emit(Opcode::Call {
                        fun_idx,
                        arity: 1,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    });
                    return;
                }
                0 => {
                    self.emit(Opcode::Pop);
                    self.emit(Opcode::Call {
                        fun_idx,
                        arity: 0,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    });
                    return;
                }
                _ => {
                    // fall through to literal fallback
                }
            }
        }

        self.emit(Opcode::Pop);
        let kind_idx = self.add_constant(Constant::Str(kind.into()));
        let message_idx = self.add_constant(Constant::Str("Pattern did not match.".into()));
        self.emit(Opcode::MakeErrorLiteral {
            kind_const_idx: kind_idx,
            message_const_idx: message_idx,
        });
    }

    fn collect_exact_list_pattern_items<'a>(
        pat: &'a TypedPattern,
    ) -> Option<Vec<&'a TypedPattern>> {
        fn walk<'a>(pat: &'a TypedPattern, out: &mut Vec<&'a TypedPattern>) -> bool {
            match pat {
                TypedPattern::As(_, inner, _) => walk(inner.as_ref(), out),
                TypedPattern::ListNil(_) => true,
                TypedPattern::ListCons(_, head, tail) => {
                    out.push(head.as_ref());
                    walk(tail.as_ref(), out)
                }
                _ => false,
            }
        }

        let mut out = Vec::new();
        if walk(pat, &mut out) {
            Some(out)
        } else {
            None
        }
    }

    fn emit_exact_list_pattern_test_from_local(
        &mut self,
        items: &[&TypedPattern],
        list_slot: u32,
        fail_shorts: &[Label],
        fail_long: Label,
        fail_mismatch: Label,
        err_span: &Span,
    ) -> Result<u32, CodegenError> {
        if fail_shorts.len() != items.len() {
            return Err(CodegenError {
                message: "internal error: fail_short label count mismatch".into(),
                span: err_span.clone(),
            });
        }

        let mut current_slot = list_slot;

        for (idx, item) in items.iter().enumerate() {
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListIsEmpty);
            self.emit_jump_if_true(fail_shorts[idx]);

            let head_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListHead);
            self.emit(Opcode::StoreLocal(head_slot));
            self.emit_pattern_test_from_local(item, head_slot, fail_mismatch, err_span)?;

            let next_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListTail);
            self.emit(Opcode::StoreLocal(next_slot));
            current_slot = next_slot;
        }

        self.emit(Opcode::LoadLocal(current_slot));
        self.emit(Opcode::ListIsEmpty);
        self.emit_jump_if_false(fail_long);
        Ok(current_slot)
    }

    fn emit_pattern_test_from_local(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
        fail_label: Label,
        err_span: &Span,
    ) -> Result<(), CodegenError> {
        self.emit_pattern_test_from_local_with_mode(pat, slot, fail_label, err_span, true)
    }

    fn emit_pattern_test_from_local_for_bind(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
        fail_label: Label,
        err_span: &Span,
    ) -> Result<(), CodegenError> {
        self.emit_pattern_test_from_local_with_mode(pat, slot, fail_label, err_span, false)
    }

    fn emit_pattern_test_from_local_with_mode(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
        fail_label: Label,
        err_span: &Span,
        propagate_result_error: bool,
    ) -> Result<(), CodegenError> {
        match pat {
            TypedPattern::Var(_, _) | TypedPattern::Wildcard(_) => {}
            TypedPattern::As(_, inner, _) => {
                self.emit_pattern_test_from_local_with_mode(
                    inner,
                    slot,
                    fail_label,
                    err_span,
                    propagate_result_error,
                )?;
            }
            TypedPattern::IntLit(_, n) => {
                self.emit(Opcode::LoadLocal(slot));
                let n_const = self.add_constant(Constant::Int(n.clone()));
                self.emit(Opcode::LoadConst(n_const));
                self.emit(Opcode::EqInt);
                self.emit_jump_if_false(fail_label);
            }
            TypedPattern::StrLit(_, s) => {
                self.emit(Opcode::LoadLocal(slot));
                let s_const = self.add_constant(Constant::Str(s.clone()));
                self.emit(Opcode::LoadConst(s_const));
                self.emit(Opcode::EqStr);
                self.emit_jump_if_false(fail_label);
            }
            TypedPattern::BoolLit(_, b) => {
                self.emit(Opcode::LoadLocal(slot));
                let b_const = self.add_constant(Constant::Bool(*b));
                self.emit(Opcode::LoadConst(b_const));
                self.emit(Opcode::EqBool);
                self.emit_jump_if_false(fail_label);
            }
            TypedPattern::ListNil(_) => {
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListIsEmpty);
                self.emit_jump_if_false(fail_label);
            }
            TypedPattern::ListCons(_, head, tail) => {
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListIsEmpty);
                self.emit_jump_if_true(fail_label);

                let head_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListHead);
                self.emit(Opcode::StoreLocal(head_slot));
                self.emit_pattern_test_from_local_with_mode(
                    head,
                    head_slot,
                    fail_label,
                    err_span,
                    propagate_result_error,
                )?;

                let tail_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListTail);
                self.emit(Opcode::StoreLocal(tail_slot));
                self.emit_pattern_test_from_local_with_mode(
                    tail,
                    tail_slot,
                    fail_label,
                    err_span,
                    propagate_result_error,
                )?;
            }
            TypedPattern::ResultOk(_, inner) => {
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::GetTag);
                let expected_tag = if propagate_result_error { 1 } else { 0 };
                let tag_const = self.add_constant(Constant::Tag(expected_tag));
                self.emit(Opcode::LoadConst(tag_const));
                self.emit(Opcode::EqTag);

                if propagate_result_error {
                    let inner_ok = self.fresh_label();
                    self.emit_jump_if_false(inner_ok);
                    self.emit_propagate_result_from_local(slot, err_span.clone())?;
                    self.patch_label(inner_ok);
                } else {
                    self.emit_jump_if_false(fail_label);
                }

                let inner_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::GetField { field_index: 0 });
                self.emit(Opcode::StoreLocal(inner_slot));
                self.emit_pattern_test_from_local_with_mode(
                    inner,
                    inner_slot,
                    fail_label,
                    err_span,
                    propagate_result_error,
                )?;
            }
        }
        Ok(())
    }

    fn emit_pattern_bind_from_local(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
    ) -> Result<(), CodegenError> {
        match pat {
            TypedPattern::Var(_, id) => {
                let bind_slot = self.alloc_slot(id.unique_id);
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::StoreLocal(bind_slot));
            }
            TypedPattern::As(_, inner, id) => {
                let bind_slot = self.alloc_slot(id.unique_id);
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::StoreLocal(bind_slot));
                self.emit_pattern_bind_from_local(inner, slot)?;
            }
            TypedPattern::Wildcard(_)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _) => {}
            TypedPattern::ListCons(_, head, tail) => {
                let head_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListHead);
                self.emit(Opcode::StoreLocal(head_slot));
                self.emit_pattern_bind_from_local(head, head_slot)?;

                let tail_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListTail);
                self.emit(Opcode::StoreLocal(tail_slot));
                self.emit_pattern_bind_from_local(tail, tail_slot)?;
            }
            TypedPattern::ResultOk(_, inner) => {
                let inner_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::GetField { field_index: 0 });
                self.emit(Opcode::StoreLocal(inner_slot));
                self.emit_pattern_bind_from_local(inner, inner_slot)?;
            }
        }
        Ok(())
    }

    fn emit_propagate_result_from_local(
        &mut self,
        result_slot: u32,
        span: Span,
    ) -> Result<(), CodegenError> {
        self.emit(Opcode::LoadLocal(result_slot));
        if self.in_function {
            self.emit(Opcode::Return);
        } else if self.top_level_returns_result {
            self.emit(Opcode::Halt);
        } else {
            self.emit(Opcode::GetField { field_index: 0 });
            let eprint_id = Self::builtin_id("eprint").ok_or_else(|| CodegenError {
                message: "Unknown builtin: eprint".into(),
                span: span.clone(),
            })?;
            self.emit(Opcode::CallBuiltin {
                builtin_id: eprint_id,
                arity: 1,
                span_start: span.start as u32,
                span_end: span.end as u32,
            });
            self.emit(Opcode::Halt);
        }
        Ok(())
    }

    fn normalize_function_table(&mut self) -> Result<(), CodegenError> {
        // Invariant: functions[idx].fun_idx == idx.
        // VM relies on O(1) array lookup by fun_idx and fails fast if this invariant is broken.
        self.state.functions.sort_by_key(|entry| entry.fun_idx);
        let mut remap = HashMap::new();
        for (idx, entry) in self.state.functions.iter().enumerate() {
            let old_idx = entry.fun_idx;
            let new_idx = idx as u32;
            if remap.insert(old_idx, new_idx).is_some() {
                return Err(CodegenError {
                    message: format!("Duplicate function index detected: {}", old_idx),
                    span: Span { start: 0, end: 0 },
                });
            }
        }

        for entry in &mut self.state.functions {
            if let Some(new_idx) = remap.get(&entry.fun_idx) {
                entry.fun_idx = *new_idx;
            }
        }

        for ir in &mut self.ir {
            if let IrOp::Op(op) = ir {
                match op {
                    Opcode::LoadFunctionRef(fun_idx) | Opcode::Call { fun_idx, .. } => {
                        if let Some(new_idx) = remap.get(fun_idx) {
                            *fun_idx = *new_idx;
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }

    // ── Function application ──

    fn emit_app(
        &mut self,
        call_span: Span,
        func: &TypedNode,
        args: &[TypedNode],
    ) -> Result<(), CodegenError> {
        match &func.ty {
            Ty::BuiltinFunc { name, .. } => {
                let builtin_id = Self::builtin_id(name).ok_or_else(|| CodegenError {
                    message: format!("Unknown builtin: {}", name),
                    span: func.span.clone(),
                })?;
                for arg in args {
                    self.emit_node(arg)?;
                }
                self.emit(Opcode::CallBuiltin {
                    builtin_id,
                    arity: args.len() as u8,
                    span_start: call_span.start as u32,
                    span_end: call_span.end as u32,
                });
            }
            Ty::UserFunc {
                fun_idx, params, ..
            } => {
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
                self.emit(Opcode::Call {
                    fun_idx: *fun_idx,
                    arity: args.len() as u8,
                    span_start: call_span.start as u32,
                    span_end: call_span.end as u32,
                });
            }
            Ty::Func(params, _) => {
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
                self.emit_node(func)?;
                for arg in args {
                    self.emit_node(arg)?;
                }
                self.emit(Opcode::CallClosure {
                    arity: args.len() as u8,
                    span_start: call_span.start as u32,
                    span_end: call_span.end as u32,
                });
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
                self.emit(Opcode::Pop); // discard then result for if_then/2
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
                    let to_string_id =
                        Self::builtin_id("to_string").ok_or_else(|| CodegenError {
                            message: "Unknown builtin: to_string".into(),
                            span: expr.span.clone(),
                        })?;
                    self.emit(Opcode::CallBuiltin {
                        builtin_id: to_string_id,
                        arity: 1,
                        span_start: expr.span.start as u32,
                        span_end: expr.span.end as u32,
                    });
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

            self.emit_match_pattern_test(pat, scrut_slot, next_arm)?;
            self.emit_match_pattern_bind(pat, scrut_slot)?;

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

    fn emit_match_pattern_test(
        &mut self,
        pat: &TypedMatchPattern,
        slot: u32,
        fail_label: Label,
    ) -> Result<(), CodegenError> {
        match pat {
            TypedMatchPattern::Binding(_) | TypedMatchPattern::Wildcard => {}
            TypedMatchPattern::As(inner, _) => {
                self.emit_match_pattern_test(inner, slot, fail_label)?;
            }
            TypedMatchPattern::BoolLit(b) => {
                self.emit(Opcode::LoadLocal(slot));
                let bool_const = self.add_constant(Constant::Bool(*b));
                self.emit(Opcode::LoadConst(bool_const));
                self.emit(Opcode::EqBool);
                self.emit_jump_if_false(fail_label);
            }
            TypedMatchPattern::IntLit(n) => {
                self.emit(Opcode::LoadLocal(slot));
                let int_const = self.add_constant(Constant::Int(n.clone()));
                self.emit(Opcode::LoadConst(int_const));
                self.emit(Opcode::EqInt);
                self.emit_jump_if_false(fail_label);
            }
            TypedMatchPattern::StrLit(s) => {
                self.emit(Opcode::LoadLocal(slot));
                let str_const = self.add_constant(Constant::Str(s.clone()));
                self.emit(Opcode::LoadConst(str_const));
                self.emit(Opcode::EqStr);
                self.emit_jump_if_false(fail_label);
            }
            TypedMatchPattern::Constructor {
                tag,
                fields,
                field_offset,
            } => {
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::GetTag);
                let tag_const = self.add_constant(Constant::Tag(*tag));
                self.emit(Opcode::LoadConst(tag_const));
                self.emit(Opcode::EqTag);
                self.emit_jump_if_false(fail_label);

                for (idx, field_pat) in fields.iter().enumerate() {
                    let inner_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(slot));
                    self.emit(Opcode::GetField {
                        field_index: *field_offset + idx as u32,
                    });
                    self.emit(Opcode::StoreLocal(inner_slot));
                    self.emit_match_pattern_test(field_pat, inner_slot, fail_label)?;
                }
            }
            TypedMatchPattern::ListNil => {
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListIsEmpty);
                self.emit_jump_if_false(fail_label);
            }
            TypedMatchPattern::ListCons(head, tail) => {
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListIsEmpty);
                self.emit_jump_if_true(fail_label);

                let head_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListHead);
                self.emit(Opcode::StoreLocal(head_slot));
                self.emit_match_pattern_test(head, head_slot, fail_label)?;

                let tail_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListTail);
                self.emit(Opcode::StoreLocal(tail_slot));
                self.emit_match_pattern_test(tail, tail_slot, fail_label)?;
            }
        }
        Ok(())
    }

    fn emit_match_pattern_bind(
        &mut self,
        pat: &TypedMatchPattern,
        slot: u32,
    ) -> Result<(), CodegenError> {
        match pat {
            TypedMatchPattern::Binding(id) => {
                let bind_slot = self.alloc_slot(id.unique_id);
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::StoreLocal(bind_slot));
            }
            TypedMatchPattern::As(inner, alias) => {
                let bind_slot = self.alloc_slot(alias.unique_id);
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::StoreLocal(bind_slot));
                self.emit_match_pattern_bind(inner, slot)?;
            }
            TypedMatchPattern::Wildcard
            | TypedMatchPattern::BoolLit(_)
            | TypedMatchPattern::IntLit(_)
            | TypedMatchPattern::StrLit(_)
            | TypedMatchPattern::ListNil => {}
            TypedMatchPattern::Constructor {
                fields,
                field_offset,
                ..
            } => {
                for (idx, field_pat) in fields.iter().enumerate() {
                    let inner_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(slot));
                    self.emit(Opcode::GetField {
                        field_index: *field_offset + idx as u32,
                    });
                    self.emit(Opcode::StoreLocal(inner_slot));
                    self.emit_match_pattern_bind(field_pat, inner_slot)?;
                }
            }
            TypedMatchPattern::ListCons(head, tail) => {
                let head_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListHead);
                self.emit(Opcode::StoreLocal(head_slot));
                self.emit_match_pattern_bind(head, head_slot)?;

                let tail_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListTail);
                self.emit(Opcode::StoreLocal(tail_slot));
                self.emit_match_pattern_bind(tail, tail_slot)?;
            }
        }
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
            Lit::Int(n) => Constant::Int(n.clone()),
            Lit::Float(f) => Constant::Float(*f),
            Lit::Str(s) => Constant::Str(s.clone()),
            Lit::Bool(b) => Constant::Bool(*b),
            Lit::Unit => Constant::Unit,
        }
    }

    fn emit_enum_eq(
        &mut self,
        op: &BinOp,
        left: &TypedNode,
        right: &TypedNode,
    ) -> Result<(), CodegenError> {
        self.emit_node(left)?;
        let left_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(left_slot));

        self.emit_node(right)?;
        let right_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(right_slot));

        self.emit(Opcode::LoadLocal(left_slot));
        self.emit(Opcode::GetField { field_index: 0 });
        self.emit(Opcode::LoadLocal(right_slot));
        self.emit(Opcode::GetField { field_index: 0 });
        self.emit(Opcode::EqInt);
        if matches!(op, BinOp::Neq) {
            self.emit(Opcode::NotBool);
        }
        Ok(())
    }

    fn binop_to_opcode(&self, op: &BinOp, left_ty: &Ty) -> Result<Opcode, CodegenError> {
        let dummy_span = Span { start: 0, end: 0 };
        match (op, left_ty) {
            (BinOp::Add, Ty::Int) => Ok(Opcode::AddInt),
            (BinOp::Sub, Ty::Int) => Ok(Opcode::SubInt),
            (BinOp::Mul, Ty::Int) => Ok(Opcode::MulInt),
            (BinOp::Add, Ty::Float) => Ok(Opcode::AddFloat),
            (BinOp::Sub, Ty::Float) => Ok(Opcode::SubFloat),
            (BinOp::Mul, Ty::Float) => Ok(Opcode::MulFloat),
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
