use super::*;

mod functions;
mod nominal;
mod traits;

impl Checker {
    pub(in crate::checker) fn validate_declaration_where_well_formedness(
        &self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        let mut constructor_trait_ids = self
            .traits
            .values()
            .filter(|info| !info.constructor_slots.is_empty())
            .map(|info| info.id.unique_id)
            .collect::<HashSet<_>>();
        constructor_trait_ids.extend(stmts.iter().filter_map(|stmt| {
            let Resolved::TraitDef(_, id, _, Some(clause), _, _) = stmt else {
                return None;
            };
            clause
                .constraints
                .iter()
                .any(|constraint| {
                    matches!(&constraint.subject, AstTy::Named(_, name) if name == "Self")
                        && constraint.bounds.iter().any(|bound| {
                            matches!(bound, ResolvedWhereConstraintRhs::TypeConstructor { .. })
                        })
                })
                .then_some(id.unique_id)
        }));
        // A trait may inherit its contextual shape through a parent (for
        // example `Applicative: Functor`).  This validation runs before the
        // trait table has been predeclared, so reproduce the same closure
        // over the resolved declarations here rather than mistaking those
        // inherited constructor traits for ordinary traits.
        loop {
            let mut changed = false;
            for stmt in stmts {
                let Resolved::TraitDef(_, id, _, Some(clause), _, _) = stmt else {
                    continue;
                };
                if constructor_trait_ids.contains(&id.unique_id) {
                    continue;
                }
                let inherits_constructor_shape = clause.constraints.iter().any(|constraint| {
                    matches!(&constraint.subject, AstTy::Named(_, name) if name == "Self")
                        && constraint.bounds.iter().any(|bound| {
                            matches!(bound, ResolvedWhereConstraintRhs::Trait { trait_id }
                                if constructor_trait_ids.contains(&trait_id.unique_id))
                        })
                });
                if inherits_constructor_shape {
                    constructor_trait_ids.insert(id.unique_id);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let mut trait_names = self
            .traits
            .values()
            .flat_map(|info| {
                [
                    Self::surface_name(&info.id.name).to_string(),
                    self.trait_key(&info.id),
                ]
            })
            .collect::<HashSet<_>>();
        trait_names.extend(stmts.iter().filter_map(|stmt| {
            let Resolved::TraitDef(_, id, ..) = stmt else {
                return None;
            };
            Some(Self::surface_name(&id.name).to_string())
        }));
        for stmt in stmts {
            match stmt {
                Resolved::Def(
                    _,
                    id,
                    return_type_arguments,
                    value_parameters,
                    return_type,
                    clause,
                    _,
                    _,
                )
                | Resolved::BuiltinDecl(
                    _,
                    id,
                    return_type_arguments,
                    value_parameters,
                    return_type,
                    clause,
                    _,
                ) => {
                    Self::validate_type_shape_clause(
                        clause.as_ref(),
                        false,
                        None,
                        &constructor_trait_ids,
                        &trait_names,
                    )?;
                    let rewritten_return_type_arguments = return_type_arguments
                        .iter()
                        .map(|argument| sigil::resolved::ResolvedReturnTypeArgument {
                            ordinal: argument.ordinal,
                            ty: sigil::resolved::ResolvedSignatureTy {
                                syntax: self
                                    .rewrite_inherent_signature_ast_ty(id, argument.ty.syntax()),
                                direct_constructor_trait: argument
                                    .ty
                                    .direct_constructor_trait
                                    .clone(),
                            },
                            span: argument.span.clone(),
                        })
                        .collect::<Vec<_>>();
                    let rewritten_value_parameters = value_parameters
                        .iter()
                        .map(|parameter| sigil::resolved::ResolvedValueParameter {
                            id: parameter.id.clone(),
                            mode: parameter.mode,
                            ty: sigil::resolved::ResolvedSignatureTy {
                                syntax: self
                                    .rewrite_inherent_signature_ast_ty(id, parameter.ty.syntax()),
                                direct_constructor_trait: parameter
                                    .ty
                                    .direct_constructor_trait
                                    .clone(),
                            },
                            span: parameter.span.clone(),
                        })
                        .collect::<Vec<_>>();
                    let rewritten_return_type =
                        return_type
                            .as_ref()
                            .map(|ty| sigil::resolved::ResolvedSignatureTy {
                                syntax: self.rewrite_inherent_signature_ast_ty(id, ty.syntax()),
                                direct_constructor_trait: ty.direct_constructor_trait.clone(),
                            });
                    // The canonical Pattern consumer derives its result from selected
                    // projection slots; $Return is not a caller-supplied type argument.
                    let pattern_result_slot = matches!(stmt, Resolved::BuiltinDecl(..))
                        && Self::surface_qualified_name(id.qualified_name.as_deref())
                            == Some("Kernel::apply_pattern")
                        && return_type_arguments.is_empty()
                        && clause.is_none()
                        && crate::checker::definitions::special_form_shape_apply_pattern(
                            value_parameters,
                            &return_type.as_ref().map(|ty| ty.syntax.clone()),
                        );
                    if !pattern_result_slot {
                        crate::checker::signatures::validate_return_type_argument_definition(
                            id.qualified_name.as_deref().unwrap_or(&id.name),
                            &rewritten_return_type_arguments,
                            &rewritten_value_parameters,
                            rewritten_return_type.as_ref(),
                        )?;
                    }
                    crate::checker::signatures::validate_constructor_variable_constraints(
                        &id.name,
                        &rewritten_return_type_arguments,
                        &rewritten_value_parameters,
                        rewritten_return_type.as_ref().map(|ty| ty.syntax()),
                        clause.as_ref(),
                        None,
                        &constructor_trait_ids,
                    )?;
                }
                Resolved::TraitDef(_, _, _, clause, methods, _) => {
                    Self::validate_type_shape_clause(
                        clause.as_ref(),
                        true,
                        None,
                        &constructor_trait_ids,
                        &trait_names,
                    )?;
                    for method in methods {
                        Self::validate_type_shape_clause(
                            method.where_clause.as_ref(),
                            false,
                            None,
                            &constructor_trait_ids,
                            &trait_names,
                        )?;
                        crate::checker::signatures::validate_return_type_argument_definition(
                            method
                                .id
                                .qualified_name
                                .as_deref()
                                .unwrap_or(&method.id.name),
                            &method.return_type_arguments,
                            &method.value_parameters,
                            Some(&method.ret_ty),
                        )?;
                        crate::checker::signatures::validate_constructor_variable_constraints(
                            &method.id.name,
                            &method.return_type_arguments,
                            &method.value_parameters,
                            Some(method.ret_ty.syntax()),
                            method.where_clause.as_ref(),
                            clause.as_ref(),
                            &constructor_trait_ids,
                        )?;
                    }
                }
                Resolved::TraitImplDef(_, _, trait_id, _, _, clause, methods) => {
                    Self::validate_type_shape_clause(
                        clause.as_ref(),
                        false,
                        Some(trait_id),
                        &constructor_trait_ids,
                        &trait_names,
                    )?;
                    for method in methods {
                        Self::validate_type_shape_clause(
                            method.where_clause.as_ref(),
                            false,
                            None,
                            &constructor_trait_ids,
                            &trait_names,
                        )?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(in crate::checker) fn predeclare_signature_aliases(
        &mut self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        for stmt in stmts {
            let Resolved::TypeAlias(span, name, params, rhs, _) = stmt else {
                continue;
            };
            if self.signature_aliases.contains_key(name) || self.env.lookup_type_def(name).is_some()
            {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Internal consistency error: duplicate type owner `{}` reached Scar after resolution",
                        name
                    ),
                    span: span.clone(),
                    hint: Some(
                        "Sigil must reject duplicate owners before type checking.".into(),
                    ),
                });
            }
            let mut used = HashSet::new();
            Self::collect_ast_ty_type_params(rhs, &mut used);
            if let Some(param) = params.iter().find(|param| !used.contains(&param.name)) {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Type alias {} has an unused type parameter {}",
                        name, param.name
                    ),
                    span: param.span.clone(),
                    hint: Some(
                        "Every alias type parameter must appear in its function signature.".into(),
                    ),
                });
            }
            self.signature_aliases.insert(
                name.clone(),
                SignatureAliasInfo {
                    params: params.clone(),
                    rhs: rhs.clone(),
                    span: span.clone(),
                },
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod shared_registry_tests {
    use super::*;

    fn resolve(source: &str) -> Vec<Resolved> {
        let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).unwrap();
        crate::test_support::resolve_ast_with_builtin_prelude(ast).unwrap()
    }

    #[test]
    fn inherited_slots_detach_only_changed_traits_from_saved_branches() {
        let source = "deftrait CowRoot where Self: Type<$A> {}\ndeftrait CowChild where Self: CowRoot {}\ndeftrait CowPlain {}";
        let mut parent = Checker::with_persistent_state(
            crate::test_support::session_from_cached_std_prelude().state,
            TypecheckContext::default(),
        );
        parent.check_program(resolve(source)).unwrap();
        let child_key = parent.trait_key_by_short_name("CowChild").unwrap();
        let root_key = parent.trait_key_by_short_name("CowRoot").unwrap();
        let plain_key = parent.trait_key_by_short_name("CowPlain").unwrap();
        assert_eq!(parent.traits[&child_key].constructor_slots, vec!["$A"]);
        assert_eq!(
            bincode::serialize(&parent.traits[&child_key]).unwrap(),
            bincode::serialize(parent.traits[&child_key].as_ref()).unwrap(),
        );
        // Model the declaration state before inherited slots are resolved.
        Arc::make_mut(parent.traits.get_mut(&child_key).unwrap())
            .constructor_slots
            .clear();
        let checkpoint = parent
            .persistent_state_with_env(parent.env.clone())
            .checkpoint(Vec::new());
        let sibling = parent.spawn_child_checker(parent.env.clone());
        let mut child = parent.spawn_child_checker(parent.env.clone());
        assert!(Arc::ptr_eq(
            &parent.traits[&child_key],
            &child.traits[&child_key]
        ));

        child.resolve_trait_constraint_closure().unwrap();
        assert_eq!(child.traits[&child_key].constructor_slots, vec!["$A"]);
        assert!(parent.traits[&child_key].constructor_slots.is_empty());
        assert!(sibling.traits[&child_key].constructor_slots.is_empty());
        assert!(checkpoint.traits[&child_key].constructor_slots.is_empty());
        assert!(!Arc::ptr_eq(
            &parent.traits[&child_key],
            &child.traits[&child_key]
        ));
        for unchanged in [&root_key, &plain_key] {
            assert!(Arc::ptr_eq(
                &parent.traits[unchanged],
                &child.traits[unchanged]
            ));
        }
        let resolved_child = child.traits[&child_key].clone();
        child.resolve_trait_constraint_closure().unwrap();
        assert!(Arc::ptr_eq(&resolved_child, &child.traits[&child_key]));

        let bytes = bincode::serialize(&checkpoint).unwrap();
        let restored: ScarCheckpoint = bincode::deserialize(&bytes).unwrap();
        let mut restored =
            Checker::with_persistent_state(restored.into(), TypecheckContext::default());
        restored.resolve_trait_constraint_closure().unwrap();
        assert_eq!(restored.traits[&child_key].constructor_slots, vec!["$A"]);
        assert!(checkpoint.traits[&child_key].constructor_slots.is_empty());
    }

    #[test]
    fn shared_callable_signatures_keep_instantiations_and_failed_sessions_independent() {
        const DEFINITION: &str = "def cow_identity(value: $A) -> $A { value }";
        let mut original = crate::test_support::session_from_cached_std_prelude();
        let definition = resolve(DEFINITION);
        let Resolved::Def(_, id, ..) = &definition[0] else {
            panic!("expected the identity definition");
        };
        let uid = id.unique_id;
        original.typecheck(definition).unwrap();
        let declaration = original.state.callable_signatures[&uid].clone();
        assert!(matches!(declaration.value_parameters[0].ty, Ty::Var(_)));
        assert_eq!(
            bincode::serialize(&declaration).unwrap(),
            bincode::serialize(declaration.as_ref()).unwrap(),
        );
        let checkpoint = original.checkpoint();
        let mut successful = original.clone();
        let mut rejected = original.clone();
        let calls = |suffix: &str| {
            let resolved = resolve(&format!("{DEFINITION}\n{suffix}"));
            let Resolved::Def(_, id, ..) = &resolved[0] else {
                panic!("expected the identity definition before calls");
            };
            assert_eq!(id.unique_id, uid);
            resolved.into_iter().skip(1).collect()
        };
        let typed = successful
            .typecheck(calls("cow_identity(1)\ncow_identity(\"ok\")"))
            .unwrap();
        assert_eq!(typed[0].ty, Ty::Int);
        assert_eq!(typed[1].ty, Ty::Str);
        assert!(Arc::ptr_eq(
            &declaration,
            &successful.state.callable_signatures[&uid]
        ));
        let error = rejected
            .typecheck(calls("wrong: Boolean = cow_identity(1)"))
            .expect_err("Boolean annotation cannot accept an Int result");
        assert!(
            error.message.contains("Boolean") && error.message.contains("Int"),
            "expected the annotation type mismatch: {error:?}"
        );
        assert!(Arc::ptr_eq(
            &declaration,
            &rejected.state.callable_signatures[&uid]
        ));
        let mut sibling = original.clone();
        assert_eq!(
            sibling
                .typecheck(calls("cow_identity(\"after failure\")"))
                .unwrap()[0]
                .ty,
            Ty::Str
        );
        assert!(Arc::ptr_eq(
            &declaration,
            &checkpoint.callable_signatures[&uid]
        ));
        assert!(matches!(declaration.value_parameters[0].ty, Ty::Var(_)));

        let bytes = bincode::serialize(&checkpoint).unwrap();
        let decoded: ScarCheckpoint = bincode::deserialize(&bytes).unwrap();
        let mut restored = ScarSession::new();
        restored.rollback(decoded);
        assert_eq!(
            restored.typecheck(calls("cow_identity(2)")).unwrap()[0].ty,
            Ty::Int
        );
    }
}

#[cfg(test)]
mod constructor_mapping_tests {
    use super::*;

    #[test]
    fn declaration_parent_proof_resolves_mapping_through_an_ordinary_trait() {
        // This test targets declaration coverage. Body checking separately
        // rejects the deliberately unconsumed capabilities of these empty impls.
        let ast = spire::parse_with_context(
            r#"
deftrait DepRoot where Self: Type<$A> {}
deftrait DepChild where Self: DepRoot {}
deftrait Marker {}
defenum Leaf<$A> { Leaf($A), }
defenum Bridge<$M> { Bridge($M), }
defenum DepShell<$L, $A> { Shell($L, $A), }
impl DepChild for DepShell<Bridge<Leaf<Int>>, $T> {}
impl DepRoot for DepShell<$L, $A>
where
  $L: Marker
  $A: DepRoot.$A
{}
impl Marker for Bridge<$M> where $M: DepChild {}
impl DepChild for Leaf<$T> {}
impl DepRoot for Leaf<$T> {}
"#,
            spire::ParserContext::project(0),
        )
        .unwrap();
        let resolved = crate::test_support::resolve_ast_with_builtin_prelude(ast).unwrap();
        let mut checker = Checker::with_persistent_state(
            crate::test_support::session_from_cached_std_prelude().state,
            TypecheckContext::default(),
        );
        checker.predeclare_type_signatures(&resolved).unwrap();
        checker.predeclare_traits(&resolved).unwrap();
        let trait_key = checker.trait_key_by_short_name("DepChild").unwrap();
        let implementation = checker
            .trait_impl_candidate_keys(&trait_key)
            .into_iter()
            .map(|key| &checker.trait_impls[&key])
            .find(|info| info.target_name.ends_with("DepShell"))
            .unwrap();
        assert_eq!(implementation.constructor_slot_positions, vec![1]);
        assert!(checker.constructor_mapping_resolution.pending.is_empty());
        assert!(checker.constructor_mapping_resolution.resolving.is_empty());
        assert!(checker
            .constructor_mapping_resolution
            .requests
            .borrow()
            .is_none());
    }

    #[test]
    fn declaration_mapping_cycle_rejects_reentry_and_clears_transient_state() {
        let ast = spire::parse_with_context(
            "deftrait MappingRoot where Self: Type<$A> {}\n\
             deftrait MappingChild where Self: MappingRoot {}\n\
             defenum MappingBox<$A> { Box($A), }\n\
             impl MappingRoot for MappingBox<$A> {}\n\
             impl MappingChild for MappingBox<$A> {}",
            spire::ParserContext::project(0),
        )
        .unwrap();
        let resolved = crate::test_support::resolve_ast_with_builtin_prelude(ast).unwrap();
        let mut checker = Checker::with_persistent_state(
            crate::test_support::session_from_cached_std_prelude().state,
            TypecheckContext::default(),
        );
        checker.check_program(resolved).unwrap();
        let trait_key = checker.trait_key_by_short_name("MappingChild").unwrap();
        let key = checker.trait_impl_candidate_keys(&trait_key)[0].clone();
        // Simulate a declaration dependency requesting its own unfinished impl.
        // Source-level trait inheritance cycles are rejected earlier by closure.
        checker
            .constructor_mapping_resolution
            .pending
            .insert(key.clone());
        checker.constructor_mapping_resolution.resolving.insert(key);
        let substitutions = checker.substitutions.clone();
        let bounds = checker.tyvar_bounds.clone();
        let witnesses = checker.constructor_witness_traits.clone();
        let rigid = checker.rigid_tyvars.clone();
        let error = checker.resolve_impl_constructor_mappings().unwrap_err();
        assert!(
            error.message.contains("mapping dependency cycle"),
            "{error}"
        );
        assert!(checker.constructor_mapping_resolution.pending.is_empty());
        assert!(checker.constructor_mapping_resolution.resolving.is_empty());
        assert!(checker
            .constructor_mapping_resolution
            .requests
            .borrow()
            .is_none());
        assert_eq!(checker.substitutions, substitutions);
        assert_eq!(checker.tyvar_bounds, bounds);
        assert_eq!(checker.constructor_witness_traits, witnesses);
        assert_eq!(checker.rigid_tyvars, rigid);
    }
}
