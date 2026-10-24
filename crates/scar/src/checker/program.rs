use super::*;
use std::time::{Duration, Instant};

impl Checker {
    pub(super) fn contextual_constructor_trait_names(&self, stmts: &[Resolved]) -> HashSet<String> {
        let mut names = self
            .traits
            .iter()
            .filter(|(_, info)| !info.constructor_slots.is_empty())
            .flat_map(|(key, info)| {
                [
                    key.clone(),
                    Self::surface_name(key).to_string(),
                    info.id.name.clone(),
                ]
            })
            .collect::<HashSet<_>>();
        for stmt in stmts {
            let Resolved::TraitDef(_, id, _, where_clause, _, _) = stmt else {
                continue;
            };
            let is_direct = where_clause.as_ref().is_some_and(|clause| {
                clause.constraints.iter().any(|constraint| {
                    matches!(constraint.subject, AstTy::Named(_, ref name) if name == "Self")
                        && constraint.bounds.iter().any(|bound| {
                            matches!(bound, ResolvedWhereConstraintRhs::TypeConstructor { .. })
                        })
                })
            });
            if is_direct {
                names.insert(id.name.clone());
                if let Some(qualified) = &id.qualified_name {
                    names.insert(qualified.clone());
                }
            }
        }
        loop {
            let before = names.len();
            for stmt in stmts {
                let Resolved::TraitDef(_, id, _, where_clause, _, _) = stmt else {
                    continue;
                };
                let inherits =
                    where_clause.as_ref().is_some_and(|clause| {
                        clause.constraints.iter().any(|constraint| {
                        matches!(constraint.subject, AstTy::Named(_, ref name) if name == "Self")
                            && constraint.bounds.iter().any(|bound| match bound {
                                ResolvedWhereConstraintRhs::Trait { trait_id } => names.contains(
                                    trait_id.qualified_name.as_deref().unwrap_or(&trait_id.name),
                                ) || names.contains(Self::surface_name(&trait_id.name)),
                                _ => false,
                            })
                    })
                    });
                if inherits {
                    names.insert(id.name.clone());
                    if let Some(qualified) = &id.qualified_name {
                        names.insert(qualified.clone());
                    }
                }
            }
            if names.len() == before {
                break;
            }
        }
        names
    }

    fn constructor_position_error(ast_ty: &AstTy) -> TypeError {
        TypeError {
            structured: None,
            message: format!(
                "ConstructorTraitApplicationPosition: {} is only allowed as a direct ordinary-function or trait-method parameter or return type",
                Self::surface_ast_ty(ast_ty)
            ),
            span: Self::ast_ty_span(ast_ty).clone(),
            hint: Some(
                "Move this constructor-trait application to a direct signature position, or use a concrete type here."
                    .into(),
            ),
        }
    }

    fn substitute_constructor_validation_alias_ty(
        ast_ty: &AstTy,
        bindings: &HashMap<String, AstTy>,
    ) -> AstTy {
        match ast_ty {
            AstTy::Named(_, name) if bindings.contains_key(name) => {
                bindings.get(name).cloned().expect("checked above")
            }
            AstTy::Generic(span, name, args) => AstTy::Generic(
                span.clone(),
                name.clone(),
                args.iter()
                    .map(|arg| Self::substitute_constructor_validation_alias_ty(arg, bindings))
                    .collect(),
            ),
            AstTy::Tuple(span, items) => AstTy::Tuple(
                span.clone(),
                items
                    .iter()
                    .map(|item| Self::substitute_constructor_validation_alias_ty(item, bindings))
                    .collect(),
            ),
            AstTy::Func(span, params, ret) => AstTy::Func(
                span.clone(),
                params
                    .iter()
                    .map(|param| Self::substitute_constructor_validation_alias_ty(param, bindings))
                    .collect(),
                Box::new(Self::substitute_constructor_validation_alias_ty(
                    ret, bindings,
                )),
            ),
            other => other.clone(),
        }
    }

    fn validate_constructor_ast_ty(
        &self,
        ast_ty: &AstTy,
        direct: bool,
        constructor_traits: &HashSet<String>,
    ) -> Result<(), TypeError> {
        self.validate_constructor_ast_ty_inner(
            ast_ty,
            direct,
            constructor_traits,
            &mut HashSet::new(),
        )
    }

