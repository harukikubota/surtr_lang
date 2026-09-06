//! Role-preserving declaration types and contract alpha-equivalence.
//! This module does not select applicable implementations.
use super::*;
use diagnostics::{
    DiagnosticData, DiagnosticOrigin, SourceFact, SourceRole, StructuredDiagnostic,
    TraitMethodConstraintData, TraitMethodTypeListData, TypeDiagnosticReason,
};
use sindr::names::TypeName;

pub(super) struct MethodTypeEnvironment {
    pub bindings: HashMap<String, Ty>,
    pub head_bindings: HashMap<String, Ty>,
    pub self_ty: Ty,
    pub direct_inputs: super::signatures::DirectConstructorInputs,
}

pub(super) struct CanonicalMethodEnvironment {
    bindings: HashMap<String, CanonicalTy>,
    self_ty: CanonicalTy,
    pub slot_positions: Vec<usize>,
}

impl CanonicalTy {
    fn new(head: CanonicalTypeHead, arguments: Vec<Self>) -> Self {
        Self { head, arguments }
    }
    fn variable(id: u32) -> Self {
        Self::new(CanonicalTypeHead::Variable(id), Vec::new())
    }
    fn builtin(head: TypeName, arguments: Vec<Self>) -> Self {
        Self::new(CanonicalTypeHead::Builtin(head), arguments)
    }
    pub(super) fn substitute(&self, mapping: &HashMap<u32, u32>) -> Self {
        let head = match self.head {
            CanonicalTypeHead::Variable(var) => {
                CanonicalTypeHead::Variable(*mapping.get(&var).unwrap_or(&var))
            }
            _ => self.head.clone(),
        };
        Self::new(
            head,
            self.arguments
                .iter()
                .map(|ty| ty.substitute(mapping))
                .collect(),
        )
    }
}

impl ImplHeadTypeList {
    fn new(arguments: Vec<CanonicalTy>, target: CanonicalTy, span: Span) -> Self {
        let mut entries: Vec<_> = arguments
            .into_iter()
            .enumerate()
            .map(|(ordinal, ty)| TypeListEntry {
                role: TypeListRole::TraitArgument,
                ordinal: ordinal as u32,
                ty,
                origin: TypeOrigin { span: span.clone() },
            })
            .collect();
        entries.push(TypeListEntry {
            role: TypeListRole::ImplTarget,
            ordinal: 0,
            ty: target,
            origin: TypeOrigin { span },
        });
        Self { entries }
    }
}

/// Equality allows a bijection between variables, never a variable-to-tree
/// binding. This preserves repeated variables and rules out every cyclic
/// substitution accepted by an inference unifier without an occurs check.
#[derive(Clone, Default)]
struct AlphaEnvironment {
    forward: HashMap<u32, u32>,
    reverse: HashMap<u32, u32>,
}
impl AlphaEnvironment {
    fn compare(&mut self, expected: &CanonicalTy, actual: &CanonicalTy) -> Result<(), Vec<u32>> {
        if let (CanonicalTypeHead::Variable(left), CanonicalTypeHead::Variable(right)) =
            (&expected.head, &actual.head)
        {
            if self.forward.get(left).is_some_and(|value| value != right)
                || self.reverse.get(right).is_some_and(|value| value != left)
            {
                return Err(Vec::new());
            }
            self.forward.insert(*left, *right);
            self.reverse.insert(*right, *left);
        } else if expected.head != actual.head {
            return Err(Vec::new());
        }
        if expected.arguments.len() != actual.arguments.len() {
            return Err(Vec::new());
        }
        for (ordinal, (left, right)) in expected.arguments.iter().zip(&actual.arguments).enumerate()
        {
            if let Err(mut path) = self.compare(left, right) {
                path.insert(0, ordinal as u32);
                return Err(path);
            }
        }
        Ok(())
    }
    fn constraint(
        &mut self,
        left: &CanonicalMethodConstraint,
        right: &CanonicalMethodConstraint,
    ) -> bool {
        if self.compare(&left.subject, &right.subject).is_err() {
            return false;
        }
        match (&left.bound, &right.bound) {
            (CanonicalMethodBound::Trait(a), CanonicalMethodBound::Trait(b)) => a == b,
            (
                CanonicalMethodBound::TraitSlot {
                    trait_id: a,
                    ordinal: x,
                },
                CanonicalMethodBound::TraitSlot {
                    trait_id: b,
                    ordinal: y,
                },
            ) => a == b && x == y,
            (
                CanonicalMethodBound::TypeConstructor(a),
                CanonicalMethodBound::TypeConstructor(b),
            ) => a.len() == b.len() && a.iter().zip(b).all(|(a, b)| self.compare(a, b).is_ok()),
            _ => false,
        }
    }
    fn constraints_equal(
        &self,
        left: &CanonicalConstraintSet,
        right: &CanonicalConstraintSet,
    ) -> bool {
        // Every constraint variable must already belong to the declaration
        // namespace. Membership is checked without creating per-clause maps.
        let matches = |a: &CanonicalMethodConstraint, b: &CanonicalMethodConstraint| {
            let mut candidate = self.clone();
            candidate.constraint(a, b)
                && candidate.forward == self.forward
                && candidate.reverse == self.reverse
        };
        left.constraints
            .iter()
            .all(|a| right.constraints.iter().any(|b| matches(a, b)))
            && right
                .constraints
                .iter()
                .all(|b| left.constraints.iter().any(|a| matches(a, b)))
    }
}

impl Checker {
    fn canonical_resolved_type(&self, ty: &Ty) -> Result<CanonicalTy, TypeError> {
        let recurse = |ty| self.canonical_resolved_type(ty);
        Ok(match ty {
            Ty::Int => CanonicalTy::builtin(TypeName::Int, vec![]),
            Ty::Float => CanonicalTy::builtin(TypeName::Float, vec![]),
            Ty::Str => CanonicalTy::builtin(TypeName::String, vec![]),
            Ty::Bool => CanonicalTy::builtin(TypeName::Boolean, vec![]),
            Ty::Unit => CanonicalTy::builtin(TypeName::Unit, vec![]),
            Ty::Error => CanonicalTy::builtin(TypeName::Error, vec![]),
            Ty::Var(var) => CanonicalTy::variable(*var),
            Ty::Hole => CanonicalTy::new(CanonicalTypeHead::Hole, vec![]),
            Ty::List(ty) => CanonicalTy::builtin(TypeName::List, vec![recurse(ty)?]),
            Ty::Lazy(ty) => CanonicalTy::builtin(TypeName::Lazy, vec![recurse(ty)?]),
            Ty::Result(ok, err) => {
                CanonicalTy::builtin(TypeName::Result, vec![recurse(ok)?, recurse(err)?])
            }
            Ty::Tuple(types) => CanonicalTy::new(
                CanonicalTypeHead::Tuple,
                types.iter().map(recurse).collect::<Result<_, _>>()?,
            ),
            Ty::Func(params, ret) => CanonicalTy::new(
                CanonicalTypeHead::Function,
                params
                    .iter()
                    .chain(std::iter::once(ret.as_ref()))
                    .map(recurse)
                    .collect::<Result<_, _>>()?,
            ),
            Ty::SelfApp(types) => CanonicalTy::new(
                CanonicalTypeHead::SelfApplication,
                types.iter().map(recurse).collect::<Result<_, _>>()?,
            ),
            Ty::Facet(kind, a, b, c, d) => CanonicalTy::new(
                CanonicalTypeHead::Facet(*kind),
                [a, b, c, d]
                    .into_iter()
                    .map(|ty| recurse(ty))
                    .collect::<Result<_, _>>()?,
            ),
            Ty::Pid(name) => CanonicalTy::new(CanonicalTypeHead::Pid(name.clone()), vec![]),
            Ty::Enum(name, args) => CanonicalTy::new(
                self.canonical_nominal_head(name)?,
                args.iter().map(recurse).collect::<Result<_, _>>()?,
            ),
            Ty::Struct(name, _) | Ty::Record(name, _) => {
                // This route is only for resolved metadata without a surface
                // type. Source nominal applications always use their AST args.
                let args = self.resolved_named_type_args(name, ty).ok_or_else(|| TypeError::new(
                    "Internal error: nominal contract arguments require their declaration source", Span { start: 0, end: 0 }))?;
                CanonicalTy::new(
                    self.canonical_nominal_head(name)?,
                    args.iter().map(recurse).collect::<Result<_, _>>()?,
                )
            }
            Ty::BuiltinFunc { .. } | Ty::UserFunc { .. } => {
                return Err(TypeError::new(
                    "Internal error: runtime callable identity in declaration type",
                    Span { start: 0, end: 0 },
                ))
            }
        })
    }

    pub(super) fn canonical_nominal_head(
        &self,
        name: &str,
    ) -> Result<CanonicalTypeHead, TypeError> {
        if let Some(def) = self.env.lookup_type_def(name) {
            return Ok(CanonicalTypeHead::Nominal(def.tag));
        }
        match sindr::names::builtin_type_name(name) {
            Some(
                kind @ (TypeName::Regex
                | TypeName::RegexCaptures
                | TypeName::RegexMatch
                | TypeName::RandomGenerator
                | TypeName::FileHandle
                | TypeName::HashMap
                | TypeName::Generator
                | TypeName::StandbyInit
                | TypeName::TaskHandle
                | TypeName::Workers
                | TypeName::WorkerLease),
            ) => Ok(CanonicalTypeHead::Builtin(kind)),
            _ => Err(TypeError::new(
                "Internal error: resolved nominal declaration is missing",
                Span { start: 0, end: 0 },
            )),
        }
    }

