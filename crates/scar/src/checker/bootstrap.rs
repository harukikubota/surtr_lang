use super::*;

pub(super) fn initialize_env() -> TypeEnv {
    let mut env = TypeEnv::new();
    // `Duration` is a stdlib-defined struct, but builtin signatures mention it
    // before stdlib declarations are typechecked. Reserve its type head here so
    // builtin signature parsing can treat it as the same struct identity.
    env.predeclare_type_def(
        "Global::Duration".into(),
        crate::env::TypeKind::Struct,
        Vec::new(),
        Vec::new(),
    );
    env.predeclare_type_def(
        "Global::SupervisorStatus".into(),
        crate::env::TypeKind::Struct,
        Vec::new(),
        Vec::new(),
    );
    for name in [
        "Global::FilePath",
        "Global::FileSystemEntry",
        "Global::FileSystemSnapshot",
        "Global::CommandResult",
    ] {
        env.predeclare_type_def(
            name.into(),
            crate::env::TypeKind::Struct,
            Vec::new(),
            Vec::new(),
        );
    }

    // Only compiler-generated runtime helpers are bound before standard declarations.
    // Public builtin functions get their UID and type from the standard source.
    for (uid, meta) in sigil::resolver::compiler_builtin_bindings() {
        let ty = builtin_ty_from_meta(meta, &mut env);
        env.bind_var(uid, ty);
    }

    env
}

pub(super) fn builtin_ty_from_meta(meta: &BuiltinMeta, env: &mut TypeEnv) -> Ty {
    let mut parser = BuiltinSignatureParser::new(meta.sig_str, env);
    let (params, ret) = parser
        .parse_signature()
        .unwrap_or_else(|err| panic!("invalid builtin signature for `{}`: {}", meta.name, err));
    Ty::BuiltinFunc {
        name: meta.name.into(),
        params,
        ret: Box::new(ret),
    }
}

struct BuiltinSignatureParser<'a, 'env> {
    input: &'a str,
    pos: usize,
    env: &'env mut TypeEnv,
    tyvars: HashMap<String, Ty>,
}

impl<'a, 'env> BuiltinSignatureParser<'a, 'env> {
    fn new(input: &'a str, env: &'env mut TypeEnv) -> Self {
        Self {
            input,
            pos: 0,
            env,
            tyvars: HashMap::new(),
        }
    }

    fn parse_signature(&mut self) -> Result<(Vec<Ty>, Ty), String> {
        self.skip_ws();
        let params = self.parse_param_list()?;
        self.skip_ws();
        self.expect("->")?;
        let ret = self.parse_type()?;
        self.skip_ws();
        if !self.is_eof() {
            return Err(format!(
                "unexpected trailing input `{}`",
                &self.input[self.pos..]
            ));
        }
        Ok((params, ret))
    }

    fn parse_param_list(&mut self) -> Result<Vec<Ty>, String> {
        self.expect("(")?;
        self.skip_ws();
        if self.consume(")") {
            return Ok(Vec::new());
        }
        let mut params = vec![self.parse_type()?];
        loop {
            self.skip_ws();
            if self.consume(")") {
                break;
            }
            self.expect(",")?;
            params.push(self.parse_type()?);
        }
        Ok(params)
    }

    fn parse_type(&mut self) -> Result<Ty, String> {
        self.skip_ws();
        let mut ty = if self.consume("(") {
            self.skip_ws();
            if self.consume(")") {
                Ty::Unit
            } else if self.consume("->") {
                let ret = self.parse_type()?;
                self.skip_ws();
                self.expect(")")?;
                Ty::Func(Vec::new(), Box::new(ret))
            } else {
                let first = self.parse_type()?;
                self.skip_ws();
                let mut items = vec![first];
                while self.consume(",") {
                    items.push(self.parse_type()?);
                    self.skip_ws();
                }
                if self.consume("->") {
                    let ret = self.parse_type()?;
                    self.skip_ws();
                    self.expect(")")?;
                    Ty::Func(items, Box::new(ret))
                } else {
                    self.expect(")")?;
                    match items.as_slice() {
                        [single] => single.clone(),
                        _ => Ty::Tuple(items),
                    }
                }
            }
        } else {
            let ident = self.parse_ident()?;
            self.skip_ws();
            if self.consume("<") {
                let mut args = vec![self.parse_type()?];
                loop {
                    self.skip_ws();
                    if self.consume(">") {
                        break;
                    }
                    self.expect(",")?;
                    args.push(self.parse_type()?);
                }
                self.build_generic_type(&ident, args)?
            } else {
                self.build_named_type(&ident)?
            }
        };

        self.skip_ws();
        while self.consume("?") {
            ty = Ty::Result(Box::new(ty), Box::new(Ty::Error));
            self.skip_ws();
        }
        Ok(ty)
    }

    fn builtin_special_enum_ty_for_query(ident: &str, args: &[Ty]) -> Option<Ty> {
        let surface = ident.strip_prefix("Global::").unwrap_or(ident);
        match surface {
            "Boolean" if args.is_empty() => Some(Ty::Bool),
            "Result" => args
                .first()
                .cloned()
                .map(|ok| Ty::Result(Box::new(ok), Box::new(Ty::Error))),
            _ => None,
        }
    }

