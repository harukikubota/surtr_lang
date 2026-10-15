use super::trait_calls::{TraitInvocationContext, TraitInvocationPreparation};
use super::*;
use sindr::primitives::int;

use std::sync::atomic::{AtomicU32, Ordering};

static SYNTHETIC_RANGE_UID: AtomicU32 = AtomicU32::new(3_000_000_000);

impl Checker {
    pub(in crate::checker) fn check_list_nil(
        &mut self,
        span: &Span,
    ) -> Result<TypedNode, TypeError> {
        let tv = self.env.fresh_tyvar();
        Ok(TypedNode {
            ty: Ty::List(Box::new(tv)),
            span: span.clone(),
            node: TypedInner::ListNil,
        })
    }

    pub(in crate::checker) fn check_list_cons(
        &mut self,
        span: &Span,
        head: &Resolved,
        tail: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_head = self.check_node(head)?;
        let typed_tail = self.check_node(tail)?;
        self.ensure_no_runtime_facet_value(&typed_head, "List construction")?;
        self.ensure_no_runtime_facet_value(&typed_tail, "List construction")?;
        let tail_elem_ty = match &typed_tail.ty {
            Ty::List(inner) => inner.as_ref().clone(),
            other => {
                return Err(TypeError {
                    structured: None,
                    message: format!("list tail must be List<...>, got {}", self.ty_name(other)),
                    span: typed_tail.span.clone(),
                    hint: Some("Use `[head, ..tail]` with a list tail value".into()),
                });
            }
        };

        if !self.types_compatible(&typed_head.ty, &tail_elem_ty)? {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "expected {}, got {}",
                    self.ty_name(&tail_elem_ty),
                    self.ty_name(&typed_head.ty)
                ),
                span: typed_head.span.clone(),
                hint: Some("List head and tail element types must match".into()),
            });
        }

        let elem_ty = self.resolve_ty(&tail_elem_ty);
        Ok(TypedNode {
            ty: Ty::List(Box::new(elem_ty.clone())),
            span: span.clone(),
            node: TypedInner::ListCons(Box::new(typed_head), Box::new(typed_tail)),
        })
    }

    pub(in crate::checker) fn check_list_literal(
        &mut self,
        span: &Span,
        elems: &[Resolved],
    ) -> Result<TypedNode, TypeError> {
        if elems.is_empty() {
            return self.check_list_nil(span);
        }

        let typed_elems: Vec<TypedNode> = elems
            .iter()
            .map(|e| self.check_node(e))
            .collect::<Result<Vec<_>, _>>()?;
        for typed in &typed_elems {
            self.ensure_no_runtime_facet_value(typed, "List literal")?;
        }

        let elem_ty = typed_elems[0].ty.clone();
        for te in typed_elems.iter().skip(1) {
            if !self.types_compatible(&elem_ty, &te.ty)? {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "expected {}, got {}",
                        self.ty_name(&elem_ty),
                        self.ty_name(&te.ty)
                    ),
                    span: te.span.clone(),
                    hint: Some("All list elements must have the same type".into()),
                });
            }
        }

        Ok(TypedNode {
            ty: Ty::List(Box::new(elem_ty)),
            span: span.clone(),
            node: TypedInner::ListLiteral(typed_elems),
        })
    }

    pub(in crate::checker) fn check_hash_map_literal(
        &mut self,
        span: &Span,
        entries: &[sigil::resolved::ResolvedHashMapLiteralEntry],
    ) -> Result<TypedNode, TypeError> {
        if entries.is_empty() {
            return Ok(TypedNode {
                ty: Ty::Enum("HashMap".into(), vec![self.env.fresh_tyvar()]),
                span: span.clone(),
                node: TypedInner::HashMapLiteral(Vec::new()),
            });
        }

        let mut typed_entries = Vec::new();
        for entry in entries {
            let typed_key = self.check_node(&entry.key)?;
            self.ensure_no_runtime_facet_value(&typed_key, "HashMap literal key")?;
            if !self.types_compatible(&Ty::Str, &typed_key.ty)? {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "HashMap literal key must be String, got {}",
                        self.ty_name(&typed_key.ty)
                    ),
                    span: typed_key.span.clone(),
                    hint: Some(
                        "Use `hash![key_expr => value]` with a key expression that returns String."
                            .into(),
                    ),
                });
            }

            let typed_value = self.check_node(&entry.value)?;
            self.ensure_no_runtime_facet_value(&typed_value, "HashMap literal value")?;
            typed_entries.push((typed_key, typed_value));
        }

        let value_ty = typed_entries[0].1.ty.clone();
        for (_, value) in typed_entries.iter().skip(1) {
            if !self.types_compatible(&value_ty, &value.ty)? {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "expected {}, got {}",
                        self.ty_name(&value_ty),
                        self.ty_name(&value.ty)
                    ),
                    span: value.span.clone(),
                    hint: Some("All HashMap literal values must have the same type".into()),
                });
            }
        }

        Ok(TypedNode {
            ty: Ty::Enum("HashMap".into(), vec![value_ty]),
            span: span.clone(),
            node: TypedInner::HashMapLiteral(typed_entries),
        })
    }

    pub(in crate::checker) fn check_range_literal(
        &mut self,
        span: &Span,
        start: &Resolved,
        stop: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_start = self.check_node(start)?;
        let typed_stop = self.check_node(stop)?;
        self.ensure_no_runtime_facet_value(&typed_start, "Range literal")?;
        self.ensure_no_runtime_facet_value(&typed_stop, "Range literal")?;

        let start_ty = self.resolve_ty(&typed_start.ty);
        let stop_ty = self.resolve_ty(&typed_stop.ty);
        match (&start_ty, &stop_ty) {
            (Ty::Int, Ty::Int) => {
                if let (Some(start_int), Some(stop_int)) = (
                    Self::typed_int_literal_value(&typed_start),
                    Self::typed_int_literal_value(&typed_stop),
                ) {
                    return Ok(self.fold_int_range_literal(span, start_int, stop_int));
                }
                self.lower_int_range_runtime(span, start.clone(), stop.clone())
            }
            (Ty::Str, Ty::Str) => {
                if let (Some(start_str), Some(stop_str)) = (
                    Self::typed_string_literal_value(&typed_start),
                    Self::typed_string_literal_value(&typed_stop),
                ) {
                    if let Some((start_cp, stop_cp)) =
                        Self::foldable_string_range_endpoints(&start_str, &stop_str)
                    {
                        return Ok(self.fold_string_range_literal(span, start_cp, stop_cp));
                    }
                }
                self.lower_string_range_runtime(span, start.clone(), stop.clone())
            }
            _ => Err(TypeError {
                structured: None,
                message: "range literal endpoints must both be Int or both be String".into(),
                span: span.clone(),
                hint: Some("Use `[start..stop]` with matching Int endpoints or matching single-char String endpoints.".into()),
            }),
        }
    }

    fn typed_int_literal_value(node: &TypedNode) -> Option<sindr::primitives::SurtrInt> {
        match &node.node {
            TypedInner::Lit(Lit::Int(value)) => Some(value.clone()),
            _ => None,
        }
    }

    fn typed_string_literal_value(node: &TypedNode) -> Option<String> {
        match &node.node {
            TypedInner::Lit(Lit::Str(value)) => Some(value.clone()),
            _ => None,
        }
    }

    fn fold_int_range_literal(
        &mut self,
        span: &Span,
        start: sindr::primitives::SurtrInt,
        stop: sindr::primitives::SurtrInt,
    ) -> TypedNode {
        let mut current = start;
        let mut elems = Vec::new();
        while current <= stop {
            elems.push(TypedNode {
                ty: Ty::Int,
                span: span.clone(),
                node: TypedInner::Lit(Lit::Int(current.clone())),
            });
            current += int(1);
        }

        TypedNode {
            ty: Ty::List(Box::new(Ty::Int)),
            span: span.clone(),
            node: TypedInner::ListLiteral(elems),
        }
    }

    fn fold_string_range_literal(&mut self, span: &Span, start_cp: u8, stop_cp: u8) -> TypedNode {
        let mut elems = Vec::new();
        let mut current = start_cp;
        while current <= stop_cp {
            elems.push(TypedNode {
                ty: Ty::Str,
                span: span.clone(),
                node: TypedInner::Lit(Lit::Str((current as char).to_string())),
            });
            current += 1;
        }

        let list_node = TypedNode {
            ty: Ty::List(Box::new(Ty::Str)),
            span: span.clone(),
            node: TypedInner::ListLiteral(elems),
        };
        TypedNode {
            ty: Ty::Result(Box::new(Ty::List(Box::new(Ty::Str))), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::ConstructorCall(0, vec![list_node]),
        }
    }

    fn foldable_string_range_endpoints(start: &str, stop: &str) -> Option<(u8, u8)> {
        Some((
            Self::single_ascii_range_endpoint_value(start)?,
            Self::single_ascii_range_endpoint_value(stop)?,
        ))
    }

    fn single_ascii_range_endpoint_value(value: &str) -> Option<u8> {
        let mut chars = value.chars();
        let ch = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        if !ch.is_ascii() {
            return None;
        }
        Some(ch as u8)
    }

    fn canonical_enum_constructor_id(
        &self,
        name: &str,
        span: &Span,
    ) -> Result<ResolvedId, TypeError> {
        let meta = sindr::names::special_enum_variant_meta(name)
            .ok_or_else(|| TypeError::new("invalid canonical variant", span.clone()))?;
        let (unique_id, variant) = self
            .env
            .enum_constructor_ids
            .iter()
            .find(|(_, variant)| variant.special_variant == Some(meta.lowering))
            .ok_or_else(|| {
                TypeError::new(format!("missing canonical variant `{name}`"), span.clone())
            })?;
        Ok(ResolvedId {
            name: variant.constructor_name.clone(),
            qualified_name: Some(variant.constructor_name.clone()),
            symbol_info: None,
            unique_id: *unique_id,
            compiler_generated: true,
            span: span.clone(),
        })
    }

    pub(super) fn runtime_helper_id(
        &self,
        qualified_name: &str,
        span: &Span,
    ) -> Result<ResolvedId, TypeError> {
        if let Some(id) = self.function_ids_by_name.get(qualified_name) {
            return Ok(id.clone());
        }
        if let Some(uid) = self.impl_method_uids.get(qualified_name) {
            if let Some(id) = self
                .function_ids_by_name
                .values()
                .find(|id| id.unique_id == *uid)
            {
                return Ok(id.clone());
            }
            let local_name = qualified_name
                .rsplit("::")
                .next()
                .unwrap_or(qualified_name)
                .to_string();
            return Ok(ResolvedId {
                name: local_name,
                qualified_name: Some(qualified_name.to_string()),
                symbol_info: None,
                unique_id: *uid,
                compiler_generated: true,
                span: span.clone(),
            });
        }
        Err(TypeError {
            structured: None,
            message: format!("internal error: missing std helper `{qualified_name}`"),
            span: span.clone(),
            hint: None,
        })
    }

    pub(in crate::checker) fn next_synthetic_range_uid() -> u32 {
        SYNTHETIC_RANGE_UID.fetch_add(1, Ordering::Relaxed)
    }

    fn lower_int_range_runtime(
        &mut self,
        span: &Span,
        start: Resolved,
        stop: Resolved,
    ) -> Result<TypedNode, TypeError> {
        let range_id = self.runtime_helper_id("Generator::range", span)?;
        let to_list_id = self.runtime_helper_id("Generator::to_list", span)?;
        let lowered = Resolved::App(
            span.clone(),
            Box::new(Resolved::Var(span.clone(), to_list_id)),
            vec![ResolvedRecordLitArg::Positional(Resolved::App(
                span.clone(),
                Box::new(Resolved::Var(span.clone(), range_id)),
                vec![
                    ResolvedRecordLitArg::Positional(start),
                    ResolvedRecordLitArg::Positional(stop),
                ],
            ))],
        );
        self.check_node(&lowered)
    }

    fn lower_string_range_runtime(
        &mut self,
        span: &Span,
        start: Resolved,
        stop: Resolved,
    ) -> Result<TypedNode, TypeError> {
        let range_char_id = self.runtime_helper_id("Generator::range_char", span)?;
        let to_list_id = self.runtime_helper_id("Generator::to_list", span)?;
        let gen_uid = Self::next_synthetic_range_uid();
        let gen_id = ResolvedId {
            name: "__range_gen".into(),
            qualified_name: None,
            symbol_info: None,
            unique_id: gen_uid,
            compiler_generated: true,
            span: span.clone(),
        };
        let lowered = Resolved::Block(
            span.clone(),
            vec![
                Resolved::SafeBind(
                    span.clone(),
                    ResolvedPattern::Var(gen_id.clone()),
                    Box::new(Resolved::App(
                        span.clone(),
                        Box::new(Resolved::Var(span.clone(), range_char_id)),
                        vec![
                            ResolvedRecordLitArg::Positional(start),
                            ResolvedRecordLitArg::Positional(stop),
                        ],
                    )),
                    span.clone(),
                ),
                Resolved::ConstructorCall(
                    span.clone(),
                    self.canonical_enum_constructor_id("Result::Ok", span)?,
                    vec![ResolvedRecordLitArg::Positional(Resolved::App(
                        span.clone(),
                        Box::new(Resolved::Var(span.clone(), to_list_id)),
                        vec![ResolvedRecordLitArg::Positional(Resolved::Var(
                            span.clone(),
                            gen_id,
                        ))],
                    ))],
                ),
            ],
        );
        self.check_node(&lowered)
    }

    pub(in crate::checker) fn check_tuple_literal(
        &mut self,
        span: &Span,
        elems: &[Resolved],
    ) -> Result<TypedNode, TypeError> {
        if elems.len() < 2 {
            return Err(TypeError {
                structured: None,
                message: "Tuple literals require at least 2 values".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let typed_elems = elems
            .iter()
            .map(|elem| self.check_node(elem))
            .collect::<Result<Vec<_>, _>>()?;
        for typed in &typed_elems {
            self.ensure_no_runtime_facet_value(typed, "Tuple literal")?;
        }
        let item_tys = typed_elems.iter().map(|elem| elem.ty.clone()).collect();

        Ok(TypedNode {
            ty: Ty::Tuple(item_tys),
            span: span.clone(),
            node: TypedInner::TupleLiteral(typed_elems),
        })
    }

    pub(in crate::checker) fn check_interpolated_str(
        &mut self,
        span: &Span,
        parts: &[ResolvedInterpolatedPart],
    ) -> Result<TypedNode, TypeError> {
        let mut typed_parts = Vec::new();
        for part in parts {
            match part {
                ResolvedInterpolatedPart::Text(s) => {
                    typed_parts.push(TypedInterpolatedPart::Text(s.clone()));
                }
                ResolvedInterpolatedPart::Expr(expr) => {
                    let show = self.trait_key_by_short_name("Show").ok_or_else(|| {
                        TypeError::new("Missing canonical Show trait", span.clone())
                    })?;
                    let args = [ResolvedRecordLitArg::Positional(expr.as_ref().clone())];
                    let call = TraitInvocationContext {
                        span: self.resolved_span(expr),
                        trait_name: &show,
                        method_name: "to_string",
                        args: &args,
                        receiver_owner_hint: None,
                        expected_ret_ty: Some(&Ty::Str),
                        expected_ret_relation: None,
                        argument_expected_relation: None,
                        constructor_failure_reason: None,
                        explicit_type_args: None,
                        operator: None,
                        receiver_hint: None,
                        defer_incompatible_result_context: false,
                    };
                    let mut prepared = match self.prepare_trait_invocation(&call)? {
                        TraitInvocationPreparation::Arguments(prepared) => prepared,
                        TraitInvocationPreparation::Complete(_) => {
                            return Err(TypeError::new(
                                "Show interpolation unexpectedly completed without checking its argument",
                                self.resolved_span(expr).clone(),
                            ));
                        }
                    };
                    let typed_args = self.check_prepared_trait_arguments(&call, &mut prepared)?;
                    let typed_expr = &typed_args[0];
                    self.ensure_no_runtime_facet_value(typed_expr, "String interpolation")?;
                    if matches!(self.resolve_ty(&typed_expr.ty), Ty::Result(_, _)) {
                        return Err(TypeError {
                            structured: None,
                            message: "Interpolation does not allow Result type".into(),
                            span: typed_expr.span.clone(),
                            hint: Some(
                                "Unwrap/match the Result first, or convert it to a printable value"
                                    .into(),
                            ),
                        });
                    }
                    let conversion = self.finish_trait_invocation(&call, prepared, typed_args)?;
                    typed_parts.push(TypedInterpolatedPart::Expr(Box::new(conversion)));
                }
            }
        }

        Ok(TypedNode {
            ty: Ty::Str,
            span: span.clone(),
            node: TypedInner::InterpolatedStr(typed_parts),
        })
    }
}