    fn canonical_ast_type(
        &mut self,
        ast: &AstTy,
        resolved: &Ty,
        raw: &MethodTypeEnvironment,
        environment: &CanonicalMethodEnvironment,
    ) -> Result<CanonicalTy, TypeError> {
        let (name, args) = match ast {
            AstTy::Named(_, name) => (Some(name), &[][..]),
            AstTy::Generic(_, name, args) => (Some(name), args.as_slice()),
            _ => (None, &[][..]),
        };
        if let Some(name) = name {
            if args.is_empty() {
                if name == "Self" {
                    return Ok(environment.self_ty.clone());
                }
                if let Some(ty) = environment.bindings.get(name) {
                    return Ok(ty.clone());
                }
            }
            if let Some(alias) = self.signature_aliases.get(name).cloned() {
                fn substitute(ast: &AstTy, mapping: &HashMap<String, AstTy>) -> AstTy {
                    match ast {
                        AstTy::Named(_, name) if mapping.contains_key(name) => {
                            mapping[name].clone()
                        }
                        AstTy::Generic(span, name, args) => AstTy::Generic(
                            span.clone(),
                            name.clone(),
                            args.iter().map(|ty| substitute(ty, mapping)).collect(),
                        ),
                        AstTy::Tuple(span, args) => AstTy::Tuple(
                            span.clone(),
                            args.iter().map(|ty| substitute(ty, mapping)).collect(),
                        ),
                        AstTy::Func(span, args, ret) => AstTy::Func(
                            span.clone(),
                            args.iter().map(|ty| substitute(ty, mapping)).collect(),
                            Box::new(substitute(ret, mapping)),
                        ),
                        other => other.clone(),
                    }
                }
                let mapping = alias
                    .params
                    .iter()
                    .zip(args)
                    .map(|(param, arg)| (param.name.clone(), arg.clone()))
                    .collect();
                let expanded = substitute(&alias.rhs, &mapping);
                return self.canonical_ast_type(&expanded, resolved, raw, environment);
            }
            if name == "Self" && !args.is_empty() {
                let canonical_args = args
                    .iter()
                    .map(|arg| self.resolve_canonical_ast_type(arg, raw, environment))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut target = environment.self_ty.clone();
                if canonical_args.len() != environment.slot_positions.len() {
                    return Err(TypeError::new(
                        "Self constructor slot arity mismatch",
                        Self::ast_ty_span(ast).clone(),
                    ));
                }
                for (&position, arg) in environment.slot_positions.iter().zip(canonical_args) {
                    let slot = target.arguments.get_mut(position).ok_or_else(|| {
                        TypeError::new(
                            "Internal error: validated constructor slot is outside its target",
                            Self::ast_ty_span(ast).clone(),
                        )
                    })?;
                    *slot = arg;
                }
                return Ok(target);
            }
        }
        match (ast, resolved) {
            (
                AstTy::Generic(_, _, args),
                Ty::Struct(owner, _) | Ty::Record(owner, _) | Ty::Enum(owner, _),
            ) => Ok(CanonicalTy::new(
                self.canonical_nominal_head(owner)?,
                args.iter()
                    .map(|arg| self.resolve_canonical_ast_type(arg, raw, environment))
                    .collect::<Result<_, _>>()?,
            )),
            (AstTy::Tuple(_, args), Ty::Tuple(types)) => Ok(CanonicalTy::new(
                CanonicalTypeHead::Tuple,
                args.iter()
                    .zip(types)
                    .map(|(arg, ty)| self.canonical_ast_type(arg, ty, raw, environment))
                    .collect::<Result<_, _>>()?,
            )),
            (AstTy::Func(_, params, ret), Ty::Func(types, result)) => {
                let mut args = params
                    .iter()
                    .zip(types)
                    .map(|(arg, ty)| self.canonical_ast_type(arg, ty, raw, environment))
                    .collect::<Result<Vec<_>, _>>()?;
                args.push(self.canonical_ast_type(ret, result, raw, environment)?);
                Ok(CanonicalTy::new(CanonicalTypeHead::Function, args))
            }
            (AstTy::Generic(_, _, args), Ty::List(inner)) => Ok(CanonicalTy::builtin(
                TypeName::List,
                vec![self.canonical_ast_type(&args[0], inner, raw, environment)?],
            )),
            (AstTy::Generic(_, _, args), Ty::Lazy(inner)) => Ok(CanonicalTy::builtin(
                TypeName::Lazy,
                vec![self.canonical_ast_type(&args[0], inner, raw, environment)?],
            )),
            (AstTy::Generic(_, _, args), Ty::Result(ok, err)) => {
                let ok = self.canonical_ast_type(&args[0], ok, raw, environment)?;
                let err = if args.len() == 2 {
                    self.canonical_ast_type(&args[1], err, raw, environment)?
                } else {
                    self.canonical_resolved_type(err)?
                };
                Ok(CanonicalTy::builtin(TypeName::Result, vec![ok, err]))
            }
            (AstTy::Generic(_, _, args), Ty::Facet(kind, a, b, c, d)) => {
                let children = args[1..]
                    .iter()
                    .zip([a, b, c, d])
                    .map(|(ast, ty)| self.canonical_ast_type(ast, ty, raw, environment))
                    .collect::<Result<_, _>>()?;
                Ok(CanonicalTy::new(CanonicalTypeHead::Facet(*kind), children))
            }
            (AstTy::Generic(_, _, args), Ty::SelfApp(items))
                if Self::constructor_application_parts(items).is_some() =>
            {
                let (witness, slots) =
                    Self::constructor_application_parts(items).expect("constructor application");
                let mut children = vec![
                    CanonicalTy::new(CanonicalTypeHead::Hole, vec![]),
                    self.canonical_resolved_type(witness)?,
                ];
                children.extend(
                    args.iter()
                        .zip(slots)
                        .map(|(ast, ty)| self.canonical_ast_type(ast, ty, raw, environment))
                        .collect::<Result<Vec<_>, _>>()?,
                );
                Ok(CanonicalTy::new(
                    CanonicalTypeHead::SelfApplication,
                    children,
                ))
            }
            (AstTy::Named(_, name), Ty::Struct(owner, _) | Ty::Record(owner, _))
                if self
                    .env
                    .lookup_type_def(owner)
                    .is_some_and(|def| !def.type_params.is_empty()) =>
            {
                // A bare constructor input introduces fresh slots. It is not a
                // fully applied nominal type with silently discarded arguments.
                let def = self
                    .env
                    .lookup_type_def(owner)
                    .expect("checked declaration")
                    .clone();
                let args = def
                    .type_params
                    .iter()
                    .map(|_| match self.env.fresh_tyvar() {
                        Ty::Var(id) => CanonicalTy::variable(id),
                        _ => unreachable!(),
                    })
                    .collect();
                let _ = name;
                Ok(CanonicalTy::new(CanonicalTypeHead::Nominal(def.tag), args))
            }
            _ => self.canonical_resolved_type(resolved),
        }
    }

    fn resolve_canonical_ast_type(
        &mut self,
        ast: &AstTy,
        raw: &MethodTypeEnvironment,
        environment: &CanonicalMethodEnvironment,
    ) -> Result<CanonicalTy, TypeError> {
        let mut bindings = raw.bindings.clone();
        let ty = self.resolve_trait_signature_ast_ty_in_context(
            ast,
            TypeSyntaxContext::General,
            &raw.self_ty,
            &mut bindings,
        )?;
        let ty =
            super::signatures::coalesce_direct_constructor_inputs(self, ty, &raw.direct_inputs);
        self.canonical_ast_type(ast, &ty, raw, environment)
    }

    pub(super) fn canonical_impl_head(
        &mut self,
        trait_args: &[AstTy],
        target: &AstTy,
        raw: &MethodTypeEnvironment,
        span: &Span,
    ) -> Result<(ImplHeadTypeList, CanonicalMethodEnvironment), TypeError> {
        let head_raw = MethodTypeEnvironment {
            bindings: raw.head_bindings.clone(),
            head_bindings: raw.head_bindings.clone(),
            self_ty: raw.self_ty.clone(),
            direct_inputs: super::signatures::DirectConstructorInputs::default(),
        };
        let raw = &head_raw;
        let bindings = raw
            .bindings
            .iter()
            .filter_map(|(name, ty)| match ty {
                Ty::Var(var) => Some((name.clone(), CanonicalTy::variable(*var))),
                _ => None,
            })
            .collect();
        let mut environment = CanonicalMethodEnvironment {
            bindings,
            self_ty: CanonicalTy::builtin(TypeName::Unit, vec![]),
            slot_positions: vec![],
        };
        let self_ty = self.canonical_ast_type(target, &raw.self_ty, raw, &environment)?;
        environment.self_ty = self_ty.clone();
        let arguments = trait_args
            .iter()
            .map(|arg| self.resolve_canonical_ast_type(arg, raw, &environment))
            .collect::<Result<_, _>>()?;
        Ok((
            ImplHeadTypeList::new(arguments, self_ty, span.clone()),
            environment,
        ))
    }

    pub(super) fn canonical_contract_environment(
        &self,
        raw: &MethodTypeEnvironment,
        trait_info: &TraitInfo,
        head: &ImplHeadTypeList,
        slots: &[usize],
    ) -> Result<CanonicalMethodEnvironment, TypeError> {
        let mut bindings = HashMap::new();
        for (name, ty) in &raw.bindings {
            if let Some(ordinal) = trait_info
                .type_params
                .iter()
                .position(|param| &param.name == name)
            {
                bindings.insert(name.clone(), head.entries[ordinal].ty.clone());
            } else {
                bindings.insert(name.clone(), self.canonical_resolved_type(ty)?);
            }
        }
        Ok(CanonicalMethodEnvironment {
            bindings,
            self_ty: head.entries.last().expect("impl target").ty.clone(),
            slot_positions: slots.to_vec(),
        })
    }

