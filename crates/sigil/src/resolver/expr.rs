use super::captures::collect_captures;
use super::declarations::{ast_ty_key, trait_instance_key};
use super::scope_init::{
    initialize_scope, is_doc_only_builtin_decl, is_runtime_builtin_decl,
    is_special_form_builtin_decl, resolve_decl_attrs,
};
use super::special_forms::{IfKind, LogicKind};
use super::*;
use sindr::names::{surface_path_name, TypeIdentity};
use spire::ast::{
    AstPath, BinOp, BulkUpdateEntry, BulkUpdateEntryKind, BulkUpdatePath, DbgArg, FacetPathSegment,
    HashMapLiteralEntry, InterpolatedPart,
};

const TUPLE_TYPE_ROOT_UID: u32 = u32::MAX - 7;
const LIST_TYPE_ROOT_UID: u32 = u32::MAX - 8;
const HASH_MAP_TYPE_ROOT_UID: u32 = u32::MAX - 9;
const STRING_PRIMITIVE_ROOT_UID: u32 = u32::MAX - 10;
const INT_PRIMITIVE_ROOT_UID: u32 = u32::MAX - 11;
const FLOAT_PRIMITIVE_ROOT_UID: u32 = u32::MAX - 12;
const BOOLEAN_PRIMITIVE_ROOT_UID: u32 = u32::MAX - 13;
const FUNCTION_PRIMITIVE_ROOT_UID: u32 = u32::MAX - 14;

fn ast_ty_owner_head(ty: &AstTy) -> Option<&str> {
    match ty {
        AstTy::Named(_, name) | AstTy::ImplTrait(_, name) | AstTy::Generic(_, name, _) => {
            Some(name)
        }
        AstTy::Tuple(..) | AstTy::Func(..) => None,
    }
}

fn synthetic_builtin_symbol_uid(name: &str, info: &SymbolIdentityInfo) -> Option<u32> {
    let name = global_surface_name(name);
    match (name, info.capabilities.facet_root_path) {
        ("Tuple", Some(FacetRootKind::Tuple)) => Some(TUPLE_TYPE_ROOT_UID),
        ("List", Some(FacetRootKind::List)) => Some(LIST_TYPE_ROOT_UID),
        ("HashMap", Some(FacetRootKind::HashMap)) => Some(HASH_MAP_TYPE_ROOT_UID),
        ("String", None) if info.capabilities.module_owner => Some(STRING_PRIMITIVE_ROOT_UID),
        ("Int", None) if info.capabilities.module_owner => Some(INT_PRIMITIVE_ROOT_UID),
        ("Float", None) if info.capabilities.module_owner => Some(FLOAT_PRIMITIVE_ROOT_UID),
        ("Boolean", Some(FacetRootKind::TypeRoot)) if info.capabilities.module_owner => {
            Some(BOOLEAN_PRIMITIVE_ROOT_UID)
        }
        ("Function", None) if info.capabilities.module_owner => Some(FUNCTION_PRIMITIVE_ROOT_UID),
        _ => None,
    }
}

fn synthetic_facet_root_uid(name: &str) -> Option<u32> {
    let info = builtin_symbol_identity_info(name)?;
    info.capabilities.facet_root_path?;
    synthetic_builtin_symbol_uid(name, &info)
}

fn synthetic_member_root(name: &str) -> Option<(u32, SymbolIdentityInfo)> {
    let info = builtin_symbol_identity_info(name)?;
    if !info.capabilities.module_owner {
        return None;
    }
    synthetic_builtin_symbol_uid(name, &info).map(|uid| (uid, info))
}

