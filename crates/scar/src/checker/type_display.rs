use super::*;

impl Checker {
    pub(super) fn diagnostic_tyvar_name(index: usize) -> String {
        let mut value = index;
        let mut name = String::new();
        loop {
            let rem = value % 26;
            name.push((b'A' + rem as u8) as char);
            if value < 26 {
                break;
            }
            value = (value / 26) - 1;
        }
        format!("${}", name.chars().rev().collect::<String>())
    }

    fn diagnostic_ty_name_with_state(
        &self,
        ty: &Ty,
        tyvars: &mut HashMap<u32, String>,
        next_tyvar_index: &mut usize,
    ) -> String {
        match ty {
            Ty::Int => "Int".into(),
            Ty::Float => "Float".into(),
            Ty::Str => "String".into(),
            Ty::Bool => "Boolean".into(),
            Ty::Unit => "Unit".into(),
            Ty::Error => "Error".into(),
            Ty::Hole => "_".into(),
            Ty::SelfApp(args) => {
                let (head, slots) = match Self::constructor_application_parts(args) {
                    Some((Ty::Var(witness), slots)) => {
                        let head = self
                            .constructor_witness_traits
                            .get(witness)
                            .map(|name| self.trait_display_name(name))
                            .unwrap_or_else(|| {
                                self.diagnostic_ty_name_with_state(
                                    &Ty::Var(*witness),
                                    tyvars,
                                    next_tyvar_index,
                                )
                            });
                        (head, slots)
                    }
                    Some((witness, slots)) => (
                        self.diagnostic_ty_name_with_state(witness, tyvars, next_tyvar_index),
                        slots,
                    ),
                    None => ("Self".into(), args.as_slice()),
                };
                if slots.is_empty() {
                    return head;
                }
                format!(
                    "{}<{}>",
                    head,
                    slots
                        .iter()
                        .map(|arg| self.diagnostic_ty_name_with_state(
                            arg,
                            tyvars,
                            next_tyvar_index
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Ty::ExtractorClosure(inner) => format!(
                "ExtractorClosure<{}>",
                self.diagnostic_ty_name_with_state(inner, tyvars, next_tyvar_index)
            ),
            Ty::MatchResult(inner) => format!(
                "MatchResult<{}>",
                self.diagnostic_ty_name_with_state(inner, tyvars, next_tyvar_index)
            ),
            Ty::List(inner) => format!(
                "List<{}>",
                self.diagnostic_ty_name_with_state(inner, tyvars, next_tyvar_index)
            ),
            Ty::Lazy(inner) => format!(
                "Lazy<{}>",
                self.diagnostic_ty_name_with_state(inner, tyvars, next_tyvar_index)
            ),
            Ty::Pid(marker) => format!(
                "PID<{}>",
                self.diagnostic_ty_name_with_state(marker, tyvars, next_tyvar_index)
            ),
            Ty::ProcessMarker(name) => Self::surface_name(name).to_string(),
            Ty::Facet(kind, source, focus, update_source, update_focus) => format!(
                "Facet<{}, {}, {}, {}, {}>",
                kind.as_str(),
                self.diagnostic_ty_name_with_state(source, tyvars, next_tyvar_index),
                self.diagnostic_ty_name_with_state(focus, tyvars, next_tyvar_index),
                self.diagnostic_ty_name_with_state(update_source, tyvars, next_tyvar_index),
                self.diagnostic_ty_name_with_state(update_focus, tyvars, next_tyvar_index)
            ),
            Ty::Tuple(items) => format!(
                "({})",
                items
                    .iter()
                    .map(|item| {
                        self.diagnostic_ty_name_with_state(item, tyvars, next_tyvar_index)
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Result(ok, _) => format!(
                "Result<{}>",
                self.diagnostic_ty_name_with_state(ok, tyvars, next_tyvar_index)
            ),
            Ty::Var(var) => tyvars
                .entry(*var)
                .or_insert_with(|| {
                    let name = Self::diagnostic_tyvar_name(*next_tyvar_index);
                    *next_tyvar_index += 1;
                    name
                })
                .clone(),
            Ty::Struct(name, _) | Ty::Record(name, _) => {
                let name_text = Self::surface_name(name);
                match self.resolved_named_type_args(name, ty) {
                    Some(args) if !args.is_empty() => format!(
                        "{}<{}>",
                        name_text,
                        args.iter()
                            .map(|arg| self.diagnostic_ty_name_with_state(
                                &self.resolve_ty(arg),
                                tyvars,
                                next_tyvar_index
                            ))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    _ => name_text.to_string(),
                }
            }
            Ty::Enum(name, args) => {
                if args.is_empty() {
                    Self::surface_name(name).to_string()
                } else {
                    format!(
                        "{}<{}>",
                        Self::surface_name(name),
                        args.iter()
                            .map(|arg| {
                                self.diagnostic_ty_name_with_state(arg, tyvars, next_tyvar_index)
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            Ty::Func(params, ret)
            | Ty::BuiltinFunc { params, ret, .. }
            | Ty::UserFunc { params, ret, .. } => {
                let param_str = params
                    .iter()
                    .map(|ty| self.diagnostic_ty_name_with_state(ty, tyvars, next_tyvar_index))
                    .collect::<Vec<_>>()
                    .join(", ");
                if param_str.is_empty() {
                    format!(
                        "(-> {})",
                        self.diagnostic_ty_name_with_state(ret, tyvars, next_tyvar_index)
                    )
                } else {
                    format!(
                        "({} -> {})",
                        param_str,
                        self.diagnostic_ty_name_with_state(ret, tyvars, next_tyvar_index)
                    )
                }
            }
        }
    }

    pub(super) fn diagnostic_ty_names(&self, tys: &[&Ty]) -> Vec<String> {
        let mut tyvars = HashMap::new();
        let mut next_tyvar_index = 0usize;
        tys.iter()
            .map(|ty| self.diagnostic_ty_name_with_state(ty, &mut tyvars, &mut next_tyvar_index))
            .collect()
    }

    pub(super) fn diagnostic_ty_name(&self, ty: &Ty) -> String {
        self.diagnostic_ty_names(&[ty])
            .into_iter()
            .next()
            .unwrap_or_default()
    }

    pub(super) fn ty_name(&self, ty: &Ty) -> String {
        match ty {
            Ty::Int => "Int".into(),
            Ty::Float => "Float".into(),
            Ty::Str => "String".into(),
            Ty::Bool => "Boolean".into(),
            Ty::Unit => "Unit".into(),
            Ty::Error => "Error".into(),
            Ty::Hole => "_".into(),
            Ty::SelfApp(args) => format!(
                "Self<{}>",
                args.iter()
                    .map(|arg| self.ty_name(arg))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::ExtractorClosure(inner) => format!("ExtractorClosure<{}>", self.ty_name(inner)),
            Ty::MatchResult(inner) => format!("MatchResult<{}>", self.ty_name(inner)),
            Ty::List(inner) => format!("List<{}>", self.ty_name(inner)),
            Ty::Lazy(inner) => format!("Lazy<{}>", self.ty_name(inner)),
            Ty::Pid(marker) => format!("PID<{}>", self.ty_name(marker)),
            Ty::ProcessMarker(name) => Self::surface_name(name).to_string(),
            Ty::Facet(kind, source, focus, update_source, update_focus) => {
                format!(
                    "Facet<{}, {}, {}, {}, {}>",
                    kind.as_str(),
                    self.ty_name(source),
                    self.ty_name(focus),
                    self.ty_name(update_source),
                    self.ty_name(update_focus)
                )
            }
            Ty::Tuple(items) => format!(
                "({})",
                items
                    .iter()
                    .map(|item| self.ty_name(item))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Result(ok, _) => format!("Result<{}>", self.ty_name(ok)),
            Ty::Var(n) => self
                .local_annotation_tyvars
                .iter()
                .find_map(|(name, ty)| matches!(ty, Ty::Var(var) if var == n).then(|| name.clone()))
                .unwrap_or_else(|| "the inferred argument type".into()),
            Ty::Struct(name, _) | Ty::Record(name, _) => Self::surface_name(name).to_string(),
            Ty::Enum(name, args) => {
                if args.is_empty() {
                    Self::surface_name(name).to_string()
                } else {
                    format!(
                        "{}<{}>",
                        Self::surface_name(name),
                        args.iter()
                            .map(|arg| self.ty_name(arg))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            Ty::Func(params, ret) => {
                let param_str = params
                    .iter()
                    .map(|ty| self.ty_name(ty))
                    .collect::<Vec<_>>()
                    .join(", ");
                if param_str.is_empty() {
                    format!("(-> {})", self.ty_name(ret))
                } else {
                    format!("({} -> {})", param_str, self.ty_name(ret))
                }
            }
            Ty::BuiltinFunc { name, .. } => format!("Builtin({})", name),
            Ty::UserFunc { .. } => "UserFunc".into(),
        }
    }

    pub(super) fn format_signature(&self, name: &str, params: &[Ty], ret: &Ty) -> String {
        format!(
            "{}: ({}) -> {}",
            name,
            params
                .iter()
                .map(|ty| self.ty_name(ty))
                .collect::<Vec<_>>()
                .join(", "),
            self.ty_name(ret)
        )
    }

    pub(super) fn find_tail_print_call<'a>(&self, node: &'a TypedNode) -> Option<&'a TypedNode> {
        match &node.node {
            TypedInner::Block(stmts) => stmts
                .last()
                .and_then(|last| self.find_tail_print_call(last)),
            TypedInner::Semi(inner) => self.find_tail_print_call(inner),
            TypedInner::App(func, _) => match &func.ty {
                Ty::BuiltinFunc { name, .. } if name == "print" => Some(node),
                _ => None,
            },
            _ => None,
        }
    }

    pub(super) fn describe_unit_return_hint(&self, body: &TypedNode) -> Option<String> {
        let call = self.find_tail_print_call(body)?;
        if let TypedInner::App(func, _) = &call.node {
            if let Ty::BuiltinFunc { name, params, ret } = &func.ty {
                return Some(format!(
                    "The function body ends with `print(...)`, which returns Unit.\n{}\nUse `print(...)` as a statement and end the function with an Int expression.",
                    self.format_signature(name, params, ret)
                ));
            }
        }
        None
    }

    pub(super) fn return_mismatch_span(&self, body: &TypedNode) -> Span {
        self.tail_expr_span(body)
            .unwrap_or_else(|| body.span.clone())
    }

    pub(super) fn tail_expr_span(&self, node: &TypedNode) -> Option<Span> {
        match &node.node {
            TypedInner::Block(stmts) => stmts.last().map(|last| {
                self.tail_expr_span(last)
                    .unwrap_or_else(|| last.span.clone())
            }),
            TypedInner::Semi(inner) => Some(
                self.tail_expr_span(inner)
                    .unwrap_or_else(|| inner.span.clone()),
            ),
            TypedInner::DoSafeBind(control) => Some(control.origins.result_span.clone()),
            _ => Some(node.span.clone()),
        }
    }
}

impl Checker {
    pub(super) fn surface_name<'a>(name: &'a str) -> &'a str {
        name.strip_prefix("Global::").unwrap_or(name)
    }

    pub(super) fn surface_ast_ty(ast_ty: &AstTy) -> String {
        match ast_ty {
            AstTy::Named(_, name) | AstTy::ImplTrait(_, name) => Self::surface_name(name).into(),
            AstTy::Generic(_, name, args) => format!(
                "{}<{}>",
                Self::surface_name(name),
                args.iter()
                    .map(Self::surface_ast_ty)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Tuple(_, items) => format!(
                "({})",
                items
                    .iter()
                    .map(Self::surface_ast_ty)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Func(_, params, ret) => format!(
                "({} -> {})",
                params
                    .iter()
                    .map(Self::surface_ast_ty)
                    .collect::<Vec<_>>()
                    .join(", "),
                Self::surface_ast_ty(ret)
            ),
        }
    }

    pub(super) fn surface_qualified_name<'a>(name: Option<&'a str>) -> Option<&'a str> {
        name.map(Self::surface_name)
    }
}
