use super::*;

impl Checker {
    pub(in crate::checker) fn check_struct_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        _type_params: &[ResolvedTypeParam],
        fields: &[ResolvedField],
    ) -> Result<TypedNode, TypeError> {
        let definition = self.env.lookup_type_def(&id.name).ok_or_else(|| {
            self.typecheck_invariant_error("struct declaration has no predeclared schema", span)
        })?;
        if definition.state != crate::env::TypeDefState::SignatureResolved {
            return Err(
                self.typecheck_invariant_error("struct declaration schema is unresolved", span)
            );
        }
        let tag = definition.tag;
        let ty_fields = definition.fields.clone();
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
        let definition = self.env.lookup_type_def(&id.name).ok_or_else(|| {
            self.typecheck_invariant_error("record declaration has no predeclared schema", span)
        })?;
        if definition.state != crate::env::TypeDefState::SignatureResolved {
            return Err(
                self.typecheck_invariant_error("record declaration schema is unresolved", span)
            );
        }
        let tag = definition.tag;
        let ty_fields = definition.fields.clone();
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
        let body_next_tyvar = self.env.next_tyvar;
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
        let body_node = self.with_deferror_body_scope(
            id.unique_id,
            &typed_params,
            body_next_tyvar,
            |checker| {
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
                    typed_body.push(checker.check_node(statement)?);
                }
                let final_node = match last {
                    Resolved::ConstructorCall(call_span, target, args)
                        if target.unique_id == id.unique_id =>
                    {
                        let mut schema = vec![("message".to_string(), Ty::Str)];
                        schema.extend(payload_fields.clone());
                        let mut values = checker.check_error_arguments(call_span, &id.name, &schema, args)?;
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
                        let message = checker.check_node(other)?;
                        if !checker.types_compatible(&Ty::Str, &message.ty)? {
                            return Err(TypeError::new(
                                "empty Payload deferror body must return String or a direct internal construction",
                                message.span.clone(),
                            ));
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
                Ok(TypedNode {
                    ty: Ty::Error,
                    span: span.clone(),
                    node: TypedInner::Block(typed_body),
                })
            },
        )?;
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

    fn with_deferror_body_scope<T>(
        &mut self,
        error_uid: u32,
        params: &[TypedValueParameter],
        body_next_tyvar: u32,
        check: impl FnOnce(&mut Self) -> Result<T, TypeError>,
    ) -> Result<T, TypeError> {
        // Spire permits no declarations in expression blocks. Declaration
        // metadata is therefore read-only here; only expression-local state
        // and inference progress need the isolation of the former child.
        // Normalization survives ordinary expression scopes. Only this
        // isolated boundary restores those writes together with local binds.
        let saved_next_tyvar = self.env.next_tyvar;
        let saved_next_tag = self.env.next_tag;
        let saved_error_uid = self.error_definition_uid.replace(error_uid);
        let saved_return_ty = self.function_return_ty.replace(Ty::Error);
        let saved_substitutions = self.substitutions.clone();
        let saved_bounds = self.tyvar_bounds.clone();
        let saved_process_markers = self.process_marker_tyvars.clone();
        let saved_obligations = self.pending_trait_obligations.clone();
        let saved_capabilities = self.active_capabilities.clone();
        let saved_facet_bindings = self.facet_bindings.clone();
        let saved_lazy_bindings = self.lazy_capture_bindings.clone();
        let saved_lazy_capture = self.active_lazy_capture.clone();
        let saved_constructor_capabilities = self.constructor_capabilities.clone();
        let saved_closure_parameters = self.explicit_closure_parameters.clone();
        let saved_constructor_witnesses = self.constructor_witness_traits.clone();
        let saved_pattern_aliases = self.pattern_binding_aliases.clone();
        let saved_safe_results = self.safe_operator_results.clone();
        let saved_warnings = std::mem::take(&mut self.warnings);
        let saved_alias_stack = std::mem::take(&mut self.alias_expansion_stack);
        let saved_mapping_resolution = std::mem::take(&mut self.constructor_mapping_resolution);
        let saved_pattern_requirements =
            std::mem::take(&mut self.direct_pattern_requirement_tyvars);
        let saved_boot_plan = std::mem::take(&mut self.boot_plan);

        self.env.push_isolated_var_scope();
        // The former child copied its environment before input annotations
        // were resolved, but inherited inference state after that resolution.
        self.env.next_tyvar = body_next_tyvar;
        for param in params {
            self.env.bind_var(param.id.unique_id, param.ty.clone());
        }
        let result = check(self);
        let mut body_warnings = std::mem::replace(&mut self.warnings, saved_warnings);
        let body_capabilities =
            std::mem::replace(&mut self.active_capabilities, saved_capabilities);
        if result.is_ok() {
            self.env.next_tyvar = self.env.next_tyvar.max(saved_next_tyvar);
            self.env.next_tag = self.env.next_tag.max(saved_next_tag);
            for (outer, checked) in self.active_capabilities.iter_mut().zip(body_capabilities) {
                outer.consumed |= checked.consumed;
            }
            self.warnings.extend(body_warnings.take());
        } else {
            self.env.next_tyvar = saved_next_tyvar;
            self.env.next_tag = saved_next_tag;
            self.substitutions = saved_substitutions;
            self.tyvar_bounds = saved_bounds;
            self.process_marker_tyvars = saved_process_markers;
        }
        self.env.pop_var_scope();
        self.error_definition_uid = saved_error_uid;
        self.function_return_ty = saved_return_ty;
        self.pending_trait_obligations = saved_obligations;
        self.facet_bindings = saved_facet_bindings;
        self.lazy_capture_bindings = saved_lazy_bindings;
        self.active_lazy_capture = saved_lazy_capture;
        self.constructor_capabilities = saved_constructor_capabilities;
        self.explicit_closure_parameters = saved_closure_parameters;
        self.constructor_witness_traits = saved_constructor_witnesses;
        self.pattern_binding_aliases = saved_pattern_aliases;
        self.safe_operator_results = saved_safe_results;
        self.alias_expansion_stack = saved_alias_stack;
        self.constructor_mapping_resolution = saved_mapping_resolution;
        self.direct_pattern_requirement_tyvars = saved_pattern_requirements;
        self.boot_plan = saved_boot_plan;
        result
    }
}

#[cfg(test)]
mod deferror_scope_tests {
    use super::*;

    fn parameter(uid: u32) -> TypedValueParameter {
        let span = Span { start: 1, end: 2 };
        TypedValueParameter {
            id: ResolvedId {
                name: "input".into(),
                qualified_name: None,
                unique_id: uid,
                compiler_generated: false,
                symbol_info: None,
                span: span.clone(),
            },
            mode: spire::ast::ValueParameterMode::PositionalOrNamed,
            ty: Ty::Int,
            span,
        }
    }

    fn warning(message: &str) -> CompilerWarning {
        CompilerWarning::new(
            WarningKind::UnusedTypeParameter,
            WarningPhase::Typecheck,
            WarningSpan { start: 1, end: 2 },
            message,
            None,
        )
    }

    #[test]
    fn deferror_body_reuses_readonly_metadata() {
        let mut checker = Checker::new(TypecheckContext::default());
        checker.env.bind_var(100, Ty::Int);
        checker
            .function_ids_by_name
            .insert("existing".into(), parameter(100).id);
        let function_key = checker
            .function_ids_by_name
            .get_key_value("existing")
            .unwrap()
            .0 as *const String;
        checker
            .with_deferror_body_scope(200, &[], 0, |body| {
                assert_eq!(
                    body.function_ids_by_name
                        .get_key_value("existing")
                        .unwrap()
                        .0 as *const String,
                    function_key,
                    "read-only declaration metadata must not be cloned"
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn deferror_body_preserves_success_and_failure_boundaries() {
        for (fail, body_next_tyvar) in [(false, 15), (false, 21), (true, 15), (true, 21)] {
            let mut checker = Checker::new(TypecheckContext::default());
            checker.env.bind_var(100, Ty::Var(10));
            checker.env.next_tyvar = 17;
            checker.env.next_tag = 9;
            checker.function_return_ty = Some(Ty::Int);
            checker.error_definition_uid = Some(99);
            checker.substitutions.insert(10, Ty::Int);
            checker.tyvar_bounds.insert(10, vec!["Before".into()]);
            checker.process_marker_tyvars.insert(10);
            checker.pending_trait_obligations.insert(10, Vec::new());
            checker.active_capabilities.push(CapabilityUse {
                subject_ty: Ty::Int,
                subject_name: "value".into(),
                trait_id: "Show".into(),
                span: Span { start: 1, end: 2 },
                consumed: false,
            });
            checker.warnings.push(warning("before"));
            let result = checker.with_deferror_body_scope(200, &[parameter(300)], 11, |body| {
                assert_eq!(body.error_definition_uid, Some(200));
                assert_eq!(body.function_return_ty, Some(Ty::Error));
                assert_eq!(body.env.lookup_var(300), Some(&Ty::Int));
                assert_eq!(body.env.next_tyvar, 11);
                assert!(body.warnings.is_empty());
                body.env.bind_var(100, Ty::Bool);
                body.env.bind_var(400, Ty::Str);
                body.env.concrete_error_bindings.insert(400, "Local".into());
                body.env.next_tyvar = body_next_tyvar;
                body.env.next_tag = 12;
                body.substitutions.insert(10, Ty::Str);
                body.tyvar_bounds.insert(10, vec!["After".into()]);
                body.process_marker_tyvars.insert(11);
                body.pending_trait_obligations.insert(11, Vec::new());
                body.active_capabilities[0].consumed = true;
                body.constructor_capabilities
                    .insert(400, ConstructorCapabilityProvenance::RequiresProof);
                body.constructor_witness_traits.insert(11, "Local".into());
                body.explicit_closure_parameters.insert(400, Ty::Str);
                body.pattern_binding_aliases.insert(400, None);
                body.warnings.push(warning("inside"));
                if fail {
                    Err(TypeError::new(
                        "constructor failed",
                        Span { start: 5, end: 6 },
                    ))
                } else {
                    Ok(42)
                }
            });
            assert_eq!(result.is_err(), fail);
            assert_eq!(checker.error_definition_uid, Some(99));
            assert_eq!(checker.function_return_ty, Some(Ty::Int));
            assert_eq!(checker.env.lookup_var(100), Some(&Ty::Var(10)));
            assert!(checker.env.lookup_var(300).is_none());
            assert!(checker.env.lookup_var(400).is_none());
            assert!(checker.env.concrete_error_bindings.is_empty());
            assert_eq!(
                checker.env.next_tyvar,
                if fail { 17 } else { body_next_tyvar.max(17) }
            );
            assert_eq!(checker.env.next_tag, if fail { 9 } else { 12 });
            assert_eq!(
                checker.substitutions[&10],
                if fail { Ty::Int } else { Ty::Str }
            );
            assert_eq!(
                checker.tyvar_bounds[&10],
                vec![if fail { "Before" } else { "After" }]
            );
            assert_eq!(checker.process_marker_tyvars.contains(&11), !fail);
            assert_eq!(checker.active_capabilities[0].consumed, !fail);
            assert_eq!(checker.pending_trait_obligations.len(), 1);
            assert!(checker.pending_trait_obligations.contains_key(&10));
            assert!(checker.constructor_capabilities.is_empty());
            assert!(checker.constructor_witness_traits.is_empty());
            assert!(checker.explicit_closure_parameters.is_empty());
            assert!(checker.pattern_binding_aliases.is_empty());
            let expected = if fail {
                vec![warning("before")]
            } else {
                vec![warning("before"), warning("inside")]
            };
            assert_eq!(checker.warnings.as_slice(), expected);
        }
    }

    #[test]
    fn failed_deferror_body_does_not_contaminate_the_next_declaration() {
        let ast = spire::parse_with_context(
            r#"deferror Broken {
                |number: Int|
                saved = number
                12
            }
            deferror Later(detail: Int) {
                |input: Int|
                Self(message: "later", detail: input)
            }"#,
            spire::ParserContext::project(0),
        )
        .unwrap();
        let resolved = crate::test_support::resolve_ast_with_builtin_prelude(ast).unwrap();
        let Resolved::DeferrorDef(_, _, _, constructor) = &resolved[0] else {
            unreachable!()
        };
        let Resolved::Closure(_, params, _, _) = constructor.as_ref() else {
            unreachable!()
        };
        let parameter_uid = params[0].id.unique_id;
        let mut checker = Checker::with_persistent_state(
            crate::test_support::session_from_cached_std_prelude().state,
            TypecheckContext::default(),
        );
        let error = checker.check_program(resolved.clone()).unwrap_err();
        assert!(
            error.message.contains("body must return String"),
            "{error:?}"
        );
        assert!(checker.env.lookup_var(parameter_uid).is_none());
        assert!(checker.error_definition_uid.is_none());
        assert!(checker.function_return_ty.is_none());
        checker
            .check_node(&resolved[1])
            .expect("a later constructor must use its own inputs and Error identity");
        assert!(checker.error_definition_uid.is_none());
        assert!(checker.function_return_ty.is_none());
    }

    #[test]
    fn do_block_retains_outer_binding_inference_for_later_annotations() {
        for element in ["Int", "String"] {
            let source = r#"xs = []
                do::<Option> {
                    ys: List<Int> = xs
                    Option::Some(())
                };
                zs: List<$ELEMENT> = xs"#
                .replace("$ELEMENT", element);
            let result =
                crate::test_support::typecheck_with_rules(&source, RuntimeSourcePolicy::script());
            if element == "Int" {
                result.expect("do inference must support a later annotation with the same type");
            } else {
                let error = result.expect_err("do inference must reject a later conflicting type");
                assert_eq!(
                    error.reason(),
                    Some(diagnostics::TypeDiagnosticReason::AnnotationTypeMismatch),
                    "{error:?}",
                );
            }
        }
    }

    #[test]
    fn deferror_body_restores_normalized_outer_bindings() {
        for fail in [false, true] {
            let mut checker = Checker::new(TypecheckContext::default());
            checker.env.bind_var(100, Ty::Var(10));
            checker.substitutions.insert(10, Ty::Str);
            let result = checker.with_deferror_body_scope(200, &[], 0, |body| {
                body.normalize_env_bindings();
                assert_eq!(body.env.lookup_var(100), Some(&Ty::Str));
                if fail {
                    Err(TypeError::new(
                        "constructor failed",
                        Span { start: 1, end: 2 },
                    ))
                } else {
                    Ok(())
                }
            });
            assert_eq!(result.is_err(), fail);
            assert_eq!(checker.env.lookup_var(100), Some(&Ty::Var(10)));
        }
    }

    #[test]
    fn deferror_body_preserves_untouched_binding_storage() {
        for fail in [false, true] {
            let mut checker = Checker::new(TypecheckContext::default());
            for uid in 0..4096 {
                checker.env.bind_var(uid, Ty::List(Box::new(Ty::Int)));
            }
            let storage = |env: &TypeEnv| match env.lookup_var(4095).unwrap() {
                Ty::List(element) => element.as_ref() as *const Ty,
                _ => unreachable!(),
            };
            let before = storage(&checker.env);
            let result = checker.with_deferror_body_scope(5000, &[], 0, |body| {
                body.env.bind_var(5001, Ty::Str);
                if fail {
                    Err(TypeError::new(
                        "constructor failed",
                        Span { start: 1, end: 2 },
                    ))
                } else {
                    Ok(())
                }
            });
            assert_eq!(result.is_err(), fail);
            assert_eq!(checker.env.vars.len(), 4096);
            assert_eq!(
                storage(&checker.env),
                before,
                "a deferror body must preserve the storage of untouched outer types",
            );
        }
    }

    #[test]
    fn deferror_body_restores_bindings_changed_inside_nested_scopes() {
        for fail in [false, true] {
            for bind_first in [false, true] {
                let mut checker = Checker::new(TypecheckContext::default());
                checker.env.bind_var(100, Ty::Var(10));
                checker.substitutions.insert(10, Ty::Str);
                checker.substitutions.insert(11, Ty::Bool);
                let result = checker.with_deferror_body_scope(200, &[], 0, |body| {
                    body.env.push_var_scope();
                    if bind_first {
                        body.env.bind_var(100, Ty::Var(11));
                        body.normalize_env_bindings();
                        assert_eq!(body.env.lookup_var(100), Some(&Ty::Bool));
                    } else {
                        body.normalize_env_bindings();
                        assert_eq!(body.env.lookup_var(100), Some(&Ty::Str));
                        body.env.bind_var(100, Ty::Var(11));
                    }
                    body.env.bind_var(300, Ty::Var(10));
                    body.normalize_env_bindings();
                    body.env.pop_var_scope();
                    assert!(body.env.lookup_var(300).is_none());
                    if fail {
                        Err(TypeError::new(
                            "constructor failed",
                            Span { start: 1, end: 2 },
                        ))
                    } else {
                        Ok(())
                    }
                });
                assert_eq!(result.is_err(), fail);
                assert_eq!(checker.env.lookup_var(100), Some(&Ty::Var(10)));
                assert!(checker.env.lookup_var(300).is_none());
            }
        }
    }
}