fn is_synthetic_builtin_symbol_uid(uid: u32) -> bool {
    matches!(
        uid,
        TUPLE_TYPE_ROOT_UID
            | LIST_TYPE_ROOT_UID
            | HASH_MAP_TYPE_ROOT_UID
            | STRING_PRIMITIVE_ROOT_UID
            | INT_PRIMITIVE_ROOT_UID
            | FLOAT_PRIMITIVE_ROOT_UID
            | BOOLEAN_PRIMITIVE_ROOT_UID
            | FUNCTION_PRIMITIVE_ROOT_UID
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CanonicalSpecialForm {
    If(IfKind),
    IfLet,
    IfLetThen,
    IsMatch,
    Assert,
    Ensure,
    MapErr,
    Cause,
    RecoverKind,
    Logic(LogicKind),
}

impl Resolver {
    fn canonical_special_form_from_qname(qualified_name: &str) -> Option<CanonicalSpecialForm> {
        match global_surface_name(qualified_name) {
            "Kernel::if" => Some(CanonicalSpecialForm::If(IfKind::If3)),
            "Kernel::if_then" => Some(CanonicalSpecialForm::If(IfKind::IfThen2)),
            "Kernel::if_let" => Some(CanonicalSpecialForm::IfLet),
            "Kernel::if_let_then" => Some(CanonicalSpecialForm::IfLetThen),
            "Kernel::is_match" => Some(CanonicalSpecialForm::IsMatch),
            "Kernel::assert" => Some(CanonicalSpecialForm::Assert),
            "Kernel::ensure" => Some(CanonicalSpecialForm::Ensure),
            "Kernel::and" => Some(CanonicalSpecialForm::Logic(LogicKind::And)),
            "Kernel::or" => Some(CanonicalSpecialForm::Logic(LogicKind::Or)),
            "Result::map_err" => Some(CanonicalSpecialForm::MapErr),
            "Result::cause" => Some(CanonicalSpecialForm::Cause),
            "Result::recover_kind" => Some(CanonicalSpecialForm::RecoverKind),
            _ => None,
        }
    }

    fn classify_canonical_special_form_callee(
        &self,
        resolved_func: &Resolved,
    ) -> Option<CanonicalSpecialForm> {
        let Resolved::Var(_, id) = resolved_func else {
            return None;
        };
        if let Some(qualified_name) = id.qualified_name.as_deref() {
            if let Some(kind) = Self::canonical_special_form_from_qname(qualified_name) {
                return Some(kind);
            }
        }
        let entry = self.declaration_entry_for_uid(id.unique_id)?;
        if let Some(kind) = Self::canonical_special_form_from_qname(&entry.fq_name) {
            return Some(kind);
        }
        if entry.auto_import && is_special_form_builtin_decl(entry.name.as_str()) {
            return Self::fallback_special_form_from_surface(&Ast::Var(
                id.span.clone(),
                entry.name.clone(),
            ));
        }
        match (
            global_surface_name(entry.module_path.as_str()),
            entry.name.as_str(),
        ) {
            ("Kernel", "if") => Some(CanonicalSpecialForm::If(IfKind::If3)),
            ("Kernel", "if_then") => Some(CanonicalSpecialForm::If(IfKind::IfThen2)),
            ("Kernel", "if_let") => Some(CanonicalSpecialForm::IfLet),
            ("Kernel", "if_let_then") => Some(CanonicalSpecialForm::IfLetThen),
            ("Kernel", "is_match") => Some(CanonicalSpecialForm::IsMatch),
            ("Kernel", "assert") => Some(CanonicalSpecialForm::Assert),
            ("Kernel", "ensure") => Some(CanonicalSpecialForm::Ensure),
            ("Kernel", "and") => Some(CanonicalSpecialForm::Logic(LogicKind::And)),
            ("Kernel", "or") => Some(CanonicalSpecialForm::Logic(LogicKind::Or)),
            ("Result", "map_err") => Some(CanonicalSpecialForm::MapErr),
            ("Result", "cause") => Some(CanonicalSpecialForm::Cause),
            ("Result", "recover_kind") => Some(CanonicalSpecialForm::RecoverKind),
            _ => None,
        }
    }

    fn fallback_special_form_from_surface(func: &Ast) -> Option<CanonicalSpecialForm> {
        match func {
            Ast::Var(_, name) | Ast::InternalVar(_, name) => match name.as_str() {
                "if" => Some(CanonicalSpecialForm::If(IfKind::If3)),
                "if_then" => Some(CanonicalSpecialForm::If(IfKind::IfThen2)),
                "if_let" => Some(CanonicalSpecialForm::IfLet),
                "if_let_then" => Some(CanonicalSpecialForm::IfLetThen),
                "is_match" => Some(CanonicalSpecialForm::IsMatch),
                "assert" => Some(CanonicalSpecialForm::Assert),
                "ensure" => Some(CanonicalSpecialForm::Ensure),
                "map_err" => Some(CanonicalSpecialForm::MapErr),
                "cause" => Some(CanonicalSpecialForm::Cause),
                "recover_kind" => Some(CanonicalSpecialForm::RecoverKind),
                "and" => Some(CanonicalSpecialForm::Logic(LogicKind::And)),
                "or" => Some(CanonicalSpecialForm::Logic(LogicKind::Or)),
                _ => None,
            },
            Ast::Path(_, path)
                if path.segments.len() == 2
                    && path.segments[0] == "Result"
                    && path.segments[1] == "map_err" =>
            {
                Some(CanonicalSpecialForm::MapErr)
            }
            Ast::Path(_, path)
                if path.segments.len() == 2
                    && path.segments[0] == "Result"
                    && path.segments[1] == "cause" =>
            {
                Some(CanonicalSpecialForm::Cause)
            }
            Ast::Path(_, path)
                if path.segments.len() == 2
                    && path.segments[0] == "Result"
                    && path.segments[1] == "recover_kind" =>
            {
                Some(CanonicalSpecialForm::RecoverKind)
            }
            _ => None,
        }
    }

    fn fallback_partial_pipeline_special_form_from_surface(
        func: &Ast,
    ) -> Option<CanonicalSpecialForm> {
        match func {
            Ast::Var(_, name) | Ast::InternalVar(_, name) => match name.as_str() {
                "if" => Some(CanonicalSpecialForm::If(IfKind::If3)),
                "if_then" => Some(CanonicalSpecialForm::If(IfKind::IfThen2)),
                "if_let" => Some(CanonicalSpecialForm::IfLet),
                "if_let_then" => Some(CanonicalSpecialForm::IfLetThen),
                "is_match" => Some(CanonicalSpecialForm::IsMatch),
                "assert" => Some(CanonicalSpecialForm::Assert),
                "ensure" => Some(CanonicalSpecialForm::Ensure),
                "map_err" => Some(CanonicalSpecialForm::MapErr),
                "cause" => Some(CanonicalSpecialForm::Cause),
                "and" => Some(CanonicalSpecialForm::Logic(LogicKind::And)),
                "or" => Some(CanonicalSpecialForm::Logic(LogicKind::Or)),
                _ => None,
            },
            _ => None,
        }
    }

    fn canonical_special_form_arity(kind: CanonicalSpecialForm) -> usize {
        match kind {
            CanonicalSpecialForm::If(IfKind::If3) => 3,
            CanonicalSpecialForm::If(IfKind::IfThen2) => 2,
            CanonicalSpecialForm::IfLet => 4,
            CanonicalSpecialForm::IfLetThen => 3,
            CanonicalSpecialForm::IsMatch => 2,
            CanonicalSpecialForm::Assert => 2,
            CanonicalSpecialForm::Ensure => 3,
            CanonicalSpecialForm::MapErr => 2,
            CanonicalSpecialForm::Cause => 2,
            CanonicalSpecialForm::RecoverKind => 3,
            CanonicalSpecialForm::Logic(LogicKind::And) => 2,
            CanonicalSpecialForm::Logic(LogicKind::Or) => 2,
        }
    }

    fn partial_pipeline_special_form_arity(kind: CanonicalSpecialForm) -> Option<usize> {
        match kind {
            CanonicalSpecialForm::If(IfKind::If3)
            | CanonicalSpecialForm::If(IfKind::IfThen2)
            | CanonicalSpecialForm::IfLet
            | CanonicalSpecialForm::IfLetThen
            | CanonicalSpecialForm::IsMatch
            | CanonicalSpecialForm::Assert
            | CanonicalSpecialForm::Ensure
            | CanonicalSpecialForm::MapErr
            | CanonicalSpecialForm::Cause
            | CanonicalSpecialForm::Logic(_) => Some(Self::canonical_special_form_arity(kind)),
            CanonicalSpecialForm::RecoverKind => None,
        }
    }

    fn resolve_canonical_special_form_call(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
        kind: CanonicalSpecialForm,
    ) -> Result<Resolved, ResolveError> {
        match kind {
            CanonicalSpecialForm::If(if_kind) => self.resolve_if(span, args, if_kind),
            CanonicalSpecialForm::IfLet => self.resolve_if_let(span, args),
            CanonicalSpecialForm::IfLetThen => self.resolve_if_let_then(span, args),
            CanonicalSpecialForm::IsMatch => self.resolve_is_match(span, args),
            CanonicalSpecialForm::Assert => self.resolve_assert(span, args),
            CanonicalSpecialForm::Ensure => self.resolve_ensure(span, args),
            CanonicalSpecialForm::MapErr => self.resolve_map_err(span, args),
            CanonicalSpecialForm::Cause => self.resolve_cause(span, args),
            CanonicalSpecialForm::RecoverKind => self.resolve_recover_kind(span, args),
            CanonicalSpecialForm::Logic(logic_kind) => {
                self.resolve_logic_call(span, args, logic_kind)
            }
        }
    }

    fn desugar_pipeline_rhs_special_form_partial(&mut self, rhs: Ast) -> Result<Ast, ResolveError> {
        let Ast::App(span, func, args) = rhs else {
            return Ok(rhs);
        };

        if matches!(func.as_ref(), Ast::FuncLiteralRef(_, pair) if pair.body == "(,)") {
            let args: [RecordLitArg; 1] = args.try_into().map_err(|_| ResolveError {
                message:
                    "quoted pair constructor pipeline call expects exactly one positional argument"
                        .into(),
                span: span.clone(),
                related_labels: Vec::new(),
            })?;
            let [RecordLitArg::Positional(right)] = args else {
                return Err(ResolveError {
                    message: "quoted pair constructor pipeline call expects exactly one positional argument"
                        .into(),
                    span: span.clone(),
                    related_labels: Vec::new(),
                });
            };
            let param_name = format!("__pipe_injected_{}_{}", span.start, span.end);
            let param_span = span.clone();
            return Ok(Ast::Closure(
                span.clone(),
                vec![ClosureParam {
                    name: param_name.clone(),
                    ty: None,
                    span: param_span.clone(),
                }],
                Box::new(Ast::TupleLiteral(
                    span,
                    vec![Ast::Var(param_span, param_name), right],
                )),
            ));
        }

        if !matches!(func.as_ref(), Ast::Var(_, _) | Ast::InternalVar(_, _)) {
            return Ok(Ast::App(span, func, args));
        }

        let kind = match self.resolve_node(*func.clone()) {
            Ok(resolved_func) => self.classify_canonical_special_form_callee(&resolved_func),
            Err(_) => Self::fallback_partial_pipeline_special_form_from_surface(func.as_ref()),
        };
        let Some(kind) = kind else {
            return Ok(Ast::App(span, func, args));
        };
        let Some(expected_arity) = Self::partial_pipeline_special_form_arity(kind) else {
            return Ok(Ast::App(span, func, args));
        };
        if args.len() + 1 != expected_arity {
            return Ok(Ast::App(span, func, args));
        }

        let param_name = format!("__pipe_injected_{}_{}", span.start, span.end);
        let param_span = span.clone();
        let mut injected_args = Vec::with_capacity(args.len() + 1);
        injected_args.push(RecordLitArg::Positional(Ast::Var(
            param_span.clone(),
            param_name.clone(),
        )));
        injected_args.extend(args);

        let call = Ast::App(span.clone(), func, injected_args);
        Ok(Ast::Closure(
            span.clone(),
            vec![ClosureParam {
                name: param_name,
                ty: None,
                span: param_span,
            }],
            Box::new(call),
        ))
    }

    fn capture_placeholder_param_name(span: &Span, index: usize) -> String {
        format!("__cap_{}_{}_{}", span.start, span.end, index)
    }

    fn pipe_slot_param_name(span: &Span) -> String {
        format!("__pipe_slot_{}_{}", span.start, span.end)
    }

    fn make_closure_from_call(
        &self,
        span: &Span,
        params: Vec<ClosureParam>,
        func: Ast,
        args: Vec<Ast>,
    ) -> Ast {
        Ast::Closure(
            span.clone(),
            params,
            Box::new(Ast::App(
                span.clone(),
                Box::new(func),
                args.into_iter().map(RecordLitArg::Positional).collect(),
            )),
        )
    }

    fn make_operator_capture_body(
        &self,
        span: &Span,
        body: &str,
        left: Ast,
        right: Ast,
    ) -> Result<Ast, ResolveError> {
        if body == "(,)" {
            return Ok(Ast::TupleLiteral(span.clone(), vec![left, right]));
        }
        let op = match body {
            "+" => BinOp::Add,
            "-" => BinOp::Sub,
            "*" => BinOp::Mul,
            "++" => BinOp::Concat,
            "==" => BinOp::Eq,
            "!=" => BinOp::Neq,
            "<" => BinOp::Lt,
            ">" => BinOp::Gt,
            "<=" => BinOp::Lte,
            ">=" => BinOp::Gte,
            _ => {
                return Err(ResolveError {
                    message: format!("unsupported operator capture target `{}`", body),
                    span: span.clone(),
                    related_labels: Vec::new(),
                });
            }
        };
        Ok(Ast::BinOp(
            span.clone(),
            op,
            Box::new(left),
            Box::new(right),
        ))
    }

    fn validate_capture_placeholders(
        &self,
        span: &Span,
        args: &[Ast],
    ) -> Result<usize, ResolveError> {
        let mut used = HashSet::new();
        for arg in args {
            self.collect_capture_placeholders(arg, true, true, &mut used)?;
        }
        if used.is_empty() {
            return Err(ResolveError {
                message: "capture call is missing placeholder arguments".into(),
                span: span.clone(),
                related_labels: Vec::new(),
            });
        }

        let Some(max_index) = used.iter().max().copied() else {
            return Err(ResolveError {
                message: "capture call is missing placeholder arguments".into(),
                span: span.clone(),
                related_labels: Vec::new(),
            });
        };
        for index in 1..=max_index {
            if !used.contains(&index) {
                return Err(ResolveError {
                    message: format!("capture placeholder &{} is missing", index),
                    span: span.clone(),
                    related_labels: Vec::new(),
                });
            }
        }
        Ok(max_index)
    }

    fn collect_capture_placeholders(
        &self,
        expr: &Ast,
        allow_placeholders: bool,
        inside_placeholder_capture: bool,
        used: &mut HashSet<usize>,
    ) -> Result<(), ResolveError> {
        fn walk_bulk_entries(
            resolver: &Resolver,
            entries: &[BulkUpdateEntry],
            allow_placeholders: bool,
            inside_placeholder_capture: bool,
            used: &mut HashSet<usize>,
        ) -> Result<(), ResolveError> {
            for entry in entries {
                match &entry.kind {
                    BulkUpdateEntryKind::Set(expr)
                    | BulkUpdateEntryKind::Over(expr)
                    | BulkUpdateEntryKind::OverResult(expr)
                    | BulkUpdateEntryKind::CaseSet(expr)
                    | BulkUpdateEntryKind::CaseOver(expr) => {
                        resolver.collect_capture_placeholders(
                            expr,
                            allow_placeholders,
                            inside_placeholder_capture,
                            used,
                        )?;
                    }
                    BulkUpdateEntryKind::Nested(entries) => walk_bulk_entries(
                        resolver,
                        entries,
                        allow_placeholders,
                        inside_placeholder_capture,
                        used,
                    )?,
                }
            }
            Ok(())
        }

        match expr {
            Ast::CapturePlaceholder(span, index) => {
                if !allow_placeholders {
                    return Err(ResolveError {
                        message: "capture placeholders are only valid in the outer capture body"
                            .into(),
                        span: span.clone(),
                        related_labels: Vec::new(),
                    });
                }
                used.insert(*index);
                Ok(())
            }
            Ast::App(_, func, args) => {
                self.collect_capture_placeholders(
                    func,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )?;
                for arg in args {
                    match arg {
                        RecordLitArg::Positional(expr) | RecordLitArg::Named(_, expr) => {
                            self.collect_capture_placeholders(
                                expr,
                                allow_placeholders,
                                inside_placeholder_capture,
                                used,
                            )?;
                        }
                    }
                }
                Ok(())
            }
            Ast::TypeApply(_, target, _) => self.collect_capture_placeholders(
                target,
                allow_placeholders,
                inside_placeholder_capture,
                used,
            ),
            Ast::Block(_, stmts) | Ast::ListLiteral(_, stmts) | Ast::TupleLiteral(_, stmts) => {
                for stmt in stmts {
                    self.collect_capture_placeholders(
                        stmt,
                        allow_placeholders,
                        inside_placeholder_capture,
                        used,
                    )?;
                }
                Ok(())
            }
            Ast::HashMapLiteral(_, entries) => {
                for entry in entries {
                    self.collect_capture_placeholders(
                        &entry.key,
                        allow_placeholders,
                        inside_placeholder_capture,
                        used,
                    )?;
                    self.collect_capture_placeholders(
                        &entry.value,
                        allow_placeholders,
                        inside_placeholder_capture,
                        used,
                    )?;
                }
                Ok(())
            }
            Ast::RangeLiteral(_, start, stop) => {
                self.collect_capture_placeholders(
                    start,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )?;
                self.collect_capture_placeholders(
                    stop,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )
            }
            Ast::Bind(_, _, rhs)
            | Ast::SafeBind(_, _, rhs)
            | Ast::Grouped(_, rhs)
            | Ast::Semi(_, rhs)
            | Ast::FieldAccess(_, rhs, _)
            | Ast::FacetSegmentAccess(_, rhs, _)
            | Ast::FacetCapture(_, rhs) => self.collect_capture_placeholders(
                rhs,
                allow_placeholders,
                inside_placeholder_capture,
                used,
            ),
            Ast::BinOp(_, _, left, right)
            | Ast::Pipe(_, left, right)
            | Ast::ContextMap(_, left, right)
            | Ast::ContextApply(_, left, right)
            | Ast::ContextBind(_, left, right)
            | Ast::Compose(_, left, right)
            | Ast::LiftedCompose(_, left, right)
            | Ast::KleisliCompose(_, left, right)
            | Ast::ListCons(_, left, right) => {
                self.collect_capture_placeholders(
                    left,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )?;
                self.collect_capture_placeholders(
                    right,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )
            }
            Ast::InterpolatedStr(_, parts) => {
                for part in parts {
                    if let InterpolatedPart::Expr(expr) = part {
                        self.collect_capture_placeholders(
                            expr,
                            allow_placeholders,
                            inside_placeholder_capture,
                            used,
                        )?;
                    }
                }
                Ok(())
            }
            Ast::Dbg(_, args) => {
                for arg in args {
                    self.collect_capture_placeholders(
                        &arg.expr,
                        allow_placeholders,
                        inside_placeholder_capture,
                        used,
                    )?;
                }
                Ok(())
            }
            Ast::Match(_, scrutinee, arms) => {
                self.collect_capture_placeholders(
                    scrutinee,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )?;
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.collect_capture_placeholders(
                            guard,
                            allow_placeholders,
                            inside_placeholder_capture,
                            used,
                        )?;
                    }
                    self.collect_capture_placeholders(
                        &arm.body,
                        allow_placeholders,
                        inside_placeholder_capture,
                        used,
                    )?;
                }
                Ok(())
            }
            Ast::BulkUpdate(_, source, entries) => {
                self.collect_capture_placeholders(
                    source,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )?;
                walk_bulk_entries(
                    self,
                    entries,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )
            }
            Ast::StructLit(_, _, fields) | Ast::InternalStructLit(_, _, fields) => {
                for field in fields {
                    match field {
                        StructLitField::Explicit(_, expr) => {
                            self.collect_capture_placeholders(
                                expr,
                                allow_placeholders,
                                inside_placeholder_capture,
                                used,
                            )?;
                        }
                        StructLitField::Shorthand(_) => {}
                    }
                }
                Ok(())
            }
            Ast::ConstructorCall(_, _, args) => {
                for arg in args {
                    match arg {
                        RecordLitArg::Positional(expr) | RecordLitArg::Named(_, expr) => {
                            self.collect_capture_placeholders(
                                expr,
                                allow_placeholders,
                                inside_placeholder_capture,
                                used,
                            )?;
                        }
                    }
                }
                Ok(())
            }
            Ast::Closure(_, _, body) => self.collect_capture_placeholders(body, false, true, used),
            Ast::Capture(span, target, args) => {
                if inside_placeholder_capture && !args.is_empty() {
                    return Err(ResolveError {
                        message: "outer capture placeholders are only valid in the outer capture body; nested capture argument blocks are not allowed".into(),
                        span: span.clone(),
                        related_labels: Vec::new(),
                    });
                }
                self.collect_capture_placeholders(
                    target,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )?;
                for arg in args {
                    self.collect_capture_placeholders(
                        arg,
                        allow_placeholders,
                        inside_placeholder_capture,
                        used,
                    )?;
                }
                Ok(())
            }
            Ast::Lit(_, _)
            | Ast::Var(_, _)
            | Ast::InternalVar(_, _)
            | Ast::Path(_, _)
            | Ast::FuncLiteralRef(_, _)
            | Ast::ListNil(_)
            | Ast::StructDef(..)
            | Ast::RecordDef(..)
            | Ast::DeferrorDef(_, _, _, _, _)
            | Ast::EnumDef(_, _, _, _, _)
            | Ast::Def(..)
            | Ast::ConstDef(_, _, _, _, _)
            | Ast::SupervisorInit(_, _)
            | Ast::ExtractorDef(_, _, _, _, _, _, _)
            | Ast::BuiltinDecl(..)
            | Ast::IntrinsicDecl(_, _, _, _)
            | Ast::BuiltinExtractorDecl(_, _, _, _, _)
            | Ast::BuiltinTypeDecl(_, _, _)
            | Ast::TypeAlias(_, _, _, _)
            | Ast::ResultCtorDecl(_, _, _, _, _)
            | Ast::Defmod(_, _, _, _)
            | Ast::Defagent(_, _, _, _, _)
            | Ast::Defgenserver(_, _, _, _, _)
            | Ast::Defsupervisor(_, _, _, _, _)
            | Ast::DefdynamicSupervisor(_, _, _, _, _)
            | Ast::Namespace(_, _, _)
            | Ast::ImplDef(_, _, _, _)
            | Ast::TraitDef(..)
            | Ast::TraitImplDef(..)
            | Ast::Import(_, _, _)
            | Ast::Include(_, _) => Ok(()),
        }
    }

    fn rewrite_capture_placeholders(
        &self,
        expr: Ast,
        capture_span: &Span,
        allow_placeholders: bool,
        inside_placeholder_capture: bool,
    ) -> Result<Ast, ResolveError> {
        match expr {
            Ast::CapturePlaceholder(span, index) => {
                if !allow_placeholders {
                    return Err(ResolveError {
                        message: "capture placeholders are only valid in the outer capture body"
                            .into(),
                        span,
                        related_labels: Vec::new(),
                    });
                }
                Ok(Ast::Var(
                    span.clone(),
                    Self::capture_placeholder_param_name(capture_span, index),
                ))
            }
            Ast::App(span, func, args) => Ok(Ast::App(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *func,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                args.into_iter()
                    .map(|arg| match arg {
                        RecordLitArg::Positional(expr) => Ok(RecordLitArg::Positional(
                            self.rewrite_capture_placeholders(
                                expr,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        )),
                        RecordLitArg::Named(name, expr) => Ok(RecordLitArg::Named(
                            name,
                            self.rewrite_capture_placeholders(
                                expr,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Ast::Block(span, stmts) => Ok(Ast::Block(
                span,
                stmts
                    .into_iter()
                    .map(|stmt| {
                        self.rewrite_capture_placeholders(
                            stmt,
                            capture_span,
                            allow_placeholders,
                            inside_placeholder_capture,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Ast::Bind(span, pat, rhs) => Ok(Ast::Bind(
                span,
                pat,
                Box::new(self.rewrite_capture_placeholders(
                    *rhs,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::SafeBind(span, pat, rhs) => Ok(Ast::SafeBind(
                span,
                pat,
                Box::new(self.rewrite_capture_placeholders(
                    *rhs,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::BinOp(span, op, left, right) => Ok(Ast::BinOp(
                span,
                op,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::Pipe(span, left, right) => Ok(Ast::Pipe(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::ContextMap(span, left, right) => Ok(Ast::ContextMap(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::ContextBind(span, left, right) => Ok(Ast::ContextBind(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::Compose(span, left, right) => Ok(Ast::Compose(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::LiftedCompose(span, left, right) => Ok(Ast::LiftedCompose(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::KleisliCompose(span, left, right) => Ok(Ast::KleisliCompose(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::ListCons(span, left, right) => Ok(Ast::ListCons(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::ListLiteral(span, elems) => Ok(Ast::ListLiteral(
                span,
                elems
                    .into_iter()
                    .map(|elem| {
                        self.rewrite_capture_placeholders(
                            elem,
                            capture_span,
                            allow_placeholders,
                            inside_placeholder_capture,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Ast::HashMapLiteral(span, entries) => Ok(Ast::HashMapLiteral(
                span,
                entries
                    .into_iter()
                    .map(|entry| {
                        Ok(HashMapLiteralEntry {
                            key: self.rewrite_capture_placeholders(
                                entry.key,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                            value: self.rewrite_capture_placeholders(
                                entry.value,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        })
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            Ast::RangeLiteral(span, start, stop) => Ok(Ast::RangeLiteral(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *start,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *stop,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::TupleLiteral(span, elems) => Ok(Ast::TupleLiteral(
                span,
                elems
                    .into_iter()
                    .map(|elem| {
                        self.rewrite_capture_placeholders(
                            elem,
                            capture_span,
                            allow_placeholders,
                            inside_placeholder_capture,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Ast::Grouped(span, inner) => Ok(Ast::Grouped(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *inner,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::InterpolatedStr(span, parts) => Ok(Ast::InterpolatedStr(
                span,
                parts
                    .into_iter()
                    .map(|part| match part {
                        InterpolatedPart::Text(text) => Ok(InterpolatedPart::Text(text)),
                        InterpolatedPart::Expr(expr) => Ok(InterpolatedPart::Expr(Box::new(
                            self.rewrite_capture_placeholders(
                                *expr,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        ))),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Ast::Dbg(span, args) => Ok(Ast::Dbg(
                span,
                args.into_iter()
                    .map(|arg| {
                        let expr = self.rewrite_capture_placeholders(
                            arg.expr,
                            capture_span,
                            allow_placeholders,
                            inside_placeholder_capture,
                        )?;
                        Ok(DbgArg {
                            span: expr.span().clone(),
                            expr,
                        })
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            Ast::Match(span, scrutinee, arms) => Ok(Ast::Match(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *scrutinee,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                arms.into_iter()
                    .map(|arm| {
                        Ok(AstMatchArm {
                            pattern: arm.pattern,
                            guard: arm
                                .guard
                                .map(|guard| {
                                    self.rewrite_capture_placeholders(
                                        guard,
                                        capture_span,
                                        allow_placeholders,
                                        inside_placeholder_capture,
                                    )
                                })
                                .transpose()?,
                            body: self.rewrite_capture_placeholders(
                                arm.body,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        })
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            Ast::FieldAccess(span, expr, field) => Ok(Ast::FieldAccess(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *expr,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                field,
            )),
            Ast::StructLit(span, name, fields) => Ok(Ast::StructLit(
                span,
                name,
                fields
                    .into_iter()
                    .map(|field| match field {
                        StructLitField::Explicit(name, expr) => Ok(StructLitField::Explicit(
                            name,
                            self.rewrite_capture_placeholders(
                                expr,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        )),
                        StructLitField::Shorthand(name) => Ok(StructLitField::Shorthand(name)),
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            Ast::InternalStructLit(span, name, fields) => Ok(Ast::InternalStructLit(
                span,
                name,
                fields
                    .into_iter()
                    .map(|field| match field {
                        StructLitField::Explicit(name, expr) => Ok(StructLitField::Explicit(
                            name,
                            self.rewrite_capture_placeholders(
                                expr,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        )),
                        StructLitField::Shorthand(name) => Ok(StructLitField::Shorthand(name)),
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            Ast::ConstructorCall(span, name, args) => Ok(Ast::ConstructorCall(
                span,
                name,
                args.into_iter()
                    .map(|arg| match arg {
                        RecordLitArg::Positional(expr) => Ok(RecordLitArg::Positional(
                            self.rewrite_capture_placeholders(
                                expr,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        )),
                        RecordLitArg::Named(name, expr) => Ok(RecordLitArg::Named(
                            name,
                            self.rewrite_capture_placeholders(
                                expr,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Ast::Closure(span, params, body) => Ok(Ast::Closure(
                span,
                params,
                Box::new(self.rewrite_capture_placeholders(*body, capture_span, false, true)?),
            )),
            Ast::Capture(span, target, args) => {
                if inside_placeholder_capture && !args.is_empty() {
                    return Err(ResolveError {
                        message: "outer capture placeholders are only valid in the outer capture body; nested capture argument blocks are not allowed".into(),
                        span,
                        related_labels: Vec::new(),
                    });
                }
                Ok(Ast::Capture(
                    span,
                    Box::new(self.rewrite_capture_placeholders(
                        *target,
                        capture_span,
                        allow_placeholders,
                        inside_placeholder_capture,
                    )?),
                    args.into_iter()
                        .map(|arg| {
                            self.rewrite_capture_placeholders(
                                arg,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                ))
            }
            other => Ok(other),
        }
    }

    fn lower_capture_expr(
        &self,
        span: Span,
        target: Ast,
        args: Vec<Ast>,
    ) -> Result<Ast, ResolveError> {
        if let Ast::FuncLiteralRef(_, func) = target {
            if args.is_empty() {
                let left_name = Self::capture_placeholder_param_name(&span, 1);
                let right_name = Self::capture_placeholder_param_name(&span, 2);
                let body = self.make_operator_capture_body(
                    &span,
                    &func.body,
                    Ast::Var(span.clone(), left_name.clone()),
                    Ast::Var(span.clone(), right_name.clone()),
                )?;
                return Ok(Ast::Closure(
                    span.clone(),
                    vec![
                        ClosureParam {
                            name: left_name,
                            ty: None,
                            span: span.clone(),
                        },
                        ClosureParam {
                            name: right_name,
                            ty: None,
                            span: span.clone(),
                        },
                    ],
                    Box::new(body),
                ));
            }

            if args.len() != 2 {
                return Err(ResolveError {
                    message: format!(
                        "operator capture `{}` expects exactly 2 argument expressions",
                        func.body
                    ),
                    span,
                    related_labels: Vec::new(),
                });
            }

            let max_index = self.validate_capture_placeholders(&span, &args)?;
            let rewritten_args = args
                .into_iter()
                .map(|arg| self.rewrite_capture_placeholders(arg, &span, true, true))
                .collect::<Result<Vec<_>, _>>()?;
            let [left, right]: [Ast; 2] = rewritten_args.try_into().map_err(|_| ResolveError {
                message: format!(
                    "operator capture `{}` expects exactly 2 argument expressions",
                    func.body
                ),
                span: span.clone(),
                related_labels: Vec::new(),
            })?;
            let body = self.make_operator_capture_body(&span, &func.body, left, right)?;
            let params = (1..=max_index)
                .map(|index| ClosureParam {
                    name: Self::capture_placeholder_param_name(&span, index),
                    ty: None,
                    span: span.clone(),
                })
                .collect();
            return Ok(Ast::Closure(span, params, Box::new(body)));
        }

        if args.is_empty() {
            return Ok(Ast::Capture(span, Box::new(target), args));
        }

        let max_index = self.validate_capture_placeholders(&span, &args)?;

        let rewritten_args = args
            .into_iter()
            .map(|arg| self.rewrite_capture_placeholders(arg, &span, true, true))
            .collect::<Result<Vec<_>, _>>()?;
        let params = (1..=max_index)
            .map(|index| ClosureParam {
                name: Self::capture_placeholder_param_name(&span, index),
                ty: None,
                span: span.clone(),
            })
            .collect();
        Ok(self.make_closure_from_call(&span, params, target, rewritten_args))
    }

    fn inferred_facet_capture_segments(expr: &Ast) -> Option<Vec<FacetPathSegment>> {
        let mut segments = Vec::new();
        let mut current = expr;
        loop {
            match current {
                Ast::FieldAccess(_, inner, field) => {
                    segments.push(FacetPathSegment::field(field.clone()));
                    current = inner.as_ref();
                }
                Ast::FacetSegmentAccess(_, inner, segment) => {
                    segments.push(segment.clone());
                    current = inner.as_ref();
                }
                Ast::Grouped(_, inner) => {
                    current = inner.as_ref();
                }
                Ast::Var(_, name) if name == "_" => {
                    segments.reverse();
                    return (!segments.is_empty()).then_some(segments);
                }
                _ => return None,
            }
        }
    }

    fn resolve_facet_path_segment(
        &mut self,
        segment: FacetPathSegment,
    ) -> Result<ResolvedFacetPathSegment, ResolveError> {
        Ok(match segment {
            FacetPathSegment::Field { name, optional } => {
                ResolvedFacetPathSegment::Field { name, optional }
            }
            FacetPathSegment::Bracket(expr) => {
                ResolvedFacetPathSegment::Bracket(ResolvedFacetBracketExpr {
                    expr: Box::new(self.resolve_node(*expr.expr)?),
                    display: expr.display,
                })
            }
        })
    }

    fn resolve_facet_path_segments(
        &mut self,
        segments: Vec<FacetPathSegment>,
    ) -> Result<Vec<ResolvedFacetPathSegment>, ResolveError> {
        segments
            .into_iter()
            .map(|segment| self.resolve_facet_path_segment(segment))
            .collect()
    }

    fn pipe_slot_span(expr: &Ast) -> Option<Span> {
        match expr {
            Ast::Var(span, name) if name == "_1" => Some(span.clone()),
            Ast::App(_, func, args) => Self::pipe_slot_span(func).or_else(|| {
                args.iter().find_map(|arg| match arg {
                    RecordLitArg::Positional(expr) | RecordLitArg::Named(_, expr) => {
                        Self::pipe_slot_span(expr)
                    }
                })
            }),
            Ast::Block(_, stmts) | Ast::ListLiteral(_, stmts) | Ast::TupleLiteral(_, stmts) => {
                stmts.iter().find_map(Self::pipe_slot_span)
            }
            Ast::HashMapLiteral(_, entries) => entries.iter().find_map(|entry| {
                Self::pipe_slot_span(&entry.key).or_else(|| Self::pipe_slot_span(&entry.value))
            }),
            Ast::RangeLiteral(_, start, stop) => {
                Self::pipe_slot_span(start).or_else(|| Self::pipe_slot_span(stop))
            }
            Ast::Bind(_, _, rhs)
            | Ast::SafeBind(_, _, rhs)
            | Ast::Grouped(_, rhs)
            | Ast::Semi(_, rhs)
            | Ast::FieldAccess(_, rhs, _)
            | Ast::FacetSegmentAccess(_, rhs, _) => Self::pipe_slot_span(rhs),
            Ast::BinOp(_, _, left, right)
            | Ast::Pipe(_, left, right)
            | Ast::ContextMap(_, left, right)
            | Ast::ContextBind(_, left, right)
            | Ast::Compose(_, left, right)
            | Ast::LiftedCompose(_, left, right)
            | Ast::KleisliCompose(_, left, right)
            | Ast::ListCons(_, left, right) => {
                Self::pipe_slot_span(left).or_else(|| Self::pipe_slot_span(right))
            }
            Ast::InterpolatedStr(_, parts) => parts.iter().find_map(|part| match part {
                InterpolatedPart::Text(_) => None,
                InterpolatedPart::Expr(expr) => Self::pipe_slot_span(expr),
            }),
            Ast::Match(_, scrutinee, arms) => Self::pipe_slot_span(scrutinee).or_else(|| {
                arms.iter().find_map(|arm| {
                    arm.guard
                        .as_ref()
                        .and_then(Self::pipe_slot_span)
                        .or_else(|| Self::pipe_slot_span(&arm.body))
                })
            }),
            Ast::StructLit(_, _, fields) | Ast::InternalStructLit(_, _, fields) => {
                fields.iter().find_map(|field| match field {
                    StructLitField::Explicit(_, expr) => Self::pipe_slot_span(expr),
                    StructLitField::Shorthand(_) => None,
                })
            }
            Ast::ConstructorCall(_, _, args) => args.iter().find_map(|arg| match arg {
                RecordLitArg::Positional(expr) | RecordLitArg::Named(_, expr) => {
                    Self::pipe_slot_span(expr)
                }
            }),
            Ast::Closure(_, _, body) => Self::pipe_slot_span(body),
            Ast::Capture(_, target, args) => {
                Self::pipe_slot_span(target).or_else(|| args.iter().find_map(Self::pipe_slot_span))
            }
            Ast::FuncLiteralRef(_, _) => None,
            _ => None,
        }
    }

    fn lower_pipe_rhs_slots(&self, rhs: Ast) -> Result<Ast, ResolveError> {
        let Ast::App(span, func, args) = rhs else {
            if let Some(slot_span) = Self::pipe_slot_span(&rhs) {
                return Err(ResolveError {
                    message: "pipe placeholder `_1` is only allowed as a direct argument of the outermost call on the right-hand side".into(),
                    span: slot_span,
                    related_labels: Vec::new(),
                });
            }
            return Ok(rhs);
        };

        let mut slot_count = 0usize;
        let mut lowered_args = Vec::with_capacity(args.len());
        let mut positional_only = Vec::with_capacity(args.len());
        for arg in args {
            match arg {
                RecordLitArg::Positional(Ast::Var(arg_span, name)) if name == "_1" => {
                    slot_count += 1;
                    let lowered = Ast::Var(arg_span.clone(), Self::pipe_slot_param_name(&span));
                    lowered_args.push(lowered.clone());
                    positional_only.push(RecordLitArg::Positional(lowered));
                }
                RecordLitArg::Positional(expr) => {
                    if let Some(slot_span) = Self::pipe_slot_span(&expr) {
                        return Err(ResolveError {
                            message: "pipe placeholder `_1` cannot be used as an expression".into(),
                            span: slot_span,
                            related_labels: Vec::new(),
                        });
                    }
                    lowered_args.push(expr.clone());
                    positional_only.push(RecordLitArg::Positional(expr));
                }
                RecordLitArg::Named(name, expr) => {
                    if let Some(slot_span) = Self::pipe_slot_span(&expr) {
                        return Err(ResolveError {
                            message: "pipe placeholder `_1` cannot be used as an expression".into(),
                            span: slot_span,
                            related_labels: Vec::new(),
                        });
                    }
                    if slot_count > 0 {
                        return Err(ResolveError {
                            message: "pipe placeholder `_1` does not support named arguments"
                                .into(),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    positional_only.push(RecordLitArg::Named(name, expr));
                }
            }
        }

        if slot_count == 0 {
            return Ok(Ast::App(span, func, positional_only));
        }
        if slot_count > 1 {
            return Err(ResolveError {
                message: "pipe placeholder `_1` can only be used once".into(),
                span,
                related_labels: Vec::new(),
            });
        }

        Ok(self.make_closure_from_call(
            &span,
            vec![ClosureParam {
                name: Self::pipe_slot_param_name(&span),
                ty: None,
                span: span.clone(),
            }],
            *func,
            lowered_args,
        ))
    }

    fn prepare_pipe_rhs(&mut self, rhs: Ast) -> Result<Ast, ResolveError> {
        let rhs = self.lower_pipe_rhs_slots(rhs)?;
        self.desugar_pipeline_rhs_special_form_partial(rhs)
    }

    fn undefined_callable_arity_message(func: &Ast, arity: usize) -> Option<String> {
        match func {
            Ast::Var(_, name) => Some(format!("Undefined function {}/{}", name, arity)),
            Ast::Path(_, path) => Some(format!(
                "Undefined function {}/{}",
                path.segments.join("::"),
                arity
            )),
            _ => None,
        }
    }

    fn callable_entry_for_name(&self, name: &str) -> Option<&DeclarationEntry> {
        self.declaration_entries.get(name).filter(|entry| {
            matches!(
                entry.kind,
                DeclarationKind::Def
                    | DeclarationKind::Extractor
                    | DeclarationKind::TraitMethod
                    | DeclarationKind::ImplMethod
            )
        })
    }

    fn private_callable_error_message(&self, fq_name: &str, arity: usize) -> String {
        format!("Function `{fq_name}/{arity}` is private")
    }

    fn restricted_callable_error_message(&self, fq_name: &str, arity: usize) -> String {
        format!("Function `{fq_name}/{arity}` cannot be called from user code")
    }

    fn private_callable_error_for_candidate(&self, fq_name: &str, arity: usize) -> Option<String> {
        let mut candidates = vec![fq_name.to_string()];
        if !fq_name.starts_with("Global::") {
            candidates.push(format!("Global::{fq_name}"));
        }
        candidates.into_iter().find_map(|candidate| {
            self.callable_entry_for_name(&candidate)
                .is_some_and(|entry| entry.visibility == Visibility::Private)
                .then(|| self.private_callable_error_message(surface_path_name(&candidate), arity))
        })
    }

    fn declaration_entry_for_uid(&self, uid: u32) -> Option<&DeclarationEntry> {
        self.declaration_uids
            .iter()
            .find_map(|(fq_name, entry_uid)| (*entry_uid == uid).then_some(fq_name))
            .and_then(|fq_name| self.declaration_entries.get(fq_name))
    }

    fn ensure_user_callable_surface(
        &self,
        resolved_func: &Resolved,
        span: &Span,
        arity: usize,
    ) -> Result<(), ResolveError> {
        let Resolved::Var(_, id) = resolved_func else {
            return Ok(());
        };
        if id.compiler_generated {
            return Ok(());
        }
        let Some(entry) = self.declaration_entry_for_uid(id.unique_id) else {
            return Ok(());
        };
        if entry.user_callable {
            return Ok(());
        }
        Err(ResolveError {
            message: self.restricted_callable_error_message(&entry.fq_name, arity),
            span: span.clone(),
            related_labels: Vec::new(),
        })
    }

    fn private_callable_hint_for_bare_name(&self, name: &str, arity: usize) -> Option<String> {
        let matches = self
            .explicit_module_imports
            .iter()
            .filter_map(|module_name| {
                let fq_name = format!("{module_name}::{name}");
                self.callable_entry_for_name(&fq_name)
                    .and_then(|entry| (entry.visibility == Visibility::Private).then_some(fq_name))
            })
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            Some(format!(" Help: `{}/{}` is private.", matches[0], arity))
        } else {
            None
        }
    }

    fn map_undefined_callable_error(
        &self,
        err: ResolveError,
        func: &Ast,
        arity: usize,
    ) -> ResolveError {
        if let Ast::Var(_, name) = func {
            if let Some(message) = self.private_callable_error_for_candidate(name, arity) {
                return ResolveError {
                    message,
                    span: err.span,
                    related_labels: Vec::new(),
                };
            }
        }
        if let Ast::Path(_, path) = func {
            let fq_name = path.segments.join("::");
            if let Some(message) = self.private_callable_error_for_candidate(&fq_name, arity) {
                return ResolveError {
                    message,
                    span: err.span,
                    related_labels: Vec::new(),
                };
            }
        }
        match func {
            Ast::Var(_, name) if err.message == format!("Undefined variable: {}", name) => {
                let message = Self::undefined_callable_arity_message(func, arity)
                    .unwrap_or_else(|| format!("Undefined variable or function: {}", name));
                ResolveError {
                    message: match self.private_callable_hint_for_bare_name(name, arity) {
                        Some(hint) => format!("{message}{hint}"),
                        None => message,
                    },
                    span: err.span,
                    related_labels: Vec::new(),
                }
            }
            Ast::Path(_, path)
                if err.message == format!("Undefined variable: {}", path.segments.join("::")) =>
            {
                ResolveError {
                    message: Self::undefined_callable_arity_message(func, arity).unwrap_or_else(
                        || {
                            format!(
                                "Undefined variable or function: {}",
                                path.segments.join("::")
                            )
                        },
                    ),
                    span: err.span,
                    related_labels: Vec::new(),
                }
            }
            _ => err,
        }
    }

    pub(super) fn new() -> Self {
        Self {
            scope: initialize_scope(),
            predeclared_ids: HashMap::new(),
            declaration_entries: HashMap::new(),
            declaration_uids: HashMap::new(),
            declaration_uid_kinds: HashMap::from([
                (0, DeclarationKind::ResultCtor),
                (1, DeclarationKind::ResultCtor),
            ]),
            declaration_hidden_by_uid: HashMap::new(),
            trait_constructor_slots: HashMap::new(),
            owner_registry: OwnerRegistry::default(),
            explicit_module_imports: HashSet::new(),
            current_module_path: None,
            current_stage_impl_targets: None,
            allow_top_level_shadowing: false,
            forbidden_top_level_value_bindings: HashMap::new(),
            current_top_level_def_name: None,
        }
    }

    pub(super) fn with_scope(scope: Scope) -> Self {
        Self {
            scope,
            predeclared_ids: HashMap::new(),
            declaration_entries: HashMap::new(),
            declaration_uids: HashMap::new(),
            declaration_uid_kinds: HashMap::from([
                (0, DeclarationKind::ResultCtor),
                (1, DeclarationKind::ResultCtor),
            ]),
            declaration_hidden_by_uid: HashMap::new(),
            trait_constructor_slots: HashMap::new(),
            owner_registry: OwnerRegistry::default(),
            explicit_module_imports: HashSet::new(),
            current_module_path: None,
            current_stage_impl_targets: None,
            allow_top_level_shadowing: false,
            forbidden_top_level_value_bindings: HashMap::new(),
            current_top_level_def_name: None,
        }
    }

    pub(super) fn into_scope(self) -> Scope {
        self.scope
    }

    pub(super) fn qualify_current_declaration_name(&self, name: &str) -> String {
        if name.contains("::") {
            return name.to_string();
        }
        match self.current_module_path.as_deref() {
            Some(module_path) if !module_path.is_empty() => format!("{module_path}::{name}"),
            _ => name.to_string(),
        }
    }

    pub(super) fn symbol_info_for_uid(&self, name: &str, uid: u32) -> Option<SymbolIdentityInfo> {
        if is_synthetic_builtin_symbol_uid(uid) {
            return builtin_symbol_identity_info(name);
        }
        let kind = self.declaration_uid_kinds.get(&uid)?;
        let entry = self.declaration_entry_for_uid(uid);
        let direct_owner_name = if matches!(kind, DeclarationKind::Const) {
            entry
                .filter(|entry| entry.fq_name.contains("::__const__::"))
                .map(|entry| entry.fq_name.as_str())
                .or_else(|| entry.map(|entry| entry.name.as_str()))
                .unwrap_or(name)
        } else {
            entry.map(|entry| entry.name.as_str()).unwrap_or(name)
        };
        let inferred_owner = match kind {
            DeclarationKind::EnumVariant | DeclarationKind::TraitMethod => {
                let canonical_member_name = entry
                    .map(|entry| entry.name.as_str())
                    .filter(|entry_name| entry_name.contains("::"))
                    .or_else(|| entry.map(|entry| entry.fq_name.as_str()))
                    .unwrap_or(name);
                canonical_member_name
                    .rsplit_once("::")
                    .map(|(owner, _)| owner.to_string())
            }
            _ => entry.and_then(|entry| {
                let module_path = entry
                    .module_path
                    .strip_suffix("::__traitimpl__")
                    .unwrap_or(&entry.module_path);
                (!module_path.is_empty() && module_path != "__traitimpl__")
                    .then(|| module_path.to_string())
            }),
        };
        declaration_symbol_identity_info(
            &self.owner_registry,
            direct_owner_name,
            kind,
            inferred_owner.as_deref(),
        )
    }

    fn symbol_info_for_declaration(
        &self,
        name: &str,
        kind: &DeclarationKind,
        enclosing_owner: Option<&str>,
    ) -> Option<SymbolIdentityInfo> {
        declaration_symbol_identity_info(&self.owner_registry, name, kind, enclosing_owner)
    }

    pub(super) fn with_child_scope<T>(
        &mut self,
        f: impl FnOnce(&mut Resolver) -> Result<T, ResolveError>,
    ) -> Result<T, ResolveError> {
        let mut child = Resolver::with_scope(self.scope.clone());
        child.declaration_uids = self.declaration_uids.clone();
        child.declaration_uid_kinds = self.declaration_uid_kinds.clone();
        child.declaration_hidden_by_uid = self.declaration_hidden_by_uid.clone();
        child.declaration_entries = self.declaration_entries.clone();
        child.owner_registry = self.owner_registry.clone();
        child.explicit_module_imports = self.explicit_module_imports.clone();
        child.current_module_path = self.current_module_path.clone();
        child.current_stage_impl_targets = self.current_stage_impl_targets.clone();
        child.allow_top_level_shadowing = self.allow_top_level_shadowing;
        child.forbidden_top_level_value_bindings = self.forbidden_top_level_value_bindings.clone();
        child.current_top_level_def_name = self.current_top_level_def_name.clone();
        let out = f(&mut child)?;
        self.scope.advance_next_id_to(child.scope.next_id());
        Ok(out)
    }

    fn top_level_value_bindings(&self) -> HashMap<u32, String> {
        self.scope
            .bindings()
            .filter(|(_, uid)| !self.declaration_uid_kinds.contains_key(uid))
            .map(|(name, uid)| (uid, name.to_string()))
            .collect()
    }

    fn forbids_top_level_value_capture_in_defs(&self) -> bool {
        match self.current_module_path.as_deref() {
            None => true,
            Some("__Repl::Session") => true,
            Some(path) if path.starts_with("__Script::") => true,
            _ => false,
        }
    }

    pub(super) fn is_constructor_style_head(name: &str) -> bool {
        name.rsplit("::")
            .next()
            .and_then(|segment| segment.chars().next())
            .is_some_and(|ch| ch.is_uppercase())
    }

    pub(super) fn declaration_fq_name_for_uid(&self, uid: u32) -> Option<String> {
        self.declaration_uids
            .iter()
            .find_map(|(fq_name, entry_uid)| (*entry_uid == uid).then(|| fq_name.clone()))
    }

    fn hidden_builtin_message(name: &str) -> String {
        let display_name = match name {
            "Agent::pid"
            | "Agent::spawn"
            | "Agent::state"
            | "Agent::store"
            | "Agent::self"
            | "Agent::context_handler"
            | "GenServer::pid"
            | "GenServer::spawn"
            | "GenServer::state"
            | "GenServer::store"
            | "GenServer::self"
            | "GenServer::context_handler"
            | "Supervisor::spawn"
            | "Supervisor::adopt"
            | "Supervisor::status"
            | "Supervisor::workers" => name,
            _ => name.rsplit("::").next().unwrap_or(name),
        };
        let guidance = match name {
            "Agent::pid"
            | "Agent::spawn"
            | "Agent::state"
            | "Agent::store"
            | "Agent::self"
            | "Agent::context_handler" => {
                "This Agent module surface is compiler-managed; use `defagent` or generated owner helpers instead."
            }
            "GenServer::pid"
            | "GenServer::spawn"
            | "GenServer::state"
            | "GenServer::store"
            | "GenServer::self"
            | "GenServer::context_handler" => {
                "This GenServer module surface is compiler-managed; use `defagent`, `defgenserver`, or generated owner helpers instead."
            }
            "Supervisor::spawn" => {
                "This Supervisor module surface is compiler-managed; use `DynamicSupervisor::spawn(...)` or a generated `SupName::spawn(...)` wrapper instead."
            }
            "Supervisor::adopt" => {
                "This Supervisor module surface is compiler-managed; use `DynamicSupervisor::adopt(...)` or a generated `SupName::adopt(...)` wrapper instead."
            }
            "Supervisor::status" => {
                "This Supervisor module surface is compiler-managed; use `DynamicSupervisor::status()` or a generated `SupName::status()` wrapper instead."
            }
            "Supervisor::workers" => {
                "This Supervisor module surface is compiler-managed; use a generated `SupName::workers(...)` wrapper or the public Workers API instead."
            }
            _ => match display_name {
            "__process_self" => "Use `Process::self()` instead.",
            "__process_sleep" => "Use `Process::sleep(...)` instead.",
            "__task_call" => "Use `Task::call(...)` instead.",
            "__task_async" => "Use `Task::async(...)` instead.",
            "__task_await" => "Use `Task::await(...)` instead.",
            "__task_launch" => "Use `Task::launch(...)` instead.",
            "__task_cast" => "Use `Task::cast(...)` instead.",
            "__task_call_timeout"
            | "__task_async_timeout"
            | "__task_await_timeout"
            | "__task_launch_timeout"
            | "__task_cast_timeout"
            | "__workers_submit_timeout"
            | "__workers_broadcast_timeout" => {
                "Use the public Task/Workers API with `@timeout(...)` instead."
            }
            "__process_pid" | "__process_spawn" | "__process_state" | "__process_store" => {
                "This helper is compiler-managed; use `defagent`, `defgenserver`, or the public process surface instead."
            }
            "__supervisor_spawn" => {
                "Use `DynamicSupervisor::spawn(...)` or a generated Supervisor `spawn` wrapper instead."
            }
            "__supervisor_adopt" => {
                "Use `DynamicSupervisor::adopt(...)` or a generated Supervisor `adopt` wrapper instead."
            }
            "__supervisor_status" => {
                "Use `DynamicSupervisor::status()` or a generated Supervisor `status` wrapper instead."
            }
            "__supervisor_workers" => {
                "Use a generated Supervisor `workers` wrapper or the public Workers surface instead."
            }
            _ => "Use the public standard-library surface instead.",
        },
        };
        format!("hidden builtin `{display_name}` is compiler-internal. {guidance}")
    }

    fn hidden_builtin_error(&self, name: &str, span: Span) -> ResolveError {
        ResolveError {
            message: Self::hidden_builtin_message(name),
            span,
            related_labels: Vec::new(),
        }
    }

    fn resolve_var_like(
        &self,
        span: Span,
        name: String,
        compiler_generated: bool,
    ) -> Result<Resolved, ResolveError> {
        let uid = self
            .scope
            .lookup(&name)
            .or_else(|| {
                if compiler_generated && is_runtime_builtin_decl(&name) {
                    builtin_function_metas()
                        .iter()
                        .position(|meta| meta.name == name)
                        .map(|idx| builtin_uid(idx as u16))
                } else {
                    None
                }
            })
            .or_else(|| synthetic_facet_root_uid(&name))
            .ok_or_else(|| ResolveError {
                message: format!("Undefined variable: {}", name),
                span: span.clone(),
                related_labels: Vec::new(),
            })?;
        let qualified_name = (!is_synthetic_builtin_symbol_uid(uid))
            .then(|| self.declaration_fq_name_for_uid(uid))
            .flatten();
        let symbol_info = self.symbol_info_for_uid(&name, uid);
        if self
            .declaration_uid_kinds
            .get(&uid)
            .is_some_and(|kind| matches!(kind, DeclarationKind::Extractor))
        {
            return Err(ResolveError {
                message: format!(
                    "Extractor '{}' can only be used in MatchBlock/LHS positions. Use it on the left side of match, =?, or =. If you need a value-level API, write a normal def that returns Result or Option explicitly.",
                    name
                ),
                span,
                related_labels: Vec::new(),
            });
        }
        if let Some(binding_name) = self.forbidden_top_level_value_bindings.get(&uid) {
            let def_name = self
                .current_top_level_def_name
                .as_deref()
                .unwrap_or("<top-level>");
            return Err(ResolveError {
                message: format!(
                    "Top-level definition `{def_name}` cannot reference value binding `{binding_name}`"
                ),
                span,
                related_labels: Vec::new(),
            });
        }
        if !compiler_generated && self.declaration_hidden_by_uid.get(&uid) == Some(&true) {
            return Err(self.hidden_builtin_error(&name, span));
        }
        Ok(Resolved::Var(
            span.clone(),
            ResolvedId {
                name,
                qualified_name,
                unique_id: uid,
                compiler_generated,
                symbol_info,
                span,
            },
        ))
    }

    pub(super) fn attached_extractor_for_struct(
        &self,
        struct_uid: u32,
        surface_head: &str,
    ) -> Option<(Option<String>, u32, DeclarationKind)> {
        // Struct heads are resolved by declaration name, not by `import`.
        // In MatchBlock, `User(...)` is sugar for `User::deconstruct(...)`.
        let surface_extractor_name = format!("{}::deconstruct", surface_head);
        if let Some(extractor_uid) = self.scope.lookup(&surface_extractor_name) {
            let extractor_kind = self.declaration_uid_kinds.get(&extractor_uid).cloned()?;
            let qualified_name = self.declaration_fq_name_for_uid(extractor_uid);
            return Some((qualified_name, extractor_uid, extractor_kind));
        }

        let struct_fq_name = self.declaration_fq_name_for_uid(struct_uid)?;
        let extractor_fq_name = format!("{}::deconstruct", struct_fq_name);
        let extractor_uid = *self.declaration_uids.get(&extractor_fq_name)?;
        let extractor_kind = self.declaration_uid_kinds.get(&extractor_uid).cloned()?;
        Some((Some(extractor_fq_name), extractor_uid, extractor_kind))
    }

    pub(super) fn resolve_program(
        &mut self,
        stmts: Vec<Ast>,
    ) -> Result<Vec<Resolved>, ResolveError> {
        let stmts = super::derive::expand_derive_annotations(stmts)?;
        let stmts = self.lower_impl_defs(stmts)?;
        self.explicit_module_imports = Self::collect_explicit_module_imports(&stmts);
        self.validate_auto_import_conflicts(&stmts)?;
        self.predeclare_functions(&stmts)?;
        self.inherit_trait_constructor_slots(&stmts);
        let mut resolved = Vec::new();
        for stmt in stmts {
            if matches!(stmt, Ast::Import(_, _, _))
                || matches!(stmt, Ast::SupervisorInit(_, _))
                || matches!(stmt, Ast::IntrinsicDecl(_, _, _, _))
                || matches!(&stmt, Ast::BuiltinDecl(_, name, _, _, _, _) if is_doc_only_builtin_decl(name))
            {
                // `import` declarations are consumed by resolver-side module/import handling.
                // Until full module resolution lands, they are intentionally no-op here.
                continue;
            }
            resolved.push(self.resolve_node(stmt)?);
        }
        validate_trait_impl_pairs_in_nodes(&resolved)?;
        self.predeclared_ids.clear();
        Ok(resolved)
    }

    fn inherit_trait_constructor_slots(&mut self, stmts: &[Ast]) {
        let mut parents = Vec::new();
        for stmt in stmts {
            let Ast::TraitDef(_, name, _, Some(clause), _, _) = stmt else {
                continue;
            };
            let Some(child_uid) = self.scope.lookup(name) else {
                continue;
            };
            for constraint in &clause.constraints {
                if !matches!(&constraint.subject, AstTy::Named(_, subject) if subject == "Self") {
                    continue;
                }
                for bound in &constraint.bounds {
                    let spire::ast::WhereConstraintRhs::Trait(_, parent_name) = bound else {
                        continue;
                    };
                    if let Some(parent_uid) = self.scope.lookup(parent_name) {
                        parents.push((child_uid, parent_uid));
                    }
                }
            }
        }
        loop {
            let mut changed = false;
            for (child, parent) in &parents {
                if self.trait_constructor_slots.contains_key(child) {
                    continue;
                }
                if let Some(slots) = self.trait_constructor_slots.get(parent).cloned() {
                    self.trait_constructor_slots.insert(*child, slots);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn collect_explicit_module_imports(stmts: &[Ast]) -> HashSet<String> {
        stmts
            .iter()
            .filter_map(|stmt| match stmt {
                Ast::Import(_, path, spire::ast::ImportSpec::All) => Some(path.segments.join("::")),
                _ => None,
            })
            .collect()
    }
}

impl Resolver {
    fn flatten_bulk_update_entries(
        prefix: Option<BulkUpdatePath>,
        entries: Vec<BulkUpdateEntry>,
        out: &mut Vec<(Span, BulkUpdatePath, BulkUpdateEntryKind)>,
    ) {
        for entry in entries {
            let path = match &prefix {
                Some(prefix) => {
                    let span = Span {
                        start: prefix.span().start,
                        end: entry.path.span().end,
                    };
                    BulkUpdatePath::Chain(span, Box::new(prefix.clone()), Box::new(entry.path))
                }
                None => entry.path,
            };
            match entry.kind {
                BulkUpdateEntryKind::Nested(children) => {
                    Self::flatten_bulk_update_entries(Some(path), children, out);
                }
                kind => out.push((entry.span, path, kind)),
            }
        }
    }

    fn make_bulk_update_capture_path(
        span: &Span,
        root_name: &str,
        path: &[FacetPathSegment],
    ) -> Result<Ast, ResolveError> {
        let mut expr = Ast::Var(span.clone(), root_name.to_string());
        for segment in path {
            expr = match segment {
                FacetPathSegment::Field {
                    name,
                    optional: false,
                } => Ast::FieldAccess(span.clone(), Box::new(expr), name.clone()),
                other => Ast::FacetSegmentAccess(span.clone(), Box::new(expr), other.clone()),
            };
        }
        Ok(Ast::FacetCapture(span.clone(), Box::new(expr)))
    }

    fn static_bulk_update_segments(
        path: &BulkUpdatePath,
    ) -> Result<Option<Vec<FacetPathSegment>>, ResolveError> {
        match path {
            BulkUpdatePath::Segments(_, segments) => Ok(Some(segments.clone())),
            BulkUpdatePath::Pin(_, _) => Ok(None),
            BulkUpdatePath::Chain(_, left, right) => {
                let Some(mut left_segments) = Self::static_bulk_update_segments(left)? else {
                    return Ok(None);
                };
                let Some(right_segments) = Self::static_bulk_update_segments(right)? else {
                    return Ok(None);
                };
                left_segments.extend(right_segments);
                Ok(Some(left_segments))
            }
            BulkUpdatePath::StripLeft(span, inner, count) => {
                let Some(segments) = Self::static_bulk_update_segments(inner)? else {
                    return Err(ResolveError {
                        message:
                            "bulk_update path strip operations require a concrete DSL path fragment"
                                .into(),
                        span: span.clone(),
                        related_labels: Vec::new(),
                    });
                };
                if *count >= segments.len() {
                    return Err(ResolveError {
                        message: "bulk_update target path cannot be empty".into(),
                        span: span.clone(),
                        related_labels: Vec::new(),
                    });
                }
                Ok(Some(segments.into_iter().skip(*count).collect()))
            }
            BulkUpdatePath::StripRight(span, inner, count) => {
                let Some(mut segments) = Self::static_bulk_update_segments(inner)? else {
                    return Err(ResolveError {
                        message:
                            "bulk_update path strip operations require a concrete DSL path fragment"
                                .into(),
                        span: span.clone(),
                        related_labels: Vec::new(),
                    });
                };
                if *count >= segments.len() {
                    return Err(ResolveError {
                        message: "bulk_update target path cannot be empty".into(),
                        span: span.clone(),
                        related_labels: Vec::new(),
                    });
                }
                let keep = segments.len() - *count;
                segments.truncate(keep);
                Ok(Some(segments))
            }
        }
    }

    fn make_bulk_update_path_expr(
        span: &Span,
        root_name: &str,
        path: &BulkUpdatePath,
    ) -> Result<Ast, ResolveError> {
        if let Some(segments) = Self::static_bulk_update_segments(path)? {
            if segments.is_empty() {
                return Err(ResolveError {
                    message: "bulk_update target path cannot be empty".into(),
                    span: span.clone(),
                    related_labels: Vec::new(),
                });
            }
            return Self::make_bulk_update_capture_path(span, root_name, &segments);
        }

        match path {
            BulkUpdatePath::Pin(pin_span, name) => Ok(Ast::Var(pin_span.clone(), name.clone())),
            BulkUpdatePath::Chain(chain_span, left, right) => {
                let left_expr = Self::make_bulk_update_path_expr(chain_span, root_name, left)?;
                let right_expr = Self::make_bulk_update_path_expr(chain_span, root_name, right)?;
                Ok(Ast::App(
                    chain_span.clone(),
                    Box::new(Ast::Path(
                        chain_span.clone(),
                        AstPath {
                            span: chain_span.clone(),
                            segments: vec!["Facet".into(), "chain".into()],
                        },
                    )),
                    vec![
                        RecordLitArg::Positional(left_expr),
                        RecordLitArg::Positional(right_expr),
                    ],
                ))
            }
            BulkUpdatePath::Segments(path_span, _)
            | BulkUpdatePath::StripLeft(path_span, _, _)
            | BulkUpdatePath::StripRight(path_span, _, _) => Err(ResolveError {
                message: "bulk_update static path could not be lowered".into(),
                span: path_span.clone(),
                related_labels: Vec::new(),
            }),
        }
    }

    fn make_facet_intrinsic_call(
        span: &Span,
        method: &str,
        path_expr: Ast,
        source_expr: Option<Ast>,
        value_expr: Ast,
    ) -> Ast {
        let mut args = vec![RecordLitArg::Positional(path_expr)];
        if let Some(source_expr) = source_expr {
            args.push(RecordLitArg::Positional(source_expr));
        }
        args.push(RecordLitArg::Positional(value_expr));
        Ast::App(
            span.clone(),
            Box::new(Ast::Path(
                span.clone(),
                AstPath {
                    span: span.clone(),
                    segments: vec!["Facet".into(), method.into()],
                },
            )),
            args,
        )
    }

    fn lower_bulk_update_special_form(
        &mut self,
        span: Span,
        source: Ast,
        entries: Vec<BulkUpdateEntry>,
    ) -> Result<Resolved, ResolveError> {
        let mut flat_entries = Vec::new();
        Self::flatten_bulk_update_entries(None, entries, &mut flat_entries);

        let mut expr = Ast::ConstructorCall(
            source.span().clone(),
            "Ok".into(),
            vec![RecordLitArg::Positional(source)],
        );

        for (index, entry_span, path, kind) in flat_entries
            .into_iter()
            .enumerate()
            .map(|(index, (span, path, kind))| (index, span, path, kind))
        {
            let param_name = format!("__bulk_state_{}_{}", span.start, index);
            let capture = Self::make_bulk_update_path_expr(&entry_span, &param_name, &path)?;
            let source_expr = if matches!(capture, Ast::FacetCapture(_, _)) {
                None
            } else {
                Some(Ast::Var(entry_span.clone(), param_name.clone()))
            };
            let body = match kind {
                BulkUpdateEntryKind::Set(value) => {
                    Self::make_facet_intrinsic_call(&entry_span, "set", capture, source_expr, value)
                }
                BulkUpdateEntryKind::Over(update_fun) => Self::make_facet_intrinsic_call(
                    &entry_span,
                    "over",
                    capture,
                    source_expr,
                    update_fun,
                ),
                BulkUpdateEntryKind::OverResult(update_fun) => Self::make_facet_intrinsic_call(
                    &entry_span,
                    "over_result",
                    capture,
                    source_expr,
                    update_fun,
                ),
                BulkUpdateEntryKind::CaseSet(value) => Self::make_facet_intrinsic_call(
                    &entry_span,
                    "case_set",
                    capture,
                    source_expr,
                    value,
                ),
                BulkUpdateEntryKind::CaseOver(update_fun) => Self::make_facet_intrinsic_call(
                    &entry_span,
                    "case_over",
                    capture,
                    source_expr,
                    update_fun,
                ),
                BulkUpdateEntryKind::Nested(_) => {
                    return Err(ResolveError {
                        message: "nested bulk_update entries must be flattened before lowering"
                            .into(),
                        span: entry_span,
                        related_labels: Vec::new(),
                    });
                }
            };
            let closure = Ast::Closure(
                entry_span.clone(),
                vec![ClosureParam {
                    name: param_name,
                    ty: None,
                    span: entry_span.clone(),
                }],
                Box::new(body),
            );
            expr = Ast::ContextBind(
                Span {
                    start: span.start,
                    end: entry_span.end,
                },
                Box::new(expr),
                Box::new(closure),
            );
        }

        self.resolve_node(expr)
    }

    pub(super) fn resolve_node(&mut self, node: Ast) -> Result<Resolved, ResolveError> {
        match node {
            Ast::Lit(span, lit) => Ok(Resolved::Lit(span, lit)),

            Ast::Var(span, name) => self.resolve_var_like(span, name, false),
            Ast::InternalVar(span, name) => self.resolve_var_like(span, name, true),
            Ast::Path(span, path) => {
                let name = path.segments.join("::");
                self.resolve_var_like(span, name, false)
            }
            Ast::FuncLiteralRef(span, func) => Err(ResolveError {
                message: format!(
                    "standalone func literal ref `{}` must be lowered before resolution",
                    func.body
                ),
                span,
                related_labels: Vec::new(),
            }),

            Ast::TypeApply(span, target, args) => {
                let resolved_target = self.resolve_node(*target)?;
                self.ensure_user_callable_surface(&resolved_target, &span, 0)?;
                let resolved_args = args
                    .into_iter()
                    .map(|arg| self.resolve_type_annotation(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::TypeApply(
                    span,
                    Box::new(resolved_target),
                    resolved_args,
                ))
            }

            Ast::App(span, func, args) => {
                if let Ast::Var(_, ref name) = *func {
                    if name == "&&" {
                        return self.resolve_logic_call(span, args, LogicKind::And);
                    }
                    if name == "||" {
                        return self.resolve_logic_call(span, args, LogicKind::Or);
                    }
                }

                let resolved_func = match self.resolve_node(*func.clone()) {
                    Ok(resolved_func) => {
                        if let Some(kind) =
                            self.classify_canonical_special_form_callee(&resolved_func)
                        {
                            return self.resolve_canonical_special_form_call(span, args, kind);
                        }
                        resolved_func
                    }
                    Err(err) => {
                        if let Some(kind) = Self::fallback_special_form_from_surface(func.as_ref())
                        {
                            return self.resolve_canonical_special_form_call(span, args, kind);
                        }
                        return Err(self.map_undefined_callable_error(err, &func, args.len()));
                    }
                };
                self.ensure_user_callable_surface(&resolved_func, &span, args.len())?;
                let resolved_args = args
                    .into_iter()
                    .map(|arg| match arg {
                        RecordLitArg::Positional(expr) => {
                            Ok(ResolvedRecordLitArg::Positional(self.resolve_node(expr)?))
                        }
                        RecordLitArg::Named(name, expr) => {
                            Ok(ResolvedRecordLitArg::Named(name, self.resolve_node(expr)?))
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::App(span, Box::new(resolved_func), resolved_args))
            }

            Ast::Bind(span, pat, rhs) => {
                // Resolve RHS first (before defining the new binding for shadowing)
                let resolved_rhs = self.resolve_node(*rhs)?;
                let resolved_pat = self.resolve_pattern(pat)?;
                Ok(Resolved::Bind(span, resolved_pat, Box::new(resolved_rhs)))
            }

            Ast::SafeBind(span, pat, rhs) => {
                // Resolve RHS first (before defining the new binding for shadowing)
                let resolved_rhs = self.resolve_node(*rhs)?;
                let resolved_pat = self.resolve_pattern(pat)?;
                Ok(Resolved::SafeBind(
                    span,
                    resolved_pat,
                    Box::new(resolved_rhs),
                ))
            }

            Ast::BinOp(span, op, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::BinOp(span, op, Box::new(l), Box::new(r)))
            }

            Ast::Pipe(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let rhs = self.prepare_pipe_rhs(*right)?;
                let r = self.resolve_node(rhs)?;
                Ok(Resolved::Pipe(span, Box::new(l), Box::new(r)))
            }

            Ast::ContextMap(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let rhs = self.prepare_pipe_rhs(*right)?;
                let r = self.resolve_node(rhs)?;
                Ok(Resolved::ContextMap(span, Box::new(l), Box::new(r)))
            }

            Ast::ContextApply(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let rhs = self.prepare_pipe_rhs(*right)?;
                let r = self.resolve_node(rhs)?;
                Ok(Resolved::ContextApply(span, Box::new(l), Box::new(r)))
            }

            Ast::ContextBind(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let rhs = self.prepare_pipe_rhs(*right)?;
                let r = self.resolve_node(rhs)?;
                Ok(Resolved::ContextBind(span, Box::new(l), Box::new(r)))
            }

            Ast::Compose(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::Compose(span, Box::new(l), Box::new(r)))
            }

            Ast::LiftedCompose(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::LiftedCompose(span, Box::new(l), Box::new(r)))
            }

            Ast::KleisliCompose(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::KleisliCompose(span, Box::new(l), Box::new(r)))
            }

            Ast::ListNil(span) => Ok(Resolved::ListNil(span)),

            Ast::ListCons(span, head, tail) => {
                let head = self.resolve_node(*head)?;
                let tail = self.resolve_node(*tail)?;
                Ok(Resolved::ListCons(span, Box::new(head), Box::new(tail)))
            }

            Ast::ListLiteral(span, elems) => {
                let resolved = elems
                    .into_iter()
                    .map(|e| self.resolve_node(e))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::ListLiteral(span, resolved))
            }

            Ast::HashMapLiteral(span, entries) => {
                let resolved = entries
                    .into_iter()
                    .map(|entry| {
                        Ok(crate::resolved::ResolvedHashMapLiteralEntry {
                            key: self.resolve_node(entry.key)?,
                            value: self.resolve_node(entry.value)?,
                        })
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::HashMapLiteral(span, resolved))
            }

            Ast::RangeLiteral(span, start, stop) => {
                let start = self.resolve_node(*start)?;
                let stop = self.resolve_node(*stop)?;
                Ok(Resolved::RangeLiteral(
                    span,
                    Box::new(start),
                    Box::new(stop),
                ))
            }

            Ast::TupleLiteral(span, elems) => {
                let resolved = elems
                    .into_iter()
                    .map(|e| self.resolve_node(e))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::TupleLiteral(span, resolved))
            }

            Ast::Grouped(span, inner) => {
                let inner = self.resolve_node(*inner)?;
                Ok(Resolved::Grouped(span, Box::new(inner)))
            }

            Ast::InterpolatedStr(span, parts) => {
                let mut resolved_parts = Vec::new();
                for part in parts {
                    match part {
                        spire::ast::InterpolatedPart::Text(s) => {
                            resolved_parts.push(ResolvedInterpolatedPart::Text(s));
                        }
                        spire::ast::InterpolatedPart::Expr(expr) => {
                            let resolved_expr = self.resolve_node(*expr)?;
                            resolved_parts
                                .push(ResolvedInterpolatedPart::Expr(Box::new(resolved_expr)));
                        }
                    }
                }
                Ok(Resolved::InterpolatedStr(span, resolved_parts))
            }
            Ast::Dbg(span, args) => Ok(Resolved::Dbg(
                span,
                args.into_iter()
                    .map(|arg| self.resolve_node(arg.expr))
                    .collect::<Result<Vec<_>, _>>()?,
            )),

            Ast::BulkUpdate(span, source, entries) => {
                self.lower_bulk_update_special_form(span, *source, entries)
            }

            Ast::FieldAccess(span, expr, field) => {
                let original = Ast::FieldAccess(span.clone(), expr.clone(), field.clone());
                if let Some(segments) = Self::inferred_facet_capture_segments(&original) {
                    return Ok(Resolved::InferredFacetCapture(
                        span,
                        self.resolve_facet_path_segments(segments)?,
                    ));
                }
                if matches!(expr.as_ref(), Ast::Var(_, name) if name == "ctx") {
                    return Ok(Resolved::ProcessContextHandler(span, field));
                }
                if let Ast::Var(root_span, name) = expr.as_ref() {
                    if let Some((unique_id, symbol_info)) = synthetic_member_root(name) {
                        let root = Resolved::Var(
                            root_span.clone(),
                            ResolvedId {
                                name: name.clone(),
                                qualified_name: None,
                                unique_id,
                                compiler_generated: true,
                                symbol_info: Some(symbol_info),
                                span: root_span.clone(),
                            },
                        );
                        return Ok(Resolved::FieldAccess(span, Box::new(root), field));
                    }
                }
                let resolved_expr = self.resolve_node(*expr)?;
                Ok(Resolved::FieldAccess(span, Box::new(resolved_expr), field))
            }
            Ast::FacetSegmentAccess(span, expr, segment) => {
                let original = Ast::FacetSegmentAccess(span.clone(), expr.clone(), segment.clone());
                if let Some(segments) = Self::inferred_facet_capture_segments(&original) {
                    return Ok(Resolved::InferredFacetCapture(
                        span,
                        self.resolve_facet_path_segments(segments)?,
                    ));
                }
                let resolved_expr = self.resolve_node(*expr)?;
                Ok(Resolved::FacetSegmentAccess(
                    span,
                    Box::new(resolved_expr),
                    self.resolve_facet_path_segment(segment)?,
                ))
            }
            Ast::FacetCapture(span, expr) => {
                let resolved_expr = self.resolve_node(*expr)?;
                Ok(Resolved::FacetCapture(span, Box::new(resolved_expr)))
            }

            Ast::Block(span, stmts) => {
                let resolved = self.with_child_scope(|child| {
                    stmts
                        .into_iter()
                        .map(|s| child.resolve_node(s))
                        .collect::<Result<Vec<_>, _>>()
                })?;
                Ok(Resolved::Block(span, resolved))
            }

            Ast::Semi(span, inner) => {
                let resolved = self.resolve_node(*inner)?;
                Ok(Resolved::Semi(span, Box::new(resolved)))
            }

            // Struct/Record/Deferror definitions — reuse predeclared IDs
            Ast::StructDef(span, name, type_params, fields, attrs) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                define_global_surface_alias(&mut self.scope, &name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let symbol_info =
                    self.symbol_info_for_declaration(&name, &DeclarationKind::Struct, None);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    compiler_generated: false,
                    symbol_info,
                    span: span.clone(),
                };
                let resolved_type_params = self.resolve_type_params(type_params)?;
                let rfields = fields
                    .into_iter()
                    .map(|f| {
                        Ok(ResolvedField {
                            id: None,
                            name: f.name,
                            ty: self.resolve_type_annotation(f.ty)?,
                            span: f.span,
                            visibility: f.visibility,
                            readonly: f.readonly,
                        })
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::StructDef(
                    span,
                    rid,
                    resolved_type_params,
                    rfields,
                    resolve_decl_attrs(&attrs),
                ))
            }

            Ast::RecordDef(span, name, fields, attrs) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                define_global_surface_alias(&mut self.scope, &name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let symbol_info =
                    self.symbol_info_for_declaration(&name, &DeclarationKind::Record, None);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    compiler_generated: false,
                    symbol_info,
                    span: span.clone(),
                };
                let rfields = fields
                    .into_iter()
                    .map(|f| {
                        Ok(ResolvedField {
                            id: None,
                            name: f.name,
                            ty: self.resolve_type_annotation(f.ty)?,
                            span: f.span,
                            visibility: f.visibility,
                            readonly: f.readonly,
                        })
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::RecordDef(
                    span,
                    rid,
                    rfields,
                    resolve_decl_attrs(&attrs),
                ))
            }

            Ast::DeferrorDef(span, name, fields, show_expr, _) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                define_global_surface_alias(&mut self.scope, &name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let symbol_info =
                    self.symbol_info_for_declaration(&name, &DeclarationKind::Deferror, None);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    compiler_generated: false,
                    symbol_info,
                    span: span.clone(),
                };
                let mut error_scope = self.scope.clone();
                let mut rfields = Vec::new();
                for f in fields {
                    let uid = error_scope.define(&f.name, f.span.clone());
                    rfields.push(ResolvedField {
                        id: Some(ResolvedId {
                            name: f.name.clone(),
                            qualified_name: None,
                            unique_id: uid,
                            compiler_generated: false,
                            symbol_info: None,
                            span: f.span.clone(),
                        }),
                        name: f.name,
                        ty: self.resolve_type_annotation(f.ty)?,
                        span: f.span,
                        visibility: f.visibility,
                        readonly: f.readonly,
                    });
                }
                let mut show_resolver = Resolver::with_scope(error_scope);
                show_resolver.declaration_uids = self.declaration_uids.clone();
                show_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                show_resolver.declaration_hidden_by_uid = self.declaration_hidden_by_uid.clone();
                show_resolver.owner_registry = self.owner_registry.clone();
                show_resolver.current_module_path = self.current_module_path.clone();
                show_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                let resolved_show = show_resolver.resolve_node(*show_expr)?;
                self.scope.advance_next_id_to(show_resolver.scope.next_id());
                Ok(Resolved::DeferrorDef(
                    span,
                    rid,
                    rfields,
                    Box::new(resolved_show),
                ))
            }

            Ast::EnumDef(span, name, type_params, variants, attrs) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                define_global_surface_alias(&mut self.scope, &name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let symbol_info =
                    self.symbol_info_for_declaration(&name, &DeclarationKind::Enum, None);
                let rid = ResolvedId {
                    name: name.clone(),
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    compiler_generated: false,
                    symbol_info,
                    span: span.clone(),
                };
                let resolved_type_params = type_params
                    .into_iter()
                    .map(|param| self.resolve_type_param(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?;

                let mut resolved_variants = Vec::new();
                for variant in variants {
                    let ctor_name = format!("{}::{}", name, variant.name);
                    let ctor_uid = self
                        .take_predeclared_id(&ctor_name)
                        .or_else(|| self.scope.lookup(&ctor_name))
                        .unwrap_or_else(|| self.scope.reserve_id());
                    self.scope.define_with_id(&ctor_name, ctor_uid);
                    define_global_surface_alias(&mut self.scope, &ctor_name, ctor_uid);
                    let qualified_ctor_name = self.qualify_current_declaration_name(&ctor_name);
                    let symbol_info = self.symbol_info_for_declaration(
                        &ctor_name,
                        &DeclarationKind::EnumVariant,
                        Some(&name),
                    );
                    resolved_variants.push(ResolvedEnumVariant {
                        id: ResolvedId {
                            name: ctor_name,
                            qualified_name: Some(qualified_ctor_name),
                            unique_id: ctor_uid,
                            compiler_generated: false,
                            symbol_info,
                            span: variant.span.clone(),
                        },
                        payload: variant
                            .payload
                            .into_iter()
                            .map(|ty| self.resolve_type_annotation(ty))
                            .collect::<Result<Vec<_>, ResolveError>>()?,
                        discriminant: variant.discriminant,
                        span: variant.span,
                    });
                }

                Ok(Resolved::EnumDef(
                    span,
                    rid,
                    resolved_type_params,
                    resolved_variants,
                    resolve_decl_attrs(&attrs),
                ))
            }

            Ast::Def(span, name, type_params, params, ret_ty, where_clause, body, attrs) => {
                let fun_uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                let mut body_scope = self.scope.clone();
                // Ensure self-recursion inside this definition binds to this declaration,
                // not to a newer same-name declaration predeclared later in the chunk.
                body_scope.define_with_id(&name, fun_uid);
                let mut body_resolver = Resolver::with_scope(body_scope);
                body_resolver.declaration_uids = self.declaration_uids.clone();
                body_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                body_resolver.declaration_hidden_by_uid = self.declaration_hidden_by_uid.clone();
                body_resolver.owner_registry = self.owner_registry.clone();
                body_resolver.current_module_path = self.current_module_path.clone();
                body_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                if self.forbids_top_level_value_capture_in_defs() {
                    body_resolver.forbidden_top_level_value_bindings =
                        self.top_level_value_bindings();
                    body_resolver.current_top_level_def_name = Some(name.clone());
                }
                let resolved_type_params = self.resolve_type_params(type_params)?;
                let resolved_params = params
                    .into_iter()
                    .map(|param| body_resolver.resolve_fun_param(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                let resolved_body = body_resolver.resolve_node(*body)?;

                self.scope.advance_next_id_to(body_resolver.scope.next_id());
                self.scope.define_with_id(&name, fun_uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                define_global_surface_alias(&mut self.scope, &qualified_name, fun_uid);
                let symbol_info = self.symbol_info_for_declaration(
                    &name,
                    &DeclarationKind::Def,
                    self.current_module_path.as_deref(),
                );
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: fun_uid,
                    compiler_generated: false,
                    symbol_info,
                    span: span.clone(),
                };

                Ok(Resolved::Def(
                    span,
                    rid,
                    resolved_type_params,
                    resolved_params,
                    ret_ty
                        .map(|ty| self.resolve_type_annotation(ty))
                        .transpose()?,
                    where_clause
                        .map(|clause| self.resolve_where_clause(clause))
                        .transpose()?,
                    Box::new(resolved_body),
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::ConstDef(span, name, ty, value, attrs) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                let resolved_value = self.resolve_node(*value)?;
                self.scope.define_with_id(&name, uid);
                let qualified_name = if attrs.visibility == Visibility::Public {
                    Some(self.qualify_current_declaration_name(&name))
                } else {
                    Some(self.qualify_current_declaration_name(&format!("__const__::{}", name)))
                };
                if let Some(qualified_name) = qualified_name.as_deref() {
                    define_global_surface_alias(&mut self.scope, qualified_name, uid);
                }
                let owner_name = if attrs.visibility == Visibility::Public {
                    name.as_str()
                } else {
                    qualified_name.as_deref().unwrap_or(&name)
                };
                let symbol_info =
                    self.symbol_info_for_declaration(owner_name, &DeclarationKind::Const, None);
                let rid = ResolvedId {
                    name,
                    qualified_name,
                    unique_id: uid,
                    compiler_generated: false,
                    symbol_info,
                    span: span.clone(),
                };
                Ok(Resolved::ConstDef(
                    span,
                    rid,
                    ty.map(|ty| self.resolve_type_annotation(ty)).transpose()?,
                    Box::new(resolved_value),
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::ExtractorDef(span, name, type_params, param, ret_ty, body, attrs) => {
                let fun_uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                let mut body_scope = self.scope.clone();
                body_scope.define_with_id(&name, fun_uid);
                let mut body_resolver = Resolver::with_scope(body_scope);
                body_resolver.declaration_uids = self.declaration_uids.clone();
                body_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                body_resolver.declaration_hidden_by_uid = self.declaration_hidden_by_uid.clone();
                body_resolver.owner_registry = self.owner_registry.clone();
                body_resolver.current_module_path = self.current_module_path.clone();
                body_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                let resolved_type_params = self.resolve_type_params(type_params)?;
                let resolved_param = body_resolver.resolve_extractor_param(param)?;
                let resolved_body = body_resolver.resolve_node(*body)?;

                self.scope.advance_next_id_to(body_resolver.scope.next_id());
                self.scope.define_with_id(&name, fun_uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                define_global_surface_alias(&mut self.scope, &qualified_name, fun_uid);
                let symbol_info = self.symbol_info_for_declaration(
                    &name,
                    &DeclarationKind::Extractor,
                    self.current_module_path.as_deref(),
                );
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: fun_uid,
                    compiler_generated: false,
                    symbol_info,
                    span: span.clone(),
                };

                Ok(Resolved::ExtractorDef(
                    span,
                    rid,
                    resolved_type_params,
                    resolved_param,
                    self.resolve_type_annotation(ret_ty)?,
                    Box::new(resolved_body),
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::TraitDef(span, name, type_params, where_clause, methods, attrs) => {
                let qualified_trait_name = self.qualify_current_declaration_name(&name);
                let trait_uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, trait_uid);
                let symbol_info =
                    self.symbol_info_for_declaration(&name, &DeclarationKind::Trait, None);
                let rid = ResolvedId {
                    name: name.clone(),
                    qualified_name: Some(qualified_trait_name.clone()),
                    unique_id: trait_uid,
                    compiler_generated: false,
                    symbol_info,
                    span: span.clone(),
                };
                let resolved_type_params = self.resolve_type_params(type_params)?;
                let mut method_headers = Vec::new();
                for method in &methods {
                    let method_alias = trait_method_qualified_name(&name, &method.name);
                    let qualified_method =
                        trait_method_qualified_name(&qualified_trait_name, &method.name);
                    let method_uid = self
                        .take_predeclared_id(&method_alias)
                        .or_else(|| self.scope.lookup(&method_alias))
                        .unwrap_or_else(|| self.scope.reserve_id());
                    self.scope.define_with_id(&method_alias, method_uid);
                    method_headers.push((method.name.clone(), method_uid, qualified_method));
                }

                let mut trait_method_scope = self.scope.clone();
                for (method_name, method_uid, _) in &method_headers {
                    trait_method_scope.define_with_id(method_name, *method_uid);
                }

                let mut resolved_methods = Vec::new();
                for (method, (_, method_uid, qualified_method)) in
                    methods.into_iter().zip(method_headers.into_iter())
                {
                    let spire::ast::TraitMethodSig {
                        name: method_name,
                        fun_params,
                        type_params,
                        params,
                        ret_ty,
                        where_clause,
                        body,
                        attrs,
                        span: method_span,
                    } = method;
                    let mut method_resolver = Resolver::with_scope(trait_method_scope.clone());
                    method_resolver.declaration_uids = self.declaration_uids.clone();
                    method_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                    method_resolver.declaration_hidden_by_uid =
                        self.declaration_hidden_by_uid.clone();
                    method_resolver.owner_registry = self.owner_registry.clone();
                    method_resolver.current_module_path = self.current_module_path.clone();
                    method_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                    let resolved_params = params
                        .into_iter()
                        .map(|param| method_resolver.resolve_fun_param(param))
                        .collect::<Result<Vec<_>, ResolveError>>()?;
                    let resolved_body = body
                        .map(|body| method_resolver.resolve_node(*body).map(Box::new))
                        .transpose()?;
                    self.scope
                        .advance_next_id_to(method_resolver.scope.next_id());
                    let symbol_info = self.symbol_info_for_declaration(
                        &qualified_method,
                        &DeclarationKind::TraitMethod,
                        Some(&name),
                    );
                    resolved_methods.push(ResolvedTraitMethodSig {
                        id: ResolvedId {
                            name: method_name,
                            qualified_name: Some(qualified_method),
                            unique_id: method_uid,
                            compiler_generated: false,
                            symbol_info,
                            span: method_span.clone(),
                        },
                        fun_params: fun_params
                            .into_iter()
                            .map(|ty| self.resolve_type_annotation(ty))
                            .collect::<Result<Vec<_>, _>>()?,
                        type_params: self.resolve_type_params(type_params)?,
                        params: resolved_params,
                        ret_ty: self.resolve_type_annotation(ret_ty)?,
                        where_clause: where_clause
                            .map(|clause| self.resolve_where_clause(clause))
                            .transpose()?,
                        body: resolved_body,
                        attrs: resolve_decl_attrs(&attrs),
                        span: method_span,
                    });
                }
                Ok(Resolved::TraitDef(
                    span,
                    rid,
                    resolved_type_params,
                    where_clause
                        .map(|clause| self.resolve_where_clause(clause))
                        .transpose()?,
                    resolved_methods,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::TraitImplDef(
                span,
                trait_name,
                trait_args,
                target_ty,
                where_clause,
                methods,
                attrs,
            ) => {
                validate_unique_callable_names(
                    &format!(
                        "impl `{}` for `{}`",
                        trait_instance_key(&trait_name, &trait_args),
                        ast_ty_key(&target_ty)
                    ),
                    &methods,
                )?;
                let (trait_uid, qualified_trait_name) =
                    self.resolve_trait_reference(&trait_name, &span)?;
                let trait_symbol_info =
                    self.symbol_info_for_declaration(&trait_name, &DeclarationKind::Trait, None);
                let trait_id = ResolvedId {
                    name: trait_name.clone(),
                    qualified_name: Some(qualified_trait_name.clone()),
                    unique_id: trait_uid,
                    compiler_generated: attrs.compiler_generated,
                    symbol_info: trait_symbol_info,
                    span: span.clone(),
                };
                let resolved_target_ty = self.resolve_type_annotation(target_ty)?;
                let target_key = ast_ty_key(&resolved_target_ty);
                let target_owner_key =
                    ast_ty_owner_head(&resolved_target_ty).unwrap_or(target_key.as_str());
                let mut resolved_methods = Vec::new();
                for method in methods {
                    let (
                        method_span,
                        method_name,
                        type_params,
                        params,
                        ret_ty,
                        method_where_clause,
                        body,
                        attrs,
                        is_builtin,
                    ) = match method {
                        Ast::Def(
                            method_span,
                            method_name,
                            type_params,
                            params,
                            ret_ty,
                            method_where_clause,
                            body,
                            attrs,
                        ) => (
                            method_span,
                            method_name,
                            type_params,
                            params,
                            ret_ty,
                            method_where_clause,
                            Some(body),
                            attrs,
                            false,
                        ),
                        Ast::BuiltinDecl(
                            method_span,
                            method_name,
                            params,
                            ret_ty,
                            method_where_clause,
                            attrs,
                        ) => (
                            method_span,
                            method_name,
                            Vec::new(),
                            params,
                            ret_ty,
                            method_where_clause,
                            None,
                            attrs,
                            true,
                        ),
                        _ => {
                            return Err(ResolveError {
                                message:
                                    "trait impl body may only contain `def` / `@builtin def` declarations"
                                        .to_string(),
                                span: span.clone(),
                                related_labels: Vec::new(),
                            });
                        }
                    };
                    let fun_params = attrs
                        .fun_params
                        .clone()
                        .into_iter()
                        .map(|ty| self.resolve_type_annotation(ty))
                        .collect::<Result<Vec<_>, _>>()?;
                    let qualified_function_name = trait_impl_method_qualified_name(
                        self.current_module_path.as_deref(),
                        &trait_name,
                        &trait_args,
                        &resolved_target_ty,
                        &method_name,
                        method_span.start,
                    );
                    let method_uid = self
                        .declaration_uids
                        .get(&qualified_function_name)
                        .copied()
                        .unwrap_or_else(|| self.scope.reserve_id());
                    let mut method_scope = self.scope.clone();
                    method_scope.define_with_id(&method_name, method_uid);
                    let mut method_resolver = Resolver::with_scope(method_scope);
                    method_resolver.declaration_uids = self.declaration_uids.clone();
                    method_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                    method_resolver.declaration_hidden_by_uid =
                        self.declaration_hidden_by_uid.clone();
                    method_resolver.owner_registry = self.owner_registry.clone();
                    method_resolver.current_module_path = self.current_module_path.clone();
                    method_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                    let resolved_params = params
                        .into_iter()
                        .map(|param| method_resolver.resolve_fun_param(param))
                        .collect::<Result<Vec<_>, ResolveError>>()?;
                    let resolved_body = if let Some(body) = body {
                        method_resolver.resolve_node(*body)?
                    } else {
                        Resolved::Lit(method_span.clone(), spire::ast::Lit::Unit)
                    };
                    self.scope
                        .advance_next_id_to(method_resolver.scope.next_id());
                    let local_function_name = if trait_args.is_empty() {
                        format!("{}::{}", target_key, method_name)
                    } else {
                        format!(
                            "{}::{}::{}",
                            trait_instance_key(&qualified_trait_name, &trait_args),
                            target_key,
                            method_name
                        )
                    };
                    let symbol_info = self.symbol_info_for_declaration(
                        &local_function_name,
                        &DeclarationKind::ImplMethod,
                        Some(target_owner_key),
                    );

                    resolved_methods.push(ResolvedTraitImplMethod {
                        method_name: method_name.clone(),
                        function_id: ResolvedId {
                            name: local_function_name,
                            qualified_name: Some(qualified_function_name),
                            unique_id: method_uid,
                            compiler_generated: false,
                            symbol_info,
                            span: method_span.clone(),
                        },
                        fun_params,
                        type_params: self.resolve_type_params(type_params)?,
                        params: resolved_params,
                        ret_ty: ret_ty
                            .map(|ty| self.resolve_type_annotation(ty))
                            .transpose()?,
                        where_clause: method_where_clause
                            .map(|clause| method_resolver.resolve_where_clause(clause))
                            .transpose()?,
                        body: Box::new(resolved_body),
                        attrs: resolve_decl_attrs(&attrs),
                        span: method_span,
                        is_builtin,
                    });
                }

                Ok(Resolved::TraitImplDef(
                    span,
                    trait_id,
                    trait_args
                        .into_iter()
                        .map(|arg| self.resolve_type_annotation(arg))
                        .collect::<Result<Vec<_>, _>>()?,
                    resolved_target_ty,
                    where_clause
                        .map(|clause| self.resolve_where_clause(clause))
                        .transpose()?,
                    resolved_methods,
                ))
            }

            Ast::BuiltinDecl(span, name, params, ret_ty, where_clause, attrs) => {
                let qualified_name = self.qualify_current_declaration_name(&name);
                let is_io_builtin =
                    sindr::builtin::builtin_meta_for_decl(&name, Some(&qualified_name)).is_some();
                if !is_runtime_builtin_decl(&name)
                    && !is_special_form_builtin_decl(&name)
                    && !is_io_builtin
                {
                    return Err(ResolveError {
                        message: format!("Unknown builtin declaration: {}", name),
                        span,
                        related_labels: Vec::new(),
                    });
                }

                let builtin_uid = self
                    .take_predeclared_id(&qualified_name)
                    .or_else(|| self.take_predeclared_id(&name))
                    .or_else(|| self.declaration_uids.get(&qualified_name).copied())
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                let mut decl_resolver = Resolver::with_scope(self.scope.clone());
                decl_resolver.declaration_uids = self.declaration_uids.clone();
                decl_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                decl_resolver.declaration_hidden_by_uid = self.declaration_hidden_by_uid.clone();
                decl_resolver.owner_registry = self.owner_registry.clone();
                decl_resolver.current_module_path = self.current_module_path.clone();
                decl_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                let resolved_params = params
                    .into_iter()
                    .map(|param| decl_resolver.resolve_fun_param(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                self.scope.advance_next_id_to(decl_resolver.scope.next_id());
                self.scope.define_with_id(&name, builtin_uid);
                let symbol_info = self.symbol_info_for_declaration(
                    &name,
                    &DeclarationKind::Def,
                    self.current_module_path.as_deref(),
                );
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: builtin_uid,
                    compiler_generated: false,
                    symbol_info,
                    span: span.clone(),
                };
                Ok(Resolved::BuiltinDecl(
                    span,
                    rid,
                    resolved_params,
                    ret_ty
                        .map(|ty| self.resolve_type_annotation(ty))
                        .transpose()?,
                    where_clause
                        .map(|clause| self.resolve_where_clause(clause))
                        .transpose()?,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::IntrinsicDecl(span, name, _, _) => Err(ResolveError {
                message: format!(
                    "Intrinsic declaration `{name}` is docs-only and should not reach resolution"
                ),
                span,
                related_labels: Vec::new(),
            }),
            Ast::BuiltinExtractorDecl(span, name, param, ret_ty, attrs) => {
                let qualified_name = self.qualify_current_declaration_name(&name);
                let uid = self
                    .take_predeclared_id(&qualified_name)
                    .or_else(|| self.take_predeclared_id(&name))
                    .or_else(|| self.declaration_uids.get(&qualified_name).copied())
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let symbol_info = self.symbol_info_for_declaration(
                    &name,
                    &DeclarationKind::Extractor,
                    self.current_module_path.as_deref(),
                );
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    compiler_generated: false,
                    symbol_info,
                    span: span.clone(),
                };
                let resolved_param = self.resolve_extractor_param(param)?;
                Ok(Resolved::BuiltinExtractorDecl(
                    span,
                    rid,
                    resolved_param,
                    self.resolve_type_annotation(ret_ty)?,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::BuiltinTypeDecl(span, head, attrs) => {
                let builtin_type_uid = self
                    .take_predeclared_id(&head.name)
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&head.name, builtin_type_uid);
                define_global_surface_alias(&mut self.scope, &head.name, builtin_type_uid);
                let qualified_name = self.qualify_current_declaration_name(&head.name);
                let symbol_info = self.symbol_info_for_declaration(
                    &head.name,
                    &DeclarationKind::BuiltinType,
                    None,
                );
                let rid = ResolvedId {
                    name: head.name,
                    qualified_name: Some(qualified_name),
                    unique_id: builtin_type_uid,
                    compiler_generated: false,
                    symbol_info,
                    span: span.clone(),
                };
                Ok(Resolved::BuiltinTypeDecl(
                    span,
                    rid,
                    head.params,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::TypeAlias(span, name, type_params, rhs) => {
                let symbol_info = self
                    .owner_registry
                    .owner_ref(&name)
                    .and_then(|owner| user_type_symbol_identity_info(&owner))
                    .expect("type aliases must have precollected owner metadata");
                Ok(Resolved::TypeAlias(
                    span,
                    name,
                    type_params
                        .into_iter()
                        .map(|param| ResolvedTypeParam {
                            name: param.name,
                            bound: param.bound,
                            span: param.span,
                        })
                        .collect(),
                    rhs,
                    symbol_info,
                ))
            }
            Ast::ResultCtorDecl(span, name, param_ty, ret_ty, attrs) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                define_global_surface_alias(&mut self.scope, &name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let result_owner = name
                    .rsplit_once("::")
                    .map(|(owner, _)| owner)
                    .unwrap_or("Result");
                let symbol_info = self.symbol_info_for_declaration(
                    &name,
                    &DeclarationKind::ResultCtor,
                    Some(result_owner),
                );
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    compiler_generated: false,
                    symbol_info,
                    span: span.clone(),
                };
                Ok(Resolved::ResultCtorDecl(
                    span,
                    rid,
                    param_ty,
                    ret_ty,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::Defmod(span, name, _, _) => Err(ResolveError {
                message: format!("Module resolution is not implemented yet: {}", name),
                span,
                related_labels: Vec::new(),
            }),
            Ast::Defagent(span, name, _, _, _)
            | Ast::Defgenserver(span, name, _, _, _)
            | Ast::Defsupervisor(span, name, _, _, _)
            | Ast::DefdynamicSupervisor(span, name, _, _, _) => Err(ResolveError {
                message: format!("Process module resolution is not implemented yet: {}", name),
                span,
                related_labels: Vec::new(),
            }),
            Ast::Import(span, _, _) => Err(ResolveError {
                message: "Import resolution is not implemented yet".to_string(),
                span,
                related_labels: Vec::new(),
            }),
            Ast::Include(span, _) => Err(ResolveError {
                message: "include directives must be resolved before name resolution".to_string(),
                span,
                related_labels: Vec::new(),
            }),
            Ast::ImplDef(span, target, _, _) => Err(ResolveError {
                message: format!("impl lowering failed for target `{}`", target),
                span,
                related_labels: Vec::new(),
            }),

            Ast::Closure(span, params, body) => {
                let mut closure_scope = self.scope.clone();
                let mut resolved_params = Vec::new();
                for param in params {
                    let uid = closure_scope.define(&param.name, param.span.clone());
                    resolved_params.push(ResolvedClosureParam {
                        id: ResolvedId {
                            name: param.name,
                            qualified_name: None,
                            unique_id: uid,
                            compiler_generated: false,
                            symbol_info: None,
                            span: param.span,
                        },
                        ty: param.ty,
                    });
                }

                let mut body_resolver = Resolver::with_scope(closure_scope);
                body_resolver.declaration_uids = self.declaration_uids.clone();
                body_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                body_resolver.declaration_hidden_by_uid = self.declaration_hidden_by_uid.clone();
                body_resolver.owner_registry = self.owner_registry.clone();
                body_resolver.current_module_path = self.current_module_path.clone();
                body_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                let resolved_body = body_resolver.resolve_node(*body)?;
                self.scope.advance_next_id_to(body_resolver.scope.next_id());

                let captures = collect_captures(&resolved_body, &resolved_params);

                Ok(Resolved::Closure(
                    span,
                    resolved_params,
                    captures,
                    Box::new(resolved_body),
                ))
            }

            Ast::Capture(span, target, args) => {
                match self.lower_capture_expr(span.clone(), *target, args)? {
                    Ast::Capture(_, target, args) => {
                        let resolved_target = self.resolve_node(*target)?;
                        let resolved_args = args
                            .into_iter()
                            .map(|arg| self.resolve_node(arg))
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok(Resolved::Capture(
                            span,
                            Box::new(resolved_target),
                            resolved_args,
                        ))
                    }
                    lowered => self.resolve_node(lowered),
                }
            }

            Ast::CapturePlaceholder(span, index) => Err(ResolveError {
                message: format!(
                    "capture placeholder &{} must appear inside a capture call",
                    index
                ),
                span,
                related_labels: Vec::new(),
            }),

            Ast::StructLit(span, type_name, field_vals) => {
                let uid = self.scope.lookup(&type_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined type: {}", type_name),
                    span: span.clone(),
                    related_labels: Vec::new(),
                })?;
                let symbol_info = self.symbol_info_for_uid(&type_name, uid);
                let rid = ResolvedId {
                    name: type_name,
                    qualified_name: None,
                    unique_id: uid,
                    compiler_generated: false,
                    symbol_info,
                    span: span.clone(),
                };
                let resolved_fields = field_vals
                    .into_iter()
                    .map(|field| match field {
                        StructLitField::Explicit(name, expr) => Ok(
                            ResolvedStructLitField::Explicit(name, self.resolve_node(expr)?),
                        ),
                        StructLitField::Shorthand(name) => Ok(ResolvedStructLitField::Shorthand(
                            name.clone(),
                            self.resolve_node(Ast::Var(span.clone(), name))?,
                        )),
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::StructLit(span, rid, resolved_fields))
            }

            Ast::InternalStructLit(span, type_name, field_vals) => {
                let uid = self.scope.lookup(&type_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined type: {}", type_name),
                    span: span.clone(),
                    related_labels: Vec::new(),
                })?;
                let symbol_info = self.symbol_info_for_uid(&type_name, uid);
                let rid = ResolvedId {
                    name: type_name,
                    qualified_name: None,
                    unique_id: uid,
                    compiler_generated: true,
                    symbol_info,
                    span: span.clone(),
                };
                let resolved_fields = field_vals
                    .into_iter()
                    .map(|field| match field {
                        StructLitField::Explicit(name, expr) => Ok(
                            ResolvedStructLitField::Explicit(name, self.resolve_node(expr)?),
                        ),
                        StructLitField::Shorthand(name) => Ok(ResolvedStructLitField::Shorthand(
                            name.clone(),
                            self.resolve_node(Ast::Var(span.clone(), name))?,
                        )),
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::StructLit(span, rid, resolved_fields))
            }

            Ast::ConstructorCall(span, type_name, args) => {
                let normalized_name = {
                    // In ExprBlock, a struct head like `User(...)` dispatches to
                    // `User::new(...)` when that constructor exists.
                    let sugared = format!("{}::new", type_name);
                    if self.scope.lookup(&sugared).is_some() {
                        sugared
                    } else {
                        type_name
                    }
                };
                if let Some(uid) = self.scope.lookup(&normalized_name) {
                    if self
                        .declaration_uid_kinds
                        .get(&uid)
                        .is_some_and(|kind| matches!(kind, DeclarationKind::Const))
                    {
                        let qualified_name = self.declaration_fq_name_for_uid(uid);
                        let symbol_info = self.symbol_info_for_uid(&normalized_name, uid);
                        let rid = ResolvedId {
                            name: normalized_name,
                            qualified_name,
                            unique_id: uid,
                            compiler_generated: false,
                            symbol_info,
                            span: span.clone(),
                        };
                        if args.is_empty() {
                            return Ok(Resolved::Var(span, rid));
                        }
                        let resolved_args = args
                            .into_iter()
                            .map(|arg| match arg {
                                spire::ast::RecordLitArg::Positional(e) => {
                                    Ok(ResolvedRecordLitArg::Positional(self.resolve_node(e)?))
                                }
                                spire::ast::RecordLitArg::Named(name, e) => {
                                    Ok(ResolvedRecordLitArg::Named(name, self.resolve_node(e)?))
                                }
                            })
                            .collect::<Result<Vec<_>, ResolveError>>()?;
                        return Ok(Resolved::App(
                            span.clone(),
                            Box::new(Resolved::Var(span, rid)),
                            resolved_args,
                        ));
                    }
                }
                let uid = self
                    .scope
                    .lookup(&normalized_name)
                    .ok_or_else(|| ResolveError {
                        message: format!("Undefined type: {}", normalized_name),
                        span: span.clone(),
                        related_labels: Vec::new(),
                    })?;
                let symbol_info = self.symbol_info_for_uid(&normalized_name, uid);
                let rid = ResolvedId {
                    name: normalized_name,
                    qualified_name: None,
                    unique_id: uid,
                    compiler_generated: false,
                    symbol_info,
                    span: span.clone(),
                };
                let resolved_args = args
                    .into_iter()
                    .map(|arg| match arg {
                        spire::ast::RecordLitArg::Positional(e) => {
                            Ok(ResolvedRecordLitArg::Positional(self.resolve_node(e)?))
                        }
                        spire::ast::RecordLitArg::Named(name, e) => {
                            Ok(ResolvedRecordLitArg::Named(name, self.resolve_node(e)?))
                        }
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::ConstructorCall(span, rid, resolved_args))
            }

            Ast::Match(span, scrutinee, arms) => {
                let resolved_scrut = self.resolve_node(*scrutinee)?;
                let resolved_arms = arms
                    .into_iter()
                    .map(|arm| self.resolve_match_arm(arm))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::Match(
                    span,
                    Box::new(resolved_scrut),
                    resolved_arms,
                ))
            }
            Ast::Namespace(span, _, _) => Err(ResolveError {
                message: "namespace declarations must be lowered before name resolution".into(),
                span,
                related_labels: Vec::new(),
            }),
            Ast::SupervisorInit(span, _) => Err(ResolveError {
                message: "supervisor_init must be collected before name resolution".into(),
                span,
                related_labels: Vec::new(),
            }),
        }
    }

    pub(super) fn resolve_fun_param(
        &mut self,
        param: FunParam,
    ) -> Result<ResolvedFunParam, ResolveError> {
        let uid = self.scope.define(&param.name, param.span.clone());
        Ok(ResolvedFunParam {
            id: ResolvedId {
                name: param.name,
                qualified_name: None,
                unique_id: uid,
                compiler_generated: false,
                symbol_info: None,
                span: param.span,
            },
            ty: self.resolve_type_annotation(param.ty)?,
        })
    }

    pub(super) fn resolve_type_params(
        &self,
        type_params: Vec<spire::ast::TypeParam>,
    ) -> Result<Vec<ResolvedTypeParam>, ResolveError> {
        type_params
            .into_iter()
            .map(|param| self.resolve_type_param(param))
            .collect()
    }

    pub(super) fn resolve_extractor_param(
        &mut self,
        param: ExtractorParam,
    ) -> Result<ResolvedExtractorParam, ResolveError> {
        let uid = self.scope.define(&param.name, param.span.clone());
        Ok(ResolvedExtractorParam {
            id: ResolvedId {
                name: param.name,
                qualified_name: None,
                unique_id: uid,
                compiler_generated: false,
                symbol_info: None,
                span: param.span,
            },
            ty: param
                .ty
                .map(|ty| self.resolve_type_annotation(ty))
                .transpose()?,
        })
    }

    fn resolve_type_param(
        &self,
        param: spire::ast::TypeParam,
    ) -> Result<ResolvedTypeParam, ResolveError> {
        Ok(ResolvedTypeParam {
            name: param.name,
            bound: param
                .bound
                .map(|bound| self.resolve_trait_bound_name(&bound, &param.span))
                .transpose()?,
            span: param.span,
        })
    }

    fn resolve_type_annotation(&self, ty: AstTy) -> Result<AstTy, ResolveError> {
        match ty {
            AstTy::Named(span, name) => Ok(AstTy::Named(span, name)),
            AstTy::ImplTrait(span, name) => Ok(AstTy::ImplTrait(
                span.clone(),
                self.resolve_trait_bound_name(&name, &span)?,
            )),
            AstTy::Generic(span, name, args) => Ok(AstTy::Generic(
                span,
                name,
                args.into_iter()
                    .map(|arg| self.resolve_type_annotation(arg))
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            AstTy::Tuple(span, items) => Ok(AstTy::Tuple(
                span,
                items
                    .into_iter()
                    .map(|item| self.resolve_type_annotation(item))
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            AstTy::Func(span, params, ret) => Ok(AstTy::Func(
                span,
                params
                    .into_iter()
                    .map(|param| self.resolve_type_annotation(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?,
                Box::new(self.resolve_type_annotation(*ret)?),
            )),
        }
    }

    fn resolve_where_clause(
        &self,
        clause: spire::ast::WhereClause,
    ) -> Result<ResolvedWhereClause, ResolveError> {
        let constraints = clause
            .constraints
            .into_iter()
            .map(|constraint| {
                let subject = self.resolve_type_annotation(constraint.subject)?;
                let bounds = constraint
                    .bounds
                    .into_iter()
                    .map(|bound| match bound {
                        spire::ast::WhereConstraintRhs::Trait(span, name) => {
                            let (unique_id, qualified_name) =
                                self.resolve_trait_reference(&name, &span)?;
                            let symbol_info = self.symbol_info_for_declaration(
                                &name,
                                &DeclarationKind::Trait,
                                None,
                            );
                            Ok(ResolvedWhereConstraintRhs::Trait {
                                trait_id: ResolvedId {
                                    name,
                                    qualified_name: Some(qualified_name),
                                    unique_id,
                                    compiler_generated: false,
                                    symbol_info,
                                    span,
                                },
                            })
                        }
                        spire::ast::WhereConstraintRhs::TypeConstructor(span, slots) => {
                            Ok(ResolvedWhereConstraintRhs::TypeConstructor {
                                span,
                                slots: slots
                                    .into_iter()
                                    .map(|slot| self.resolve_type_annotation(slot))
                                    .collect::<Result<Vec<_>, _>>()?,
                            })
                        }
                        spire::ast::WhereConstraintRhs::TraitSlot(span, owner, slot_name) => {
                            let (unique_id, qualified_name) =
                                self.resolve_trait_reference(&owner, &span)?;
                            let symbol_info = self.symbol_info_for_declaration(
                                &owner,
                                &DeclarationKind::Trait,
                                None,
                            );
                            if symbol_info.as_ref().map(|info| info.identity)
                                != Some(TypeIdentity::TypeConstructor)
                            {
                                return Err(ResolveError {
                                    message: format!(
                                        "Trait {} is not a TypeConstructor trait",
                                        owner
                                    ),
                                    span,
                                    related_labels: Vec::new(),
                                });
                            }
                            let slot_ordinal = self
                                .trait_constructor_slots
                                .get(&unique_id)
                                .and_then(|slots| slots.iter().position(|slot| slot == &slot_name))
                                .ok_or_else(|| ResolveError {
                                    message: format!(
                                        "Trait {} has no constructor slot {}",
                                        owner, slot_name
                                    ),
                                    span: span.clone(),
                                    related_labels: Vec::new(),
                                })? as u32;
                            Ok(ResolvedWhereConstraintRhs::TraitSlot {
                                trait_id: ResolvedId {
                                    name: owner,
                                    qualified_name: Some(qualified_name),
                                    unique_id,
                                    compiler_generated: false,
                                    symbol_info,
                                    span: span.clone(),
                                },
                                slot_name,
                                slot_ordinal,
                                span,
                            })
                        }
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(ResolvedWhereConstraint {
                    subject,
                    bounds,
                    span: constraint.span,
                })
            })
            .collect::<Result<Vec<_>, ResolveError>>()?;
        Ok(ResolvedWhereClause {
            constraints,
            span: clause.span,
        })
    }

    fn resolve_trait_reference(
        &self,
        trait_name: &str,
        span: &Span,
    ) -> Result<(u32, String), ResolveError> {
        let trait_uid = self.scope.lookup(trait_name).ok_or_else(|| ResolveError {
            message: format!("Undefined trait: {}", trait_name),
            span: span.clone(),
            related_labels: Vec::new(),
        })?;
        match self.declaration_uid_kinds.get(&trait_uid) {
            Some(DeclarationKind::Trait) => {}
            _ => {
                return Err(ResolveError {
                    message: format!("{} is not a trait", trait_name),
                    span: span.clone(),
                    related_labels: Vec::new(),
                });
            }
        }
        let qualified_name = self
            .declaration_fq_name_for_uid(trait_uid)
            .unwrap_or_else(|| trait_name.to_string());
        Ok((trait_uid, qualified_name))
    }

    fn resolve_trait_bound_name(
        &self,
        trait_name: &str,
        span: &Span,
    ) -> Result<String, ResolveError> {
        self.resolve_trait_reference(trait_name, span)
            .map(|(_, qualified_name)| qualified_name)
    }
}

pub(super) fn validate_trait_impl_pairs_in_nodes(
    resolved: &[Resolved],
) -> Result<(), ResolveError> {
    let mut seen_pairs: HashMap<String, (Span, bool)> = HashMap::new();
    for node in resolved {
        let Resolved::TraitImplDef(span, trait_id, trait_args, target_ty, _, _) = node else {
            continue;
        };
        let trait_name = trait_instance_key(
            trait_id.qualified_name.as_deref().unwrap_or(&trait_id.name),
            trait_args,
        );
        let pair_key = format!("{} for {}", trait_name, ast_ty_key(target_ty));
        if let Some((first_span, first_generated)) = seen_pairs.get(&pair_key) {
            return Err(ResolveError {
                message: if *first_generated || trait_id.compiler_generated {
                    format!("DerivedImplConflict: multiple trait impl blocks for `{pair_key}`")
                } else {
                    format!("Multiple trait impl blocks for `{pair_key}` are not allowed")
                },
                span: span.clone(),
                related_labels: vec![
                    ResolveErrorLabel {
                        span: first_span.clone(),
                        message: "first definition".to_string(),
                        source: None,
                    },
                    ResolveErrorLabel {
                        span: span.clone(),
                        message: "conflicting definition".to_string(),
                        source: None,
                    },
                ],
            });
        } else {
            seen_pairs.insert(
                pair_key.clone(),
                (span.clone(), trait_id.compiler_generated),
            );
        }
    }
    Ok(())
}
