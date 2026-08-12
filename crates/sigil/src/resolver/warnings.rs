use std::collections::HashSet;

use sindr::warning::{CompilerWarning, WarningKind, WarningPhase, WarningSpan};
use spire::ast::Span;

use super::{DeclarationKind, ExplicitFunctionImport};
use crate::resolved::{
    Resolved, ResolvedHashMapLiteralEntry, ResolvedId, ResolvedInterpolatedPart, ResolvedMatchArm,
    ResolvedPattern, ResolvedRecordLitArg, ResolvedStructLitField,
};

#[derive(Debug, Clone)]
struct VariableBinding {
    uid: u32,
    name: String,
    span: Span,
}

#[derive(Debug, Default)]
struct WarningUsage {
    bindings: Vec<VariableBinding>,
    seen_bindings: HashSet<u32>,
    used_uids: HashSet<u32>,
    short_import_uses: HashSet<(u32, String)>,
}

impl WarningUsage {
    fn bind_id(&mut self, id: &ResolvedId) {
        if id.compiler_generated || id.name == "_" || !self.seen_bindings.insert(id.unique_id) {
            return;
        }
        self.bindings.push(VariableBinding {
            uid: id.unique_id,
            name: id.name.clone(),
            span: id.span.clone(),
        });
    }

    fn use_id(&mut self, id: &ResolvedId) {
        if id.compiler_generated {
            return;
        }
        self.used_uids.insert(id.unique_id);
        self.short_import_uses
            .insert((id.unique_id, id.name.clone()));
    }
}

pub(super) fn collect_resolution_warnings(
    resolved: &[Resolved],
    explicit_function_imports: &[ExplicitFunctionImport],
) -> Vec<CompilerWarning> {
    let mut usage = WarningUsage::default();
    for node in resolved {
        collect_node_usage(node, &mut usage);
    }

    let mut warnings = Vec::new();
    for binding in &usage.bindings {
        if usage.used_uids.contains(&binding.uid) {
            continue;
        }
        warnings.push(CompilerWarning::new(
            WarningKind::UnusedVariable,
            WarningPhase::Resolve,
            warning_span(&binding.span),
            format!("Unused variable `{}`", binding.name),
            Some("Use the binding or replace it with `_`.".to_string()),
        ));
    }

    for import in explicit_function_imports {
        if !is_function_import_kind(&import.kind) {
            continue;
        }
        if usage
            .short_import_uses
            .contains(&(import.uid, import.alias.clone()))
        {
            continue;
        }
        warnings.push(CompilerWarning::new(
            WarningKind::UnusedImportFunction,
            WarningPhase::Resolve,
            warning_span(&import.span),
            format!("Unused function import `{}`", import.fq_name),
            Some(format!(
                "Remove `import {}` or call `{}` unqualified.",
                import.fq_name, import.alias
            )),
        ));
    }

    warnings
}

fn is_function_import_kind(kind: &DeclarationKind) -> bool {
    matches!(
        kind,
        DeclarationKind::Def
            | DeclarationKind::Extractor
            | DeclarationKind::TraitMethod
            | DeclarationKind::ImplMethod
    )
}

fn warning_span(span: &Span) -> WarningSpan {
    WarningSpan {
        start: span.start,
        end: span.end,
    }
}

