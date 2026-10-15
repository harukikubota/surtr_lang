use super::*;
use diagnostics::TypeDiagnosticReason;

enum FacetPathInput<'a> {
    Expr(&'a Resolved),
    Capture(PendingFacetPath),
}

struct PreparedFacetInput {
    typed_source: TypedNode,
    source_is_result: bool,
    path: TypedFacetPath,
}

impl Checker {
    fn facet_intrinsic_kind(&self, func: &Resolved) -> Option<&'static str> {
        let Resolved::Var(_, id) = func else {
            return None;
        };
        if let Some(qualified_name) = id.qualified_name.as_deref() {
            return match Self::surface_name(qualified_name) {
                "Facet::view" => Some("view"),
                "Facet::preview" => Some("preview"),
                "Facet::compose" => Some("compose"),
                "Facet::put" => Some("put"),
                "Facet::set" => Some("set"),
                "Facet::over" => Some("over"),
                "Facet::over_result" => Some("over_result"),
                "Facet::case_set" => Some("case_set"),
                "Facet::case_over" => Some("case_over"),
                _ => None,
            };
        }
        None
    }

    pub(super) fn check_facet_source_value(
        &mut self,
        op_name: &str,
        source_expr: &Resolved,
    ) -> Result<(TypedNode, bool, Ty), TypeError> {
        let typed_source = self.check_node(source_expr)?;
        if matches!(typed_source.ty, Ty::Facet(..)) {
            return Err(self.policy_error(
                TypeDiagnosticReason::FacetOperationPolicyViolation,
                diagnostics::TypePolicy::FacetOperation,
                Some(format!("{} source value cannot be a Facet", op_name)),
                None,
                Some(&typed_source.ty),
                None,
                None,
                &typed_source.span,
                None,
            ));
        }

        let (source_is_result, source_value_ty) = match self.resolve_ty(&typed_source.ty) {
            Ty::Result(ok, _) => (true, ok.as_ref().clone()),
            other => (false, other),
        };

        Ok((typed_source, source_is_result, source_value_ty))
    }

    fn parse_facet_read_intrinsic_args<'a>(
        &self,
        span: &Span,
        op_name: &str,
        args: &'a [ResolvedRecordLitArg],
    ) -> Result<(Resolved, FacetPathInput<'a>), TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                structured: None,
                message: format!("{op_name} does not accept named arguments"),
                span: span.clone(),
                hint: None,
            });
        }
        if args.len() != 1 && args.len() != 2 {
            return Err(TypeError {
                structured: None,
                message: format!("{op_name} expects 1 or 2 argument(s), got {}", args.len()),
                span: span.clone(),
                hint: None,
            });
        }

        match args {
            [ResolvedRecordLitArg::Positional(Resolved::FacetCapture(capture_span, expr))] => {
                let (source_expr, path) =
                    self.expand_facet_capture_path(op_name, capture_span, expr)?;
                Ok((source_expr, FacetPathInput::Capture(path)))
            }
            [ResolvedRecordLitArg::Positional(path_expr), ResolvedRecordLitArg::Positional(source_expr)] => {
                Ok((source_expr.clone(), FacetPathInput::Expr(path_expr)))
            }
            _ => unreachable!("validated argument form above"),
        }
    }

    fn parse_facet_mutating_intrinsic_args<'a>(
        &self,
        span: &Span,
        op_name: &str,
        args: &'a [ResolvedRecordLitArg],
    ) -> Result<(Resolved, FacetPathInput<'a>, &'a Resolved), TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                structured: None,
                message: format!("{op_name} does not accept named arguments"),
                span: span.clone(),
                hint: None,
            });
        }
        if args.len() != 2 && args.len() != 3 {
            return Err(TypeError {
                structured: None,
                message: format!("{op_name} expects 2 or 3 argument(s), got {}", args.len()),
                span: span.clone(),
                hint: None,
            });
        }

        match args {
            [ResolvedRecordLitArg::Positional(Resolved::FacetCapture(capture_span, expr)), ResolvedRecordLitArg::Positional(value_expr)] =>
            {
                let (source_expr, path) =
                    self.expand_facet_capture_path(op_name, capture_span, expr)?;
                Ok((source_expr, FacetPathInput::Capture(path), value_expr))
            }
            [ResolvedRecordLitArg::Positional(path_expr), ResolvedRecordLitArg::Positional(source_expr), ResolvedRecordLitArg::Positional(value_expr)] => {
                Ok((
                    source_expr.clone(),
                    FacetPathInput::Expr(path_expr),
                    value_expr,
                ))
            }
            _ => unreachable!("validated argument form above"),
        }
    }

    fn prepare_facet_input(
        &mut self,
        span: &Span,
        op_name: &str,
        source_expr: &Resolved,
        path_input: FacetPathInput<'_>,
    ) -> Result<PreparedFacetInput, TypeError> {
        // Common fields normalize to an Error path, so check the resolved root first.
        if self.is_capture_closure
            && matches!(&path_input, FacetPathInput::Expr(expr)
                if self.has_concrete_error_facet_root(expr))
        {
            return Err(TypeError::new(
                "Concrete Error Facet paths cannot be captured",
                span.clone(),
            ));
        }
        let (typed_source, source_is_result, source_value_ty) =
            self.check_facet_source_value(op_name, source_expr)?;
        let path = self.check_facet_path_input(
            span,
            op_name,
            path_input,
            &source_value_ty,
            &typed_source.ty,
        )?;
        if self.is_capture_closure
            && path
                .segments
                .iter()
                .any(|segment| matches!(segment, TypedFacetSegment::ErrorPayload { .. }))
        {
            return Err(TypeError::new(
                "Concrete Error Facet paths cannot be captured",
                span.clone(),
            ));
        }
        for (index, segment) in path.segments.iter().enumerate() {
            if let TypedFacetSegment::ErrorPayload { kind, .. } = segment {
                if index != 0
                    || self.resolved_concrete_error_identity(source_expr) != Some(kind.as_str())
                {
                    return Err(TypeError::new(
                        "Concrete Error Payload Facet requires a matching narrowed local binding",
                        span.clone(),
                    ));
                }
            }
        }
        Ok(PreparedFacetInput {
            typed_source,
            source_is_result,
            path,
        })
    }

    pub(super) fn ensure_deferred_facet_slots(
        &self,
        op_name: &str,
        path: &TypedFacetPath,
        span: &Span,
    ) -> Result<(), TypeError> {
        if matches!(
            (&path.update_source_ty, &path.update_focus_ty),
            (Ty::Hole, Ty::Hole)
        ) {
            return Ok(());
        }
        Err(TypeError {
            structured: None,
            message: format!("{op_name} requires a Facet with deferred update slots `_, _`"),
            span: span.clone(),
            hint: Some("Use a deferred Facet path for read-only consumption.".into()),
        })
    }

    fn check_facet_view_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input) =
            self.parse_facet_read_intrinsic_args(span, "Facet::view", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            path,
            ..
        } = self.prepare_facet_input(span, "Facet::view", &source_expr, path_input)?;
        self.ensure_deferred_facet_slots("Facet::view", &path, span)?;

        let focus_ty = self.resolve_ty(&path.focus_ty);
        let out_ty = if source_is_result || path.may_fail {
            Ty::Result(Box::new(focus_ty.clone()), Box::new(Ty::Error))
        } else {
            focus_ty
        };

        Ok(TypedNode {
            ty: out_ty,
            span: span.clone(),
            node: TypedInner::FacetView {
                api: TypedFacetApi::View,
                source: Box::new(typed_source),
                path,
                source_is_result,
            },
        })
    }

    fn check_facet_preview_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input) =
            self.parse_facet_read_intrinsic_args(span, "Facet::preview", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            path,
            ..
        } = self.prepare_facet_input(span, "Facet::preview", &source_expr, path_input)?;
        self.ensure_deferred_facet_slots("Facet::preview", &path, span)?;
        if !path.has_variant_segment() {
            return Err(self.policy_error(
                TypeDiagnosticReason::FacetOperationPolicyViolation,
                diagnostics::TypePolicy::FacetOperation,
                Some("Facet::preview requires a variant Facet".into()),
                None,
                Some(&Ty::Facet(
                    Self::facet_kind_from_path_kind(path.path_kind),
                    Box::new(path.source_ty.clone()),
                    Box::new(path.focus_ty.clone()),
                    Box::new(path.update_source_ty.clone()),
                    Box::new(path.update_focus_ty.clone()),
                )),
                None,
                None,
                span,
                Some("Use Facet::view for structural field and tuple paths.".into()),
            ));
        }

        let focus_ty = self.resolve_ty(&path.focus_ty);
        Ok(TypedNode {
            ty: Ty::Result(Box::new(focus_ty), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::FacetView {
                api: TypedFacetApi::Preview,
                source: Box::new(typed_source),
                path,
                source_is_result,
            },
        })
    }

    pub(in crate::checker::expr) fn try_check_facet_intrinsic_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        match self.facet_intrinsic_kind(func) {
            Some("view") => Ok(Some(self.check_facet_view_intrinsic(span, args)?)),
            Some("preview") => Ok(Some(self.check_facet_preview_intrinsic(span, args)?)),
            Some("compose") => Ok(Some(self.check_facet_compose_intrinsic(span, args)?)),
            Some("put") => Ok(Some(self.check_facet_put_intrinsic(span, args)?)),
            Some("set") => Ok(Some(self.check_facet_set_intrinsic(span, args)?)),
            Some("over") => Ok(Some(self.check_facet_over_intrinsic(span, args)?)),
            Some("over_result") => Ok(Some(self.check_facet_over_result_intrinsic(span, args)?)),
            Some("case_set") => Ok(Some(self.check_facet_case_set_intrinsic(span, args)?)),
            Some("case_over") => Ok(Some(self.check_facet_case_over_intrinsic(span, args)?)),
            _ => Ok(None),
        }
    }

    pub(in crate::checker) fn ensure_no_runtime_facet_args(
        &self,
        args: &[TypedNode],
        span: &Span,
        callee: &str,
    ) -> Result<(), TypeError> {
        for arg in args {
            self.ensure_no_match_result_value(&arg.ty, &arg.span)?;
        }
        if let Some(arg) = args.iter().find(|arg| self.ty_contains_facet(&arg.ty)) {
            return Err(self.policy_error(
                TypeDiagnosticReason::FacetCompileTimeOnly,
                diagnostics::TypePolicy::FacetStageRestriction,
                Some(format!(
                    "{} cannot accept Facet values in Stage1 (Facet is compile-time only)",
                    callee
                )),
                None,
                Some(&arg.ty),
                Some("Stage1".into()),
                None,
                span,
                Some("Apply Facet::view(...) before passing the value.".into()),
            ));
        }
        Ok(())
    }

    pub(in crate::checker) fn ensure_no_runtime_facet_value(
        &self,
        value: &TypedNode,
        context: &str,
    ) -> Result<(), TypeError> {
        self.ensure_no_match_result_value(&value.ty, &value.span)?;
        if self.ty_contains_facet(&value.ty) {
            return Err(self.policy_error(
                TypeDiagnosticReason::FacetCompileTimeOnly,
                diagnostics::TypePolicy::FacetStageRestriction,
                Some(format!(
                    "{} cannot contain Facet values in Stage1 (Facet is compile-time only)",
                    context
                )),
                None,
                Some(&value.ty),
                Some("Stage1".into()),
                None,
                &value.span,
                Some(
                    "Consume Facet with Facet::view/set/over first, then pass the plain value."
                        .into(),
                ),
            ));
        }
        Ok(())
    }
}

mod path;
mod update;