    fn validate_constructor_ast_ty_inner(
        &self,
        ast_ty: &AstTy,
        direct: bool,
        constructor_traits: &HashSet<String>,
        expanding_aliases: &mut HashSet<String>,
    ) -> Result<(), TypeError> {
        let alias_head = match ast_ty {
            AstTy::Named(_, name) => Some((name, Vec::new())),
            AstTy::Generic(_, name, args) => Some((name, args.clone())),
            _ => None,
        };
        if let Some((name, args)) = alias_head {
            if let Some(alias) = self.signature_aliases.get(name) {
                if expanding_aliases.insert(name.clone()) {
                    let bindings = alias
                        .params
                        .iter()
                        .zip(args)
                        .map(|(param, arg)| (param.name.clone(), arg))
                        .collect::<HashMap<_, _>>();
                    let expanded =
                        Self::substitute_constructor_validation_alias_ty(&alias.rhs, &bindings);
                    let result = self.validate_constructor_ast_ty_inner(
                        &expanded,
                        direct,
                        constructor_traits,
                        expanding_aliases,
                    );
                    expanding_aliases.remove(name);
                    return result;
                }
            }
        }
        let root = match ast_ty {
            AstTy::Named(_, name) | AstTy::Generic(_, name, _) => Some(name.as_str()),
            _ => None,
        };
        let is_constructor = root.is_some_and(|name| {
            constructor_traits.contains(name)
                || constructor_traits.contains(Self::surface_name(name))
        });
        if is_constructor && !direct {
            return Err(Self::constructor_position_error(ast_ty));
        }
        match ast_ty {
            AstTy::Generic(_, _, args) => {
                for arg in args {
                    self.validate_constructor_ast_ty_inner(
                        arg,
                        false,
                        constructor_traits,
                        expanding_aliases,
                    )?;
                }
            }
            AstTy::Tuple(_, items) => {
                for item in items {
                    self.validate_constructor_ast_ty_inner(
                        item,
                        false,
                        constructor_traits,
                        expanding_aliases,
                    )?;
                }
            }
            AstTy::Func(_, params, ret) => {
                for param in params {
                    self.validate_constructor_ast_ty_inner(
                        param,
                        false,
                        constructor_traits,
                        expanding_aliases,
                    )?;
                }
                self.validate_constructor_ast_ty_inner(
                    ret,
                    false,
                    constructor_traits,
                    expanding_aliases,
                )?;
            }
            AstTy::Named(..) | AstTy::ImplTrait(..) => {}
        }
        Ok(())
    }

    pub(super) fn validate_constructor_pattern(
        &self,
        pattern: &ResolvedPattern,
        constructor_traits: &HashSet<String>,
    ) -> Result<(), TypeError> {
        match pattern {
            ResolvedPattern::Located(_, inner) => {
                self.validate_constructor_pattern(inner, constructor_traits)?
            }
            ResolvedPattern::Projection {
                inner, annotation, ..
            } => {
                if let Some(ty) = annotation {
                    self.validate_constructor_ast_ty(ty, false, constructor_traits)?;
                }
                self.validate_constructor_pattern(inner, constructor_traits)?;
            }
            ResolvedPattern::Deferred { pattern, .. } => {
                self.validate_constructor_pattern(pattern, constructor_traits)?
            }
            ResolvedPattern::ExtractorApplication { .. } => {}
            ResolvedPattern::Annotated(_, ty)
            | ResolvedPattern::AnnotatedWildcard(_, ty)
            | ResolvedPattern::As(_, _, Some(ty)) => {
                self.validate_constructor_ast_ty(ty, false, constructor_traits)?;
            }
            ResolvedPattern::ListCons(head, tail) => {
                self.validate_constructor_pattern(head, constructor_traits)?;
                self.validate_constructor_pattern(tail, constructor_traits)?;
            }
            ResolvedPattern::HashMap(_, entries) => {
                for (key, child) in entries {
                    self.validate_constructor_body_positions(key, constructor_traits)?;
                    self.validate_constructor_pattern(child, constructor_traits)?;
                }
            }
            ResolvedPattern::Extractor(_, pre_args, items) => {
                for arg in pre_args {
                    self.validate_constructor_body_positions(arg, constructor_traits)?;
                }
                for item in items {
                    self.validate_constructor_pattern(item, constructor_traits)?;
                }
            }
            ResolvedPattern::Record(_, fields) => {
                for (_, item) in fields {
                    self.validate_constructor_pattern(item, constructor_traits)?;
                }
            }
            ResolvedPattern::Constructor(_, items)
            | ResolvedPattern::Tuple(items)
            | ResolvedPattern::Or(items) => {
                for item in items {
                    self.validate_constructor_pattern(item, constructor_traits)?;
                }
            }
            ResolvedPattern::As(inner, _, None) => {
                self.validate_constructor_pattern(inner, constructor_traits)?;
            }
            ResolvedPattern::Var(_)
            | ResolvedPattern::Pin(_)
            | ResolvedPattern::Wildcard(_)
            | ResolvedPattern::ListNil(_)
            | ResolvedPattern::IntLit(..)
            | ResolvedPattern::StrLit(..)
            | ResolvedPattern::BoolLit(..)
            | ResolvedPattern::DurationLit(..) => {}
        }
        Ok(())
    }