    pub(super) fn canonical_method_list(
        &mut self,
        rta: &[Ty],
        params: &[Ty],
        ret: &Ty,
        rta_sources: &[ResolvedReturnTypeArgument],
        param_sources: &[ResolvedValueParameter],
        ret_source: &AstTy,
        clause: Option<&TypedWhereClause>,
        raw: &MethodTypeEnvironment,
        environment: &CanonicalMethodEnvironment,
        return_environment: Option<&CanonicalMethodEnvironment>,
    ) -> Result<MethodSignatureTypeList, TypeError> {
        let mut entries = Vec::new();
        for (role, types, sources) in [
            (
                TypeListRole::ReturnTypeArgument,
                rta,
                rta_sources.iter().map(|arg| &arg.ty).collect::<Vec<_>>(),
            ),
            (
                TypeListRole::ValueParameter,
                params,
                param_sources.iter().map(|arg| &arg.ty).collect(),
            ),
            (
                TypeListRole::ReturnType,
                std::slice::from_ref(ret),
                vec![ret_source],
            ),
        ] {
            for (ordinal, ty) in types.iter().enumerate() {
                let ast = sources[ordinal];
                entries.push(TypeListEntry {
                    role,
                    ordinal: ordinal as u32,
                    ty: self.canonical_ast_type(
                        ast,
                        ty,
                        raw,
                        if role == TypeListRole::ReturnType {
                            return_environment.unwrap_or(environment)
                        } else {
                            environment
                        },
                    )?,
                    origin: TypeOrigin {
                        span: Self::ast_ty_span(ast).clone(),
                    },
                });
            }
        }
        let mut where_constraints = CanonicalConstraintSet::default();
        if let Some(clause) = clause {
            for constraint in &clause.constraints {
                let subject =
                    self.resolve_canonical_ast_type(&constraint.subject, raw, environment)?;
                for bound in &constraint.bounds {
                    let bound = match bound {
                        TypedWhereConstraintRhs::Trait { trait_id } => {
                            CanonicalMethodBound::Trait(trait_id.unique_id)
                        }
                        TypedWhereConstraintRhs::TraitSlot {
                            trait_id,
                            slot_ordinal,
                            ..
                        } => CanonicalMethodBound::TraitSlot {
                            trait_id: trait_id.unique_id,
                            ordinal: *slot_ordinal,
                        },
                        TypedWhereConstraintRhs::TypeConstructor { slots, .. } => {
                            CanonicalMethodBound::TypeConstructor(
                                slots
                                    .iter()
                                    .map(|ty| self.resolve_canonical_ast_type(ty, raw, environment))
                                    .collect::<Result<_, _>>()?,
                            )
                        }
                    };
                    where_constraints
                        .constraints
                        .push(CanonicalMethodConstraint {
                            subject: subject.clone(),
                            bound,
                            origin: TypeOrigin {
                                span: constraint.span.clone(),
                            },
                        });
                }
            }
        }
        Ok(MethodSignatureTypeList {
            entries,
            where_constraints,
        })
    }

    pub(super) fn validate_trait_method_contract(
        &self,
        expected_head: &ImplHeadTypeList,
        actual_head: &ImplHeadTypeList,
        expected: &MethodSignatureTypeList,
        actual: &MethodSignatureTypeList,
        method: &str,
        contract_span: &Span,
        impl_span: &Span,
        contract_where: Option<&TypedWhereClause>,
        impl_where: Option<&TypedWhereClause>,
    ) -> Result<HashMap<u32, u32>, TypeError> {
        let mut environment = AlphaEnvironment::default();
        for role in [
            TypeListRole::ReturnTypeArgument,
            TypeListRole::ValueParameter,
            TypeListRole::ReturnType,
        ] {
            let expected_count = expected
                .entries
                .iter()
                .filter(|entry| entry.role == role)
                .count();
            let actual_count = actual
                .entries
                .iter()
                .filter(|entry| entry.role == role)
                .count();
            if expected_count != actual_count {
                return Err(self.contract_error(
                    method,
                    TypeDiagnosticReason::TraitMethodTypeListArityMismatch,
                    role,
                    expected_count.min(actual_count) as u32,
                    vec![],
                    String::new(),
                    String::new(),
                    Some(expected_count as u32),
                    Some(actual_count as u32),
                    contract_span.clone(),
                    impl_span.clone(),
                ));
            }
        }
        for (left, right) in expected_head
            .entries
            .iter()
            .chain(&expected.entries)
            .zip(actual_head.entries.iter().chain(&actual.entries))
        {
            if let Err(path) = environment.compare(&left.ty, &right.ty) {
                return Err(self.contract_error(
                    method,
                    TypeDiagnosticReason::TraitMethodTypeListMismatch,
                    left.role,
                    left.ordinal,
                    path,
                    self.canonical_type_name(&left.ty),
                    self.canonical_type_name(&right.ty),
                    None,
                    None,
                    left.origin.span.clone(),
                    right.origin.span.clone(),
                ));
            }
        }
        if !environment.constraints_equal(&expected.where_constraints, &actual.where_constraints) {
            let describe = |set: &CanonicalConstraintSet| {
                set.constraints
                    .iter()
                    .map(|constraint| {
                        let subject = self.canonical_type_name(&constraint.subject);
                        match &constraint.bound {
                            CanonicalMethodBound::Trait(id) => {
                                let name = &self
                                    .traits
                                    .values()
                                    .find(|info| info.id.unique_id == *id)
                                    .expect("resolved constraint trait")
                                    .id
                                    .name;
                                format!("{subject}: {name}")
                            }
                            CanonicalMethodBound::TypeConstructor(types) => format!(
                                "{subject}: Type<{}>",
                                types
                                    .iter()
                                    .map(|ty| self.canonical_type_name(ty))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            CanonicalMethodBound::TraitSlot { .. } => {
                                format!("{subject}: constructor slot")
                            }
                        }
                    })
                    .collect()
            };
            return Err(TypeError::new(
                format!("Trait impl method {method} has incompatible trait constraints"),
                impl_span.clone(),
            )
            .with_structured(StructuredDiagnostic {
                reason: TypeDiagnosticReason::TraitMethodConstraintMismatch,
                origin: DiagnosticOrigin::Declaration,
                data: DiagnosticData::TraitMethodConstraint(TraitMethodConstraintData {
                    method_name: method.into(),
                    expected_constraints: describe(&expected.where_constraints),
                    actual_constraints: describe(&actual.where_constraints),
                }),
                primary: SourceFact::untyped(
                    SourceRole::Impl,
                    diagnostics::SourceId(0),
                    impl_where.map_or(impl_span, |clause| &clause.span).clone(),
                ),
                related: vec![SourceFact::untyped(
                    SourceRole::Contract,
                    diagnostics::SourceId(0),
                    contract_where
                        .map_or(contract_span, |clause| &clause.span)
                        .clone(),
                )],
                remediation: None,
            }));
        }
        Ok(environment.reverse)
    }

    fn canonical_type_name(&self, ty: &CanonicalTy) -> String {
        fn render(checker: &Checker, ty: &CanonicalTy, vars: &mut HashMap<u32, String>) -> String {
            let args = ty
                .arguments
                .iter()
                .map(|ty| render(checker, ty, vars))
                .collect::<Vec<_>>();
            let name = match &ty.head {
                CanonicalTypeHead::Variable(var) => {
                    let next = vars.len();
                    return vars
                        .entry(*var)
                        .or_insert_with(|| Checker::diagnostic_tyvar_name(next))
                        .clone();
                }
                CanonicalTypeHead::Builtin(name) => name.as_str().to_string(),
                CanonicalTypeHead::Nominal(tag) => {
                    let def = checker
                        .env
                        .type_defs
                        .values()
                        .find(|def| def.tag == *tag)
                        .expect("canonical nominal tag");
                    Checker::surface_name(&def.name).to_string()
                }
                CanonicalTypeHead::Tuple => return format!("({})", args.join(", ")),
                CanonicalTypeHead::Function => {
                    return format!(
                        "({} -> {})",
                        args[..args.len() - 1].join(", "),
                        args.last().expect("return type")
                    )
                }
                CanonicalTypeHead::SelfApplication => "Self".into(),
                CanonicalTypeHead::Facet(kind) => {
                    return format!("Facet<{}, {}>", kind.as_str(), args.join(", "))
                }
                CanonicalTypeHead::Pid(name) => {
                    return format!("PID<{}>", Checker::surface_name(name))
                }
                CanonicalTypeHead::Hole => return "_".into(),
            };
            if args.is_empty() {
                name
            } else {
                format!("{name}<{}>", args.join(", "))
            }
        }
        render(self, ty, &mut HashMap::new())
    }
    fn contract_error(
        &self,
        method: &str,
        reason: TypeDiagnosticReason,
        role: TypeListRole,
        ordinal: u32,
        nested_path: Vec<u32>,
        expected_type: String,
        actual_type: String,
        expected_count: Option<u32>,
        actual_count: Option<u32>,
        contract_span: Span,
        impl_span: Span,
    ) -> TypeError {
        let description = if reason == TypeDiagnosticReason::TraitMethodTypeListArityMismatch {
            "has incompatible arity"
        } else {
            "has an incompatible signature"
        };
        let source_fact = |role, span, ty| {
            if reason == TypeDiagnosticReason::TraitMethodTypeListArityMismatch {
                SourceFact::untyped(role, diagnostics::SourceId(0), span)
            } else {
                SourceFact::typed(role, diagnostics::SourceId(0), span, ty)
            }
        };
        TypeError::new(
            format!("Trait impl method {method} {description}"),
            impl_span.clone(),
        )
        .with_structured(StructuredDiagnostic {
            reason,
            origin: DiagnosticOrigin::Declaration,
            primary: source_fact(SourceRole::Impl, impl_span, actual_type.clone()),
            related: vec![source_fact(
                SourceRole::Contract,
                contract_span,
                expected_type.clone(),
            )],
            data: DiagnosticData::TraitMethodTypeList(TraitMethodTypeListData {
                role,
                ordinal,
                nested_path,
                expected_type,
                actual_type,
                method_name: method.into(),
                expected_count,
                actual_count,
            }),
            remediation: None,
        })
    }
}

impl CanonicalTraitImplPatternKey {
    pub(super) fn from_head(trait_id: u32, head: &ImplHeadTypeList) -> Self {
        fn normalize(ty: &CanonicalTy, variables: &mut HashMap<u32, u32>) -> CanonicalTy {
            let head = match ty.head {
                CanonicalTypeHead::Variable(var) => {
                    let ordinal = variables.len() as u32;
                    CanonicalTypeHead::Variable(*variables.entry(var).or_insert(ordinal))
                }
                _ => ty.head.clone(),
            };
            CanonicalTy {
                head,
                arguments: ty
                    .arguments
                    .iter()
                    .map(|arg| normalize(arg, variables))
                    .collect(),
            }
        }
        let mut variables = HashMap::new();
        let arguments = head
            .entries
            .iter()
            .filter(|entry| entry.role == TypeListRole::TraitArgument)
            .map(|entry| normalize(&entry.ty, &mut variables))
            .collect();
        let target = normalize(
            &head
                .entries
                .iter()
                .find(|entry| entry.role == TypeListRole::ImplTarget)
                .expect("impl head contains target")
                .ty,
            &mut variables,
        );
        Self {
            trait_ref: CanonicalTraitRef {
                trait_id,
                arguments,
            },
            target,
        }
    }
}

/// Structural unification over complete nominal applications. Fresh namespaces
/// separate candidate binders from requested inference variables.
#[derive(Default)]
struct CanonicalUnifier {
    bindings: HashMap<u32, CanonicalTy>,
    allow_ignored_callable_inputs: bool,
    rigid_variables: HashSet<u32>,
}
impl CanonicalUnifier {
    fn resolve(&self, ty: &CanonicalTy) -> CanonicalTy {
        if let CanonicalTypeHead::Variable(var) = ty.head {
            if let Some(bound) = self.bindings.get(&var) {
                return self.resolve(bound);
            }
        }
        CanonicalTy {
            head: ty.head.clone(),
            arguments: ty.arguments.iter().map(|arg| self.resolve(arg)).collect(),
        }
    }
    fn unify(&mut self, left: &CanonicalTy, right: &CanonicalTy) -> bool {
        let left = self.resolve(left);
        let right = self.resolve(right);
        if left == right {
            return true;
        }
        if let CanonicalTypeHead::Variable(var) = left.head {
            if self.rigid_variables.contains(&var) {
                return match right.head {
                    CanonicalTypeHead::Variable(other)
                        if !self.rigid_variables.contains(&other) =>
                    {
                        self.unify(&right, &left)
                    }
                    _ => false,
                };
            }
            fn occurs(var: u32, ty: &CanonicalTy) -> bool {
                ty.head == CanonicalTypeHead::Variable(var)
                    || ty.arguments.iter().any(|arg| occurs(var, arg))
            }
            if occurs(var, &right) {
                return false;
            }
            self.bindings.insert(var, right);
            return true;
        }
        if matches!(right.head, CanonicalTypeHead::Variable(_)) {
            return self.unify(&right, &left);
        }
        left.head == right.head
            && left.arguments.len() == right.arguments.len()
            && left
                .arguments
                .iter()
                .zip(&right.arguments)
                .enumerate()
                .all(|(ordinal, (a, b))| {
                    (self.allow_ignored_callable_inputs
                        && left.head == CanonicalTypeHead::Function
                        && ordinal + 1 < left.arguments.len()
                        && b.head == CanonicalTypeHead::Hole)
                        || self.unify(a, b)
                })
    }
}

impl Checker {
    pub(super) fn canonical_patterns_overlap(
        left: &CanonicalTraitImplPatternKey,
        right: &CanonicalTraitImplPatternKey,
    ) -> bool {
        fn rename(ty: &CanonicalTy, side: u32) -> CanonicalTy {
            CanonicalTy {
                head: match ty.head {
                    CanonicalTypeHead::Variable(var) => CanonicalTypeHead::Variable(var * 2 + side),
                    _ => ty.head.clone(),
                },
                arguments: ty.arguments.iter().map(|ty| rename(ty, side)).collect(),
            }
        }
        let mut unifier = CanonicalUnifier::default();
        left.trait_ref.arguments.len() == right.trait_ref.arguments.len()
            && left
                .trait_ref
                .arguments
                .iter()
                .chain(std::iter::once(&left.target))
                .zip(
                    right
                        .trait_ref
                        .arguments
                        .iter()
                        .chain(std::iter::once(&right.target)),
                )
                .all(|(a, b)| unifier.unify(&rename(a, 0), &rename(b, 1)))
    }
}

#[cfg(test)]
mod applicability_tests {
    use super::*;

