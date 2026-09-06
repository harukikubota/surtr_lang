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

    fn canonical_nominal_head(&self, name: &str) -> Result<CanonicalTypeHead, TypeError> {
        if let Some(name) = sindr::names::builtin_type_name(name) {
            return Ok(CanonicalTypeHead::Builtin(name));
        }
        let def = self.env.lookup_type_def(name).ok_or_else(|| {
            TypeError::new(
                "Internal error: resolved nominal declaration is missing",
                Span { start: 0, end: 0 },
            )
        })?;
        Ok(CanonicalTypeHead::Nominal(def.tag))
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
