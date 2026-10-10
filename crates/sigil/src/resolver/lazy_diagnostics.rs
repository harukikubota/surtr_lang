//! Diagnostic provenance for canonical Lazy captures, without a new resolution path.
//! Classify placeholders by their nearest call and declaration-selected Pattern roles.
//! Optional metadata lookup failures preserve ordinary resolver errors and precedence.
//! Bare special-form captures are rejected instead of producing an implicit wrapper.

use super::expr::{visit_pattern_expressions, CanonicalSpecialForm};
use super::special_forms::{IfKind, LogicKind};
use super::*;
use sindr::pattern::PatternConsumer;
use spire::ast::{
    BulkUpdateEntry, BulkUpdateEntryKind, BulkUpdatePath, FacetPathSegment, InterpolatedPart,
};

pub(super) struct LazyCaptureSource {
    kind: LazyCaptureKind,
    uses: Vec<(usize, u32, Span, bool)>,
}

impl Resolver {
    pub(super) fn lazy_capture_source(
        &mut self,
        target: &Ast,
        args: &[Ast],
    ) -> Result<Option<LazyCaptureSource>, ResolveError> {
        let (kind, arguments, lazy_ordinals) =
            if let Ast::PatternConsumerCall(_, callee, arguments) = target {
                let kind = match self.resolve_pattern_consumer_identity(callee).ok() {
                    Some(PatternConsumer::IfLet) => LazyCaptureKind::IfLet,
                    Some(PatternConsumer::IfLetThen) => LazyCaptureKind::IfLetThen,
                    _ => return Ok(None),
                };
                let Some(pattern) = arguments.get(1).and_then(|arg| arg.pattern.as_deref()) else {
                    return Ok(None);
                };
                // Optional diagnostics follow the declaration-selected roles;
                // an invalid Pattern keeps the normal resolver's error order.
                let Ok(pattern) = self.select_pattern_argument_roles(pattern.clone()) else {
                    return Ok(None);
                };
                let binding = Self::pattern_expression_roles_selected(&pattern)
                    .then(|| super::special_forms::pattern_has_binding_vars(&pattern));
                let lazy = if binding == Some(false) {
                    vec![2, 3]
                } else {
                    vec![3]
                };
                (
                    kind,
                    arguments
                        .iter()
                        .enumerate()
                        .filter_map(|(ordinal, arg)| {
                            if ordinal == 2 && binding.is_none() {
                                return None;
                            }
                            arg.expression.as_deref().map(|expr| (ordinal as u32, expr))
                        })
                        .collect::<Vec<_>>(),
                    lazy,
                )
            } else {
                let resolved = match target {
                    Ast::Var(span, name) => {
                        self.resolve_var_like(span.clone(), name.clone(), false)
                    }
                    Ast::Path(span, path) => {
                        self.resolve_var_like(span.clone(), path.segments.join("::"), false)
                    }
                    _ => return Ok(None),
                };
                // Diagnostic metadata must not change ordinary validation precedence.
                // Invalid targets are diagnosed by the existing capture resolver.
                let Ok(resolved) = resolved else {
                    return Ok(None);
                };
                let (kind, lazy) = match self.classify_canonical_special_form_callee(&resolved) {
                    Some(CanonicalSpecialForm::Logic(LogicKind::And)) => {
                        (LazyCaptureKind::And, vec![1])
                    }
                    Some(CanonicalSpecialForm::Logic(LogicKind::Or)) => {
                        (LazyCaptureKind::Or, vec![1])
                    }
                    Some(CanonicalSpecialForm::If(IfKind::If3)) => {
                        (LazyCaptureKind::If, vec![1, 2])
                    }
                    Some(CanonicalSpecialForm::If(IfKind::IfThen2)) => {
                        (LazyCaptureKind::IfThen, vec![1])
                    }
                    Some(CanonicalSpecialForm::Require) => (LazyCaptureKind::Require, vec![1]),
                    Some(CanonicalSpecialForm::Ensure) => (LazyCaptureKind::Ensure, vec![2]),
                    Some(CanonicalSpecialForm::MapErr) => (LazyCaptureKind::MapErr, vec![1]),
                    Some(CanonicalSpecialForm::Cause) => (LazyCaptureKind::Cause, vec![1]),
                    _ => return Ok(None),
                };
                (
                    kind,
                    args.iter()
                        .enumerate()
                        .map(|(i, arg)| (i as u32, arg))
                        .collect(),
                    lazy,
                )
            };
        fn direct(expr: &Ast) -> Option<(usize, Span)> {
            match expr {
                Ast::CapturePlaceholder(span, index) => Some((*index, span.clone())),
                Ast::Grouped(_, inner) => direct(inner),
                _ => None,
            }
        }
        let mut uses = Vec::new();
        for (ordinal, expr) in arguments {
            if let Some((index, span)) = direct(expr) {
                uses.push((index, ordinal, span, lazy_ordinals.contains(&ordinal)));
            } else {
                let mut ordinary = HashSet::new();
                self.collect_ordinary_capture_uses(expr, &mut ordinary)?;
                uses.extend(
                    ordinary
                        .into_iter()
                        .map(|index| (index, ordinal, expr.span().clone(), false)),
                );
            }
        }
        Ok(Some(LazyCaptureSource { kind, uses }))
    }