    fn checker(source: &str) -> Checker {
        let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).unwrap();
        let resolved = sigil::resolve(ast).unwrap();
        let mut checker = Checker::new(TypecheckContext::default());
        checker.check_program(resolved).unwrap();
        checker
    }

    #[test]
    fn declared_composite_capability_does_not_alpha_rename_distinct_rigid_slots() {
        let mut checker = checker("deftrait Marker {}\n");
        let Ty::Var(declared) = checker.env.fresh_tyvar() else {
            unreachable!()
        };
        let Ty::Var(requested) = checker.env.fresh_tyvar() else {
            unreachable!()
        };
        checker.rigid_tyvars.extend([declared, requested]);
        let trait_key = checker.trait_key_by_short_name("Marker").unwrap();
        checker.active_capabilities.push(CapabilityUse {
            subject_ty: Ty::List(Box::new(Ty::Var(declared))),
            subject_name: "Self".into(),
            trait_id: trait_key.clone(),
            span: Span { start: 0, end: 0 },
            consumed: false,
        });

        let proof = checker
            .probe_trait_head(&trait_key, &[], &Ty::List(Box::new(Ty::Var(requested))))
            .unwrap();

        assert!(matches!(proof, ApplicabilityProof::Unsatisfied));
        assert!(!checker.active_capabilities[0].consumed);
        assert!(
            matches!(checker.probe_trait_head(&trait_key,&[],&Ty::List(Box::new(Ty::Var(declared)))).unwrap(),ApplicabilityProof::Satisfied(indices) if indices==vec![0])
        );
    }

    #[test]
    fn concrete_impl_cannot_bind_a_nested_rigid_subject() {
        let mut checker = checker("deftrait Marker {}\nimpl Marker for List<Int> {}\n");
        let Ty::Var(var) = checker.env.fresh_tyvar() else {
            unreachable!()
        };
        checker.rigid_tyvars.insert(var);
        let subject = Ty::List(Box::new(Ty::Var(var)));
        let trait_key = checker.trait_key_by_short_name("Marker").unwrap();
        assert!(matches!(
            checker.probe_trait_head(&trait_key, &[], &subject).unwrap(),
            ApplicabilityProof::Unsatisfied
        ));
        assert!(matches!(
            checker
                .prove_trait_capability(&trait_key, &subject)
                .unwrap(),
            ApplicabilityProof::Unsatisfied
        ));
        assert_eq!(checker.resolve_ty(&subject), subject);
        assert!(matches!(
            checker
                .probe_trait_head(&trait_key, &[], &Ty::List(Box::new(Ty::Int)))
                .unwrap(),
            ApplicabilityProof::Satisfied(_)
        ));
        let mut generic_checker =
            self::checker("deftrait Marker {}\nimpl Marker for List<$T> {}\n");
        let Ty::Var(generic_var) = generic_checker.env.fresh_tyvar() else {
            unreachable!()
        };
        generic_checker.rigid_tyvars.insert(generic_var);
        let generic_trait = generic_checker.trait_key_by_short_name("Marker").unwrap();
        assert!(matches!(
            generic_checker
                .probe_trait_head(
                    &generic_trait,
                    &[],
                    &Ty::List(Box::new(Ty::Var(generic_var)))
                )
                .unwrap(),
            ApplicabilityProof::Satisfied(_)
        ));
    }