fn collect_node_usage(node: &Resolved, usage: &mut WarningUsage) {
    match node {
        Resolved::Lit(..)
        | Resolved::ListNil(_)
        | Resolved::InferredFacetCapture(_, _)
        | Resolved::ProcessContextHandler(_, _)
        | Resolved::BuiltinTypeDecl(..)
        | Resolved::ResultCtorDecl(..) => {}
        Resolved::Var(_, id) => usage.use_id(id),
        Resolved::App(_, func, args) => {
            collect_node_usage(func, usage);
            for arg in args {
                collect_record_arg_usage(arg, usage);
            }
        }
        Resolved::TypeApply(_, target, _) => collect_node_usage(target, usage),
        Resolved::Block(_, nodes)
        | Resolved::ListLiteral(_, nodes)
        | Resolved::TupleLiteral(_, nodes)
        | Resolved::Dbg(_, nodes) => {
            for node in nodes {
                collect_node_usage(node, usage);
            }
        }
        Resolved::HashMapLiteral(_, entries) => {
            for ResolvedHashMapLiteralEntry { key, value } in entries {
                collect_node_usage(key, usage);
                collect_node_usage(value, usage);
            }
        }
        Resolved::Bind(_, pattern, rhs) | Resolved::SafeBind(_, pattern, rhs) => {
            collect_node_usage(rhs, usage);
            collect_pattern_usage(pattern, usage);
        }
        Resolved::BinOp(_, _, left, right)
        | Resolved::Pipe(_, left, right)
        | Resolved::ContextMap(_, left, right)
        | Resolved::ContextApply(_, left, right)
        | Resolved::ContextBind(_, left, right)
        | Resolved::Compose(_, left, right)
        | Resolved::LiftedCompose(_, left, right)
        | Resolved::KleisliCompose(_, left, right)
        | Resolved::ListCons(_, left, right) => {
            collect_node_usage(left, usage);
            collect_node_usage(right, usage);
        }
        Resolved::RangeLiteral(_, start, stop) => {
            collect_node_usage(start, usage);
            collect_node_usage(stop, usage);
        }
        Resolved::Grouped(_, inner)
        | Resolved::FieldAccess(_, inner, _)
        | Resolved::FacetSegmentAccess(_, inner, _)
        | Resolved::FacetCapture(_, inner)
        | Resolved::Semi(_, inner) => collect_node_usage(inner, usage),
        Resolved::InterpolatedStr(_, parts) => {
            for part in parts {
                if let ResolvedInterpolatedPart::Expr(expr) = part {
                    collect_node_usage(expr, usage);
                }
            }
        }
        Resolved::If(_, cond, then_branch, else_branch) => {
            collect_node_usage(cond, usage);
            collect_node_usage(then_branch, usage);
            if let Some(else_branch) = else_branch {
                collect_node_usage(else_branch, usage);
            }
        }
        Resolved::Assert(_, flag, err) => {
            collect_node_usage(flag, usage);
            collect_node_usage(err, usage);
        }
        Resolved::Ensure(_, value, pred, err) => {
            collect_node_usage(value, usage);
            collect_node_usage(pred, usage);
            collect_node_usage(err, usage);
        }
        Resolved::MapErr(_, value, err) | Resolved::Cause(_, value, err) => {
            collect_node_usage(value, usage);
            collect_node_usage(err, usage);
        }
        Resolved::RecoverKind(_, value, marker, handler) => {
            collect_node_usage(value, usage);
            collect_node_usage(marker, usage);
            collect_node_usage(handler, usage);
        }
        Resolved::Match(_, scrutinee, arms) => {
            collect_node_usage(scrutinee, usage);
            for ResolvedMatchArm {
                pattern,
                guard,
                body,
            } in arms
            {
                collect_pattern_usage(pattern, usage);
                if let Some(guard) = guard {
                    collect_node_usage(guard, usage);
                }
                collect_node_usage(body, usage);
            }
        }
        Resolved::StructLit(_, _, fields) => {
            for field in fields {
                match field {
                    ResolvedStructLitField::Explicit(_, expr)
                    | ResolvedStructLitField::Shorthand(_, expr) => {
                        collect_node_usage(expr, usage);
                    }
                }
            }
        }
        Resolved::ConstructorCall(_, _, args) => {
            for arg in args {
                collect_record_arg_usage(arg, usage);
            }
        }
        Resolved::Capture(_, target, args) => {
            collect_node_usage(target, usage);
            for arg in args {
                collect_node_usage(arg, usage);
            }
        }
        Resolved::StructDef(_, _, _, _, _) | Resolved::RecordDef(_, _, _, _) => {}
        Resolved::DeferrorDef(_, _, _, show_expr) => collect_node_usage(show_expr, usage),
        Resolved::EnumDef(_, _, _, _, _) => {}
        Resolved::Def(_, _, _, params, _, _, body, _) => {
            for param in params {
                usage.bind_id(&param.id);
            }
            collect_node_usage(body, usage);
        }
        Resolved::ConstDef(_, _, _, value, _) => collect_node_usage(value, usage),
        Resolved::ExtractorDef(_, _, _, param, _, body, _) => {
            usage.bind_id(&param.id);
            collect_node_usage(body, usage);
        }
        Resolved::TraitDef(_, _, _, _, methods, _) => {
            for method in methods {
                if let Some(body) = method.body.as_deref() {
                    for param in &method.params {
                        usage.bind_id(&param.id);
                    }
                    collect_node_usage(body, usage);
                }
            }
        }
        Resolved::TraitImplDef(_, _, _, _, _, methods) => {
            for method in methods {
                for param in &method.params {
                    usage.bind_id(&param.id);
                }
                collect_node_usage(&method.body, usage);
            }
        }
        Resolved::BuiltinDecl(_, _, _, _, _) | Resolved::BuiltinExtractorDecl(_, _, _, _, _) => {}
        Resolved::Closure(_, params, captures, body) => {
            for param in params {
                usage.bind_id(&param.id);
            }
            for capture in captures {
                usage.use_id(capture);
            }
            collect_node_usage(body, usage);
        }
    }
}

fn collect_record_arg_usage(arg: &ResolvedRecordLitArg, usage: &mut WarningUsage) {
    match arg {
        ResolvedRecordLitArg::Positional(expr) | ResolvedRecordLitArg::Named(_, expr) => {
            collect_node_usage(expr, usage);
        }
    }
}

fn collect_pattern_usage(pattern: &ResolvedPattern, usage: &mut WarningUsage) {
    match pattern {
        ResolvedPattern::Var(id) | ResolvedPattern::Annotated(id, _) => usage.bind_id(id),
        ResolvedPattern::Pin(id) => usage.use_id(id),
        ResolvedPattern::Wildcard(_)
        | ResolvedPattern::ListNil(_)
        | ResolvedPattern::IntLit(..)
        | ResolvedPattern::StrLit(..)
        | ResolvedPattern::BoolLit(..)
        | ResolvedPattern::DurationLit(..) => {}
        ResolvedPattern::ListCons(head, tail) => {
            collect_pattern_usage(head, usage);
            collect_pattern_usage(tail, usage);
        }
        ResolvedPattern::Constructor(_, inners) | ResolvedPattern::Extractor(_, inners) => {
            for inner in inners {
                collect_pattern_usage(inner, usage);
            }
        }
        ResolvedPattern::Tuple(inners) | ResolvedPattern::Or(inners) => {
            for inner in inners {
                collect_pattern_usage(inner, usage);
            }
        }
        ResolvedPattern::As(inner, alias, _) => {
            collect_pattern_usage(inner, usage);
            usage.bind_id(alias);
        }
    }
}