    fn pattern_expression_roles_selected(pattern: &AstPattern) -> bool {
        match pattern {
            AstPattern::HashMap(_, entries) => entries
                .iter()
                .all(|(_, child)| Self::pattern_expression_roles_selected(child)),
            AstPattern::Call(_, _, arguments) => arguments.iter().all(|argument| {
                !(argument.expression.is_some() && argument.pattern.is_some())
                    && argument
                        .pattern
                        .as_deref()
                        .is_none_or(Self::pattern_expression_roles_selected)
            }),
            AstPattern::Projection { inner, .. } | AstPattern::As(_, inner, ..) => {
                Self::pattern_expression_roles_selected(inner)
            }
            AstPattern::Constructor(_, _, items)
            | AstPattern::Tuple(_, items)
            | AstPattern::Or(_, items) => items.iter().all(Self::pattern_expression_roles_selected),
            AstPattern::ListCons(_, head, tail) => {
                Self::pattern_expression_roles_selected(head)
                    && Self::pattern_expression_roles_selected(tail)
            }
            _ => true,
        }
    }

    fn collect_ordinary_capture_uses(
        &mut self,
        expr: &Ast,
        used: &mut HashSet<usize>,
    ) -> Result<(), ResolveError> {
        match expr {
            Ast::CapturePlaceholder(_, index) => {
                used.insert(*index);
            }
            Ast::App(_, callee, arguments) => {
                let args = arguments
                    .iter()
                    .map(|arg| match arg {
                        RecordLitArg::Positional(expr) | RecordLitArg::Named(_, expr) => {
                            expr.clone()
                        }
                    })
                    .collect::<Vec<_>>();
                if let Some(source) = self.lazy_capture_source(callee, &args)? {
                    used.extend(
                        source
                            .uses
                            .into_iter()
                            .filter(|(_, _, _, lazy)| !lazy)
                            .map(|(index, ..)| index),
                    );
                } else {
                    self.collect_ordinary_capture_uses(callee, used)?;
                    for arg in &args {
                        self.collect_ordinary_capture_uses(arg, used)?;
                    }
                }
            }
            Ast::PatternConsumerCall(_, callee, arguments) => {
                let kind = self.resolve_pattern_consumer_identity(callee).ok();
                if let Some(source) = self.lazy_capture_source(expr, &[])? {
                    used.extend(
                        source
                            .uses
                            .into_iter()
                            .filter(|(_, _, _, lazy)| !lazy)
                            .map(|(index, ..)| index),
                    );
                } else {
                    // The canonical consumer selects exactly one Pattern slot;
                    // ordinary builtins use ordinary call syntax.
                    for (index, argument) in arguments.iter().enumerate() {
                        if kind.is_some_and(|kind| index == kind.pattern_index()) {
                            continue;
                        }
                        if let Some(expr) = argument.expression.as_deref() {
                            self.collect_ordinary_capture_uses(expr, used)?;
                        }
                    }
                }
                if let Some(argument) = kind.and_then(|kind| arguments.get(kind.pattern_index())) {
                    if let Some(pattern) = argument.pattern.as_deref() {
                        // Use declaration-selected pre-argument Expr roles; never
                        // visit both parser candidates or reparse Pattern text.
                        if let Ok(pattern) = self.select_pattern_argument_roles(pattern.clone()) {
                            if !Self::pattern_expression_roles_selected(&pattern) {
                                return Ok(());
                            }
                            visit_pattern_expressions(&pattern, &mut |expr| {
                                self.collect_ordinary_capture_uses(expr, used)
                            })?;
                        }
                    }
                }
            }
            Ast::BinOp(_, _, left, right)
            | Ast::Pipe(_, left, right)
            | Ast::ContextMap(_, left, right)
            | Ast::ContextApply(_, left, right)
            | Ast::ContextBind(_, left, right)
            | Ast::Compose(_, left, right)
            | Ast::LiftedCompose(_, left, right)
            | Ast::KleisliCompose(_, left, right)
            | Ast::ListCons(_, left, right)
            | Ast::RangeLiteral(_, left, right) => {
                self.collect_ordinary_capture_uses(left, used)?;
                self.collect_ordinary_capture_uses(right, used)?;
            }
            Ast::Grouped(_, inner)
            | Ast::Semi(_, inner)
            | Ast::Bind(_, _, inner)
            | Ast::SafeBind(_, _, inner, _)
            | Ast::StatementQuestion(_, inner)
            | Ast::FieldAccess(_, inner, _)
            | Ast::FacetCapture(_, inner)
            | Ast::ReturnTypeArgumentApply(_, inner, _) => {
                self.collect_ordinary_capture_uses(inner, used)?
            }
            Ast::FacetSegmentAccess(_, inner, segment) => {
                self.collect_ordinary_capture_uses(inner, used)?;
                if let FacetPathSegment::Bracket(bracket) = segment {
                    self.collect_ordinary_capture_uses(&bracket.expr, used)?;
                }
            }
            Ast::ConstructorCall(_, _, arguments)
            | Ast::EnumConstructorCall(_, _, _, _, arguments) => {
                for arg in arguments {
                    let (RecordLitArg::Positional(expr) | RecordLitArg::Named(_, expr)) = arg;
                    self.collect_ordinary_capture_uses(expr, used)?;
                }
            }
            Ast::StructLit(_, _, fields) | Ast::InternalStructLit(_, _, fields) => {
                for field in fields {
                    if let StructLitField::Explicit(_, expr) = field {
                        self.collect_ordinary_capture_uses(expr, used)?;
                    }
                }
            }
            Ast::HashMapLiteral(_, entries) => {
                for entry in entries {
                    self.collect_ordinary_capture_uses(&entry.key, used)?;
                    self.collect_ordinary_capture_uses(&entry.value, used)?;
                }
            }
            Ast::InterpolatedStr(_, parts) => {
                for part in parts {
                    if let InterpolatedPart::Expr(expr) = part {
                        self.collect_ordinary_capture_uses(expr, used)?;
                    }
                }
            }
            Ast::Dbg(_, args) => {
                for arg in args {
                    self.collect_ordinary_capture_uses(&arg.expr, used)?;
                }
            }
            Ast::Do(_, _, statements, _) => {
                for statement in statements {
                    let expr = match statement {
                        AstDoStatement::Extract { rhs, .. }
                        | AstDoStatement::SafeBind { rhs, .. } => rhs,
                        AstDoStatement::Statement(expr) => expr,
                    };
                    self.collect_ordinary_capture_uses(expr, used)?;
                }
            }
            Ast::BulkUpdate(_, source, entries) => {
                self.collect_ordinary_capture_uses(source, used)?;
                self.collect_ordinary_bulk_entries(entries, used)?;
            }
            Ast::ListLiteral(_, items) | Ast::TupleLiteral(_, items) | Ast::Block(_, items) => {
                for item in items {
                    self.collect_ordinary_capture_uses(item, used)?;
                }
            }
            Ast::Cond(_, clauses) => {
                for (condition, body) in clauses {
                    self.collect_ordinary_capture_uses(condition, used)?;
                    self.collect_ordinary_capture_uses(body, used)?;
                }
            }
            Ast::Match(_, value, arms) => {
                self.collect_ordinary_capture_uses(value, used)?;
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.collect_ordinary_capture_uses(guard, used)?;
                    }
                    self.collect_ordinary_capture_uses(&arm.body, used)?;
                }
            }
            // Literal closures and nested captures have their own scope. The
            // ordinary capture validator retains their existing restrictions.
            Ast::Capture(..)
            | Ast::Closure(..)
            | Ast::ExtractorClosure(..)
            | Ast::NumberedPlaceholder(..)
            | Ast::Lit(..)
            | Ast::Var(..)
            | Ast::InternalVar(..)
            | Ast::Path(..)
            | Ast::FuncLiteralRef(..)
            | Ast::NamedInfixRef(..)
            | Ast::ListNil(..)
            | Ast::StructDef(..)
            | Ast::RecordDef(..)
            | Ast::DeferrorDef(..)
            | Ast::EnumDef(..)
            | Ast::Def(..)
            | Ast::ConstDef(..)
            | Ast::SupervisorInit(..)
            | Ast::ExtractorDef(..)
            | Ast::BuiltinDecl(..)
            | Ast::IntrinsicDecl(..)
            | Ast::BuiltinExtractorDecl(..)
            | Ast::BuiltinTypeDecl(..)
            | Ast::TypeAlias(..)
            | Ast::Defmod(..)
            | Ast::Defagent(..)
            | Ast::Defgenserver(..)
            | Ast::Defsupervisor(..)
            | Ast::DefdynamicSupervisor(..)
            | Ast::Namespace(..)
            | Ast::ImplDef(..)
            | Ast::TraitDef(..)
            | Ast::TraitImplDef(..)
            | Ast::Import(..)
            | Ast::Include(..) => {}
        }
        Ok(())
    }

    fn collect_ordinary_bulk_entries(
        &mut self,
        entries: &[BulkUpdateEntry],
        used: &mut HashSet<usize>,
    ) -> Result<(), ResolveError> {
        for entry in entries {
            self.collect_ordinary_bulk_path(&entry.path, used)?;
            match &entry.kind {
                BulkUpdateEntryKind::Set(expr)
                | BulkUpdateEntryKind::Over(expr)
                | BulkUpdateEntryKind::OverResult(expr)
                | BulkUpdateEntryKind::CaseSet(expr)
                | BulkUpdateEntryKind::CaseOver(expr) => {
                    self.collect_ordinary_capture_uses(expr, used)?
                }
                BulkUpdateEntryKind::Nested(entries) => {
                    self.collect_ordinary_bulk_entries(entries, used)?
                }
            }
        }
        Ok(())
    }

    fn collect_ordinary_bulk_path(
        &mut self,
        path: &BulkUpdatePath,
        used: &mut HashSet<usize>,
    ) -> Result<(), ResolveError> {
        match path {
            BulkUpdatePath::Segments(_, segments) => {
                for segment in segments {
                    if let FacetPathSegment::Bracket(bracket) = segment {
                        self.collect_ordinary_capture_uses(&bracket.expr, used)?;
                    }
                }
            }
            BulkUpdatePath::Chain(_, left, right) => {
                self.collect_ordinary_bulk_path(left, used)?;
                self.collect_ordinary_bulk_path(right, used)?;
            }
            BulkUpdatePath::StripLeft(_, inner, _) | BulkUpdatePath::StripRight(_, inner, _) => {
                self.collect_ordinary_bulk_path(inner, used)?
            }
            BulkUpdatePath::Pin(..) => {}
        }
        Ok(())
    }

    pub(super) fn reject_bare_lazy_capture(
        &self,
        target: &Resolved,
        span: &Span,
    ) -> Result<(), ResolveError> {
        let (name, help) = match self.classify_canonical_special_form_callee(target) {
            Some(CanonicalSpecialForm::Logic(LogicKind::And)) => ("and", "Use explicit placeholders: &and(&1, &2). Its right operand becomes (-> Boolean) and runs only when the left operand is True."),
            Some(CanonicalSpecialForm::Logic(LogicKind::Or)) => ("or", "Use explicit placeholders: &or(&1, &2). Its right operand becomes (-> Boolean) and runs only when the left operand is False."),
            Some(CanonicalSpecialForm::If(IfKind::If3)) => ("if", "Use explicit placeholders, such as &if(&1, &2, 0), or give &if(&1, &2, &3) a concrete callable annotation. Branch placeholders require normalized zero-argument function types."),
            Some(CanonicalSpecialForm::If(IfKind::IfThen2)) => ("if_then", "Use explicit placeholders: &if_then(&1, &2). Its branch becomes (-> Unit) and runs only when the condition is True."),
            Some(CanonicalSpecialForm::Require) => ("require", "Use an explicit capture with the error fixed inside it, such as &require(&1, NoneError). An error placeholder accepts a normal (-> Error) callable."),
            Some(CanonicalSpecialForm::Ensure) => ("ensure", "Use an explicit capture with the error fixed inside it, such as `f: (Int -> Result<Int>) = &ensure(&1, {|value| True}, NoneError)`. An error placeholder accepts a normal (-> Error) callable."),
            Some(CanonicalSpecialForm::MapErr) => ("Result::map_err", "Use an explicit capture with the replacement error fixed inside it, such as `f: (Result<Int> -> Result<Int>) = &Result::map_err(&1, NoneError)`. The replacement runs only for Err; an error placeholder accepts a normal (-> Error) callable."),
            Some(CanonicalSpecialForm::Cause) => ("Result::cause", "Use an explicit capture with the cause error fixed inside it, such as `f: (Result<Int> -> Result<Int>) = &Result::cause(&1, NoneError)`. The cause runs only for Err; an error placeholder accepts a normal (-> Error) callable."),
            _ => return Ok(()),
        };
        Err(ResolveError {
            message: format!("{name} has Lazy arguments and cannot be captured without explicit arguments. {help}"),
            span: span.clone(),
            diagnostic: crate::error::ResolveErrorDiagnostic { reason: crate::error::ResolveErrorReason::Capture, subject: Some(name.into()) },
            related_labels: Vec::new(),
        })
    }

    pub(super) fn annotate_lazy_capture_params(
        mut params: Vec<ResolvedClosureParam>,
        source: Option<&LazyCaptureSource>,
    ) -> Vec<ResolvedClosureParam> {
        if let Some(source) = source {
            for (index, param) in params.iter_mut().enumerate() {
                let uses = source
                    .uses
                    .iter()
                    .filter(|(placeholder, ..)| *placeholder == index + 1)
                    .collect::<Vec<_>>();
                param.lazy_capture = Some(ResolvedLazyCaptureParam {
                    kind: source.kind,
                    ordinary: uses.iter().any(|(_, _, _, lazy)| !lazy),
                    lazy_uses: uses
                        .into_iter()
                        .filter(|(_, _, _, lazy)| *lazy)
                        .map(|(_, ordinal, span, _)| (*ordinal, span.clone()))
                        .collect(),
                });
            }
        }
        params
    }
}