    #[test]
    fn method_target_uses_the_original_declaration_index() {
        let source="deftrait Only { def value(self: Self) -> Int }\nimpl Only for List<$A> { def value(self: Self) -> Int { 1 } }\n";
        let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).unwrap();
        let resolved = sigil::resolve(ast).unwrap();
        let mut checker = Checker::new(TypecheckContext::default());
        let typed = checker.check_program(resolved).unwrap();
        let key = checker.trait_impl_candidate_keys("Only").pop().unwrap();
        let method = checker.trait_impls[&key].methods["value"].clone();
        let Some(Ty::UserFunc {
            fun_idx: original, ..
        }) = checker
            .env
            .lookup_var(method.function_id.unique_id)
            .cloned()
        else {
            unreachable!()
        };
        let mut generated = typed
            .iter()
            .find(|node| matches!(&node.node,TypedInner::Def(idx,..) if *idx==original))
            .cloned()
            .expect("original emitted definition");
        let generated_idx = checker.env.next_fun_idx + 10;
        let TypedInner::Def(fun_idx, ..) = &mut generated.node else {
            unreachable!()
        };
        *fun_idx = generated_idx;
        checker.specializable_defs.clear();
        checker.specializable_defs.insert(generated_idx, generated);
        assert!(
            matches!(checker.impl_method_dispatch_target(&method),Some(TraitDispatchTarget::UserFunction {fun_idx,..}) if fun_idx==original)
        );
    }

    #[test]
    fn one_visible_impl_does_not_determine_unknown_subject() {
        let mut checker = checker("deftrait Only { def value(self: Self) -> Int }\nimpl Only for Int { def value(self: Self) -> Int { self } }\n");
        let unknown = checker.env.fresh_tyvar();
        let Ty::Var(var) = unknown else {
            unreachable!()
        };
        let trait_key = checker.trait_key_by_short_name("Only").unwrap();
        let result = checker
            .select_trait_method_instantiation(
                &trait_key,
                "value",
                &unknown,
                &[],
                &[unknown.clone()],
                &Ty::Int,
            )
            .unwrap();
        assert!(
            matches!(result,CandidateApplicability::Deferred(PendingTraitCandidate { waiting_on, .. }) if waiting_on==vec![var])
        );
        assert_eq!(checker.resolve_ty(&unknown), unknown);
    }

    #[test]
    fn one_visible_impl_does_not_determine_unknown_trait_argument() {
        let mut checker = checker("deftrait Only<$A> { def value(self: Self) -> Int }\nimpl Only<Int> for Int { def value(self: Self) -> Int { self } }\n");
        let unknown = checker.env.fresh_tyvar();
        let Ty::Var(var) = unknown else {
            unreachable!()
        };
        let trait_key = checker.trait_key_by_short_name("Only").unwrap();
        let result = checker
            .select_trait_method_instantiation(
                &trait_key,
                "value",
                &Ty::Int,
                &[unknown.clone()],
                &[Ty::Int],
                &Ty::Int,
            )
            .unwrap();
        assert!(
            matches!(result, CandidateApplicability::Deferred(PendingTraitCandidate { waiting_on, candidates }) if waiting_on == vec![var] && candidates.len() == 1)
        );
        assert_eq!(checker.resolve_ty(&unknown), unknown);
        let result = checker
            .select_trait_method_instantiation(
                &trait_key,
                "value",
                &Ty::Int,
                &[Ty::Str],
                &[Ty::Int],
                &Ty::Int,
            )
            .unwrap();
        assert!(
            matches!(result, CandidateApplicability::Rejected(rejection) if rejection.failures.iter().any(|failure| failure.kind == CandidateFailureKind::TraitImplHeadMismatch))
        );
    }

    #[test]
    fn pending_alias_preserves_nested_receiver_and_trait_arguments() {
        let mut checker = checker("defstruct Box<$T> { value: $T }\nimpl Box { def new(value: $T) -> Box<$T> { Box { value: value } } }\ndeftrait Rel<$A> {}\nimpl Rel<List<Int>> for Box<Int> {}\n");
        let Ty::Var(source) = checker.env.fresh_tyvar() else {
            unreachable!()
        };
        let Ty::Var(alias) = checker.env.fresh_tyvar() else {
            unreachable!()
        };
        let trait_key = checker.trait_key_by_short_name("Rel").unwrap();
        let name = checker.env.lookup_type_def("Box").unwrap().name.clone();
        checker.pending_trait_obligations.insert(
            source,
            vec![PendingTraitObligation {
                trait_id: trait_key.clone(),
                args: vec![Ty::List(Box::new(Ty::Var(source)))],
                receiver: Ty::Struct(name.clone(), vec![("value".into(), Ty::Var(source))]),
            }],
        );
        assert!(checker.bind_tyvar(source, &Ty::Var(alias)));
        assert!(!checker.pending_trait_obligations.contains_key(&source));
        assert_eq!(
            checker.pending_trait_obligations[&alias],
            vec![PendingTraitObligation {
                trait_id: trait_key,
                args: vec![Ty::List(Box::new(Ty::Var(alias)))],
                receiver: Ty::Struct(name, vec![("value".into(), Ty::Var(alias))]),
            }]
        );
        assert!(
            !checker.bind_tyvar(alias, &Ty::Bool),
            "aliased nested obligation must still reject a mismatched concrete binding"
        );
        assert_eq!(checker.resolve_ty(&Ty::Var(alias)), Ty::Var(alias));
        assert!(checker.bind_tyvar(alias, &Ty::Int));
        assert_eq!(checker.resolve_ty(&Ty::Var(source)), Ty::Int);
    }

    #[test]
    fn where_probe_waits_only_on_call_site_variables() {
        let mut checker = checker("deftrait Marker { def mark(self: Self) -> Int }\ndefstruct Box<$T> { value: $T }\nimpl Box { def new(value: $T) -> Box<$T> { Box { value: value } } }\ndeftrait Read { def read(self: Self) -> Int }\nimpl Read for Box<$T> where $T: Marker { def read(self: Self) -> Int { Marker::mark(self.value) } }\n");
        let unknown = checker.env.fresh_tyvar();
        let Ty::Var(var) = unknown else {
            unreachable!()
        };
        let trait_key = checker.trait_key_by_short_name("Read").unwrap();
        let result = checker.probe_trait_head(&trait_key, &[], &unknown).unwrap();
        assert!(matches!(result,ApplicabilityProof::Deferred(waiting_on) if waiting_on==vec![var]));
        assert_eq!(checker.resolve_ty(&unknown), unknown);
    }

    #[test]
    fn rejected_where_does_not_bind_either_requested_argument_for_the_next_candidate() {
        let mut checker = checker(
            r#"
deftrait Marker { def mark(self: Self) -> Int }
defstruct Box<$T> { value: $T }
impl Box { def new(value: $T) -> Box<$T> { Box { value: value } } }
deftrait Rel<$A,$B> { def apply(self: Self, a: $A, b: $B) -> Int }
impl Rel<Int,String> for Box<$T> where $T: Marker { def apply(self: Self, a: Int, b: String) -> Int { Marker::mark(self.value) } }
impl Rel<String,Int> for Box<$T> { def apply(self: Self, a: String, b: Int) -> Int { 1 } }
"#,
        );
        let a = checker.env.fresh_tyvar();
        let b = checker.env.fresh_tyvar();
        let trait_key = checker.trait_key_by_short_name("Rel").unwrap();
        let def = checker.env.lookup_type_def("Box").unwrap();
        let receiver = Ty::Struct(def.name.clone(), vec![("value".into(), Ty::Bool)]);
        let substitutions = checker.substitutions.clone();
        let pending = checker.pending_trait_obligations.clone();
        let witnesses = checker.constructor_witness_traits.clone();
        let bounds = checker.tyvar_bounds.clone();
        let next_variable = checker.env.next_tyvar;
        assert!(matches!(
            checker
                .probe_trait_head(&trait_key, &[a.clone(), b.clone()], &receiver)
                .unwrap(),
            ApplicabilityProof::Deferred(_)
        ));
        assert_eq!(checker.resolve_ty(&a), a);
        assert_eq!(checker.resolve_ty(&b), b);
        assert_eq!(checker.substitutions, substitutions);
        assert_eq!(checker.pending_trait_obligations, pending);
        assert_eq!(checker.constructor_witness_traits, witnesses);
        assert_eq!(checker.tyvar_bounds, bounds);
        assert_eq!(checker.env.next_tyvar, next_variable);
    }

    #[test]
    fn rejected_head_rolls_back_both_trait_argument_bindings() {
        let mut checker = checker("deftrait Rel<$A, $B> {}\nimpl Rel<Int, String> for Int {}\n");
        let a = checker.env.fresh_tyvar();
        let b = checker.env.fresh_tyvar();
        let trait_key = checker.trait_key_by_short_name("Rel").unwrap();
        assert!(!checker.trait_impl_exists_for_args(
            &trait_key,
            &[a.clone(), b.clone()],
            &Ty::Bool
        ));
        assert_eq!(checker.resolve_ty(&a), a);
        assert_eq!(checker.resolve_ty(&b), b);
    }
}

impl Checker {
    pub(super) fn impl_method_instantiation_contract(
        &mut self,
        _pattern: &CanonicalTraitImplPatternKey,
        trait_info: &TraitInfo,
        trait_args: &[AstTy],
        target: &AstTy,
        method: &TraitImplMethodInfo,
        fallback_ret: &AstTy,
        params: &[Ty],
        ret: &Ty,
        rtas: &[Ty],
        raw: &MethodTypeEnvironment,
        impl_clause: Option<&TypedWhereClause>,
        slots: &[usize],
    ) -> Result<ImplMethodInstantiationContract, TypeError> {
        let (head, mut environment) =
            self.canonical_impl_head(trait_args, target, raw, &method.span)?;
        environment.slot_positions = slots.to_vec();
        if method.display_name_override.is_some() {
            environment = self.canonical_contract_environment(raw, trait_info, &head, slots)?;
        }
        let return_environment = if method.ret_ty.is_none() {
            Some(self.canonical_contract_environment(raw, trait_info, &head, slots)?)
        } else {
            None
        };
        let signature = self.canonical_method_list(
            rtas,
            params,
            ret,
            &method.return_type_arguments,
            &method.value_parameters,
            method.ret_ty.as_ref().unwrap_or(fallback_ret),
            method.where_clause.as_ref(),
            raw,
            &environment,
            return_environment.as_ref(),
        )?;
        let impl_constraints = self
            .canonical_method_list(
                &[],
                &[],
                &Ty::Unit,
                &[],
                &[],
                &AstTy::Named(method.span.clone(), "Unit".into()),
                impl_clause,
                raw,
                &environment,
                None,
            )?
            .where_constraints;
        Ok(ImplMethodInstantiationContract {
            head,
            signature,
            impl_constraints,
        })
    }

    fn canonical_to_ty(&self, ty: &CanonicalTy) -> Result<Ty, TypeError> {
        let args = ty
            .arguments
            .iter()
            .map(|arg| self.canonical_to_ty(arg))
            .collect::<Result<Vec<_>, _>>()?;
        let invalid = || {
            TypeError::new(
                "Internal error: invalid canonical type application",
                Span { start: 0, end: 0 },
            )
        };
        let nominal = |def: &crate::env::TypeDefInfo| {
            let fields = self.instantiate_type_def_fields(def, &args);
            match def.kind {
                TypeKind::Struct => Ty::Struct(def.name.clone(), fields),
                TypeKind::Record | TypeKind::ConcreteError => Ty::Record(def.name.clone(), fields),
                TypeKind::Enum => Ty::Enum(def.name.clone(), args.clone()),
            }
        };
        Ok(match &ty.head {
            CanonicalTypeHead::Variable(var) => Ty::Var(*var),
            CanonicalTypeHead::Builtin(TypeName::Int) => Ty::Int,
            CanonicalTypeHead::Builtin(TypeName::Float) => Ty::Float,
            CanonicalTypeHead::Builtin(TypeName::String) => Ty::Str,
            CanonicalTypeHead::Builtin(TypeName::Boolean) => Ty::Bool,
            CanonicalTypeHead::Builtin(TypeName::Unit) => Ty::Unit,
            CanonicalTypeHead::Builtin(TypeName::Error) => Ty::Error,
            CanonicalTypeHead::Builtin(TypeName::List) if args.len() == 1 => {
                Ty::List(Box::new(args[0].clone()))
            }
            CanonicalTypeHead::Builtin(TypeName::Lazy) if args.len() == 1 => {
                Ty::Lazy(Box::new(args[0].clone()))
            }
            CanonicalTypeHead::Builtin(TypeName::Result) if args.len() == 2 => {
                Ty::Result(Box::new(args[0].clone()), Box::new(args[1].clone()))
            }
            CanonicalTypeHead::Builtin(
                name @ (TypeName::Regex
                | TypeName::RegexCaptures
                | TypeName::RegexMatch
                | TypeName::RandomGenerator
                | TypeName::FileHandle),
            ) if args.is_empty() => Ty::Enum(name.as_str().into(), args),
            CanonicalTypeHead::Builtin(
                name @ (TypeName::HashMap
                | TypeName::StandbyInit
                | TypeName::TaskHandle
                | TypeName::Workers
                | TypeName::WorkerLease),
            ) if args.len() == 1 => Ty::Enum(name.as_str().into(), args),
            CanonicalTypeHead::Builtin(TypeName::Generator) if args.len() == 2 => {
                Ty::Enum(TypeName::Generator.as_str().into(), args)
            }
            CanonicalTypeHead::Builtin(name) => nominal(
                self.env
                    .lookup_type_def(name.as_str())
                    .ok_or_else(invalid)?,
            ),
            CanonicalTypeHead::Nominal(tag) => nominal(
                self.env
                    .type_defs
                    .values()
                    .find(|def| def.tag == *tag)
                    .ok_or_else(invalid)?,
            ),
            CanonicalTypeHead::Tuple => Ty::Tuple(args),
            CanonicalTypeHead::Function if !args.is_empty() => Ty::Func(
                args[..args.len() - 1].to_vec(),
                Box::new(args.last().unwrap().clone()),
            ),
            CanonicalTypeHead::SelfApplication => Ty::SelfApp(args),
            CanonicalTypeHead::Facet(kind) if args.len() == 4 => Ty::Facet(
                *kind,
                Box::new(args[0].clone()),
                Box::new(args[1].clone()),
                Box::new(args[2].clone()),
                Box::new(args[3].clone()),
            ),
            CanonicalTypeHead::Pid(name) => Ty::Pid(name.clone()),
            CanonicalTypeHead::Hole => Ty::Hole,
            _ => return Err(invalid()),
        })
    }
}

