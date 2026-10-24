use super::*;
use diagnostics::TypeDiagnosticReason;
use sindr::names::FacetRootKind;
use spire::ast::Symbol;

impl Checker {
    pub(in crate::checker::expr) fn parse_tuple_index_name(name: &str) -> Option<usize> {
        let suffix = name.strip_prefix('_')?;
        if suffix.is_empty() || !suffix.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        suffix.parse::<usize>().ok()
    }

    /// A user-facing Facet annotation describes a path template; it does not
    /// coerce or select a different path.  Preserve concrete update slots on
    /// the compile-time path so every later API use can validate them against
    /// its independently-derived rebuild result.
    pub(in crate::checker::expr) fn apply_facet_annotation(
        &mut self,
        typed: &mut TypedNode,
        expected: &Ty,
        span: &Span,
    ) -> Result<(), TypeError> {
        let expected = self.resolve_ty(expected);
        let actual = self.resolve_ty(&typed.ty);
        let (
            Ty::Facet(
                expected_kind,
                expected_source,
                expected_focus,
                expected_update_source,
                expected_update_focus,
            ),
            Ty::Facet(actual_kind, actual_source, actual_focus, _, _),
        ) = (&expected, &actual)
        else {
            return Ok(());
        };

        if !expected_kind.is_atomic()
            || expected_kind != actual_kind
            || !self.types_compatible(expected_source, actual_source)?
            || !self.types_compatible(expected_focus, actual_focus)?
        {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Facet annotation does not match this path: expected {}, got {}",
                    self.ty_name(&expected),
                    self.ty_name(&actual)
                ),
                span: span.clone(),
                hint: Some(
                    "Facet K, S, and A are derived from the path and must match exactly.".into(),
                ),
            });
        }

        match &mut typed.node {
            TypedInner::FacetPath(path) => {
                path.update_source_ty = expected_update_source.as_ref().clone();
                path.update_focus_ty = expected_update_focus.as_ref().clone();
            }
            // A pending path will be specialized with the annotated source
            // before it can be bound.  Its update slots remain on `typed.ty`.
            TypedInner::PendingFacetPath(_) => {}
            _ => return Ok(()),
        }
        typed.ty = expected;
        Ok(())
    }

    pub(in crate::checker::expr) fn stored_facet_path_from_node(
        &self,
        typed: TypedNode,
        span: &Span,
    ) -> Result<StoredFacetPath, TypeError> {
        let slots = match self.resolve_ty(&typed.ty) {
            Ty::Facet(_, _, _, update_source, update_focus) => (
                update_source.as_ref().clone(),
                update_focus.as_ref().clone(),
            ),
            _ => (Ty::Hole, Ty::Hole),
        };
        match typed.node {
            TypedInner::FacetPath(path) => Ok(StoredFacetPath::Concrete(TypedFacetPath {
                source_ty: self.resolve_ty(&path.source_ty),
                focus_ty: self.resolve_ty(&path.focus_ty),
                update_source_ty: self.resolve_ty(&slots.0),
                update_focus_ty: self.resolve_ty(&slots.1),
                path_kind: path.path_kind,
                may_fail: path.may_fail,
                segment_focus_tys: path.segment_focus_tys.iter().map(|ty| self.resolve_ty(ty)).collect(),
                segments: path.segments,
            })),
            TypedInner::PendingFacetPath(path) => Ok(StoredFacetPath::Pending(path)),
            _ => Err(self.policy_error(
                TypeDiagnosticReason::FacetCompileTimeOnly,
                diagnostics::TypePolicy::FacetStageRestriction,
                Some("Facet values are compile-time only in Stage1 and cannot be stored or passed around".into()),
                None,
                Some(&typed.ty),
                Some("Stage1".into()),
                None,
                span,
                Some("Use type-root path expressions inline (e.g. User.name).".into()),
            )),
        }
    }

    pub(in crate::checker::expr) fn bind_facet_pattern_bindings(
        &mut self,
        pattern: &TypedPattern,
        path: &StoredFacetPath,
        span: &Span,
    ) -> Result<(), TypeError> {
        match pattern {
            TypedPattern::Located(_, inner) => self.bind_facet_pattern_bindings(inner, path, span),
            TypedPattern::Var(_, id) => {
                self.facet_bindings.insert(id.unique_id, path.clone());
                Ok(())
            }
            TypedPattern::As(_, inner, alias) => {
                self.bind_facet_pattern_bindings(inner, path, span)?;
                self.facet_bindings.insert(alias.unique_id, path.clone());
                Ok(())
            }
            TypedPattern::Wildcard(_) => Ok(()),
            _ => Err(self.policy_error(
                TypeDiagnosticReason::FacetPatternBindingForbidden,
                diagnostics::TypePolicy::FacetPatternBinding,
                Some("Facet value binding pattern".into()),
                None,
                None,
                Some("Stage1".into()),
                None,
                span,
                Some("Use `facet = User.name` or `_ = User.name`.".into()),
            )),
        }
    }

    pub(in crate::checker::expr) fn clear_facet_pattern_bindings(
        &mut self,
        pattern: &TypedPattern,
    ) {
        match pattern {
            TypedPattern::HashMap(_, entries) => {
                for entry in entries {
                    self.clear_facet_pattern_bindings(&entry.pattern);
                }
            }
            TypedPattern::Located(_, inner) => self.clear_facet_pattern_bindings(inner),
            TypedPattern::Var(_, id) => {
                self.facet_bindings.remove(&id.unique_id);
            }
            TypedPattern::As(_, inner, alias) => {
                self.clear_facet_pattern_bindings(inner);
                self.facet_bindings.remove(&alias.unique_id);
            }
            TypedPattern::ListCons(_, head, tail) => {
                self.clear_facet_pattern_bindings(head);
                self.clear_facet_pattern_bindings(tail);
            }
            TypedPattern::Tuple(_, items) => {
                for item in items {
                    self.clear_facet_pattern_bindings(item);
                }
            }
            TypedPattern::Constructor { fields, .. } => {
                for field in fields {
                    self.clear_facet_pattern_bindings(field);
                }
            }
            TypedPattern::Extractor { items, .. } => {
                for item in items {
                    self.clear_facet_pattern_bindings(item);
                }
            }
            TypedPattern::Wildcard(_)
            | TypedPattern::Pin(_, _, _)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _)
            | TypedPattern::DurationLit(_, _) => {}
        }
    }

    fn pending_segment_from_syntax(segment: &ResolvedFacetPathSegment) -> PendingFacetSegment {
        match segment {
            ResolvedFacetPathSegment::Field { name, optional } => PendingFacetSegment::Field {
                name: name.clone(),
                optional: *optional,
            },
            ResolvedFacetPathSegment::Bracket(expr) => match expr.expr.as_ref() {
                Resolved::RangeLiteral(_, start, end) => PendingFacetSegment::RangeBracket {
                    start: PendingFacetExpr::Resolved(start.clone()),
                    end: PendingFacetExpr::Resolved(end.clone()),
                    display: expr.display.clone(),
                },
                _ => PendingFacetSegment::Bracket {
                    expr: PendingFacetExpr::Resolved(expr.expr.clone()),
                    display: expr.display.clone(),
                },
            },
        }
    }

    fn pending_field_segment(name: impl Into<String>) -> PendingFacetSegment {
        PendingFacetSegment::Field {
            name: name.into(),
            optional: false,
        }
    }

    fn pending_segment_display(segment: &PendingFacetSegment) -> String {
        match segment {
            PendingFacetSegment::Field { name, optional } => {
                if *optional {
                    format!("{name}?")
                } else {
                    name.clone()
                }
            }
            PendingFacetSegment::Bracket { display, .. }
            | PendingFacetSegment::RangeBracket { display, .. } => format!("[{display}]"),
        }
    }

    pub(in crate::checker::expr) fn inferred_facet_capture_display(
        segments: &[ResolvedFacetPathSegment],
    ) -> String {
        if segments.is_empty() {
            "_".to_string()
        } else {
            format!(
                "_.{}",
                segments
                    .iter()
                    .map(|segment| match segment {
                        ResolvedFacetPathSegment::Field { name, optional } => {
                            if *optional {
                                format!("{name}?")
                            } else {
                                name.clone()
                            }
                        }
                        ResolvedFacetPathSegment::Bracket(expr) => format!("[{}]", expr.display),
                    })
                    .collect::<Vec<_>>()
                    .join(".")
            )
        }
    }

    pub(in crate::checker::expr) fn check_inferred_facet_capture(
        &mut self,
        span: &Span,
        segments: &[ResolvedFacetPathSegment],
        expected_ty: &Ty,
    ) -> Result<TypedNode, TypeError> {
        let expected_ty = self.resolve_ty(expected_ty);
        let Ty::Func(params, _) = &expected_ty else {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{} requires expected unary function context",
                    Self::inferred_facet_capture_display(segments)
                ),
                span: span.clone(),
                hint: Some(format!(
                    "Expected a function type like `(A -> B)`, got {}. Use an explicit capture such as `&Type.field` when no context is available.",
                    self.ty_name(&expected_ty)
                )),
            });
        };
        if params.len() != 1 {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{} requires expected unary function context",
                    Self::inferred_facet_capture_display(segments)
                ),
                span: span.clone(),
                hint: Some(format!(
                    "Expected a one-argument function type, got {}.",
                    self.ty_name(&expected_ty)
                )),
            });
        }

        let param_id = ResolvedId {
            name: "__inferred_facet_capture_arg".to_string(),
            qualified_name: None,
            symbol_info: None,
            unique_id: Self::next_synthetic_range_uid(),
            compiler_generated: true,
            span: span.clone(),
        };
        let body = Self::inferred_capture_body(span, &param_id, segments);
        let synthetic = Resolved::CaptureClosure(
            span.clone(),
            vec![ResolvedClosureParam {
                lazy_capture: None,
                id: param_id,
                ty: None,
            }],
            Vec::new(),
            Box::new(body),
        );
        let typed = self.check_node_with_expected(&synthetic, Some(&expected_ty))?;
        if !self.types_compatible(&expected_ty, &typed.ty)? {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{} inferred type mismatch: expected {}, got {}",
                    Self::inferred_facet_capture_display(segments),
                    self.ty_name(&expected_ty),
                    self.ty_name(&typed.ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }
        Ok(typed)
    }

    fn tuple_index_hint(tuple_len: usize) -> String {
        if tuple_len == 0 {
            "This tuple has 0 elements, so no tuple selectors are available.".into()
        } else {
            let selectors = (0..tuple_len)
                .map(|idx| format!("._{}", idx))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "This tuple has {} element(s); valid selectors are {}.",
                tuple_len, selectors
            )
        }
    }

    fn facet_path_root(expr: &Resolved) -> &Resolved {
        match expr {
            Resolved::FieldAccess(_, inner, _)
            | Resolved::FacetSegmentAccess(_, inner, _)
            | Resolved::Grouped(_, inner) => Self::facet_path_root(inner),
            _ => expr,
        }
    }

    pub(in crate::checker::expr) fn has_concrete_error_facet_root(&self, expr: &Resolved) -> bool {
        matches!(Self::facet_path_root(expr), Resolved::Var(_, root)
            if self.env.is_error_constructor(root.unique_id)
                || self.env.concrete_error_bindings.contains_key(&root.unique_id))
    }

    pub(in crate::checker::expr) fn facet_capture_root(expr: &Resolved) -> Option<&Resolved> {
        match expr {
            Resolved::Grouped(_, inner) => Self::facet_capture_root(inner),
            Resolved::Capture(_, target, _) => Some(Self::facet_path_root(target)),
            _ => None,
        }
    }

    pub(in crate::checker::expr) fn facet_capture_segments(
        expr: &Resolved,
    ) -> Option<Vec<ResolvedFacetPathSegment>> {
        fn path_segments(expr: &Resolved, segments: &mut Vec<ResolvedFacetPathSegment>) {
            match expr {
                Resolved::FieldAccess(_, inner, name) => {
                    path_segments(inner, segments);
                    segments.push(ResolvedFacetPathSegment::Field {
                        name: name.clone(),
                        optional: false,
                    });
                }
                Resolved::FacetSegmentAccess(_, inner, segment) => {
                    path_segments(inner, segments);
                    segments.push(segment.clone());
                }
                Resolved::Grouped(_, inner) => path_segments(inner, segments),
                _ => {}
            }
        }
        match expr {
            Resolved::Grouped(_, inner) => Self::facet_capture_segments(inner),
            Resolved::InferredFacetCapture(_, segments) => Some(segments.clone()),
            Resolved::Capture(_, target, args)
                if args.is_empty() && Self::capture_target_is_facet_path(target) =>
            {
                let mut segments = Vec::new();
                path_segments(target, &mut segments);
                Some(segments)
            }
            _ => None,
        }
    }

    // This reads dependencies only. Unsupported selectors return no dependency
    // and are diagnosed by the ordinary root/segment checker; no error is
    // caught and no candidate type or owner is inferred here.
    pub(in crate::checker::expr) fn facet_capture_dependency_focus(
        &self,
        source: &Ty,
        segment: &ResolvedFacetPathSegment,
    ) -> Option<Ty> {
        match (source, segment) {
            (
                Ty::Tuple(items),
                ResolvedFacetPathSegment::Field {
                    name,
                    optional: false,
                },
            ) => items.get(Self::parse_tuple_index_name(name)?).cloned(),
            (
                Ty::Record(owner, fields) | Ty::Struct(owner, fields),
                ResolvedFacetPathSegment::Field {
                    name,
                    optional: false,
                },
            ) => {
                let fields = self
                    .nominal_fields(owner, fields, &Span { start: 0, end: 0 })
                    .expect("Facet dependency probes require completed nominal schemas");
                let (field_name, field_ty) = if matches!(source, Ty::Record(..)) {
                    if let Some(index) = Self::parse_tuple_index_name(name) {
                        fields.get(index)?
                    } else {
                        fields.iter().find(|(field, _)| field == name)?
                    }
                } else {
                    fields.iter().find(|(field, _)| field == name)?
                };
                if self
                    .env
                    .field_policy(owner, field_name)
                    .is_some_and(|policy| policy.private)
                    && self.current_private_field_owner.as_deref()
                        != Some(Self::surface_name(owner))
                    && !self.allow_private_facet_inspection
                {
                    return None;
                }
                Some(field_ty.clone())
            }
            (Ty::List(inner), ResolvedFacetPathSegment::Bracket(bracket)) => Some(
                if matches!(bracket.expr.as_ref(), Resolved::RangeLiteral(..)) {
                    source.clone()
                } else {
                    inner.as_ref().clone()
                },
            ),
            (Ty::Enum(name, args), ResolvedFacetPathSegment::Bracket(_))
                if Self::surface_name(name) == "HashMap" && args.len() == 1 =>
            {
                Some(args[0].clone())
            }
            (
                source,
                ResolvedFacetPathSegment::Field {
                    name,
                    optional: false,
                },
            ) => {
                let enum_name = Self::facet_enum_source_name(source)?;
                let variant = self.lookup_enum_variant_by_short_name(&enum_name, name)?;
                let (template_args, actual_args) = match (&variant.enum_ty, source) {
                    (Ty::Enum(_, template), Ty::Enum(_, actual)) => {
                        (template.clone(), actual.clone())
                    }
                    (Ty::Result(t_ok, t_err), Ty::Result(a_ok, a_err)) => (
                        vec![*t_ok.clone(), *t_err.clone()],
                        vec![*a_ok.clone(), *a_err.clone()],
                    ),
                    (Ty::Bool, Ty::Bool) => (Vec::new(), Vec::new()),
                    _ => return None,
                };
                let bindings = template_args
                    .iter()
                    .zip(actual_args)
                    .filter_map(|(template, actual)| {
                        if let Ty::Var(var) = template {
                            Some((*var, actual))
                        } else {
                            None
                        }
                    })
                    .collect::<HashMap<_, _>>();
                let payload = variant
                    .payload
                    .iter()
                    .map(|ty| self.substitute_type_def_ty(ty, &bindings))
                    .collect::<Vec<_>>();
                Some(match payload.as_slice() {
                    [] => Ty::Unit,
                    [only] => only.clone(),
                    _ => Ty::Tuple(payload),
                })
            }
            _ => None,
        }
    }

    fn pending_segment_from_typed(segment: &TypedFacetSegment) -> PendingFacetSegment {
        Self::pending_field_segment(match segment {
            TypedFacetSegment::Field {
                origin_index: Some(index),
                ..
            } => format!("_{index}"),
            TypedFacetSegment::ReadonlyBuiltin { field_name, .. }
            | TypedFacetSegment::ErrorPayload { field_name, .. }
            | TypedFacetSegment::Field { field_name, .. } => field_name.clone(),
            TypedFacetSegment::Tuple { field_index, .. } => format!("_{field_index}"),
            TypedFacetSegment::Variant { variant_name, .. } => variant_name.clone(),
            TypedFacetSegment::ListIndex { index, display, .. } => {
                return PendingFacetSegment::Bracket {
                    expr: PendingFacetExpr::Typed(index.clone()),
                    display: display.clone(),
                };
            }
            TypedFacetSegment::ListRange {
                start,
                end,
                display,
                ..
            } => {
                return PendingFacetSegment::RangeBracket {
                    start: PendingFacetExpr::Typed(start.clone()),
                    end: PendingFacetExpr::Typed(end.clone()),
                    display: display.clone(),
                };
            }
            TypedFacetSegment::MapKey { key, display, .. } => {
                return PendingFacetSegment::Bracket {
                    expr: PendingFacetExpr::Typed(key.clone()),
                    display: display.clone(),
                };
            }
        })
    }

    fn facet_path_kind_for_segments(segments: &[TypedFacetSegment]) -> TypedFacetPathKind {
        if segments
            .iter()
            .any(|segment| matches!(segment, TypedFacetSegment::Variant { .. }))
        {
            TypedFacetPathKind::VariantPath
        } else if segments.iter().any(|segment| {
            matches!(
                segment,
                TypedFacetSegment::ListIndex { .. }
                    | TypedFacetSegment::ListRange { .. }
                    | TypedFacetSegment::MapKey { .. }
            )
        }) {
            TypedFacetPathKind::FallibleStructural
        } else {
            TypedFacetPathKind::InfallibleStructural
        }
    }

    pub(in crate::checker::expr) fn facet_kind_from_path_kind(
        path_kind: TypedFacetPathKind,
    ) -> crate::types::FacetKind {
        match path_kind {
            TypedFacetPathKind::InfallibleStructural => {
                crate::types::FacetKind::InfallibleStructural
            }
            TypedFacetPathKind::FallibleStructural => crate::types::FacetKind::FallibleStructural,
            TypedFacetPathKind::VariantPath => crate::types::FacetKind::VariantPath,
        }
    }

    fn deferred_facet_ty(path_kind: TypedFacetPathKind, source: Ty, focus: Ty) -> Ty {
        Ty::Facet(
            Self::facet_kind_from_path_kind(path_kind),
            Box::new(source),
            Box::new(focus),
            Box::new(Ty::Hole),
            Box::new(Ty::Hole),
        )
    }

    fn pending_facet_node(&mut self, span: &Span, path: PendingFacetPath) -> TypedNode {
        let source_tv = self.env.fresh_tyvar();
        let focus_tv = self.env.fresh_tyvar();
        TypedNode {
            ty: Self::deferred_facet_ty(
                TypedFacetPathKind::InfallibleStructural,
                source_tv,
                focus_tv,
            ),
            span: span.clone(),
            node: TypedInner::PendingFacetPath(path),
        }
    }

    fn validate_pending_root_source(
        &mut self,
        root_path_name: &str,
        source_ty: &Ty,
        span: &Span,
    ) -> Result<(), TypeError> {
        match (root_path_name, self.resolve_ty(source_ty)) {
            ("Tuple", Ty::Tuple(_)) => Ok(()),
            ("List", Ty::List(_)) => Ok(()),
            ("HashMap", Ty::Enum(name, args))
                if Self::surface_name(&name) == "HashMap" && args.len() == 1 =>
            {
                Ok(())
            }
            ("Tuple", actual) => Err(TypeError {
                structured: None,
                message: format!(
                    "Tuple root Facet path requires tuple source context, got {}",
                    self.ty_name(&actual)
                ),
                span: span.clone(),
                hint: Some("Expected source type like (A, B, ...) for Tuple._N.".into()),
            }),
            ("List", actual) => Err(TypeError {
                structured: None,
                message: format!(
                    "List root Facet path requires List<T>, got {}",
                    self.ty_name(&actual)
                ),
                span: span.clone(),
                hint: Some("Use List.[N] with a List source value.".into()),
            }),
            ("HashMap", actual) => Err(TypeError {
                structured: None,
                message: format!(
                    "HashMap root Facet path requires HashMap<T>, got {}",
                    self.ty_name(&actual)
                ),
                span: span.clone(),
                hint: Some("Use HashMap.[\"key\"] with a HashMap source value.".into()),
            }),
            _ => Ok(()),
        }
    }

    fn specialize_pending_facet_path(
        &mut self,
        path: PendingFacetPath,
        span: &Span,
        expected_source: Option<&Ty>,
    ) -> Result<TypedFacetPath, TypeError> {
        let mut current_source = if let Some(source_ty_hint) = path.source_ty_hint.clone() {
            if let Some(expected_source) = expected_source {
                if !self.types_compatible(&source_ty_hint, expected_source)? {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Facet path source type mismatch: path expects {}, got {}",
                            self.ty_name(&source_ty_hint),
                            self.ty_name(expected_source)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
            }
            self.resolve_ty(&source_ty_hint)
        } else {
            let Some(expected_source) = expected_source else {
                return Err(TypeError {
                    structured: None,
                    message:
                        "Tuple._N requires Facet type context (e.g. Facet::view(Tuple._1, source_tuple))"
                            .into(),
                    span: span.clone(),
                    hint: Some(
                        "Use Tuple._N only where a Facet<K, (...), ..., T, B> is expected."
                            .into(),
                    ),
                });
            };
            self.resolve_ty(expected_source)
        };

        let source_ty = current_source.clone();
        if let Some(root_path_name) = &path.root_path_name {
            self.validate_pending_root_source(root_path_name, &source_ty, span)?;
        }
        let mut may_fail = false;
        let mut segments = Vec::with_capacity(path.segments.len());
        let mut segment_focus_tys = Vec::with_capacity(path.segments.len());
        for pending_segment in path.segments {
            if let Ty::Result(ok, _) = self.resolve_ty(&current_source) {
                current_source = ok.as_ref().clone();
                may_fail = true;
            }
            let (segment, focus_ty, segment_may_fail) = self.resolve_facet_segment_for_source_ty(
                &current_source,
                &pending_segment,
                span,
                true,
            )?;
            segments.push(segment);
            current_source = self.resolve_ty(&focus_ty);
            segment_focus_tys.push(current_source.clone());
            may_fail |= segment_may_fail;
        }

        Ok(TypedFacetPath {
            source_ty: source_ty.clone(),
            focus_ty: current_source,
            segment_focus_tys,
            update_source_ty: Ty::Hole,
            update_focus_ty: Ty::Hole,
            path_kind: Self::facet_path_kind_for_segments(&segments),
            may_fail,
            segments,
        })
    }

    fn resolve_facet_path_from_node(
        &mut self,
        typed: TypedNode,
        span: &Span,
        expected_source: Option<&Ty>,
    ) -> Result<TypedFacetPath, TypeError> {
        let facet_ty = self.resolve_ty(&typed.ty);
        if !matches!(facet_ty, Ty::Facet(..)) {
            return Err(TypeError {
                structured: None,
                message: format!("Expected Facet<...> value, got {}", self.ty_name(&typed.ty)),
                span: typed.span.clone(),
                hint: None,
            });
        }
        let update_slots = match facet_ty {
            Ty::Facet(_, _, _, update_source, update_focus) => (
                update_source.as_ref().clone(),
                update_focus.as_ref().clone(),
            ),
            _ => unreachable!("Facet type checked above"),
        };
        match typed.node {
            TypedInner::FacetPath(path) => Ok(TypedFacetPath {
                source_ty: self.resolve_ty(&path.source_ty),
                focus_ty: self.resolve_ty(&path.focus_ty),
                update_source_ty: self.resolve_ty(&update_slots.0),
                update_focus_ty: self.resolve_ty(&update_slots.1),
                path_kind: path.path_kind,
                may_fail: path.may_fail,
                segment_focus_tys: path.segment_focus_tys.iter().map(|ty| self.resolve_ty(ty)).collect(),
                segments: path.segments,
            }),
            TypedInner::PendingFacetPath(path) => {
                self.specialize_pending_facet_path(path, span, expected_source)
            }
            _ => Err(self.policy_error(
                TypeDiagnosticReason::FacetCompileTimeOnly,
                diagnostics::TypePolicy::FacetStageRestriction,
                Some("Facet values are compile-time only in Stage1 and cannot be stored or passed around".into()),
                None,
                Some(&typed.ty),
                Some("Stage1".into()),
                None,
                span,
                Some("Use type-root path expressions inline (e.g. User.name).".into()),
            )),
        }
    }

    fn compose_facet_paths(
        &mut self,
        span: &Span,
        left_path: TypedFacetPath,
        right_expr: &Resolved,
        operator_name: &str,
    ) -> Result<TypedNode, TypeError> {
        self.ensure_deferred_facet_slots(operator_name, &left_path, span)?;
        let expected_right_focus = self.env.fresh_tyvar();
        let expected_right_ty = Ty::Facet(
            crate::types::FacetKind::ReadablePath,
            Box::new(self.resolve_ty(&left_path.focus_ty)),
            Box::new(expected_right_focus),
            Box::new(Ty::Hole),
            Box::new(Ty::Hole),
        );
        let right = self.check_node_with_expected(right_expr, Some(&expected_right_ty))?;
        let right_path =
            self.resolve_facet_path_from_node(right, span, Some(&left_path.focus_ty))?;
        self.ensure_deferred_facet_slots(operator_name, &right_path, span)?;

        if !self.types_compatible(&left_path.focus_ty, &right_path.source_ty)? {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{} source/focus mismatch: left focus is {}, right source is {}",
                    operator_name,
                    self.ty_name(&left_path.focus_ty),
                    self.ty_name(&right_path.source_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let source_ty = self.resolve_ty(&left_path.source_ty);
        let focus_ty = self.resolve_ty(&right_path.focus_ty);
        let mut segments = left_path.segments;
        segments.extend(right_path.segments);
        let mut segment_focus_tys = left_path.segment_focus_tys;
        segment_focus_tys.extend(right_path.segment_focus_tys);
        let path = TypedFacetPath {
            source_ty: source_ty.clone(),
            focus_ty: focus_ty.clone(),
            segment_focus_tys,
            update_source_ty: Ty::Hole,
            update_focus_ty: Ty::Hole,
            path_kind: if left_path.path_kind == TypedFacetPathKind::VariantPath
                || right_path.path_kind == TypedFacetPathKind::VariantPath
            {
                TypedFacetPathKind::VariantPath
            } else if left_path.path_kind == TypedFacetPathKind::FallibleStructural
                || right_path.path_kind == TypedFacetPathKind::FallibleStructural
            {
                TypedFacetPathKind::FallibleStructural
            } else {
                TypedFacetPathKind::InfallibleStructural
            },
            may_fail: left_path.may_fail || right_path.may_fail,
            segments,
        };
        Ok(TypedNode {
            ty: Self::deferred_facet_ty(path.path_kind, source_ty, focus_ty),
            span: span.clone(),
            node: TypedInner::FacetPath(path),
        })
    }

    fn compose_pending_facet_paths(
        &mut self,
        span: &Span,
        mut left_path: PendingFacetPath,
        right_expr: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let right = self.check_node(right_expr)?;
        match right.node {
            TypedInner::FacetPath(path) => {
                left_path
                    .segments
                    .extend(path.segments.iter().map(Self::pending_segment_from_typed));
                Ok(self.pending_facet_node(span, left_path))
            }
            TypedInner::PendingFacetPath(path) => {
                left_path.segments.extend(path.segments);
                if left_path.root_path_name.is_none() {
                    left_path.root_path_name = path.root_path_name;
                }
                Ok(self.pending_facet_node(span, left_path))
            }
            _ => Err(self.policy_error(
                TypeDiagnosticReason::FacetCompileTimeOnly,
                diagnostics::TypePolicy::FacetStageRestriction,
                Some("Facet values are compile-time only in Stage1 and cannot be stored or passed around".into()),
                None,
                Some(&right.ty),
                Some("Stage1".into()),
                None,
                span,
                Some("Use type-root path expressions inline (e.g. User.name).".into()),
            )),
        }
    }

    pub(in crate::checker::expr) fn check_facet_compose_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        if args.len() != 2 {
            return Err(TypeError {
                structured: None,
                message: format!("Facet::compose expects 2 argument(s), got {}", args.len()),
                span: span.clone(),
                hint: None,
            });
        }
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                structured: None,
                message: "Facet::compose does not accept named arguments".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let ResolvedRecordLitArg::Positional(left_expr) = &args[0] else {
            unreachable!("validated argument form above")
        };
        let ResolvedRecordLitArg::Positional(right_expr) = &args[1] else {
            unreachable!("validated argument form above")
        };

        if let Resolved::FacetCapture(capture_span, expr) = left_expr {
            let (source_expr, pending_path) =
                self.expand_facet_capture_path("Facet::compose", capture_span, expr)?;
            let (_, _, source_value_ty) =
                self.check_facet_source_value("Facet::compose", &source_expr)?;
            let left_path =
                self.specialize_pending_facet_path(pending_path, span, Some(&source_value_ty))?;
            return self.compose_facet_paths(span, left_path, right_expr, "Facet::compose");
        }

        let left = self.check_node(left_expr)?;
        match left.node {
            TypedInner::FacetPath(path) => {
                self.compose_facet_paths(span, path, right_expr, "Facet::compose")
            }
            TypedInner::PendingFacetPath(path) => {
                self.compose_pending_facet_paths(span, path, right_expr)
            }
            _ => Err(TypeError {
                structured: None,
                message: format!("Expected Facet<...> value, got {}", self.ty_name(&left.ty)),
                span: left.span.clone(),
                hint: None,
            }),
        }
    }

    pub(super) fn expand_facet_capture_path(
        &self,
        op_name: &str,
        span: &Span,
        expr: &Resolved,
    ) -> Result<(Resolved, PendingFacetPath), TypeError> {
        let mut segments = Vec::new();
        let mut current = expr.clone();
        loop {
            match current {
                Resolved::FieldAccess(_, inner, field) => {
                    segments.push(Self::pending_field_segment(field));
                    current = *inner;
                }
                Resolved::FacetSegmentAccess(_, inner, segment) => {
                    segments.push(Self::pending_segment_from_syntax(&segment));
                    current = *inner;
                }
                Resolved::Grouped(_, inner) => {
                    current = *inner;
                }
                _ => break,
            }
        }
        if self.has_concrete_error_facet_root(&current) {
            return Err(TypeError::new(
                "Concrete Error Facet paths cannot be captured",
                span.clone(),
            ));
        }
        if segments.is_empty() {
            return Err(TypeError {
                structured: None,
                message: format!("{op_name} shorthand requires a field or tuple path").into(),
                span: span.clone(),
                hint: Some(
                    "Write `~source.field` or `~source._0`. Use canonical `Facet::compose(...)` or a type-root path when you need a standalone Facet path."
                        .into(),
                ),
            });
        }
        segments.reverse();
        Ok((
            current,
            PendingFacetPath {
                root_path_name: None,
                source_ty_hint: None,
                segments,
            },
        ))
    }

    pub(super) fn check_facet_path_input(
        &mut self,
        span: &Span,
        op_name: &str,
        path_input: FacetPathInput<'_>,
        source_value_ty: &Ty,
        source_input_ty: &Ty,
    ) -> Result<TypedFacetPath, TypeError> {
        match path_input {
            FacetPathInput::Expr(path_expr) => self.check_facet_path_argument(
                span,
                op_name,
                path_expr,
                source_value_ty,
                source_input_ty,
            ),
            FacetPathInput::Capture(path) => {
                let path = self.specialize_pending_facet_path(path, span, Some(source_value_ty))?;
                if !self.types_compatible(&path.source_ty, source_value_ty)? {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "{} source type mismatch: facet expects {}, got {}",
                            op_name,
                            self.ty_name(&path.source_ty),
                            self.ty_name(source_input_ty)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(path)
            }
        }
    }

    fn check_facet_path_argument(
        &mut self,
        span: &Span,
        op_name: &str,
        path_expr: &Resolved,
        source_value_ty: &Ty,
        source_input_ty: &Ty,
    ) -> Result<TypedFacetPath, TypeError> {
        let expected_focus_ty = self.env.fresh_tyvar();
        let expected_path_ty = Ty::Facet(
            crate::types::FacetKind::ReadablePath,
            Box::new(self.resolve_ty(source_value_ty)),
            Box::new(expected_focus_ty),
            Box::new(Ty::Hole),
            Box::new(Ty::Hole),
        );
        // view can obtain its plain source from a concrete path. An unresolved
        // source is not an authored owner constraint; containers still require
        // receiver structure from the ordinary expected/call context.
        let path_expected =
            if op_name == "Facet::view" && matches!(self.resolve_ty(source_value_ty), Ty::Var(_)) {
                None
            } else {
                Some(&expected_path_ty)
            };
        let path_node = self.check_node_with_expected(path_expr, path_expected)?;
        let path = self.resolve_facet_path_from_node(path_node, span, Some(source_value_ty))?;

        if !self.types_compatible(&path.source_ty, source_value_ty)? {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{} source type mismatch: facet expects {}, got {}",
                    op_name,
                    self.ty_name(&path.source_ty),
                    self.ty_name(source_input_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        Ok(path)
    }

    fn resolve_facet_segment_for_source_ty(
        &mut self,
        source_ty: &Ty,
        segment: &PendingFacetSegment,
        span: &Span,
        _for_capability: bool,
    ) -> Result<(TypedFacetSegment, Ty, bool), TypeError> {
        if let PendingFacetSegment::Bracket { expr, display } = segment {
            let typed_expr = match expr {
                PendingFacetExpr::Resolved(expr) => self.check_node(expr)?,
                PendingFacetExpr::Typed(expr) => *self.resolve_typed_node((**expr).clone()),
            };
            return match self.resolve_ty(source_ty) {
                Ty::List(inner) => {
                    let expr_ty = self.resolve_ty(&typed_expr.ty);
                    if let Ty::Result(ok, _) = &expr_ty {
                        if self.types_compatible(ok.as_ref(), &Ty::Int)? {
                            return Err(TypeError {
                                structured: None,
                                message: "Facet bracket expression must be plain Int; unwrap Result<Int> before using it".into(),
                                span: typed_expr.span.clone(),
                                hint: None,
                            });
                        }
                    }
                    if !self.types_compatible(&Ty::Int, &expr_ty)? {
                        return Err(TypeError {
                            structured: None,
                            message: "List Facet index expression must be Int".into(),
                            span: typed_expr.span.clone(),
                            hint: None,
                        });
                    }

                    let literal_index = match &typed_expr.node {
                        TypedInner::Lit(Lit::Int(index)) => Some(index.clone()),
                        _ => None,
                    };
                    let focus_ty = inner.as_ref().clone();
                    Ok((
                        TypedFacetSegment::ListIndex {
                            index: Box::new(typed_expr),
                            display: display.clone(),
                            literal_index,
                        },
                        focus_ty,
                        true,
                    ))
                }
                Ty::Enum(name, args)
                    if Self::surface_name(&name) == "HashMap" && args.len() == 1 =>
                {
                    let expr_ty = self.resolve_ty(&typed_expr.ty);
                    if let Ty::Result(ok, _) = &expr_ty {
                        if self.types_compatible(ok.as_ref(), &Ty::Str)? {
                            return Err(TypeError {
                                structured: None,
                                message: "Facet bracket expression must be plain String; unwrap Result<String> before using it".into(),
                                span: typed_expr.span.clone(),
                                hint: None,
                            });
                        }
                    }
                    if !self.types_compatible(&Ty::Str, &expr_ty)? {
                        return Err(TypeError {
                            structured: None,
                            message: "HashMap Facet key expression must be String".into(),
                            span: typed_expr.span.clone(),
                            hint: None,
                        });
                    }

                    let literal_key = match &typed_expr.node {
                        TypedInner::Lit(Lit::Str(key)) => Some(key.clone()),
                        _ => None,
                    };
                    let value_ty = args[0].clone();
                    Ok((
                        TypedFacetSegment::MapKey {
                            key: Box::new(typed_expr),
                            display: display.clone(),
                            literal_key,
                        },
                        value_ty,
                        true,
                    ))
                }
                other => Err(TypeError {
                    structured: None,
                    message: format!(
                        "Facet segment {} is not supported for {} yet",
                        Self::pending_segment_display(segment),
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: None,
                }),
            };
        }
        if let PendingFacetSegment::RangeBracket {
            start,
            end,
            display,
        } = segment
        {
            let typed_start = match start {
                PendingFacetExpr::Resolved(expr) => self.check_node(expr)?,
                PendingFacetExpr::Typed(expr) => *self.resolve_typed_node((**expr).clone()),
            };
            let typed_end = match end {
                PendingFacetExpr::Resolved(expr) => self.check_node(expr)?,
                PendingFacetExpr::Typed(expr) => *self.resolve_typed_node((**expr).clone()),
            };
            return match self.resolve_ty(source_ty) {
                Ty::List(inner) => {
                    for typed_expr in [&typed_start, &typed_end] {
                        let expr_ty = self.resolve_ty(&typed_expr.ty);
                        if let Ty::Result(ok, _) = &expr_ty {
                            if self.types_compatible(ok.as_ref(), &Ty::Int)? {
                                return Err(TypeError {
                                    structured: None,
                                    message: "Facet bracket expression must be plain Int; unwrap Result<Int> before using it".into(),
                                    span: typed_expr.span.clone(),
                                    hint: None,
                                });
                            }
                        }
                        if !self.types_compatible(&Ty::Int, &expr_ty)? {
                            return Err(TypeError {
                                structured: None,
                                message: "List Facet index expression must be Int".into(),
                                span: typed_expr.span.clone(),
                                hint: None,
                            });
                        }
                    }

                    let literal_start = match &typed_start.node {
                        TypedInner::Lit(Lit::Int(index)) => Some(index.clone()),
                        _ => None,
                    };
                    let literal_end = match &typed_end.node {
                        TypedInner::Lit(Lit::Int(index)) => Some(index.clone()),
                        _ => None,
                    };
                    let focus_ty = Ty::List(Box::new(inner.as_ref().clone()));
                    Ok((
                        TypedFacetSegment::ListRange {
                            start: Box::new(typed_start),
                            end: Box::new(typed_end),
                            display: display.clone(),
                            literal_start,
                            literal_end,
                        },
                        focus_ty,
                        true,
                    ))
                }
                other => Err(TypeError {
                    structured: None,
                    message: format!(
                        "Facet segment {} is not supported for {} yet",
                        Self::pending_segment_display(segment),
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: None,
                }),
            };
        }
        let PendingFacetSegment::Field {
            name: field,
            optional,
        } = segment
        else {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Facet segment {} is not supported for {} yet",
                    Self::pending_segment_display(segment),
                    self.ty_name(source_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        };
        if *optional {
            return Err(TypeError {
                structured: None,
                message: "optional Facet selectors are no longer supported".into(),
                span: span.clone(),
                hint: Some(
                    "Facet variant selectors are required because an optional mismatch cannot preserve a type-changing rebuild."
                        .into(),
                ),
            });
        }
        match self.resolve_ty(source_ty) {
            Ty::Error if matches!(field.as_str(), "kind" | "message") => {
                let builtin_id =
                    sindr::builtin::builtin_meta_for_decl(field, Some(&format!("Error::{field}")))
                        .map(|meta| meta.runtime_id())
                        .ok_or_else(|| {
                            TypeError::new("Missing Error observation builtin", span.clone())
                        })?;
                Ok((
                    TypedFacetSegment::ReadonlyBuiltin {
                        field_name: field.clone(),
                        builtin_id,
                    },
                    Ty::Str,
                    false,
                ))
            }
            Ty::Tuple(items) => {
                let index = field
                    .strip_prefix('_')
                    .ok_or_else(|| TypeError {
                        structured: None,
                        message: "Tuple elements are accessed with ._0, ._1, ...".into(),
                        span: span.clone(),
                        hint: None,
                    })?
                    .parse::<usize>()
                    .map_err(|_| TypeError {
                        structured: None,
                        message: "Tuple elements are accessed with ._0, ._1, ...".into(),
                        span: span.clone(),
                        hint: None,
                    })?;
                let field_ty = items.get(index).cloned().ok_or_else(|| TypeError {
                    structured: None,
                    message: format!(
                        "Tuple index ._{} is out of bounds for {}",
                        index,
                        self.ty_name(source_ty)
                    ),
                    span: span.clone(),
                    hint: Some(Self::tuple_index_hint(items.len())),
                })?;
                Ok((
                    TypedFacetSegment::Tuple {
                        field_index: index as u32,
                        tuple_len: items.len() as u32,
                    },
                    field_ty,
                    false,
                ))
            }
            Ty::Struct(name, nominal) | Ty::Record(name, nominal) => {
                let fields = self.nominal_fields(&name, &nominal, span)?;
                let origin_index = if matches!(self.resolve_ty(source_ty), Ty::Record(..)) {
                    let numbered = field.strip_prefix('_').is_some_and(|digits| {
                        !digits.is_empty() && digits.bytes().all(|ch| ch.is_ascii_digit())
                    });
                    let parsed = Self::parse_tuple_index_name(field);
                    if numbered && parsed.is_none() {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Record index .{field} is out of bounds for {}",
                                self.ty_name(source_ty)
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    parsed
                } else {
                    None
                };
                let resolved_field = if let Some(index) = origin_index {
                    fields
                        .get(index)
                        .map(|(name, _)| name.as_str())
                        .ok_or_else(|| TypeError {
                            structured: None,
                            message: format!(
                                "Record index ._{index} is out of bounds for {}",
                                self.ty_name(source_ty)
                            ),
                            span: span.clone(),
                            hint: None,
                        })?
                } else {
                    field.as_str()
                };
                let field_policy = self.env.field_policy(&name, resolved_field);
                if field_policy.is_some_and(|policy| policy.private) {
                    let display_name = Self::surface_name(&name);
                    let outside_impl =
                        self.current_private_field_owner.as_deref() != Some(display_name);
                    if outside_impl && !self.allow_private_facet_inspection {
                        return Err(TypeError {
                            structured: None,
                            message: format!("Field '{}.{}' is private", display_name, field),
                            span: span.clone(),
                            hint: Some(format!(
                                "Expose the value through a public method on {} instead.",
                                display_name
                            )),
                        });
                    }
                }
                let (field_index, field_ty) = fields
                    .iter()
                    .enumerate()
                    .find(|(_, (field_name, _))| field_name == resolved_field)
                    .map(|(i, (_, ty))| (i as u32, ty.clone()))
                    .ok_or_else(|| TypeError {
                        structured: None,
                        message: format!("No field '{}' on {}", field, self.ty_name(source_ty)),
                        span: span.clone(),
                        hint: None,
                    })?;
                Ok((
                    TypedFacetSegment::Field {
                        field_name: resolved_field.to_string(),
                        origin_index: origin_index.map(|index| index as u32),
                        field_index,
                        container_field_count: fields.len() as u32,
                        container_type_name: Self::surface_name(&name).to_string(),
                        readonly: field_policy.is_some_and(|policy| policy.readonly),
                        private: field_policy.is_some_and(|policy| policy.private),
                    },
                    field_ty,
                    false,
                ))
            }
            source if Self::facet_enum_source_name(&source).is_some() => {
                let enum_name =
                    Self::facet_enum_source_name(&source).expect("checked enum-like Facet source");
                if self.lookup_enum_variants_of(&enum_name).is_none() {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "No variants found for enum {}",
                            Self::surface_name(&enum_name)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let Some(variant) = self.lookup_enum_variant_by_short_name(&enum_name, field)
                else {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "No variant selector '{}' on {} (use PascalCase constructor names)",
                            field,
                            Self::surface_name(&enum_name)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                };
                let variant = self.instantiate_enum_variant(&variant);
                if !self.types_compatible(&variant.enum_ty, source_ty)? {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Variant selector {}.{} does not match {}",
                            enum_name,
                            field,
                            self.ty_name(source_ty)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let payload_arity = variant.payload.len() as u32;
                let focus_ty = match variant.payload.len() {
                    0 => Ty::Unit,
                    1 => variant.payload[0].clone(),
                    _ => Ty::Tuple(variant.payload.clone()),
                };
                Ok((
                    TypedFacetSegment::Variant {
                        enum_name,
                        variant_name: variant.short_name,
                        variant_tag: variant.tag,
                        discriminant: variant.discriminant,
                        payload_arity,
                        optional: *optional,
                    },
                    focus_ty,
                    true,
                ))
            }
            other => {
                let message = if field.starts_with('_')
                    && field
                        .strip_prefix('_')
                        .is_some_and(|suffix| suffix.chars().all(|ch| ch.is_ascii_digit()))
                {
                    format!(
                        "Tuple-style access .{} is only available on tuples, got {}",
                        field,
                        self.ty_name(&other)
                    )
                } else {
                    format!("Cannot access field on {}", self.ty_name(&other))
                };
                Err(TypeError {
                    structured: None,
                    message,
                    span: span.clone(),
                    hint: None,
                })
            }
        }
    }

    fn facet_enum_source_name(source_ty: &Ty) -> Option<Symbol> {
        match source_ty {
            Ty::Enum(enum_name, _) => Some(enum_name.clone()),
            Ty::Bool => Some("Boolean".into()),
            _ => None,
        }
    }

    fn try_check_tuple_type_root_facet_path(
        &mut self,
        span: &Span,
        field: &str,
        expected: Option<&Ty>,
    ) -> Result<Option<TypedNode>, TypeError> {
        let Some(index) = Self::parse_tuple_index_name(field) else {
            return Ok(None);
        };

        let Some(expected_ty) = expected else {
            return Ok(Some(self.pending_facet_node(
                span,
                PendingFacetPath {
                    root_path_name: Some("Tuple".into()),
                    source_ty_hint: None,
                    segments: vec![Self::pending_field_segment(field)],
                },
            )));
        };
        let expected_ty = self.resolve_ty(expected_ty);
        let (expected_source, expected_focus) = match expected_ty {
            Ty::Facet(_, source, focus, ..) => (source.as_ref().clone(), focus.as_ref().clone()),
            other => {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Tuple.{} requires expected Facet<..., ...> context, got {}",
                        field,
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: Some(
                        "Use Tuple._N as a Facet path argument in Facet::view/set/over.".into(),
                    ),
                });
            }
        };

        let tuple_items = match self.resolve_ty(&expected_source) {
            Ty::Tuple(items) => items,
            other => {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Tuple.{} requires tuple source context, got {}",
                        field,
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: Some("Expected source type like (A, B, ...) for Tuple._N.".into()),
                });
            }
        };

        let focus_ty = tuple_items.get(index).cloned().ok_or_else(|| TypeError {
            structured: None,
            message: format!(
                "Tuple index ._{} is out of bounds for ({})",
                index,
                tuple_items
                    .iter()
                    .map(|item| self.ty_name(item))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            span: span.clone(),
            hint: Some(Self::tuple_index_hint(tuple_items.len())),
        })?;

        if !self.types_compatible(&focus_ty, &expected_focus)? {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Tuple.{} focus type mismatch: expected {}, got {}",
                    field,
                    self.ty_name(&expected_focus),
                    self.ty_name(&focus_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let source_ty = Ty::Tuple(tuple_items);
        let path = TypedFacetPath {
            source_ty: source_ty.clone(),
            focus_ty: focus_ty.clone(),
            segment_focus_tys: vec![focus_ty.clone()],
            update_source_ty: Ty::Hole,
            update_focus_ty: Ty::Hole,
            path_kind: TypedFacetPathKind::InfallibleStructural,
            may_fail: false,
            segments: vec![TypedFacetSegment::Tuple {
                field_index: index as u32,
                tuple_len: match &source_ty {
                    Ty::Tuple(items) => items.len() as u32,
                    _ => unreachable!("source_ty is always Tuple here"),
                },
            }],
        };

        Ok(Some(TypedNode {
            ty: Self::deferred_facet_ty(path.path_kind, source_ty, focus_ty),
            span: span.clone(),
            node: TypedInner::FacetPath(path),
        }))
    }

    fn try_check_container_type_root_facet_path(
        &mut self,
        span: &Span,
        root_kind: FacetRootKind,
        segment: &PendingFacetSegment,
        expected: Option<&Ty>,
    ) -> Result<Option<TypedNode>, TypeError> {
        let root_path_name = match (root_kind, segment) {
            (
                FacetRootKind::List,
                PendingFacetSegment::Bracket { .. } | PendingFacetSegment::RangeBracket { .. },
            ) => "List",
            (FacetRootKind::HashMap, PendingFacetSegment::Bracket { .. }) => "HashMap",
            _ => return Ok(None),
        };

        let Some(expected_ty) = expected else {
            return Ok(Some(self.pending_facet_node(
                span,
                PendingFacetPath {
                    root_path_name: Some(root_path_name.into()),
                    source_ty_hint: None,
                    segments: vec![segment.clone()],
                },
            )));
        };
        let expected_ty = self.resolve_ty(expected_ty);
        let (expected_source, expected_focus) = match expected_ty {
            Ty::Facet(_, source, focus, ..) => (source.as_ref().clone(), focus.as_ref().clone()),
            other => {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "{root_path_name} root Facet path requires expected Facet<..., ...> context, got {}",
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: Some(format!(
                        "Use {root_path_name} root paths as Facet path arguments in Facet::view/set/over."
                    )),
                });
            }
        };

        self.validate_pending_root_source(root_path_name, &expected_source, span)?;
        let (typed_segment, focus_ty, may_fail) =
            self.resolve_facet_segment_for_source_ty(&expected_source, segment, span, true)?;
        let focus_ty = self.resolve_ty(&focus_ty);
        if !self.types_compatible(&focus_ty, &expected_focus)? {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{root_path_name} root Facet path focus type mismatch: expected {}, got {}",
                    self.ty_name(&expected_focus),
                    self.ty_name(&focus_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let source_ty = self.resolve_ty(&expected_source);
        let path = TypedFacetPath {
            source_ty: source_ty.clone(),
            focus_ty: focus_ty.clone(),
            segment_focus_tys: vec![focus_ty.clone()],
            update_source_ty: Ty::Hole,
            update_focus_ty: Ty::Hole,
            path_kind: Self::facet_path_kind_for_segments(std::slice::from_ref(&typed_segment)),
            may_fail,
            segments: vec![typed_segment],
        };

        Ok(Some(TypedNode {
            ty: Self::deferred_facet_ty(path.path_kind, source_ty, focus_ty),
            span: span.clone(),
            node: TypedInner::FacetPath(path),
        }))
    }

    pub(in crate::checker::expr) fn facet_type_root_matches_source(
        root: &Resolved,
        source: &Ty,
    ) -> bool {
        let Resolved::Var(_, id) = root else {
            return false;
        };
        let authored_owner = id.qualified_name.as_deref().unwrap_or(&id.name);
        let expected_owner = match source {
            Ty::Struct(name, _) | Ty::Record(name, _) | Ty::Enum(name, _) => name.as_str(),
            Ty::Bool => "Boolean",
            Ty::Result(_, _) => "Result",
            Ty::Error => "Error",
            _ => return false,
        };
        Self::canonical_user_type_name(authored_owner)
            == Self::canonical_user_type_name(expected_owner)
    }

    fn check_type_root_facet_path_with_expected(
        &mut self,
        span: &Span,
        expr: &Resolved,
        segment: &PendingFacetSegment,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let (source_ty, expected_focus_ty) = match expected.map(|ty| self.resolve_ty(ty)) {
            Some(Ty::Facet(_, source, focus, ..)) => {
                let expected_source_ty = source.as_ref().clone();
                let authored_owner = match expr {
                    Resolved::Var(_, id) => id.qualified_name.as_deref().unwrap_or(&id.name),
                    _ => unreachable!("type-root Facet paths are resolved variables"),
                };
                let same_owner = Self::facet_type_root_matches_source(
                    expr,
                    &self.resolve_ty(&expected_source_ty),
                );
                if !same_owner {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Facet path source owner mismatch: path root is {}, source is {}",
                            Self::surface_name(authored_owner),
                            self.ty_name(&expected_source_ty)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                (expected_source_ty, Some(focus.as_ref().clone()))
            }
            _ => {
                if let Resolved::Var(_, id) = expr {
                    if sindr::names::builtin_type_name(Self::surface_name(
                        id.qualified_name.as_deref().unwrap_or(&id.name),
                    )) == Some(sindr::names::TypeName::Error)
                    {
                        (Ty::Error, None)
                    } else {
                        let typed_root = self.check_node(expr)?;
                        (self.resolve_ty(&typed_root.ty), None)
                    }
                } else {
                    unreachable!("type-root Facet paths are resolved variables")
                }
            }
        };
        let (segment, focus_ty, may_fail) =
            self.resolve_facet_segment_for_source_ty(&source_ty, segment, span, true)?;
        let focus_ty = self.resolve_ty(&focus_ty);
        if let Some(expected_focus_ty) = expected_focus_ty {
            if !self.types_compatible(&focus_ty, &expected_focus_ty)? {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Facet path focus type mismatch: expected {}, got {}",
                        self.ty_name(&expected_focus_ty),
                        self.ty_name(&focus_ty)
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }
        }
        let path = TypedFacetPath {
            source_ty: source_ty.clone(),
            focus_ty: focus_ty.clone(),
            segment_focus_tys: vec![focus_ty.clone()],
            update_source_ty: Ty::Hole,
            update_focus_ty: Ty::Hole,
            path_kind: Self::facet_path_kind_for_segments(std::slice::from_ref(&segment)),
            may_fail,
            segments: vec![segment],
        };
        Ok(TypedNode {
            ty: Self::deferred_facet_ty(path.path_kind, source_ty, focus_ty),
            span: span.clone(),
            node: TypedInner::FacetPath(path),
        })
    }

    pub(in crate::checker::expr) fn facet_root_kind(expr: &Resolved) -> Option<FacetRootKind> {
        let Resolved::Var(_, id) = expr else {
            return None;
        };
        id.symbol_info
            .as_ref()
            .and_then(|info| info.capabilities.facet_root_path.clone())
    }

    pub(in crate::checker::expr) fn known_non_facet_root_name(expr: &Resolved) -> Option<&str> {
        let Resolved::Var(_, id) = expr else {
            return None;
        };
        id.symbol_info
            .as_ref()
            .filter(|info| info.capabilities.facet_root_path.is_none())
            .map(|_| id.name.as_str())
    }

    fn known_non_facet_root_error(root: &str, span: &Span) -> TypeError {
        TypeError {
            structured: None,
            message: format!(
                "{root} is not a Facet path root; primitive types are not path-constructable"
            ),
            span: span.clone(),
            hint: Some(
                "Use a defstruct/defrecord/defenum root, Tuple._N, List.[N], HashMap.[\"key\"], or a visible Facet binding."
                    .into(),
            ),
        }
    }

    pub(in crate::checker::expr) fn resolved_concrete_error_identity<'a>(
        &'a self,
        expression: &Resolved,
    ) -> Option<&'a str> {
        match expression {
            Resolved::Var(_, id) => self
                .canonical_pattern_id(id)
                .ok()
                .and_then(|id| self.env.concrete_error_bindings.get(&id.unique_id))
                .map(String::as_str),
            Resolved::Grouped(_, inner) => self.resolved_concrete_error_identity(inner),
            _ => None,
        }
    }

    pub(in crate::checker) fn concrete_error_annotation_identity(
        &self,
        annotation: &AstTy,
    ) -> Option<String> {
        let AstTy::Named(_, name) = annotation else {
            return None;
        };
        self.env
            .lookup_type_def(name)
            .filter(|def| def.kind == crate::env::TypeKind::ConcreteError)
            .map(|def| def.name.clone())
    }

    fn error_payload_facet_path(
        &self,
        kind: &str,
        field: &str,
        span: &Span,
    ) -> Result<TypedFacetPath, TypeError> {
        let schema = self.env.lookup_type_def(kind).ok_or_else(|| {
            TypeError::new("Concrete Error Payload schema is missing", span.clone())
        })?;
        let (field_index, (_, focus_ty)) = schema
            .fields
            .iter()
            .enumerate()
            .find(|(_, (name, _))| name == field)
            .ok_or_else(|| {
                TypeError::new(
                    format!("Unknown Error Payload field: {}", field),
                    span.clone(),
                )
            })?;
        Ok(TypedFacetPath {
            source_ty: Ty::Error,
            focus_ty: focus_ty.clone(),
            segment_focus_tys: vec![focus_ty.clone()],
            update_source_ty: Ty::Hole,
            update_focus_ty: Ty::Hole,
            path_kind: TypedFacetPathKind::InfallibleStructural,
            may_fail: false,
            segments: vec![TypedFacetSegment::ErrorPayload {
                kind: kind.to_string(),
                field_name: field.to_string(),
                field_index: field_index as u32,
                payload_len: schema.fields.len() as u32,
            }],
        })
    }

    pub(in crate::checker::expr) fn check_field_access_with_expected(
        &mut self,
        span: &Span,
        expr: &Resolved,
        field: &str,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let segment = ResolvedFacetPathSegment::Field {
            name: field.to_string(),
            optional: false,
        };
        self.check_facet_segment_access_with_expected(span, expr, &segment, expected)
    }

    pub(in crate::checker::expr) fn check_facet_segment_access_with_expected(
        &mut self,
        span: &Span,
        expr: &Resolved,
        syntax_segment: &ResolvedFacetPathSegment,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let pending_segment = Self::pending_segment_from_syntax(syntax_segment);
        let field = match syntax_segment {
            ResolvedFacetPathSegment::Field {
                name,
                optional: false,
            } => Some(name.as_str()),
            _ => None,
        };

        if let (Resolved::Var(_, id), Some(field)) = (expr, field) {
            if self.env.is_error_constructor(id.unique_id) {
                let kind = id.qualified_name.as_deref().ok_or_else(|| {
                    TypeError::new(
                        "Error Facet root is missing declaration identity",
                        span.clone(),
                    )
                })?;
                if matches!(field, "kind" | "message") {
                    let segment = Self::pending_field_segment(field.to_string());
                    let (segment, focus_ty, may_fail) =
                        self.resolve_facet_segment_for_source_ty(&Ty::Error, &segment, span, true)?;
                    let path = TypedFacetPath {
                        source_ty: Ty::Error,
                        focus_ty: focus_ty.clone(),
                        segment_focus_tys: vec![focus_ty.clone()],
                        update_source_ty: Ty::Hole,
                        update_focus_ty: Ty::Hole,
                        path_kind: TypedFacetPathKind::InfallibleStructural,
                        may_fail,
                        segments: vec![segment],
                    };
                    return Ok(TypedNode {
                        ty: Self::deferred_facet_ty(path.path_kind, Ty::Error, focus_ty),
                        span: span.clone(),
                        node: TypedInner::FacetPath(path),
                    });
                }
                let path = self.error_payload_facet_path(kind, field, span)?;
                return Ok(TypedNode {
                    ty: Self::deferred_facet_ty(path.path_kind, Ty::Error, path.focus_ty.clone()),
                    span: span.clone(),
                    node: TypedInner::FacetPath(path),
                });
            }
            if let Some(kind) = self.env.concrete_error_bindings.get(&id.unique_id).cloned() {
                if !matches!(field, "kind" | "message") {
                    let path = self.error_payload_facet_path(&kind, field, span)?;
                    let source = self.check_node(expr)?;
                    return Ok(TypedNode {
                        ty: path.focus_ty.clone(),
                        span: span.clone(),
                        node: TypedInner::FacetView {
                            api: TypedFacetApi::View,
                            source: Box::new(source),
                            path,
                            source_is_result: false,
                        },
                    });
                }
            }
        }
        if let Some(root_kind) = Self::facet_root_kind(expr) {
            match root_kind {
                FacetRootKind::TypeRoot => {
                    return self.check_type_root_facet_path_with_expected(
                        span,
                        expr,
                        &pending_segment,
                        expected,
                    );
                }
                FacetRootKind::Tuple => {
                    if let Some(field) = field {
                        if let Some(tuple_root_path) =
                            self.try_check_tuple_type_root_facet_path(span, field, expected)?
                        {
                            return Ok(tuple_root_path);
                        }
                    }
                }
                FacetRootKind::List => {
                    if let Some(container_root_path) = self
                        .try_check_container_type_root_facet_path(
                            span,
                            FacetRootKind::List,
                            &pending_segment,
                            expected,
                        )?
                    {
                        return Ok(container_root_path);
                    }
                }
                FacetRootKind::HashMap => {
                    if let Some(container_root_path) = self
                        .try_check_container_type_root_facet_path(
                            span,
                            FacetRootKind::HashMap,
                            &pending_segment,
                            expected,
                        )?
                    {
                        return Ok(container_root_path);
                    }
                }
            }
        }
        if let Some(root) = Self::known_non_facet_root_name(expr) {
            return Err(Self::known_non_facet_root_error(root, span));
        }
        let typed_expr = self.check_node(expr)?;

        if matches!(typed_expr.ty, Ty::Facet(..)) {
            let path = self.resolve_facet_path_from_node(typed_expr, span, None)?;
            let (segment_source_ty, result_transparent) = match self.resolve_ty(&path.focus_ty) {
                Ty::Result(ok, _) => (ok.as_ref().clone(), true),
                other => (other, false),
            };
            let (segment, focus_ty, may_fail) = self.resolve_facet_segment_for_source_ty(
                &segment_source_ty,
                &pending_segment,
                span,
                true,
            )?;
            let source_ty = self.resolve_ty(&path.source_ty);
            let focus_ty = self.resolve_ty(&focus_ty);
            let mut segments = path.segments;
            segments.push(segment);
            let mut segment_focus_tys = path.segment_focus_tys;
            segment_focus_tys.push(focus_ty.clone());
            let combined = TypedFacetPath {
                source_ty: source_ty.clone(),
                focus_ty: focus_ty.clone(),
                update_source_ty: Ty::Hole,
                update_focus_ty: Ty::Hole,
                path_kind: Self::facet_path_kind_for_segments(&segments),
                may_fail: path.may_fail || may_fail || result_transparent,
                segment_focus_tys,
                segments,
            };
            return Ok(TypedNode {
                ty: Self::deferred_facet_ty(combined.path_kind, source_ty, focus_ty),
                span: span.clone(),
                node: TypedInner::FacetPath(combined),
            });
        }

        let (source_is_result, source_focus_ty) = match self.resolve_ty(&typed_expr.ty) {
            Ty::Result(ok, _) => (true, ok.as_ref().clone()),
            other => (false, other),
        };
        let (segment, focus_ty, may_fail) = self.resolve_facet_segment_for_source_ty(
            &source_focus_ty,
            &pending_segment,
            span,
            false,
        )?;
        let focus_ty = self.resolve_ty(&focus_ty);
        let path = TypedFacetPath {
            source_ty: source_focus_ty,
            focus_ty: focus_ty.clone(),
            segment_focus_tys: vec![focus_ty.clone()],
            update_source_ty: Ty::Hole,
            update_focus_ty: Ty::Hole,
            path_kind: Self::facet_path_kind_for_segments(std::slice::from_ref(&segment)),
            may_fail,
            segments: vec![segment],
        };
        let out_ty = if source_is_result || path.may_fail {
            Ty::Result(Box::new(focus_ty), Box::new(Ty::Error))
        } else {
            focus_ty
        };

        Ok(TypedNode {
            ty: out_ty,
            span: span.clone(),
            node: TypedInner::FacetView {
                api: TypedFacetApi::View,
                source: Box::new(typed_expr),
                path,
                source_is_result,
            },
        })
    }

    pub(in crate::checker) fn check_field_access(
        &mut self,
        span: &Span,
        expr: &Resolved,
        field: &str,
    ) -> Result<TypedNode, TypeError> {
        self.check_field_access_with_expected(span, expr, field, None)
    }

    // Only direct unary FacetPath captures depend on a source supplied by the
    // surrounding call. Callable shape errors are checked immediately.
    pub(in crate::checker::expr) fn pending_facet_capture_source(
        &self,
        expr: &Resolved,
        expected: &Ty,
    ) -> Option<Ty> {
        let segments = Self::facet_capture_segments(expr)?;
        let Ty::Func(params, _) = self.resolve_ty(expected) else {
            return None;
        };
        let [source] = params.as_slice() else {
            return None;
        };
        let root = Self::facet_capture_root(expr);
        if root.is_some_and(|root| Self::known_non_facet_root_name(root).is_some()) {
            return None;
        }
        if !matches!(source, Ty::Var(_) | Ty::Hole) {
            if let Some(root) = root {
                let valid_shape = match Self::facet_root_kind(root) {
                    Some(FacetRootKind::TypeRoot) => {
                        Self::facet_type_root_matches_source(root, source)
                    }
                    Some(FacetRootKind::Tuple) => matches!(source, Ty::Tuple(_)),
                    Some(FacetRootKind::List) => matches!(source, Ty::List(_)),
                    Some(FacetRootKind::HashMap) => matches!(source, Ty::Enum(name, args)
                        if Self::surface_name(name) == "HashMap" && args.len() == 1),
                    None => true,
                };
                if !valid_shape {
                    return None;
                }
            }
        }
        let mut receiver = source.clone();
        for (index, segment) in segments.iter().enumerate() {
            receiver = self.resolve_ty(&receiver);
            if matches!(&receiver, Ty::Var(var) if !self.rigid_tyvars.contains(var)) {
                return Some(receiver);
            }
            // The final focus need not be fixed: checking the capture against
            // its expected return type can determine it. Only receivers of
            // remaining segments require a known shape.
            if index + 1 == segments.len() {
                break;
            }
            receiver = self.facet_capture_dependency_focus(&receiver, segment)?;
        }
        None
    }
}