    fn validate_constructor_body_positions(
        &self,
        node: &Resolved,
        constructor_traits: &HashSet<String>,
    ) -> Result<(), TypeError> {
        match node {
            Resolved::Bind(_, pattern, rhs)
            | Resolved::SafeBind(_, pattern, rhs, _)
            | Resolved::ApplyPattern(_, rhs, pattern) => {
                self.validate_constructor_pattern(pattern, constructor_traits)?;
                self.validate_constructor_body_positions(rhs, constructor_traits)?;
            }
            Resolved::Do(_, _, _, _, statements) => {
                for statement in statements {
                    match statement {
                        sigil::resolved::ResolvedDoStatement::Extract { pattern, rhs, .. }
                        | sigil::resolved::ResolvedDoStatement::SafeBind { pattern, rhs, .. } => {
                            self.validate_constructor_pattern(pattern, constructor_traits)?;
                            self.validate_constructor_body_positions(rhs, constructor_traits)?;
                        }
                        sigil::resolved::ResolvedDoStatement::Statement(statement) => {
                            self.validate_constructor_body_positions(
                                statement,
                                constructor_traits,
                            )?;
                        }
                    }
                }
            }
            Resolved::Closure(_, params, _, body)
            | Resolved::ExtractorClosure(_, params, _, body)
            | Resolved::CaptureClosure(_, params, _, body) => {
                for param in params {
                    if let Some(ty) = &param.ty {
                        self.validate_constructor_ast_ty(ty, false, constructor_traits)?;
                    }
                }
                self.validate_constructor_body_positions(body, constructor_traits)?;
            }
            Resolved::LazyApp(_, _, function, args) => {
                self.validate_constructor_body_positions(function, constructor_traits)?;
                for argument in args {
                    self.validate_constructor_body_positions(argument, constructor_traits)?;
                }
            }
            Resolved::App(_, function, args) => {
                self.validate_constructor_body_positions(function, constructor_traits)?;
                for arg in args {
                    match arg {
                        ResolvedRecordLitArg::Positional(expr)
                        | ResolvedRecordLitArg::Named(_, expr) => {
                            self.validate_constructor_body_positions(expr, constructor_traits)?;
                        }
                    }
                }
            }
            Resolved::Block(_, nodes)
            | Resolved::ListLiteral(_, nodes)
            | Resolved::TupleLiteral(_, nodes)
            | Resolved::Dbg(_, nodes) => {
                for node in nodes {
                    self.validate_constructor_body_positions(node, constructor_traits)?;
                }
            }
            Resolved::BinOp(_, _, left, right)
            | Resolved::Pipe(_, left, right)
            | Resolved::ContextMap(_, left, right)
            | Resolved::ContextApply(_, left, right)
            | Resolved::ContextBind(_, left, right)
            | Resolved::Compose(_, left, right)
            | Resolved::LiftedCompose(_, left, right)
            | Resolved::KleisliCompose(_, left, right)
            | Resolved::ListCons(_, left, right)
            | Resolved::RangeLiteral(_, left, right)
            | Resolved::Cause(_, left, right) => {
                self.validate_constructor_body_positions(left, constructor_traits)?;
                self.validate_constructor_body_positions(right, constructor_traits)?;
            }
            Resolved::Cond(_, clauses) => {
                for (condition, body) in clauses {
                    self.validate_constructor_body_positions(condition, constructor_traits)?;
                    self.validate_constructor_body_positions(body, constructor_traits)?;
                }
            }
            Resolved::If(_, cond, then_branch, else_branch) => {
                self.validate_constructor_body_positions(cond, constructor_traits)?;
                self.validate_constructor_body_positions(then_branch, constructor_traits)?;
                if let Some(branch) = else_branch {
                    self.validate_constructor_body_positions(branch, constructor_traits)?;
                }
            }
            Resolved::Match(_, scrutinee, arms)
            | Resolved::IsMatch(_, scrutinee, arms)
            | Resolved::IfLet(_, scrutinee, arms, _) => {
                self.validate_constructor_body_positions(scrutinee, constructor_traits)?;
                for arm in arms {
                    self.validate_constructor_pattern(&arm.pattern, constructor_traits)?;
                    if let Some(guard) = &arm.guard {
                        self.validate_constructor_body_positions(guard, constructor_traits)?;
                    }
                    self.validate_constructor_body_positions(&arm.body, constructor_traits)?;
                }
            }
            Resolved::Grouped(_, inner)
            | Resolved::FieldAccess(_, inner, _)
            | Resolved::FacetCapture(_, inner)
            | Resolved::Semi(_, inner)
            | Resolved::StatementQuestion(_, inner) => {
                self.validate_constructor_body_positions(inner, constructor_traits)?;
            }
            Resolved::Capture(_, function, args) => {
                self.validate_constructor_body_positions(function, constructor_traits)?;
                for arg in args {
                    self.validate_constructor_body_positions(arg, constructor_traits)?;
                }
            }
            Resolved::InterpolatedStr(_, parts) => {
                for part in parts {
                    if let ResolvedInterpolatedPart::Expr(expr) = part {
                        self.validate_constructor_body_positions(expr, constructor_traits)?;
                    }
                }
            }
            Resolved::HashMapLiteral(_, entries) => {
                for entry in entries {
                    self.validate_constructor_body_positions(&entry.key, constructor_traits)?;
                    self.validate_constructor_body_positions(&entry.value, constructor_traits)?;
                }
            }
            Resolved::StructLit(_, _, fields) => {
                for field in fields {
                    match field {
                        ResolvedStructLitField::Explicit(_, expr)
                        | ResolvedStructLitField::Shorthand(_, expr) => {
                            self.validate_constructor_body_positions(expr, constructor_traits)?;
                        }
                    }
                }
            }
            Resolved::ConstructorCall(_, _, args)
            | Resolved::EnumConstructorCall(_, _, _, args) => {
                for arg in args {
                    match arg {
                        ResolvedRecordLitArg::Positional(expr)
                        | ResolvedRecordLitArg::Named(_, expr) => {
                            self.validate_constructor_body_positions(expr, constructor_traits)?;
                        }
                    }
                }
            }
            Resolved::FacetSegmentAccess(..)
            | Resolved::InferredFacetCapture(..)
            | Resolved::ReturnTypeArgumentApply(..)
            | Resolved::Lit(..)
            | Resolved::Var(..)
            | Resolved::ErrorKind(..)
            | Resolved::ListNil(..)
            | Resolved::ProcessContextHandler(..)
            | Resolved::StructDef(..)
            | Resolved::RecordDef(..)
            | Resolved::DeferrorDef(..)
            | Resolved::EnumDef(..)
            | Resolved::Def(..)
            | Resolved::ConstDef(..)
            | Resolved::ExtractorDef(..)
            | Resolved::TraitDef(..)
            | Resolved::TraitImplDef(..)
            | Resolved::BuiltinDecl(..)
            | Resolved::BuiltinExtractorDecl(..)
            | Resolved::BuiltinTypeDecl(..)
            | Resolved::TypeAlias(..) => {}
        }
        Ok(())
    }