#[derive(Debug)]
pub(super) enum ApplicabilityProof {
    Satisfied(Vec<usize>),
    Deferred(Vec<u32>),
    Unsatisfied,
}

impl Checker {
    pub(super) fn canonical_request(&self, ty: &Ty) -> Result<CanonicalTy, TypeError> {
        let ty = self.resolve_ty(ty);
        let ty = match ty {
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                Ty::Func(params, ret)
            }
            ty => ty,
        };
        self.canonical_resolved_type(&ty)
    }

    fn fresh_canonical(
        &self,
        ty: &CanonicalTy,
        fresh: &mut HashMap<u32, u32>,
        next_variable: &mut u32,
    ) -> CanonicalTy {
        let head = match ty.head {
            CanonicalTypeHead::Variable(var) => {
                let new_var = *fresh.entry(var).or_insert_with(|| {
                    let var = *next_variable;
                    *next_variable += 1;
                    var
                });
                CanonicalTypeHead::Variable(new_var)
            }
            _ => ty.head.clone(),
        };
        CanonicalTy {
            head,
            arguments: ty
                .arguments
                .iter()
                .map(|arg| self.fresh_canonical(arg, fresh, next_variable))
                .collect(),
        }
    }

    fn canonical_waiting_variables(&self, ty: &CanonicalTy, waiting: &mut Vec<u32>) {
        if let CanonicalTypeHead::Variable(var) = ty.head {
            if !self.rigid_tyvars.contains(&var) && !waiting.contains(&var) {
                waiting.push(var);
            }
        }
        for arg in &ty.arguments {
            self.canonical_waiting_variables(arg, waiting);
        }
    }

    fn rigid_capability_evidence(&self, var: u32, trait_name: &str) -> Option<Vec<usize>> {
        if !self.rigid_tyvar_entails_trait(var, trait_name, &mut HashSet::new()) {
            return None;
        }
        Some(
            self.active_capabilities
                .iter()
                .position(|capability| {
                    matches!(self.resolve_ty(&capability.subject_ty), Ty::Var(subject_var) if subject_var == var)
                        && self.trait_bound_entails(
                            &capability.trait_id,
                            trait_name,
                            &mut HashSet::new(),
                        )
                })
                .into_iter()
                .collect(),
        )
    }

    fn exact_capability_evidence(
        &self,
        subject: &CanonicalTy,
        trait_name: &str,
    ) -> Option<Vec<usize>> {
        let receiver = self.canonical_to_ty(subject).ok()?;
        self.active_capabilities
            .iter()
            .position(|capability| {
                self.capability_receiver_matches(
                    &capability.trait_id,
                    &capability.subject_ty,
                    &receiver,
                ) && self.trait_bound_entails(&capability.trait_id, trait_name, &mut HashSet::new())
            })
            .map(|index| vec![index])
    }

    fn prove_canonical_constraints(
        &self,
        constraints: &CanonicalConstraintSet,
        fresh: &mut HashMap<u32, u32>,
        unifier: &CanonicalUnifier,
        visiting: &mut HashSet<(u32, CanonicalTy)>,
        next_variable: &mut u32,
    ) -> Result<ApplicabilityProof, TypeError> {
        let mut waiting = Vec::new();
        let mut evidence = Vec::new();
        for constraint in &constraints.constraints {
            let CanonicalMethodBound::Trait(trait_id) = constraint.bound else {
                continue;
            };
            let subject = self.fresh_canonical(&constraint.subject, fresh, next_variable);
            let subject = unifier.resolve(&subject);
            match self.prove_canonical_capability(trait_id, &subject, visiting, next_variable)? {
                ApplicabilityProof::Unsatisfied => return Ok(ApplicabilityProof::Unsatisfied),
                ApplicabilityProof::Deferred(vars) => waiting.extend(vars),
                ApplicabilityProof::Satisfied(indices) => evidence.extend(indices),
            }
        }
        waiting.sort_unstable();
        waiting.dedup();
        evidence.sort_unstable();
        evidence.dedup();
        Ok(if waiting.is_empty() {
            ApplicabilityProof::Satisfied(evidence)
        } else {
            ApplicabilityProof::Deferred(waiting)
        })
    }

    fn prove_canonical_capability(
        &self,
        trait_id: u32,
        subject: &CanonicalTy,
        visiting: &mut HashSet<(u32, CanonicalTy)>,
        next_variable: &mut u32,
    ) -> Result<ApplicabilityProof, TypeError> {
        let trait_key = self
            .traits
            .iter()
            .find(|(_, info)| info.id.unique_id == trait_id)
            .map(|(key, _)| key.clone())
            .ok_or_else(|| {
                TypeError::new(
                    "Internal error: canonical Trait identity missing",
                    Span { start: 0, end: 0 },
                )
            })?;
        if let Some(evidence) = self.exact_capability_evidence(subject, &trait_key) {
            return Ok(ApplicabilityProof::Satisfied(evidence));
        }
        if let CanonicalTypeHead::Variable(var) = subject.head {
            if self.rigid_tyvars.contains(&var) {
                return Ok(self
                    .rigid_capability_evidence(var, &trait_key)
                    .map(ApplicabilityProof::Satisfied)
                    .unwrap_or(ApplicabilityProof::Unsatisfied));
            }
        }
        let mut waiting = Vec::new();
        self.canonical_waiting_variables(subject, &mut waiting);
        if !waiting.is_empty() {
            return Ok(ApplicabilityProof::Deferred(waiting));
        }
        let proof_key = (trait_id, subject.clone());
        if !visiting.insert(proof_key.clone()) {
            return Err(TypeError::new(
                format!(
                    "CyclicTraitObligation: {} for {}",
                    self.trait_display_name(&trait_key),
                    self.canonical_type_name(subject)
                ),
                Span { start: 0, end: 0 },
            ));
        }
        let candidates = self.trait_impl_candidate_keys(&trait_key);
        let mut deferred = Vec::new();
        for key in candidates {
            let info = self.trait_impls[&key].clone();
            let mut fresh = HashMap::new();
            let target = &info
                .head_type_list
                .entries
                .iter()
                .find(|entry| entry.role == TypeListRole::ImplTarget)
                .expect("target")
                .ty;
            let target = self.fresh_canonical(target, &mut fresh, next_variable);
            let mut unifier = CanonicalUnifier {
                rigid_variables: self.rigid_tyvars.clone(),
                ..Default::default()
            };
            if !unifier.unify(&target, subject) {
                continue;
            }
            match self.prove_canonical_constraints(
                &info.impl_constraints,
                &mut fresh,
                &unifier,
                visiting,
                next_variable,
            )? {
                ApplicabilityProof::Satisfied(evidence) => {
                    visiting.remove(&proof_key);
                    return Ok(ApplicabilityProof::Satisfied(evidence));
                }
                ApplicabilityProof::Deferred(vars) => deferred.extend(vars),
                ApplicabilityProof::Unsatisfied => {}
            }
        }
        visiting.remove(&proof_key);
        if !deferred.is_empty() {
            deferred.sort_unstable();
            deferred.dedup();
            return Ok(ApplicabilityProof::Deferred(deferred));
        }
        let subject_ty = self.canonical_to_ty(subject)?;
        Ok(
            if self.compiler_trait_impl_exists(&trait_key, &subject_ty) {
                ApplicabilityProof::Satisfied(Vec::new())
            } else {
                ApplicabilityProof::Unsatisfied
            },
        )
    }

    fn requested_head_type_list(
        &self,
        trait_args: &[Ty],
        receiver: &Ty,
    ) -> Result<RequestedHeadTypeList, TypeError> {
        let arguments = trait_args
            .iter()
            .map(|ty| self.canonical_request(ty))
            .collect::<Result<Vec<_>, _>>()?;
        let target = self.canonical_request(receiver)?;
        Ok(RequestedHeadTypeList {
            entries: ImplHeadTypeList::new(arguments, target, Span { start: 0, end: 0 }).entries,
        })
    }

    pub(super) fn select_trait_method_instantiation(
        &mut self,
        trait_name: &str,
        method_name: &str,
        receiver: &Ty,
        trait_args: &[Ty],
        argument_tys: &[Ty],
        result_ty: &Ty,
    ) -> Result<CandidateApplicability, TypeError> {
        let profile = self.profiler.start();
        let result = self.probe_trait_method_instantiation(
            trait_name,
            method_name,
            receiver,
            trait_args,
            argument_tys,
            result_ty,
        );
        self.profiler
            .finish(ProfileEvent::GenericTraitCandidateScan, profile);
        if let Ok(CandidateApplicability::Applicable(instantiation)) = &result {
            if !instantiation.caller_substitution.is_empty() {
                let checkpoint = self.candidate_probe_checkpoint();
                for (var, ty) in &instantiation.caller_substitution {
                    if !self.types_compatible(&Ty::Var(*var), ty) {
                        self.rollback_candidate_probe(checkpoint);
                        return Err(TypeError::new(
                            "SelectedTraitMethodInferenceConflict",
                            Span { start: 0, end: 0 },
                        ));
                    }
                }
            }
            for index in &instantiation.proof_evidence {
                self.active_capabilities[*index].consumed = true;
            }
        }
        result
    }

    fn probe_trait_method_instantiation(
        &mut self,
        trait_name: &str,
        method_name: &str,
        receiver: &Ty,
        trait_args: &[Ty],
        argument_tys: &[Ty],
        result_ty: &Ty,
    ) -> Result<CandidateApplicability, TypeError> {
        let requested_head = self.requested_head_type_list(trait_args, receiver)?;
        let receiver = requested_head
            .entries
            .iter()
            .find(|entry| entry.role == TypeListRole::ImplTarget)
            .expect("requested target")
            .ty
            .clone();
        let requested_args = requested_head
            .entries
            .iter()
            .filter(|entry| entry.role == TypeListRole::TraitArgument)
            .map(|entry| entry.ty.clone())
            .collect::<Vec<_>>();
        let invocation = argument_tys
            .iter()
            .map(|ty| self.canonical_request(ty))
            .collect::<Result<Vec<_>, _>>()?;
        let result = self.canonical_request(result_ty)?;
        let candidate_keys = self.trait_impl_candidate_keys(trait_name);
        let declarations = candidate_keys
            .iter()
            .map(|key| self.trait_impls[key].declaration_key.clone())
            .collect::<Vec<_>>();
        let mut waiting = Vec::new();
        fn method_variables(ty: &CanonicalTy, variables: &mut Vec<u32>) {
            if let CanonicalTypeHead::Variable(var) = ty.head {
                if !variables.contains(&var) {
                    variables.push(var);
                }
            }
            for child in &ty.arguments {
                method_variables(child, variables);
            }
        }
        for ty in requested_args.iter().chain(std::iter::once(&receiver)) {
            method_variables(ty, &mut waiting);
        }
        if !waiting.is_empty() {
            waiting.sort_unstable();
            waiting.dedup();
            return Ok(CandidateApplicability::Deferred(PendingTraitCandidate {
                waiting_on: waiting,
                candidates: declarations,
            }));
        }
        let mut caller_waiting = waiting;
        for ty in &invocation {
            self.canonical_waiting_variables(ty, &mut caller_waiting);
        }
        self.canonical_waiting_variables(&result, &mut caller_waiting);
        let mut selected = None;
        let mut failures = Vec::new();
        let mut deferred = Vec::new();
        for key in candidate_keys {
            let info = self.trait_impls[&key].clone();
            let Some(method) = info.methods.get(method_name) else {
                continue;
            };
            let reject = |kind| CandidateFailure {
                declaration: info.declaration_key.clone(),
                kind,
                span: method.span.clone(),
            };
            // The checkpoint owns every candidate binding and fresh variable.
            // Shared inference, proof, and carrier state is read-only while probing.
            let candidate = &*self;
            let mut next_variable = self.env.next_tyvar;
            let contract = method.instantiation_contract.clone().ok_or_else(|| {
                TypeError::new(
                    "Internal error: missing canonical impl method contract",
                    method.span.clone(),
                )
            })?;
            let mut fresh = HashMap::new();
            let mut unifier = CanonicalUnifier {
                rigid_variables: self.rigid_tyvars.clone(),
                allow_ignored_callable_inputs: true,
                ..Default::default()
            };
            let head_args = contract
                .head
                .entries
                .iter()
                .filter(|entry| entry.role == TypeListRole::TraitArgument)
                .collect::<Vec<_>>();
            if head_args.len() != requested_args.len() {
                failures.push(reject(CandidateFailureKind::TraitImplHeadMismatch));
                continue;
            }
            let mut matches = true;
            for (entry, requested) in head_args.iter().zip(&requested_args) {
                let expected = candidate.fresh_canonical(&entry.ty, &mut fresh, &mut next_variable);
                matches &= unifier.unify(&expected, requested);
            }
            let target = &contract
                .head
                .entries
                .iter()
                .find(|entry| entry.role == TypeListRole::ImplTarget)
                .expect("target")
                .ty;
            let target = candidate.fresh_canonical(target, &mut fresh, &mut next_variable);
            matches &= unifier.unify(&target, &receiver);
            let parameters = contract
                .signature
                .entries
                .iter()
                .filter(|entry| entry.role == TypeListRole::ValueParameter)
                .collect::<Vec<_>>();
            if !matches {
                failures.push(reject(CandidateFailureKind::TraitImplHeadMismatch));
                continue;
            }
            if parameters.len() != invocation.len() {
                failures.push(reject(CandidateFailureKind::TraitMethodInvocationMismatch));
                continue;
            }
            for (entry, requested) in parameters.iter().zip(&invocation) {
                let expected = candidate.fresh_canonical(&entry.ty, &mut fresh, &mut next_variable);
                matches &= unifier.unify(&expected, requested);
            }
            let return_ty = &contract
                .signature
                .entries
                .iter()
                .find(|entry| entry.role == TypeListRole::ReturnType)
                .expect("return type")
                .ty;
            let return_ty = candidate.fresh_canonical(return_ty, &mut fresh, &mut next_variable);
            matches &= unifier.unify(&return_ty, &result);
            if !matches {
                failures.push(reject(CandidateFailureKind::TraitMethodInvocationMismatch));
                continue;
            }
            let mut candidate_waiting = Vec::new();
            let mut proof_evidence = Vec::new();
            let mut rejected = false;
            for constraints in [
                &contract.impl_constraints,
                &contract.signature.where_constraints,
            ] {
                match candidate.prove_canonical_constraints(
                    constraints,
                    &mut fresh,
                    &unifier,
                    &mut HashSet::new(),
                    &mut next_variable,
                )? {
                    ApplicabilityProof::Satisfied(indices) => proof_evidence.extend(indices),
                    ApplicabilityProof::Deferred(vars) => candidate_waiting.extend(vars),
                    ApplicabilityProof::Unsatisfied => {
                        rejected = true;
                        break;
                    }
                }
            }
            if rejected {
                failures.push(reject(CandidateFailureKind::TraitImplWhereUnsatisfied));
                continue;
            }
            for obligation in &method.body_obligations {
                let subject = candidate.canonical_request(&obligation.receiver)?;
                let subject = candidate.fresh_canonical(&subject, &mut fresh, &mut next_variable);
                let subject = candidate.canonical_to_ty(&unifier.resolve(&subject))?;
                let arguments = obligation
                    .trait_args
                    .iter()
                    .map(|ty| {
                        let argument = candidate.canonical_request(ty)?;
                        let argument =
                            candidate.fresh_canonical(&argument, &mut fresh, &mut next_variable);
                        candidate.canonical_to_ty(&unifier.resolve(&argument))
                    })
                    .collect::<Result<Vec<_>, TypeError>>()?;
                let proof =
                    candidate.probe_trait_head(&obligation.trait_id, &arguments, &subject)?;
                match proof {
                    ApplicabilityProof::Satisfied(indices) => proof_evidence.extend(indices),
                    ApplicabilityProof::Deferred(vars) => candidate_waiting.extend(vars),
                    ApplicabilityProof::Unsatisfied => {
                        rejected = true;
                        break;
                    }
                }
            }
            if rejected {
                failures.push(reject(CandidateFailureKind::TraitImplWhereUnsatisfied));
                continue;
            }
            // Include every method-list role, including return-only inputs, in
            // the same instantiation namespace even when a role has no value slot.
            for entry in &contract.signature.entries {
                candidate.fresh_canonical(&entry.ty, &mut fresh, &mut next_variable);
            }
            let mut substitution = HashMap::new();
            for (original, instantiated) in &fresh {
                let resolved = unifier.resolve(&CanonicalTy::variable(*instantiated));
                candidate.canonical_waiting_variables(&resolved, &mut candidate_waiting);
                substitution.insert(*original, candidate.canonical_to_ty(&resolved)?);
            }
            if !candidate_waiting.is_empty() {
                if caller_waiting.is_empty() {
                    failures.push(reject(CandidateFailureKind::TraitMethodInvocationMismatch));
                }
                deferred.extend(caller_waiting.clone());
                continue;
            }
            let dispatch = candidate
                .impl_method_dispatch_target(method)
                .ok_or_else(|| {
                    TypeError::new(
                        "MissingTraitDispatchTarget: applicable impl has no concrete target",
                        method.span.clone(),
                    )
                })?;
            if selected.is_some() {
                if !caller_waiting.is_empty() {
                    return Ok(CandidateApplicability::Deferred(PendingTraitCandidate {
                        waiting_on: caller_waiting,
                        candidates: declarations,
                    }));
                }
                return Err(TypeError::new(
                    "Internal error: multiple applicable coherent trait implementations",
                    method.span.clone(),
                ));
            }
            let caller_substitution = caller_waiting
                .iter()
                .map(|var| {
                    self.canonical_to_ty(&unifier.resolve(&CanonicalTy::variable(*var)))
                        .map(|ty| (*var, ty))
                })
                .collect::<Result<HashMap<_, _>, _>>()?;
            selected = Some(MethodInstantiation {
                dispatch,
                substitution,
                caller_substitution,
                proof_evidence,
            });
        }
        if let Some(instantiation) = selected {
            return Ok(CandidateApplicability::Applicable(instantiation));
        }
        if !deferred.is_empty() {
            deferred.sort_unstable();
            deferred.dedup();
            return Ok(CandidateApplicability::Deferred(PendingTraitCandidate {
                waiting_on: deferred,
                candidates: declarations,
            }));
        }
        let receiver_ty = self.canonical_to_ty(&receiver)?;
        if let Some(dispatch) =
            self.compiler_trait_dispatch_target(trait_name, method_name, &receiver_ty)
        {
            return Ok(CandidateApplicability::Applicable(MethodInstantiation {
                dispatch,
                substitution: HashMap::new(),
                caller_substitution: HashMap::new(),
                proof_evidence: Vec::new(),
            }));
        }
        Ok(CandidateApplicability::Rejected(CandidateRejection {
            failures,
        }))
    }

    pub(super) fn candidate_rejection_note(
        &self,
        rejection: &CandidateRejection,
    ) -> Option<String> {
        let failure = rejection.failures.iter().min_by_key(|failure| {
            let priority = match failure.kind {
                CandidateFailureKind::TraitImplWhereUnsatisfied => 0,
                CandidateFailureKind::TraitMethodInvocationMismatch => 1,
                CandidateFailureKind::TraitImplHeadMismatch => 2,
            };
            (priority, failure.declaration.declaration_id)
        })?;
        let reason = match failure.kind {
            CandidateFailureKind::TraitImplHeadMismatch => {
                "the complete trait arguments and target do not match"
            }
            CandidateFailureKind::TraitImplWhereUnsatisfied => {
                "an implementation or method obligation is not satisfied"
            }
            CandidateFailureKind::TraitMethodInvocationMismatch => {
                "the complete method invocation does not match"
            }
        };
        Some(format!(
            "Impl declaration {} at {}..{} was rejected because {}.",
            failure.declaration.declaration_id, failure.span.start, failure.span.end, reason
        ))
    }

    pub(super) fn impl_method_dispatch_target(
        &self,
        method: &TraitImplMethodInfo,
    ) -> Option<TraitDispatchTarget> {
        if let Some(target) = &method.dispatch_override {
            return Some(target.clone());
        }
        let key = method
            .function_id
            .qualified_name
            .as_ref()
            .unwrap_or(&method.function_id.name);
        let fun_idx = match self.env.lookup_var(method.function_id.unique_id) {
            Some(Ty::UserFunc { fun_idx, .. }) => *fun_idx,
            _ => return None,
        };
        Some(TraitDispatchTarget::UserFunction {
            name: method.display_name_override.clone().unwrap_or_else(|| {
                super::expr::callable_definition_display_name(key, &method.function_id.name)
            }),
            fun_idx,
        })
    }
}

