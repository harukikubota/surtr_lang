use super::super::operators::ExpectedCallableSlot;
use super::*;
use diagnostics::TypeDiagnosticReason;

impl Checker {
    fn rebuild_facet_source_type(
        &mut self,
        source_ty: &Ty,
        segments: &[TypedFacetSegment],
        replacement_ty: &Ty,
        span: &Span,
    ) -> Result<Ty, TypeError> {
        if segments.is_empty() {
            return Ok(self.resolve_ty(replacement_ty));
        }

        let source_ty = self.resolve_ty(source_ty);
        // Result values encountered *inside* a path are transparent to
        // traversal, but remain part of the rebuilt source shape.
        if let Ty::Result(ok, err) = source_ty {
            return Ok(Ty::Result(
                Box::new(self.rebuild_facet_source_type(&ok, segments, replacement_ty, span)?),
                err,
            ));
        }

        let segment = &segments[0];
        let rest = &segments[1..];
        match (source_ty, segment) {
            (Ty::Tuple(mut items), TypedFacetSegment::Tuple { field_index, .. }) => {
                let index = *field_index as usize;
                let Some(field_ty) = items.get(index).cloned() else {
                    return Err(TypeError {
                        structured: None,
                        message: "Facet tuple path is inconsistent with its source type".into(),
                        span: span.clone(),
                        hint: None,
                    });
                };
                items[index] =
                    self.rebuild_facet_source_type(&field_ty, rest, replacement_ty, span)?;
                Ok(Ty::Tuple(items))
            }
            (Ty::Struct(name, mut fields), TypedFacetSegment::Field { field_index, .. }) => {
                let index = *field_index as usize;
                let Some((_, field_ty)) = fields.get(index).cloned() else {
                    return Err(TypeError {
                        structured: None,
                        message: "Facet field path is inconsistent with its source type".into(),
                        span: span.clone(),
                        hint: None,
                    });
                };
                let rebuilt_field =
                    self.rebuild_facet_source_type(&field_ty, rest, replacement_ty, span)?;
                self.ensure_named_facet_rebuild_is_unique(
                    &name,
                    index,
                    &fields.arguments,
                    &field_ty,
                    &rebuilt_field,
                    span,
                )?;
                fields.arguments = self.rebuild_named_facet_arguments(
                    &name,
                    index,
                    &fields.arguments,
                    &rebuilt_field,
                    span,
                )?;
                fields[index].1 = rebuilt_field;
                Ok(Ty::Struct(name, fields))
            }
            (Ty::Record(name, mut fields), TypedFacetSegment::Field { field_index, .. }) => {
                let index = *field_index as usize;
                let Some((_, field_ty)) = fields.get(index).cloned() else {
                    return Err(TypeError {
                        structured: None,
                        message: "Facet field path is inconsistent with its source type".into(),
                        span: span.clone(),
                        hint: None,
                    });
                };
                let rebuilt_field =
                    self.rebuild_facet_source_type(&field_ty, rest, replacement_ty, span)?;
                self.ensure_named_facet_rebuild_is_unique(
                    &name,
                    index,
                    &fields.arguments,
                    &field_ty,
                    &rebuilt_field,
                    span,
                )?;
                fields.arguments = self.rebuild_named_facet_arguments(
                    &name,
                    index,
                    &fields.arguments,
                    &rebuilt_field,
                    span,
                )?;
                fields[index].1 = rebuilt_field;
                Ok(Ty::Record(name, fields))
            }
            (Ty::List(inner), TypedFacetSegment::ListIndex { .. }) => {
                let rebuilt = self.rebuild_facet_source_type(&inner, rest, replacement_ty, span)?;
                if !self.types_compatible(&inner, &rebuilt)? {
                    return Err(TypeError {
                        structured: None,
                        message:
                            "Facet updates through List segments cannot change the element type"
                                .into(),
                        span: span.clone(),
                        hint: Some("Replace with the existing list element type.".into()),
                    });
                }
                Ok(Ty::List(inner))
            }
            (Ty::List(inner), TypedFacetSegment::ListRange { .. }) => {
                let expected = Ty::List(inner.clone());
                let rebuilt =
                    self.rebuild_facet_source_type(&expected, rest, replacement_ty, span)?;
                if !self.types_compatible(&expected, &rebuilt)? {
                    return Err(TypeError {
                        structured: None,
                        message:
                            "Facet updates through List segments cannot change the element type"
                                .into(),
                        span: span.clone(),
                        hint: Some("Replace with the existing list element type.".into()),
                    });
                }
                Ok(Ty::List(inner))
            }
            (Ty::Enum(name, args), TypedFacetSegment::MapKey { .. })
                if Self::surface_name(&name) == "HashMap" && args.len() == 1 =>
            {
                let value_ty = args[0].clone();
                let rebuilt =
                    self.rebuild_facet_source_type(&value_ty, rest, replacement_ty, span)?;
                if !self.types_compatible(&value_ty, &rebuilt)? {
                    return Err(TypeError {
                        structured: None,
                        message:
                            "Facet updates through HashMap segments cannot change the value type"
                                .into(),
                        span: span.clone(),
                        hint: Some("Replace with the existing HashMap value type.".into()),
                    });
                }
                Ok(Ty::Enum(name, args))
            }
            (
                source,
                TypedFacetSegment::Variant {
                    enum_name,
                    variant_name,
                    ..
                },
            ) => {
                let variant = self
                    .lookup_enum_variant_by_short_name(enum_name, variant_name)
                    .map(|variant| self.instantiate_enum_variant(&variant))
                    .ok_or_else(|| TypeError {
                        structured: None,
                        message: "Facet enum path is inconsistent with its source type".into(),
                        span: span.clone(),
                        hint: None,
                    })?;
                let enum_template = variant.enum_ty.clone();
                if !self.types_compatible(&variant.enum_ty, &source)? {
                    return Err(TypeError {
                        structured: None,
                        message: "Facet enum path is inconsistent with its source type".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let payload_template = match variant.payload.as_slice() {
                    [] => Ty::Unit,
                    [only] => only.clone(),
                    payload => Ty::Tuple(payload.to_vec()),
                };
                let original = self.resolve_ty(&payload_template);
                let focus =
                    self.rebuild_facet_source_type(&original, rest, replacement_ty, span)?;
                if self.resolve_ty(&original) == self.resolve_ty(&focus) {
                    return Ok(source);
                }
                let mut replacements = HashMap::new();
                self.collect_facet_rebuild_tyvar_replacements(
                    &payload_template,
                    &focus,
                    &mut replacements,
                    span,
                )?;
                self.ensure_enum_facet_rebuild_is_unique(
                    enum_name,
                    &enum_template,
                    &replacements,
                    span,
                )?;
                Ok(self.replace_facet_rebuild_tyvars(&enum_template, &replacements))
            }
            _ => Err(TypeError {
                structured: None,
                message: "Facet path is inconsistent with its source type".into(),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    fn count_tyvar_occurrences(&self, ty: &Ty, needle: u32) -> usize {
        match ty {
            Ty::Var(var) => usize::from(*var == needle),
            Ty::List(inner)
            | Ty::MatchResult(inner)
            | Ty::ExtractorClosure(inner)
            | Ty::Lazy(inner)
            | Ty::Pid(inner) => self.count_tyvar_occurrences(inner, needle),
            Ty::Tuple(items) | Ty::SelfApp(items) => items
                .iter()
                .map(|item| self.count_tyvar_occurrences(item, needle))
                .sum(),
            Ty::Func(params, ret) => {
                params
                    .iter()
                    .map(|param| self.count_tyvar_occurrences(param, needle))
                    .sum::<usize>()
                    + self.count_tyvar_occurrences(ret, needle)
            }
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                self.count_tyvar_occurrences(source, needle)
                    + self.count_tyvar_occurrences(focus, needle)
                    + self.count_tyvar_occurrences(update_source, needle)
                    + self.count_tyvar_occurrences(update_focus, needle)
            }
            Ty::Result(ok, err) => {
                self.count_tyvar_occurrences(ok, needle) + self.count_tyvar_occurrences(err, needle)
            }
            Ty::Enum(_, args) => args
                .iter()
                .map(|arg| self.count_tyvar_occurrences(arg, needle))
                .sum(),
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => nominal
                .arguments
                .iter()
                .map(|argument| self.count_tyvar_occurrences(argument, needle))
                .sum(),
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                params
                    .iter()
                    .map(|param| self.count_tyvar_occurrences(param, needle))
                    .sum::<usize>()
                    + self.count_tyvar_occurrences(ret, needle)
            }
            Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Error
            | Ty::Hole
            | Ty::ProcessMarker(_) => 0,
        }
    }

    fn collect_facet_rebuild_tyvar_replacements(
        &mut self,
        template: &Ty,
        replacement: &Ty,
        replacements: &mut HashMap<u32, Ty>,
        span: &Span,
    ) -> Result<(), TypeError> {
        if self.resolve_ty(template) == self.resolve_ty(replacement) {
            return Ok(());
        }
        match (template, replacement) {
            (Ty::Var(var), _) => {
                let replacement = self.resolve_ty(replacement);
                if let Some(existing) = replacements.get(var) {
                    if self.resolve_ty(existing) != replacement {
                        return Err(TypeError {
                            structured: None,
                            message: "Facet enum case update would assign incompatible types to the same generic parameter".into(),
                            span: span.clone(),
                            hint: Some("A type-changing enum case update must determine each generic parameter exactly once.".into()),
                        });
                    }
                } else {
                    replacements.insert(*var, replacement);
                }
                Ok(())
            }
            (Ty::MatchResult(template), Ty::MatchResult(replacement))
            | (Ty::ExtractorClosure(template), Ty::ExtractorClosure(replacement))
            | (Ty::List(template), Ty::List(replacement))
            | (Ty::Lazy(template), Ty::Lazy(replacement))
            | (Ty::Pid(template), Ty::Pid(replacement)) => self
                .collect_facet_rebuild_tyvar_replacements(
                    template,
                    replacement,
                    replacements,
                    span,
                ),
            (Ty::Tuple(templates), Ty::Tuple(replacements_ty))
            | (Ty::Enum(_, templates), Ty::Enum(_, replacements_ty)) => {
                if templates.len() != replacements_ty.len() {
                    return self.facet_rebuild_not_generic_error(span);
                }
                for (template, replacement) in templates.iter().zip(replacements_ty) {
                    self.collect_facet_rebuild_tyvar_replacements(
                        template,
                        replacement,
                        replacements,
                        span,
                    )?;
                }
                Ok(())
            }
            (
                Ty::Struct(template_name, template_fields),
                Ty::Struct(replacement_name, replacement_fields),
            )
            | (
                Ty::Record(template_name, template_fields),
                Ty::Record(replacement_name, replacement_fields),
            ) if template_name == replacement_name
                && template_fields.arguments.len() == replacement_fields.arguments.len() =>
            {
                for (template, replacement) in template_fields
                    .arguments
                    .iter()
                    .zip(&replacement_fields.arguments)
                {
                    self.collect_facet_rebuild_tyvar_replacements(
                        template,
                        replacement,
                        replacements,
                        span,
                    )?;
                }
                Ok(())
            }
            (
                Ty::Result(template_ok, template_err),
                Ty::Result(replacement_ok, replacement_err),
            ) => {
                self.collect_facet_rebuild_tyvar_replacements(
                    template_ok,
                    replacement_ok,
                    replacements,
                    span,
                )?;
                self.collect_facet_rebuild_tyvar_replacements(
                    template_err,
                    replacement_err,
                    replacements,
                    span,
                )
            }
            (Ty::SelfApp(template_items), replacement) => {
                let Some((template_witness, template_slots)) =
                    Self::constructor_application_parts(template_items)
                else {
                    return self.facet_rebuild_not_generic_error(span);
                };
                let resolved_replacement = self.resolve_ty(replacement);
                let replacement_slots = match &resolved_replacement {
                    Ty::SelfApp(replacement_items) => {
                        let Some((replacement_witness, replacement_slots)) =
                            Self::constructor_application_parts(replacement_items)
                        else {
                            return self.facet_rebuild_not_generic_error(span);
                        };
                        if self.resolve_ty(template_witness) != self.resolve_ty(replacement_witness)
                        {
                            return self.facet_rebuild_not_generic_error(span);
                        }
                        replacement_slots.to_vec()
                    }
                    _ => match self
                        .constructor_application_slots_for_witness(
                            template_witness,
                            template_slots.len(),
                            &resolved_replacement,
                        )
                        .into_checked()
                        .map_err(|error| error.at_span(span))?
                    {
                        ConstructorSlotsOutcome::Projected(slots) => slots,
                        ConstructorSlotsOutcome::Deferred { .. }
                        | ConstructorSlotsOutcome::Rejected { .. } => {
                            return self.facet_rebuild_not_generic_error(span);
                        }
                    },
                };
                if template_slots.len() != replacement_slots.len() {
                    return self.facet_rebuild_not_generic_error(span);
                }
                for (template_slot, replacement_slot) in
                    template_slots.iter().zip(&replacement_slots)
                {
                    self.collect_facet_rebuild_tyvar_replacements(
                        template_slot,
                        replacement_slot,
                        replacements,
                        span,
                    )?;
                }
                Ok(())
            }
            _ => self.facet_rebuild_not_generic_error(span),
        }
    }

    fn rebuild_named_facet_arguments(
        &mut self,
        type_name: &str,
        field_index: usize,
        arguments: &[Ty],
        rebuilt_field: &Ty,
        span: &Span,
    ) -> Result<Vec<Ty>, TypeError> {
        let Some(def) = self.env.lookup_type_def(type_name).cloned() else {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Facet cannot derive a rebuilt type for {}",
                    Self::surface_name(type_name)
                ),
                span: span.clone(),
                hint: None,
            });
        };
        if def.type_param_vars.is_empty() {
            return Ok(arguments.to_vec());
        }
        if arguments.len() != def.type_param_vars.len() {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Facet found inconsistent generic metadata for {}",
                    Self::surface_name(type_name)
                ),
                span: span.clone(),
                hint: Some(
                    "The nominal type argument list must match its declaration parameters.".into(),
                ),
            });
        }
        let Some((_, field_template)) = def.fields.get(field_index) else {
            return Err(TypeError {
                structured: None,
                message: "Facet field path is inconsistent with its source type".into(),
                span: span.clone(),
                hint: None,
            });
        };
        let type_param_vars = def.type_param_vars.clone();
        let field_template = field_template.clone();
        let mut replacements = HashMap::new();
        self.collect_facet_rebuild_tyvar_replacements(
            &field_template,
            rebuilt_field,
            &mut replacements,
            span,
        )?;
        let rebuilt_arguments = type_param_vars
            .iter()
            .zip(arguments)
            .map(|(variable, argument)| {
                replacements
                    .get(variable)
                    .cloned()
                    .unwrap_or_else(|| argument.clone())
            })
            .collect::<Vec<_>>();
        let rebuilt_fields = self.instantiate_type_def_fields(&def, &rebuilt_arguments);
        let Some((_, expected_field)) = rebuilt_fields.get(field_index) else {
            return Err(TypeError {
                structured: None,
                message: "Facet field path is inconsistent with its rebuilt source type".into(),
                span: span.clone(),
                hint: None,
            });
        };
        if !self.types_compatible(expected_field, rebuilt_field)? {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Facet replacement changes the constructor family of {}",
                    Self::surface_name(type_name)
                ),
                span: span.clone(),
                hint: Some(
                    "A type-changing Facet update must preserve constructor parameters and may only rebuild uniquely determined payload parameters."
                        .into(),
                ),
            });
        }
        Ok(rebuilt_arguments)
    }

    fn facet_rebuild_not_generic_error(&self, span: &Span) -> Result<(), TypeError> {
        Err(TypeError {
            structured: None,
            message: "Facet cannot change this enum case payload because it does not map to a generic parameter".into(),
            span: span.clone(),
            hint: Some("Only uniquely rebuildable generic enum cases support a type-changing Facet update.".into()),
        })
    }

    fn ensure_enum_facet_rebuild_is_unique(
        &self,
        enum_name: &str,
        enum_template: &Ty,
        replacements: &HashMap<u32, Ty>,
        span: &Span,
    ) -> Result<(), TypeError> {
        let enum_args = match enum_template {
            Ty::Enum(_, args) => args.iter().collect::<Vec<_>>(),
            Ty::Result(ok, _) => vec![ok.as_ref()],
            _ => return self.facet_rebuild_not_generic_error(span),
        };
        let variants = self
            .lookup_enum_variants_of(enum_name)
            .ok_or_else(|| TypeError {
                structured: None,
                message: "Facet enum path is inconsistent with its source type".into(),
                span: span.clone(),
                hint: None,
            })?;
        for changed_var in replacements.keys() {
            let Some(index) = enum_args
                .iter()
                .position(|arg| matches!(arg, Ty::Var(var) if *var == *changed_var))
            else {
                return self.facet_rebuild_not_generic_error(span);
            };
            let Some(original_var) = variants
                .first()
                .and_then(|variant| match &variant.enum_ty {
                    Ty::Enum(_, args) => args.get(index),
                    Ty::Result(ok, _) if index == 0 => Some(ok.as_ref()),
                    _ => None,
                })
                .and_then(|arg| match arg {
                    Ty::Var(var) => Some(*var),
                    _ => None,
                })
            else {
                return self.facet_rebuild_not_generic_error(span);
            };
            let occurrences = variants
                .iter()
                .flat_map(|variant| variant.payload.iter())
                .map(|payload| self.count_tyvar_occurrences(payload, original_var))
                .sum::<usize>();
            if occurrences != 1 {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Facet cannot rebuild {} because a generic parameter occurs outside the updated case payload",
                        Self::surface_name(enum_name)
                    ),
                    span: span.clone(),
                    hint: Some("A type-changing enum case update must determine each generic parameter uniquely.".into()),
                });
            }
        }
        Ok(())
    }

    fn replace_facet_rebuild_tyvars(&self, ty: &Ty, replacements: &HashMap<u32, Ty>) -> Ty {
        match ty {
            Ty::Var(var) => replacements
                .get(var)
                .cloned()
                .unwrap_or_else(|| self.resolve_ty(ty)),
            Ty::ExtractorClosure(inner) => Ty::ExtractorClosure(Box::new(
                self.replace_facet_rebuild_tyvars(inner, replacements),
            )),
            Ty::MatchResult(inner) => Ty::MatchResult(Box::new(
                self.replace_facet_rebuild_tyvars(inner, replacements),
            )),
            Ty::List(inner) => Ty::List(Box::new(
                self.replace_facet_rebuild_tyvars(inner, replacements),
            )),
            Ty::Lazy(inner) => Ty::Lazy(Box::new(
                self.replace_facet_rebuild_tyvars(inner, replacements),
            )),
            Ty::Pid(inner) => Ty::Pid(Box::new(
                self.replace_facet_rebuild_tyvars(inner, replacements),
            )),
            Ty::Tuple(items) => Ty::Tuple(
                items
                    .iter()
                    .map(|item| self.replace_facet_rebuild_tyvars(item, replacements))
                    .collect(),
            ),
            Ty::SelfApp(items) => Ty::SelfApp(
                items
                    .iter()
                    .map(|item| self.replace_facet_rebuild_tyvars(item, replacements))
                    .collect(),
            ),
            Ty::Func(params, ret) => Ty::Func(
                params
                    .iter()
                    .map(|param| self.replace_facet_rebuild_tyvars(param, replacements))
                    .collect(),
                Box::new(self.replace_facet_rebuild_tyvars(ret, replacements)),
            ),
            Ty::Facet(kind, source, focus, update_source, update_focus) => Ty::Facet(
                kind.clone(),
                Box::new(self.replace_facet_rebuild_tyvars(source, replacements)),
                Box::new(self.replace_facet_rebuild_tyvars(focus, replacements)),
                Box::new(self.replace_facet_rebuild_tyvars(update_source, replacements)),
                Box::new(self.replace_facet_rebuild_tyvars(update_focus, replacements)),
            ),
            Ty::Result(ok, err) => Ty::Result(
                Box::new(self.replace_facet_rebuild_tyvars(ok, replacements)),
                Box::new(self.replace_facet_rebuild_tyvars(err, replacements)),
            ),
            Ty::Enum(name, args) => Ty::Enum(
                name.clone(),
                args.iter()
                    .map(|arg| self.replace_facet_rebuild_tyvars(arg, replacements))
                    .collect(),
            ),
            Ty::Struct(name, fields) => Ty::Struct(
                name.clone(),
                fields.map_types(|ty| self.replace_facet_rebuild_tyvars(ty, replacements)),
            ),
            Ty::Record(name, fields) => Ty::Record(
                name.clone(),
                fields.map_types(|ty| self.replace_facet_rebuild_tyvars(ty, replacements)),
            ),
            Ty::BuiltinFunc { name, params, ret } => Ty::BuiltinFunc {
                name: name.clone(),
                params: params
                    .iter()
                    .map(|param| self.replace_facet_rebuild_tyvars(param, replacements))
                    .collect(),
                ret: Box::new(self.replace_facet_rebuild_tyvars(ret, replacements)),
            },
            Ty::UserFunc {
                fun_idx,
                type_params,
                call_substitution,
                params,
                ret,
            } => Ty::UserFunc {
                fun_idx: *fun_idx,
                type_params: type_params.clone(),
                call_substitution: call_substitution
                    .iter()
                    .map(|(var, ty)| (*var, self.replace_facet_rebuild_tyvars(ty, replacements)))
                    .collect(),
                params: params
                    .iter()
                    .map(|param| self.replace_facet_rebuild_tyvars(param, replacements))
                    .collect(),
                ret: Box::new(self.replace_facet_rebuild_tyvars(ret, replacements)),
            },
            _ => ty.clone(),
        }
    }

    fn ensure_named_facet_rebuild_is_unique(
        &mut self,
        type_name: &str,
        field_index: usize,
        arguments: &[Ty],
        before: &Ty,
        after: &Ty,
        span: &Span,
    ) -> Result<(), TypeError> {
        if self.resolve_ty(before) == self.resolve_ty(after) {
            return Ok(());
        }
        let Some(def) = self.env.lookup_type_def(type_name).cloned() else {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Facet cannot derive a rebuilt type for {}",
                    Self::surface_name(type_name)
                ),
                span: span.clone(),
                hint: None,
            });
        };
        // Concrete named records remain structurally rebuildable.  The
        // uniqueness constraint matters when a declaration-level generic
        // parameter would need a new instantiation.
        if def.type_param_vars.is_empty() {
            return Ok(());
        }
        if arguments.len() != def.type_param_vars.len() {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Facet found inconsistent generic metadata for {}",
                    Self::surface_name(type_name)
                ),
                span: span.clone(),
                hint: Some(
                    "The nominal type argument list must match its declaration parameters.".into(),
                ),
            });
        }
        let Some((_, selected_template)) = def.fields.get(field_index) else {
            return Err(TypeError {
                structured: None,
                message: "Facet field path is inconsistent with its source type".into(),
                span: span.clone(),
                hint: None,
            });
        };
        let mut after_replacements = HashMap::new();
        self.collect_facet_rebuild_tyvar_replacements(
            selected_template,
            after,
            &mut after_replacements,
            span,
        )?;
        let changed_vars = def
            .type_param_vars
            .iter()
            .zip(arguments)
            .filter_map(|(var, current)| match after_replacements.get(var) {
                Some(after) if self.resolve_ty(current) != self.resolve_ty(after) => Some(*var),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut rebuildable = false;
        for var in &changed_vars {
            let selected_count = def
                .fields
                .get(field_index)
                .map(|(_, ty)| self.count_tyvar_occurrences(ty, *var))
                .unwrap_or(0);
            if selected_count == 0 {
                continue;
            }
            let other_count = def
                .fields
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != field_index)
                .map(|(_, (_, ty))| self.count_tyvar_occurrences(ty, *var))
                .sum::<usize>();
            if selected_count == 1 && other_count == 0 {
                rebuildable = true;
                continue;
            }
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Facet cannot rebuild {} because a generic parameter occurs outside the updated field",
                    Self::surface_name(type_name)
                ),
                span: span.clone(),
                hint: Some("Use a path whose replacement determines each changed generic parameter uniquely.".into()),
            });
        }
        if rebuildable {
            Ok(())
        } else {
            Err(TypeError {
                structured: None,
                message: format!(
                    "Facet cannot rebuild {} because the updated field does not determine a generic parameter",
                    Self::surface_name(type_name)
                ),
                span: span.clone(),
                hint: Some("Only uniquely rebuildable generic named types support a type-changing Facet update.".into()),
            })
        }
    }

    fn finalize_facet_update(
        &mut self,
        path: &mut TypedFacetPath,
        replacement_ty: &Ty,
        span: &Span,
    ) -> Result<Ty, TypeError> {
        let replacement_ty = self.resolve_ty(replacement_ty);
        let update_source_ty =
            self.rebuild_facet_source_type(&path.source_ty, &path.segments, &replacement_ty, span)?;
        self.validate_nominal_type_well_formed(&update_source_ty, span, false)?;

        if !matches!(path.update_focus_ty, Ty::Hole)
            && !self.types_compatible(&path.update_focus_ty, &replacement_ty)?
        {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Facet replacement type mismatch: annotation requires {}, got {}",
                    self.ty_name(&path.update_focus_ty),
                    self.ty_name(&replacement_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }
        if !matches!(path.update_source_ty, Ty::Hole)
            && !self.types_compatible(&path.update_source_ty, &update_source_ty)?
        {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Facet rebuilt source type mismatch: annotation requires {}, got {}",
                    self.ty_name(&path.update_source_ty),
                    self.ty_name(&update_source_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        path.update_focus_ty = replacement_ty;
        path.update_source_ty = update_source_ty.clone();
        Ok(update_source_ty)
    }

    pub(super) fn check_facet_set_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input, value_expr) =
            self.parse_facet_mutating_intrinsic_args(span, "Facet::set", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            mut path,
            ..
        } = self.prepare_facet_input(span, "Facet::set", &source_expr, path_input)?;
        self.check_mutating_facet_path_permissions("Facet::set", &path, span)?;

        let typed_value = self.check_node(value_expr)?;
        if matches!(self.resolve_ty(&path.focus_ty), Ty::Result(_, _))
            && !matches!(self.resolve_ty(&typed_value.ty), Ty::Result(_, _))
        {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Facet::set value type mismatch: expected {}, got {}",
                    self.ty_name(&path.focus_ty),
                    self.ty_name(&typed_value.ty)
                ),
                span: typed_value.span.clone(),
                hint: Some("Facet::set replaces a Result property as a whole; use Facet::over to update its Ok payload.".into()),
            });
        }
        let update_source_ty = self.finalize_facet_update(&mut path, &typed_value.ty, span)?;

        Ok(TypedNode {
            ty: Ty::Result(Box::new(update_source_ty), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::FacetSet {
                api: TypedFacetApi::Set,
                source: Box::new(typed_source),
                path,
                value: Box::new(typed_value),
                source_is_result,
                mode: TypedFacetSetMode::Exact,
            },
        })
    }

    pub(super) fn check_facet_put_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input, value_expr) =
            self.parse_facet_mutating_intrinsic_args(span, "Facet::put", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            mut path,
            ..
        } = self.prepare_facet_input(span, "Facet::put", &source_expr, path_input)?;
        if source_is_result {
            return Err(self.policy_error(
                TypeDiagnosticReason::FacetOperationPolicyViolation,
                diagnostics::TypePolicy::FacetOperation,
                Some("Facet::put requires a plain source value".into()),
                None,
                Some(&typed_source.ty),
                None,
                None,
                &typed_source.span,
                Some("Use Facet::set when the source is already Result<T>.".into()),
            ));
        }
        if !path.is_infallible_structural() {
            return Err(self.policy_error(
                TypeDiagnosticReason::FacetOperationPolicyViolation,
                diagnostics::TypePolicy::FacetOperation,
                Some("Facet::put requires an infallible structural Facet path".into()),
                None,
                None,
                None,
                None,
                span,
                Some("Use Facet::set for fallible or variant-sensitive updates.".into()),
            ));
        }
        self.check_mutating_facet_path_permissions("Facet::put", &path, span)?;

        let typed_value = self.check_node(value_expr)?;
        let update_source_ty = self.finalize_facet_update(&mut path, &typed_value.ty, span)?;

        Ok(TypedNode {
            ty: update_source_ty,
            span: span.clone(),
            node: TypedInner::FacetSet {
                api: TypedFacetApi::Put,
                source: Box::new(typed_source),
                path,
                value: Box::new(typed_value),
                source_is_result,
                mode: TypedFacetSetMode::Exact,
            },
        })
    }

    pub(super) fn check_facet_over_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input, update_expr) =
            self.parse_facet_mutating_intrinsic_args(span, "Facet::over", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            mut path,
            ..
        } = self.prepare_facet_input(span, "Facet::over", &source_expr, path_input)?;
        self.check_mutating_facet_path_permissions("Facet::over", &path, span)?;

        let typed_update = self.check_node(update_expr)?;
        let (mode, replacement_ty) = self.check_facet_over_callable(
            "Facet::over",
            span,
            &path.focus_ty,
            &typed_update,
            false,
            false,
        )?;
        let update_source_ty = self.finalize_facet_update(&mut path, &replacement_ty, span)?;

        Ok(TypedNode {
            ty: Ty::Result(Box::new(update_source_ty), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::FacetOver {
                api: TypedFacetApi::Over,
                source: Box::new(typed_source),
                path,
                update_fun: Box::new(typed_update),
                source_is_result,
                mode,
            },
        })
    }

    fn require_enum_facet_path(
        &self,
        op_name: &str,
        path: &TypedFacetPath,
        span: &Span,
    ) -> Result<(), TypeError> {
        if !path.has_variant_segment() {
            return Err(self.policy_error(
                TypeDiagnosticReason::FacetOperationPolicyViolation,
                diagnostics::TypePolicy::FacetOperation,
                Some(format!("{op_name} requires an enum Facet path")),
                None,
                None,
                None,
                None,
                span,
                Some("Use Facet::set/over for structural, list, or map paths.".into()),
            ));
        }
        Ok(())
    }

    pub(super) fn check_facet_case_set_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input, value_expr) =
            self.parse_facet_mutating_intrinsic_args(span, "Facet::case_set", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            mut path,
            ..
        } = self.prepare_facet_input(span, "Facet::case_set", &source_expr, path_input)?;
        self.require_enum_facet_path("Facet::case_set", &path, span)?;
        if !path.final_segment_is_variant() {
            return Err(self.policy_error(
                TypeDiagnosticReason::FacetOperationPolicyViolation,
                diagnostics::TypePolicy::FacetOperation,
                Some("Facet::case_set requires the final Facet segment to be an enum case".into()),
                None,
                None,
                None,
                None,
                span,
                Some("Use Facet::case_over when updating inside a selected case payload.".into()),
            ));
        }
        self.check_mutating_facet_path_permissions("Facet::case_set", &path, span)?;

        let typed_value = self.check_node(value_expr)?;
        let update_source_ty = self.finalize_facet_update(&mut path, &typed_value.ty, span)?;

        Ok(TypedNode {
            ty: Ty::Result(Box::new(update_source_ty), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::FacetSet {
                api: TypedFacetApi::CaseSet,
                source: Box::new(typed_source),
                path,
                value: Box::new(typed_value),
                source_is_result,
                mode: TypedFacetSetMode::CaseSet,
            },
        })
    }

    pub(super) fn check_facet_case_over_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input, update_expr) =
            self.parse_facet_mutating_intrinsic_args(span, "Facet::case_over", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            mut path,
            ..
        } = self.prepare_facet_input(span, "Facet::case_over", &source_expr, path_input)?;
        self.require_enum_facet_path("Facet::case_over", &path, span)?;
        self.check_mutating_facet_path_permissions("Facet::case_over", &path, span)?;

        let typed_update = self.check_node(update_expr)?;
        let (mode, replacement_ty) = match self.check_facet_over_callable(
            "Facet::case_over",
            span,
            &path.focus_ty,
            &typed_update,
            false,
            true,
        )? {
            (TypedFacetOverMode::FocusValue, replacement) => {
                (TypedFacetOverMode::CaseFocusValue, replacement)
            }
            (TypedFacetOverMode::FocusResult, replacement) => {
                (TypedFacetOverMode::CaseFocusResult, replacement)
            }
            (
                mode @ (TypedFacetOverMode::CaseFocusValue | TypedFacetOverMode::CaseFocusResult),
                replacement,
            ) => (mode, replacement),
        };
        let update_source_ty = self.finalize_facet_update(&mut path, &replacement_ty, span)?;

        Ok(TypedNode {
            ty: Ty::Result(Box::new(update_source_ty), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::FacetOver {
                api: TypedFacetApi::CaseOver,
                source: Box::new(typed_source),
                path,
                update_fun: Box::new(typed_update),
                source_is_result,
                mode,
            },
        })
    }

    pub(super) fn check_facet_over_result_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input, update_expr) =
            self.parse_facet_mutating_intrinsic_args(span, "Facet::over_result", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            mut path,
            ..
        } = self.prepare_facet_input(span, "Facet::over_result", &source_expr, path_input)?;
        self.check_mutating_facet_path_permissions("Facet::over_result", &path, span)?;

        if !matches!(self.resolve_ty(&path.focus_ty), Ty::Result(_, _)) {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Facet::over_result requires Result focus, got {}",
                    self.ty_name(&path.focus_ty)
                ),
                span: span.clone(),
                hint: Some("Use Facet::over for plain focus updates.".into()),
            });
        }

        let inner_ty = self.env.fresh_tyvar();
        let expected_update = self.callable_contract(
            &path.focus_ty,
            Some(Ty::Result(
                Box::new(Ty::Result(Box::new(inner_ty), Box::new(Ty::Error))),
                Box::new(Ty::Error),
            )),
            ExpectedCallableSlot::Plain,
        );
        let typed_update = self.check_apply_callable_with_contract(
            update_expr,
            &expected_update,
            "Facet::over_result",
        )?;
        let (mode, replacement_ty) = self.check_facet_over_callable(
            "Facet::over_result",
            span,
            &path.focus_ty,
            &typed_update,
            true,
            true,
        )?;
        let update_source_ty = self.finalize_facet_update(&mut path, &replacement_ty, span)?;

        Ok(TypedNode {
            ty: Ty::Result(Box::new(update_source_ty), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::FacetOver {
                api: TypedFacetApi::OverResult,
                source: Box::new(typed_source),
                path,
                update_fun: Box::new(typed_update),
                source_is_result,
                mode,
            },
        })
    }

    fn check_facet_over_callable(
        &mut self,
        op_name: &str,
        span: &Span,
        focus_ty: &Ty,
        typed_update: &TypedNode,
        require_result_focus: bool,
        allow_result_focus_input: bool,
    ) -> Result<(TypedFacetOverMode, Ty), TypeError> {
        let (in_ty, out_ty) = self.unary_function_parts(&typed_update.ty, op_name, span)?;
        let resolved_focus_ty = self.resolve_ty(focus_ty);
        let value_focus_ty = match &resolved_focus_ty {
            Ty::Result(ok, _) => Some(ok.as_ref().clone()),
            _ => None,
        };

        let mode = if require_result_focus {
            TypedFacetOverMode::FocusResult
        } else if let Some(value_focus_ty) = &value_focus_ty {
            if self.types_compatible(value_focus_ty, &in_ty)? {
                TypedFacetOverMode::FocusValue
            } else if allow_result_focus_input {
                TypedFacetOverMode::FocusResult
            } else {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "{op_name} update function input mismatch: expected {}, got {}",
                        self.ty_name(value_focus_ty),
                        self.ty_name(&in_ty)
                    ),
                    span: typed_update.span.clone(),
                    hint: Some(
                        "Use Facet::over_result to update the Result property as a whole.".into(),
                    ),
                });
            }
        } else {
            TypedFacetOverMode::FocusValue
        };

        let expected_input_ty = match (&mode, &value_focus_ty) {
            (TypedFacetOverMode::FocusValue, Some(value_focus_ty)) => value_focus_ty,
            _ => &resolved_focus_ty,
        };
        if !self.types_compatible(expected_input_ty, &in_ty)? {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{op_name} update function input mismatch: expected {}, got {}",
                    self.ty_name(expected_input_ty),
                    self.ty_name(&in_ty)
                ),
                span: typed_update.span.clone(),
                hint: None,
            });
        }

        let (out_ok, out_err) = match self.resolve_ty(&out_ty) {
            Ty::Result(ok, err) => (ok.as_ref().clone(), err.as_ref().clone()),
            _ => {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "{op_name} update function must return Result<...>, got {}",
                        self.ty_name(&out_ty)
                    ),
                    span: typed_update.span.clone(),
                    hint: None,
                });
            }
        };
        if require_result_focus && !matches!(out_ok, Ty::Result(_, _)) {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{op_name} update function output mismatch: must return Result<Result<...>>, got {}",
                    self.ty_name(&out_ty)
                ),
                span: typed_update.span.clone(),
                hint: Some(
                    "Return an outer Result for mapper failure and an inner Result property value."
                        .into(),
                ),
            });
        }
        if !self.types_compatible(&Ty::Error, &out_err)? {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{op_name} update function error type must be Error-compatible, got {}",
                    self.ty_name(&out_err)
                ),
                span: typed_update.span.clone(),
                hint: None,
            });
        }

        let replacement_ty = match (&mode, &resolved_focus_ty) {
            (TypedFacetOverMode::FocusValue, Ty::Result(_, err)) => {
                Ty::Result(Box::new(out_ok), err.clone())
            }
            _ => out_ok,
        };
        Ok((mode, replacement_ty))
    }

    pub(in crate::checker::expr) fn check_mutating_facet_path_permissions(
        &self,
        facet_name: &str,
        path: &TypedFacetPath,
        span: &Span,
    ) -> Result<(), TypeError> {
        for (index, segment) in path.segments.iter().enumerate() {
            let is_final = index + 1 == path.segments.len();
            match segment {
                TypedFacetSegment::ReadonlyBuiltin { field_name, .. }
                | TypedFacetSegment::ErrorPayload { field_name, .. } => {
                    return Err(TypeError::new(
                        format!("{facet_name} cannot update readonly Error.{field_name}"),
                        span.clone(),
                    ));
                }
                TypedFacetSegment::Field {
                    field_name,
                    container_type_name,
                    readonly,
                    ..
                } => {
                    if *readonly {
                        let owner_can_replace = is_final
                            && self
                                .current_impl_struct_target
                                .as_deref()
                                .map(Self::surface_name)
                                == Some(container_type_name.as_str());
                        if !owner_can_replace {
                            let hint = if is_final {
                                format!(
                                    "Only impl {} may replace the property itself.",
                                    container_type_name
                                )
                            } else {
                                format!(
                                    "Readonly field {}.{} can only be replaced as a whole; it cannot be traversed to update nested state.",
                                    container_type_name, field_name
                                )
                            };
                            return Err(TypeError {
                                structured: None,
                                message: format!(
                                    "{} cannot mutably traverse readonly field {}.{}; only the owner can replace the property itself",
                                    facet_name, container_type_name, field_name
                                ),
                                span: span.clone(),
                                hint: Some(hint),
                            });
                        }
                    }
                }
                TypedFacetSegment::Tuple { .. }
                | TypedFacetSegment::Variant { .. }
                | TypedFacetSegment::ListIndex { .. }
                | TypedFacetSegment::ListRange { .. }
                | TypedFacetSegment::MapKey { .. } => {}
            }
        }

        Ok(())
    }
}
