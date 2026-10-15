use super::*;

impl Checker {
    pub(in crate::checker) fn check_struct_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        type_params: &[ResolvedTypeParam],
        fields: &[ResolvedField],
    ) -> Result<TypedNode, TypeError> {
        let mut tyvars = HashMap::new();
        self.seed_signature_type_params(type_params, &mut tyvars);
        let ty_fields: Vec<(String, Ty)> = fields
            .iter()
            .map(|f| {
                Ok((
                    f.name.clone(),
                    self.resolve_signature_ast_ty_in_context(
                        &f.ty,
                        TypeSyntaxContext::General,
                        &mut tyvars,
                    )?,
                ))
            })
            .collect::<Result<Vec<_>, TypeError>>()?;
        let private_fields = fields
            .iter()
            .filter(|field| field.visibility == spire::ast::Visibility::Private)
            .map(|field| field.name.clone())
            .collect::<HashSet<_>>();

        let readonly_fields = fields
            .iter()
            .filter(|field| field.readonly)
            .map(|field| field.name.clone())
            .collect::<HashSet<_>>();
        let type_param_vars = type_params
            .iter()
            .filter_map(|param| match tyvars.get(&param.name) {
                Some(Ty::Var(var)) => Some(*var),
                _ => None,
            })
            .collect::<Vec<_>>();

        let tag = self
            .env
            .resolve_type_def_signature(
                &id.name,
                ty_fields.clone(),
                type_param_vars.clone(),
                private_fields,
                readonly_fields,
            )
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown struct type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;

        self.env.bind_var(
            id.unique_id,
            Ty::Struct(
                id.name.clone(),
                NominalType::new(
                    type_param_vars.iter().copied().map(Ty::Var).collect(),
                    ty_fields.clone(),
                ),
            ),
        );

        let field_names: Vec<String> = ty_fields.iter().map(|(n, _)| n.clone()).collect();
        let field_policies = fields
            .iter()
            .map(|field| crate::typed::TypedFieldPolicy {
                private: field.visibility == spire::ast::Visibility::Private,
                readonly: field.readonly,
            })
            .collect();

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::StructDef(tag, id.name.clone(), field_names, field_policies),
        })
    }

    pub(in crate::checker) fn check_enum_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        type_params: &[ResolvedTypeParam],
        variants: &[ResolvedEnumVariant],
        attrs: &ResolvedDeclAttrs,
    ) -> Result<TypedNode, TypeError> {
        self.validate_builtin_enum_shape(span, id, type_params, variants, attrs)?;

        let enum_variants = self
            .lookup_enum_variants_of(&id.name)
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown enum type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;

        if enum_variants.len() != variants.len() {
            return Err(TypeError {
                structured: None,
                message: format!("Enum variant metadata mismatch: {}", id.name),
                span: span.clone(),
                hint: None,
            });
        }

        let typed_variants = enum_variants.iter().map(Into::into).collect::<Vec<_>>();

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::EnumDef(id.name.clone(), typed_variants),
        })
    }

    pub(in crate::checker) fn check_record_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        fields: &[ResolvedField],
    ) -> Result<TypedNode, TypeError> {
        if let Some(field) = fields
            .iter()
            .find(|field| field.visibility != spire::ast::Visibility::Public || field.readonly)
        {
            return Err(TypeError {
                structured: None,
                message: "record fields do not support visibility or readonly modifiers".into(),
                span: field.span.clone(),
                hint: None,
            });
        }
        let ty_fields: Vec<(String, Ty)> = fields
            .iter()
            .map(|f| {
                Ok((
                    f.name.clone(),
                    self.resolve_ast_ty_in_context(&f.ty, TypeSyntaxContext::General)?,
                ))
            })
            .collect::<Result<Vec<_>, TypeError>>()?;

        let tag = self
            .env
            .resolve_type_def_signature(
                &id.name,
                ty_fields.clone(),
                Vec::new(),
                HashSet::new(),
                HashSet::new(),
            )
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown record type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;

        self.env.bind_var(
            id.unique_id,
            Ty::Record(id.name.clone(), NominalType::monomorphic(ty_fields.clone())),
        );

        let field_names: Vec<String> = ty_fields.iter().map(|(n, _)| n.clone()).collect();
        let field_policies = fields
            .iter()
            .map(|_| crate::typed::TypedFieldPolicy {
                private: false,
                readonly: false,
            })
            .collect();

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::RecordDef(tag, id.name.clone(), field_names, field_policies),
        })
    }

    pub(in crate::checker) fn check_deferror_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        fields: &[ResolvedField],
        show_expr: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let kind = id.qualified_name.clone().ok_or_else(|| {
            TypeError::new(
                "Error declaration is missing canonical identity",
                span.clone(),
            )
        })?;
        let mut seen = HashSet::new();
        let payload_fields = fields
            .iter()
            .map(|field| {
                if matches!(field.name.as_str(), "kind" | "message")
                    || !seen.insert(field.name.clone())
                {
                    return Err(TypeError::new(
                        format!("Invalid or duplicate Error Payload field: {}", field.name),
                        field.span.clone(),
                    ));
                }
                let ty = self.resolve_ast_ty_in_context(&field.ty, TypeSyntaxContext::General)?;
                self.ensure_no_match_result_value(&ty, &field.span)?;
                if self.ty_contains_facet(&ty) {
                    return Err(TypeError::new(
                    "Facet is compile-time only in Stage1 and cannot appear in Error Payload types",
                    field.span.clone(),
                ));
                }
                Ok((field.name.clone(), ty))
            })
            .collect::<Result<Vec<_>, TypeError>>()?;
        let tag = self
            .env
            .lookup_type_def(&id.name)
            .ok_or_else(|| {
                TypeError::new(
                    format!("Unknown Error declaration: {}", id.name),
                    span.clone(),
                )
            })?
            .tag;
        let Resolved::Closure(_, params, _, body) = show_expr else {
            return Err(TypeError::new(
                "deferror body must be a constructor block",
                span.clone(),
            ));
        };
        let mut body_env = self.env.clone();
        let typed_params = params.iter().map(|param| {
            let ast_ty = param.ty.as_ref().ok_or_else(|| TypeError::new(
                "deferror constructor inputs require type annotations", param.id.span.clone()))?;
            let ty = self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)?;
            self.ensure_no_match_result_value(&ty, &param.id.span)?;
            if self.ty_contains_facet(&ty) {
                return Err(TypeError::new(
                    "Facet is compile-time only in Stage1 and cannot appear in Error constructor input types",
                    param.id.span.clone(),
                ));
            }
            body_env.bind_var(param.id.unique_id, ty.clone());
            Ok(TypedValueParameter { id: param.id.clone(), mode: spire::ast::ValueParameterMode::PositionalOrNamed,
                ty, span: param.id.span.clone() })
        }).collect::<Result<Vec<_>, TypeError>>()?;
        let fun_idx = match self.env.lookup_var(id.unique_id) {
            Some(Ty::UserFunc { fun_idx, .. }) => *fun_idx,
            _ => {
                return Err(TypeError::new(
                    format!("Undefined Error constructor: {}", id.name),
                    span.clone(),
                ))
            }
        };
        let mut child = self.spawn_child_checker(body_env);
        child.error_definition_uid = Some(id.unique_id);
        child.function_return_ty = Some(Ty::Error);
        let statements: Vec<&Resolved> = match body.as_ref() {
            Resolved::Block(_, items) => items.iter().collect(),
            other => vec![other],
        };
        let Some((last, initial)) = statements.split_last() else {
            return Err(TypeError::new(
                "deferror constructor body cannot be empty",
                span.clone(),
            ));
        };
        let mut typed_body = Vec::new();
        for statement in initial {
            typed_body.push(child.check_node(statement)?);
        }
        let final_node = match last {
            Resolved::ConstructorCall(call_span, target, args)
                if target.unique_id == id.unique_id =>
            {
                let mut schema = vec![("message".to_string(), Ty::Str)];
                schema.extend(payload_fields.clone());
                let mut values = child.check_error_arguments(call_span, &id.name, &schema, args)?;
                let message = Box::new(values.remove(0));
                TypedNode {
                    ty: Ty::Error,
                    span: call_span.clone(),
                    node: TypedInner::ErrorConstruct {
                        kind,
                        message,
                        payload: values,
                        payload_fields,
                    },
                }
            }
            other => {
                if !payload_fields.is_empty() {
                    return Err(TypeError::new(
                        "deferror with Payload requires a direct terminal internal construction",
                        span.clone(),
                    ));
                }
                let message = child.check_node(other)?;
                if !child.types_compatible(&Ty::Str, &message.ty)? {
                    return Err(TypeError::new("empty Payload deferror body must return String or a direct internal construction", message.span.clone()));
                }
                TypedNode {
                    ty: Ty::Error,
                    span: message.span.clone(),
                    node: TypedInner::ErrorConstruct {
                        kind,
                        message: Box::new(message),
                        payload: Vec::new(),
                        payload_fields,
                    },
                }
            }
        };
        typed_body.push(final_node);
        self.absorb_child_progress(&child);
        let body_node = TypedNode {
            ty: Ty::Error,
            span: span.clone(),
            node: TypedInner::Block(typed_body),
        };
        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::DeferrorDef(
                tag,
                fun_idx,
                id.clone(),
                typed_params,
                Box::new(body_node),
            ),
        })
    }
}