impl Checker {
    pub(super) fn probe_trait_head(
        &self,
        trait_name: &str,
        trait_args: &[Ty],
        ty: &Ty,
    ) -> Result<ApplicabilityProof, TypeError> {
        let requested_head = self.requested_head_type_list(trait_args, ty)?;
        let receiver = requested_head
            .entries
            .iter()
            .find(|entry| entry.role == TypeListRole::ImplTarget)
            .expect("requested target")
            .ty
            .clone();
        let requested = requested_head
            .entries
            .iter()
            .filter(|entry| entry.role == TypeListRole::TraitArgument)
            .map(|entry| entry.ty.clone())
            .collect::<Vec<_>>();
        if let Some(evidence) = self.exact_capability_evidence(&receiver, trait_name) {
            return Ok(ApplicabilityProof::Satisfied(evidence));
        }
        let mut request_waiting = Vec::new();
        for ty in requested.iter().chain(std::iter::once(&receiver)) {
            self.canonical_waiting_variables(ty, &mut request_waiting);
        }
        if let CanonicalTypeHead::Variable(var) = receiver.head {
            if self.rigid_tyvars.contains(&var) {
                return Ok(self
                    .rigid_capability_evidence(var, trait_name)
                    .map(ApplicabilityProof::Satisfied)
                    .or_else(|| {
                        self.tyvar_satisfies_compiler_trait(var, trait_name)
                            .then(|| ApplicabilityProof::Satisfied(Vec::new()))
                    })
                    .unwrap_or(ApplicabilityProof::Unsatisfied));
            }
        }
        let mut applicable = None;
        let mut waiting = Vec::new();
        for key in self.trait_impl_candidate_keys(trait_name) {
            let info = self.trait_impls[&key].clone();
            let candidate = &*self;
            let mut next_variable = self.env.next_tyvar;
            let mut fresh = HashMap::new();
            let mut unifier = CanonicalUnifier {
                rigid_variables: self.rigid_tyvars.clone(),
                allow_ignored_callable_inputs: true,
                ..Default::default()
            };
            let args = info
                .head_type_list
                .entries
                .iter()
                .filter(|entry| entry.role == TypeListRole::TraitArgument)
                .collect::<Vec<_>>();
            if args.len() != requested.len() {
                continue;
            }
            let mut matches = true;
            for (entry, request) in args.iter().zip(&requested) {
                let pattern = candidate.fresh_canonical(&entry.ty, &mut fresh, &mut next_variable);
                matches &= unifier.unify(&pattern, request);
            }
            let target = &info
                .head_type_list
                .entries
                .iter()
                .find(|entry| entry.role == TypeListRole::ImplTarget)
                .expect("target")
                .ty;
            let target = candidate.fresh_canonical(target, &mut fresh, &mut next_variable);
            matches &= unifier.unify(&target, &receiver);
            if !matches {
                continue;
            }
            match candidate.prove_canonical_constraints(
                &info.impl_constraints,
                &mut fresh,
                &unifier,
                &mut HashSet::new(),
                &mut next_variable,
            )? {
                ApplicabilityProof::Satisfied(evidence) if request_waiting.is_empty() => {
                    if applicable.is_some() {
                        return Err(TypeError::new(
                            "Internal error: multiple applicable coherent trait implementations",
                            Span { start: 0, end: 0 },
                        ));
                    }
                    applicable = Some(evidence);
                }
                ApplicabilityProof::Satisfied(_) => waiting.extend(request_waiting.clone()),
                ApplicabilityProof::Deferred(_) => {
                    waiting.extend(request_waiting.clone());
                }
                ApplicabilityProof::Unsatisfied => {}
            }
        }
        if let Some(evidence) = applicable {
            return Ok(ApplicabilityProof::Satisfied(evidence));
        }
        if !waiting.is_empty() {
            waiting.sort_unstable();
            waiting.dedup();
            return Ok(ApplicabilityProof::Deferred(waiting));
        }
        if !request_waiting.is_empty() && matches!(receiver.head, CanonicalTypeHead::Variable(_)) {
            return Ok(ApplicabilityProof::Deferred(request_waiting));
        }
        let receiver_ty = self.canonical_to_ty(&receiver)?;
        Ok(
            if self.compiler_trait_impl_exists(trait_name, &receiver_ty) {
                ApplicabilityProof::Satisfied(Vec::new())
            } else {
                ApplicabilityProof::Unsatisfied
            },
        )
    }