    fn validate_constructor_application_positions(
        &self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        let constructor_traits = self.contextual_constructor_trait_names(stmts);
        let type_owners = stmts
            .iter()
            .filter_map(|stmt| match stmt {
                Resolved::StructDef(_, id, ..)
                | Resolved::RecordDef(_, id, ..)
                | Resolved::DeferrorDef(_, id, ..)
                | Resolved::EnumDef(_, id, ..)
                | Resolved::BuiltinTypeDecl(_, id, ..) => {
                    Some(id.qualified_name.as_ref().unwrap_or(&id.name).clone())
                }
                _ => None,
            })
            .collect::<HashSet<_>>();
        for stmt in stmts {
            match stmt {
                Resolved::Def(_, id, _, params, ret, _, body, _) => {
                    let owner = id
                        .qualified_name
                        .as_deref()
                        .unwrap_or(&id.name)
                        .rsplit_once("::")
                        .map(|(owner, _)| owner);
                    let direct = owner.is_none_or(|owner| {
                        !type_owners.contains(owner) && self.env.lookup_type_def(owner).is_none()
                    });
                    for param in params {
                        self.validate_constructor_ast_ty(&param.ty, direct, &constructor_traits)?;
                    }
                    if let Some(ret) = ret {
                        self.validate_constructor_ast_ty(ret, direct, &constructor_traits)?;
                    }
                    self.validate_constructor_body_positions(body, &constructor_traits)?;
                }
                Resolved::BuiltinDecl(_, id, _, _, _, _, _)
                    if Self::is_compiler_flow_operator_decl(id) => {}
                Resolved::BuiltinDecl(_, _, _, params, ret, _, _) => {
                    for param in params {
                        self.validate_constructor_ast_ty(&param.ty, false, &constructor_traits)?;
                    }
                    if let Some(ret) = ret {
                        self.validate_constructor_ast_ty(ret, false, &constructor_traits)?;
                    }
                }
                Resolved::ExtractorDef(_, _, _, param, ret, body, _) => {
                    for param in param {
                        if let Some(param) = &param.ty {
                            self.validate_constructor_ast_ty(param, false, &constructor_traits)?;
                        }
                    }
                    self.validate_constructor_ast_ty(ret, false, &constructor_traits)?;
                    self.validate_constructor_body_positions(body, &constructor_traits)?;
                }
                Resolved::BuiltinExtractorDecl(_, _, param, ret, _) => {
                    for param in param {
                        if let Some(param) = &param.ty {
                            self.validate_constructor_ast_ty(param, false, &constructor_traits)?;
                        }
                    }
                    self.validate_constructor_ast_ty(ret, false, &constructor_traits)?;
                }
                Resolved::TraitDef(_, _, _, _, methods, _) => {
                    for method in methods {
                        for param in &method.value_parameters {
                            self.validate_constructor_ast_ty(&param.ty, true, &constructor_traits)?;
                        }
                        self.validate_constructor_ast_ty(
                            &method.ret_ty,
                            true,
                            &constructor_traits,
                        )?;
                        if let Some(body) = &method.body {
                            self.validate_constructor_body_positions(body, &constructor_traits)?;
                        }
                    }
                }
                Resolved::TraitImplDef(_, _, _, _, _, _, methods) => {
                    for method in methods {
                        for param in &method.value_parameters {
                            self.validate_constructor_ast_ty(&param.ty, true, &constructor_traits)?;
                        }
                        if let Some(ret) = &method.ret_ty {
                            self.validate_constructor_ast_ty(ret, true, &constructor_traits)?;
                        }
                        self.validate_constructor_body_positions(
                            &method.body,
                            &constructor_traits,
                        )?;
                    }
                }
                Resolved::StructDef(_, _, _, fields, _)
                | Resolved::RecordDef(_, _, fields, _)
                | Resolved::DeferrorDef(_, _, fields, _) => {
                    for field in fields {
                        self.validate_constructor_ast_ty(&field.ty, false, &constructor_traits)?;
                    }
                }
                Resolved::EnumDef(_, _, _, variants, _) => {
                    for variant in variants {
                        for payload in &variant.payload {
                            self.validate_constructor_ast_ty(payload, false, &constructor_traits)?;
                        }
                    }
                }
                Resolved::TypeAlias(_, _, _, rhs, _) => {
                    self.validate_constructor_ast_ty(rhs, false, &constructor_traits)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) fn check_program(
        &mut self,
        stmts: Vec<Resolved>,
    ) -> Result<Vec<TypedNode>, TypeError> {
        let profile_enabled = self.profiler.enabled();
        if profile_enabled {
            self.profiler.reset();
        }
        let profile_start = profile_enabled.then(Instant::now);
        let mut predeclare_error_types_dur = Duration::ZERO;
        let mut predeclare_type_signatures_dur = Duration::ZERO;
        let mut predeclare_traits_dur = Duration::ZERO;
        let mut predeclare_functions_dur = Duration::ZERO;
        let mut ensure_struct_impl_new_contract_dur = Duration::ZERO;
        let mut check_stmt_loop_dur = Duration::ZERO;
        let mut ensure_builtin_type_contracts_dur = Duration::ZERO;
        let mut specialize_program_dur = Duration::ZERO;
        let mut stmt_count = 0usize;
        let mut slow_stmts = Vec::<(Duration, String)>::new();
        let mut stmt_kind_totals = HashMap::<String, (u64, Duration)>::new();

        let result = (|| -> Result<Vec<TypedNode>, TypeError> {
            self.validate_declaration_where_well_formedness(&stmts)?;
            self.predeclare_signature_aliases(&stmts)?;
            self.validate_constructor_application_positions(&stmts)?;
            self.collect_unused_type_parameter_warnings(&stmts)?;

            let t = profile_enabled.then(Instant::now);
            self.predeclare_error_types(&stmts);
            if let Some(start) = t {
                predeclare_error_types_dur = start.elapsed();
            }

            let t = profile_enabled.then(Instant::now);
            self.predeclare_type_signatures(&stmts)?;
            if let Some(start) = t {
                predeclare_type_signatures_dur = start.elapsed();
            }

            self.predeclare_consts(&stmts)?;

            let t = profile_enabled.then(Instant::now);
            self.predeclare_traits(&stmts)?;
            self.activate_nominal_declaration_bounds(&stmts)?;
            if let Some(start) = t {
                predeclare_traits_dur = start.elapsed();
            }

            let t = profile_enabled.then(Instant::now);
            self.predeclare_functions(&stmts)?;
            if let Some(start) = t {
                predeclare_functions_dur = start.elapsed();
            }

            self.validate_process_state_contracts()?;

            let t = profile_enabled.then(Instant::now);
            self.ensure_struct_impl_new_contract(&stmts)?;
            if let Some(start) = t {
                ensure_struct_impl_new_contract_dur = start.elapsed();
            }

            let mut typed = Vec::new();
            let t = profile_enabled.then(Instant::now);
            for stmt in stmts {
                // Source spans can repeat across definition sources and REPL inputs.
                // Provenance belongs only to this expression checking tree.
                self.safe_operator_results.clear();
                stmt_count += 1;
                // Inference substitutions are expression-local. Letting them leak
                // across sibling top-level statements can accidentally monomorphize
                // later generic calls based on earlier ones.
                self.substitutions.clear();
                let stmt_label = profile_enabled.then(|| Self::profile_stmt_label(&stmt));
                let stmt_start = profile_enabled.then(Instant::now);
                if let Resolved::ConstDef(..) = &stmt {
                    continue;
                }
                if let Resolved::TraitImplDef(
                    span,
                    declaration_id,
                    trait_id,
                    trait_args,
                    target_ty,
                    where_clause,
                    methods,
                ) = &stmt
                {
                    let nodes = self.check_trait_impl_items(
                        span,
                        *declaration_id,
                        trait_id,
                        trait_args,
                        target_ty,
                        where_clause.as_ref(),
                        methods,
                    )?;
                    typed.extend(nodes.into_iter().map(|node| {
                        let profile = self.profiler.start();
                        let node = *self.resolve_typed_node(node);
                        self.profiler
                            .finish(ProfileEvent::ResolveTypedNode, profile);
                        node
                    }));
                    if let (Some(start), Some(label)) = (stmt_start, stmt_label.as_ref()) {
                        let elapsed = start.elapsed();
                        slow_stmts.push((elapsed, label.clone()));
                        let kind = Self::profile_stmt_kind(&stmt).to_string();
                        let entry = stmt_kind_totals.entry(kind).or_insert((0, Duration::ZERO));
                        entry.0 += 1;
                        entry.1 += elapsed;
                    }
                    continue;
                }
                let node = self.check_node(&stmt)?;
                let profile = self.profiler.start();
                let node = *self.resolve_typed_node(node);
                self.profiler
                    .finish(ProfileEvent::ResolveTypedNode, profile);
                typed.push(node);
                if let (Some(start), Some(label)) = (stmt_start, stmt_label.as_ref()) {
                    let elapsed = start.elapsed();
                    slow_stmts.push((elapsed, label.clone()));
                    let kind = Self::profile_stmt_kind(&stmt).to_string();
                    let entry = stmt_kind_totals.entry(kind).or_insert((0, Duration::ZERO));
                    entry.0 += 1;
                    entry.1 += elapsed;
                }
            }
            if let Some(start) = t {
                check_stmt_loop_dur = start.elapsed();
            }

            let t = profile_enabled.then(Instant::now);
            self.ensure_builtin_type_contracts()?;
            if let Some(start) = t {
                ensure_builtin_type_contracts_dur = start.elapsed();
            }

            let t = profile_enabled.then(Instant::now);
            if self.test_script {
                if let Some(result) = typed.last() {
                    if self.resolve_ty(&result.ty) != Ty::Unit {
                        return Err(self.policy_error(
                            diagnostics::TypeDiagnosticReason::TestScriptReturnType,
                            diagnostics::TypePolicy::TestScriptReturn,
                            None,
                            Some(&Ty::Unit),
                            Some(&result.ty),
                            None,
                            None,
                            &result.span,
                            None,
                        ));
                    }
                }
            }
            let specialized = self.specialize_program(typed)?;
            if let Some(start) = t {
                specialize_program_dur = start.elapsed();
            }
            for node in &specialized {
                self.validate_direct_pattern_branches(node)?;
            }
            if let Some((trait_name, method_name, subject, span)) = specialized
                .iter()
                .find_map(|node| self.first_pending_trait_helper(node))
            {
                return Err(self.pending_trait_helper_error(
                    trait_name,
                    method_name,
                    subject,
                    span,
                ));
            }
            if let Some((outcome, span)) = specialized
                .iter()
                .find_map(|node| self.unresolved_executable_constructor_application(node))
            {
                return match outcome
                    .into_checked()
                    .map_err(|error| error.at_span(&span))?
                {
                    ConstructorApplicationOutcome::Deferred { waiting_on } => {
                        let mut error =
                            self.ambiguous_constructor_result("constructor", "application", &span);
                        error.hint = Some(format!(
                            "Resolve all {} constructor input(s) before this value is executed.",
                            waiting_on.len()
                        ));
                        Err(error)
                    }
                    ConstructorApplicationOutcome::Rejected { failures } => {
                        Err(signatures::constructor_signature_metadata_error(
                            "constructor application",
                            &span,
                            Self::constructor_projection_failure_detail(&failures),
                        ))
                    }
                    ConstructorApplicationOutcome::Applied(_) => unreachable!(
                        "an applied constructor cannot remain in executable typed output"
                    ),
                };
            }
            self.collect_unused_value_warnings_in_sequence(&specialized);
            Ok(specialized)
        })();

        if let Some(start) = profile_start {
            let total = start.elapsed();
            self.profiler.print_summary(total);
            if total >= Duration::from_millis(5) {
                eprintln!(
                    "scar-phase predeclare_error_types={:.3}ms predeclare_type_signatures={:.3}ms predeclare_traits={:.3}ms predeclare_functions={:.3}ms ensure_struct_impl_new_contract={:.3}ms check_stmt_loop={:.3}ms ensure_builtin_type_contracts={:.3}ms specialize_program={:.3}ms",
                    predeclare_error_types_dur.as_secs_f64() * 1000.0,
                    predeclare_type_signatures_dur.as_secs_f64() * 1000.0,
                    predeclare_traits_dur.as_secs_f64() * 1000.0,
                    predeclare_functions_dur.as_secs_f64() * 1000.0,
                    ensure_struct_impl_new_contract_dur.as_secs_f64() * 1000.0,
                    check_stmt_loop_dur.as_secs_f64() * 1000.0,
                    ensure_builtin_type_contracts_dur.as_secs_f64() * 1000.0,
                    specialize_program_dur.as_secs_f64() * 1000.0,
                );
                if !slow_stmts.is_empty() {
                    slow_stmts.sort_by(|a, b| b.0.cmp(&a.0));
                    let top = slow_stmts
                        .iter()
                        .take(8)
                        .map(|(dur, label)| {
                            format!("{}:{:.3}ms", label, dur.as_secs_f64() * 1000.0)
                        })
                        .collect::<Vec<_>>()
                        .join(" | ");
                    eprintln!("scar-phase stmt_count={} slow_top {}", stmt_count, top);
                }
                if !stmt_kind_totals.is_empty() {
                    let mut kinds = stmt_kind_totals
                        .iter()
                        .map(|(kind, (count, dur))| (kind.clone(), *count, *dur))
                        .collect::<Vec<_>>();
                    kinds.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
                    let summary = kinds
                        .iter()
                        .take(8)
                        .map(|(kind, count, dur)| {
                            format!("{}:{} ({:.3}ms)", kind, count, dur.as_secs_f64() * 1000.0)
                        })
                        .collect::<Vec<_>>()
                        .join(" | ");
                    eprintln!("scar-phase kind_top {}", summary);
                }
            }
        }

        result
    }
}