    fn build_named_type(&mut self, ident: &str) -> Result<Ty, String> {
        if let Some(def) = self.env.lookup_type_def(ident) {
            return Ok(match &def.kind {
                crate::env::TypeKind::Struct => Ty::Struct(
                    def.name.clone(),
                    NominalType::new(
                        def.type_param_vars.iter().copied().map(Ty::Var).collect(),
                        def.fields.clone(),
                    ),
                ),
                crate::env::TypeKind::Record => Ty::Record(
                    def.name.clone(),
                    NominalType::new(
                        def.type_param_vars.iter().copied().map(Ty::Var).collect(),
                        def.fields.clone(),
                    ),
                ),
                crate::env::TypeKind::ConcreteError => Ty::Error,
                crate::env::TypeKind::Enum => Self::builtin_special_enum_ty_for_query(
                    def.name.strip_prefix("Global::").unwrap_or(&def.name),
                    &[],
                )
                .unwrap_or_else(|| Ty::Enum(def.name.clone(), Vec::new())),
            });
        }

        Ok(match ident {
            "Int" => Ty::Int,
            "Float" => Ty::Float,
            "String" => Ty::Str,
            "Boolean" => Ty::Bool,
            "Unit" => Ty::Unit,
            "Error" => Ty::Error,
            name if name.starts_with('$') => self
                .tyvars
                .entry(name.to_string())
                .or_insert_with(|| self.env.fresh_tyvar())
                .clone(),
            other => Ty::Enum(other.to_string(), Vec::new()),
        })
    }

    fn build_generic_type(&mut self, ident: &str, args: Vec<Ty>) -> Result<Ty, String> {
        Ok(match ident {
            "List" => {
                let [inner] = args.as_slice() else {
                    return Err("List requires exactly 1 type argument".into());
                };
                Ty::List(Box::new(inner.clone()))
            }
            "Result" => match args.as_slice() {
                [ok] => Ty::Result(Box::new(ok.clone()), Box::new(Ty::Error)),
                [ok, _err] => Ty::Result(Box::new(ok.clone()), Box::new(Ty::Error)),
                _ => return Err("Result requires 1 or 2 type arguments".into()),
            },
            "Lazy" => {
                let [inner] = args.as_slice() else {
                    return Err("Lazy requires exactly 1 type argument".into());
                };
                Ty::Lazy(Box::new(inner.clone()))
            }
            "Facet" => {
                let [kind, source, focus, update_source, update_focus] = args.as_slice() else {
                    return Err("Facet requires exactly 5 type arguments".into());
                };
                // Builtin polymorphic signatures use `$K` / `$L` for the
                // path-kind position.  `Ty` deliberately keeps Facet's kind
                // concrete, so use the broad readable capability while the
                // intrinsic checker preserves the path's actual atomic kind.
                let kind = match kind {
                    Ty::Enum(name, _) => crate::types::FacetKind::from_surface_name(&name)
                        .ok_or_else(|| format!("unknown Facet kind `{name}`"))?,
                    Ty::Var(_) => crate::types::FacetKind::ReadablePath,
                    _ => return Err("Facet kind must be a path kind name".into()),
                };
                Ty::Facet(
                    kind,
                    Box::new(source.clone()),
                    Box::new(focus.clone()),
                    Box::new(update_source.clone()),
                    Box::new(update_focus.clone()),
                )
            }
            "PID" => {
                let [inner] = args.as_slice() else {
                    return Err("PID requires exactly 1 type argument".into());
                };
                Ty::Pid(Box::new(builtin_pid_marker_from_ty(inner)?))
            }
            "Workers" | "WorkerLease" => {
                let [inner] = args.as_slice() else {
                    return Err(format!("{ident} requires exactly 1 type argument"));
                };
                Ty::Enum(
                    ident.to_string(),
                    vec![Ty::Pid(Box::new(builtin_pid_marker_from_ty(inner)?))],
                )
            }
            other => Self::builtin_special_enum_ty_for_query(other, &args)
                .unwrap_or_else(|| Ty::Enum(other.to_string(), args)),
        })
    }

    fn parse_ident(&mut self) -> Result<String, String> {
        self.skip_ws();
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        if self.pos == start {
            Err(format!("expected type identifier at byte {}", self.pos))
        } else {
            Ok(self.input[start..self.pos].to_string())
        }
    }

    fn expect(&mut self, needle: &str) -> Result<(), String> {
        self.skip_ws();
        if self.consume(needle) {
            Ok(())
        } else {
            Err(format!("expected `{}` at byte {}", needle, self.pos))
        }
    }

    fn consume(&mut self, needle: &str) -> bool {
        if self.input[self.pos..].starts_with(needle) {
            self.pos += needle.len();
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.input.len()
    }
}

fn builtin_pid_marker_from_ty(ty: &Ty) -> Result<Ty, String> {
    match ty {
        Ty::Var(_) => Ok(ty.clone()),
        Ty::Enum(name, args)
            if args.is_empty() && matches!(name.as_str(), "OutHandler" | "InHandler") =>
        {
            Ok(Ty::ProcessMarker(format!("Global::{name}")))
        }
        _ => Err(format!("invalid builtin PID marker: {ty:?}")),
    }
}

#[cfg(test)]
mod builtin_signature_tests {
    use super::{builtin_ty_from_meta, TypeEnv};
    use crate::types::Ty;
    use sindr::builtin::builtin_function_metas;

    #[test]
    fn builtin_meta_signatures_bootstrap_into_type_env() {
        let mut env = TypeEnv::new();
        for meta in builtin_function_metas() {
            let ty = builtin_ty_from_meta(meta, &mut env);
            match ty {
                Ty::BuiltinFunc { name, params, .. } => {
                    assert_eq!(name, meta.name);
                    assert_eq!(params.len(), meta.arity as usize);
                }
                other => panic!("expected builtin function type, got {:?}", other),
            }
        }
    }
}

pub(super) fn format_builtin_type_param_suffix(params: &[&str]) -> String {
    if params.is_empty() {
        String::new()
    } else {
        format!("<{}>", params.join(", "))
    }
}