    pub(super) fn prove_trait_capability(
        &mut self,
        trait_name: &str,
        ty: &Ty,
    ) -> Result<ApplicabilityProof, TypeError> {
        let Some(trait_id) = self.traits.get(trait_name).map(|info| info.id.unique_id) else {
            return Ok(ApplicabilityProof::Unsatisfied);
        };
        let subject = self.canonical_request(ty)?;
        let mut next_variable = self.env.next_tyvar;
        let outcome = self.prove_canonical_capability(
            trait_id,
            &subject,
            &mut HashSet::new(),
            &mut next_variable,
        )?;
        if matches!(outcome, ApplicabilityProof::Deferred(_)) {
            let mut waiting = Vec::new();
            self.canonical_waiting_variables(&subject, &mut waiting);
            Ok(if waiting.is_empty() {
                ApplicabilityProof::Unsatisfied
            } else {
                ApplicabilityProof::Deferred(waiting)
            })
        } else {
            Ok(outcome)
        }
    }
}

impl Checker {
    /// Project an already known constructor shape without selecting a method.
    /// Candidate patterns cannot supply missing call-site type information.
    pub(super) fn constructor_projection(
        &self,
        trait_name: &str,
        container: &Ty,
    ) -> Option<(TraitImplInfo, HashMap<u32, Ty>)> {
        let requested = self.canonical_request(container).ok()?;
        if matches!(requested.head, CanonicalTypeHead::Variable(_)) {
            return None;
        }
        let mut request_variables = Vec::new();
        fn variables(ty: &CanonicalTy, out: &mut Vec<u32>) {
            if let CanonicalTypeHead::Variable(var) = ty.head {
                out.push(var);
            }
            for child in &ty.arguments {
                variables(child, out);
            }
        }
        variables(&requested, &mut request_variables);
        let mut projections = Vec::new();
        for key in self.trait_impl_candidate_keys(trait_name) {
            let info = &self.trait_impls[&key];
            if info.constructor_slot_vars.is_empty() {
                continue;
            }
            let target = &info
                .head_type_list
                .entries
                .iter()
                .find(|entry| entry.role == TypeListRole::ImplTarget)?
                .ty;
            let mut fresh = HashMap::new();
            let mut next_variable = self.env.next_tyvar;
            let target = self.fresh_canonical(target, &mut fresh, &mut next_variable);
            let mut unifier = CanonicalUnifier {
                rigid_variables: self.rigid_tyvars.clone(),
                ..Default::default()
            };
            if !unifier.unify(&target, &requested) {
                continue;
            }
            if request_variables.iter().any(|var| {
                unifier.resolve(&CanonicalTy::variable(*var)) != CanonicalTy::variable(*var)
            }) {
                continue;
            }
            let mapping = fresh
                .into_iter()
                .map(|(original, var)| {
                    self.canonical_to_ty(&unifier.resolve(&CanonicalTy::variable(var)))
                        .map(|ty| (original, ty))
                })
                .collect::<Result<HashMap<_, _>, _>>()
                .ok()?;
            projections.push((info.clone(), mapping));
        }
        if projections.len() == 1 {
            projections.pop()
        } else {
            None
        }
    }

    pub(super) fn select_method_dispatch(
        &mut self,
        trait_name: &str,
        method_name: &str,
        receiver: &Ty,
        trait_args: &[Ty],
        argument_tys: &[Ty],
        result: &Ty,
    ) -> Result<Option<TraitDispatch>, TypeError> {
        let profile = self.profiler.start();
        let selected = self.select_trait_method_instantiation(
            trait_name,
            method_name,
            receiver,
            trait_args,
            argument_tys,
            result,
        );
        self.profiler
            .finish(ProfileEvent::TraitDispatchLookup, profile);
        match selected? {
            CandidateApplicability::Applicable(instantiation) => {
                Ok(Some(TraitDispatch::Static(instantiation.dispatch)))
            }
            CandidateApplicability::Deferred(_) => {
                self.trait_dispatch_target_for_args(trait_name, method_name, receiver, trait_args)
            }
            CandidateApplicability::Rejected(_) => Ok(None),
        }
    }
}
