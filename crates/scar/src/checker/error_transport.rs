use super::*;

impl Checker {
    /// Transport changes only Error positions. A whole Result retains its success type.
    fn error_transport_children(
        &mut self,
        target: &Ty,
        source: &Ty,
        span: &Span,
    ) -> Result<Option<Vec<(Ty, Ty)>>, TypeError> {
        if !self.type_contains_error_position(target) && !self.type_contains_error_position(source)
        {
            return Ok(None);
        }
        let pairs = match (target, source) {
            (Ty::Result(ok, error), Ty::Result(other_ok, other_error)) => {
                if !self.types_match_without_error_conversion(ok, other_ok)?
                    || self.canonical_request(ok)? != self.canonical_request(other_ok)?
                {
                    return Err(TypeError::new(
                        "Result conversion must preserve the success type",
                        span.clone(),
                    ));
                }
                vec![((**error).clone(), (**other_error).clone())]
            }
            (Ty::List(a), Ty::List(b)) | (Ty::Lazy(a), Ty::Lazy(b)) => {
                vec![((**a).clone(), (**b).clone())]
            }
            (Ty::Tuple(a), Ty::Tuple(b)) if a.len() == b.len() => {
                a.iter().cloned().zip(b.iter().cloned()).collect()
            }
            (Ty::Enum(a, xs), Ty::Enum(b, ys))
                if a == b
                    && matches!(a.as_str(), "HashMap" | "Generator" | "InfiniteGenerator")
                    && xs.len() == 1
                    && ys.len() == 1 =>
            {
                vec![(xs[0].clone(), ys[0].clone())]
            }
            (Ty::Enum(a, xs), Ty::Enum(b, ys)) if a == b && xs.len() == ys.len() => {
                let invariant = self.nominal_error_transport_variance(a, xs, span)?;
                for (index, (target, source)) in xs.iter().zip(ys).enumerate() {
                    if invariant[index]
                        && (!self.types_match_without_error_conversion(target, source)?
                            || self.canonical_request(target)? != self.canonical_request(source)?)
                    {
                        return Err(TypeError::new(
                            "Error transport must preserve invariant storage positions",
                            span.clone(),
                        ));
                    }
                }
                xs.iter().cloned().zip(ys.iter().cloned()).collect()
            }
            (Ty::Struct(a, xs), Ty::Struct(b, ys))
                if a == b
                    && Self::canonical_user_type_name(a) == "Global::ResultT"
                    && xs.arguments.len() == 3
                    && ys.arguments.len() == 3 =>
            {
                for index in 0..2 {
                    if !self.types_match_without_error_conversion(
                        &xs.arguments[index],
                        &ys.arguments[index],
                    )? || self.canonical_request(&xs.arguments[index])?
                        != self.canonical_request(&ys.arguments[index])?
                    {
                        return Err(TypeError::new(
                            "ResultT conversion must preserve the base carrier and success type",
                            span.clone(),
                        ));
                    }
                }
                vec![(xs.arguments[2].clone(), ys.arguments[2].clone())]
            }
            (Ty::Struct(a, xs), Ty::Struct(b, ys)) | (Ty::Record(a, xs), Ty::Record(b, ys))
                if a == b && xs.arguments.len() == ys.arguments.len() =>
            {
                // A nominal identity has one canonical field schema. Only its
                // arguments can differ; expanding recursive fields here would
                // create an unbounded concrete type graph.
                let invariant = self.nominal_error_transport_variance(a, &xs.arguments, span)?;
                for (index, (target, source)) in xs.arguments.iter().zip(&ys.arguments).enumerate()
                {
                    if invariant[index]
                        && (!self.types_match_without_error_conversion(target, source)?
                            || self.canonical_request(target)? != self.canonical_request(source)?)
                    {
                        return Err(TypeError::new(
                            "Error transport must preserve invariant storage positions",
                            span.clone(),
                        ));
                    }
                }
                xs.arguments
                    .iter()
                    .cloned()
                    .zip(ys.arguments.iter().cloned())
                    .collect()
            }
            _ => return Ok(None),
        };
        Ok(Some(pairs))
    }

    pub(super) fn error_transport_requires_conversion(
        &mut self,
        target: &Ty,
        source: &Ty,
        span: &Span,
    ) -> Result<bool, TypeError> {
        let mut pending = vec![(target.clone(), source.clone())];
        let mut visited = Vec::new();
        let mut required = false;
        while let Some((target, source)) = pending.pop() {
            let pair = (self.resolve_ty(&target), self.resolve_ty(&source));
            if pair.0 == pair.1 || visited.contains(&pair) {
                continue;
            }
            visited.push(pair.clone());
            if self.is_error_slot_ty(&pair.0) && self.is_error_slot_ty(&pair.1) {
                required = true;
                continue;
            }
            if let Some(children) = self.error_transport_children(&pair.0, &pair.1, span)? {
                pending.extend(children);
            }
        }
        Ok(required)
    }

    pub(super) fn validate_aggregate_error_transport(
        &mut self,
        node: &TypedNode,
        source: &TypedNode,
    ) -> Result<bool, TypeError> {
        let target = self.resolve_ty(&node.ty);
        let actual = self.resolve_ty(&source.ty);
        if self.is_error_slot_ty(&target) && self.is_error_slot_ty(&actual) {
            return Ok(false);
        }
        let Some(children) = self.error_transport_children(&target, &actual, &node.span)? else {
            return Ok(false);
        };
        let mut pending = children;
        let mut visited = Vec::new();
        while let Some((target, actual)) = pending.pop() {
            let pair = (self.resolve_ty(&target), self.resolve_ty(&actual));
            if pair.0 == pair.1 || visited.contains(&pair) {
                continue;
            }
            visited.push(pair.clone());
            if self.is_error_slot_ty(&pair.0) && self.is_error_slot_ty(&pair.1) {
                let value = TypedNode {
                    ty: pair.1,
                    span: source.span.clone(),
                    node: TypedInner::Lit(Lit::Unit),
                };
                let conversion = TypedNode {
                    ty: pair.0,
                    span: node.span.clone(),
                    node: TypedInner::ErrorCoerce {
                        value: Box::new(value),
                        concrete_kind: None,
                    },
                };
                self.validate_error_coercion_node(&conversion)?;
            } else if let Some(children) =
                self.error_transport_children(&pair.0, &pair.1, &node.span)?
            {
                pending.extend(children);
            } else if !self.types_match_without_error_conversion(&pair.0, &pair.1)? {
                return Err(TypeError::new(
                    "Error transport must preserve non-Error positions",
                    node.span.clone(),
                ));
            }
        }
        Ok(true)
    }

    pub(super) fn callable_error_return_widens(
        &mut self,
        expected: &Ty,
        actual: &Ty,
    ) -> Result<bool, TypeError> {
        let target = self.resolve_ty(expected);
        let source = self.resolve_ty(actual);
        let Some((inputs, output)) = self.function_parts(&target) else {
            return Ok(false);
        };
        let inputs = inputs.to_vec();
        let output = output.clone();
        let Some((actual_inputs, actual_output)) = self.function_parts(&source) else {
            return Ok(false);
        };
        let actual_inputs = actual_inputs.to_vec();
        let actual_output = actual_output.clone();
        let widens = match (&output, &actual_output) {
            (Ty::Error, source) => self.enum_error_name(source).is_some(),
            (Ty::Result(_, error), Ty::Result(_, source)) => {
                **error == Ty::Error && self.enum_error_name(source).is_some()
            }
            _ => false,
        };
        if !widens || inputs.len() != actual_inputs.len() {
            return Ok(false);
        }
        for (target, source) in inputs.iter().zip(&actual_inputs) {
            if !self.types_match_without_error_conversion(target, source)? {
                return Ok(false);
            }
        }
        if let (Ty::Result(ok, _), Ty::Result(actual_ok, _)) = (&output, &actual_output) {
            if !self.types_match_without_error_conversion(ok, actual_ok)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// A callable keeps its original signature; the adapter widens its result after one call.
    pub(super) fn adapt_error_return_callable(
        &mut self,
        typed: &mut TypedNode,
        expected: &Ty,
    ) -> Result<bool, TypeError> {
        if !self.callable_error_return_widens(expected, &typed.ty)? {
            return Ok(false);
        }
        let target = self.resolve_ty(expected);
        let source = self.resolve_ty(&typed.ty);
        let (inputs, output) = self
            .function_parts(&target)
            .expect("checked callable target");
        let inputs = inputs.to_vec();
        let output = output.clone();
        let (_, actual_output) = self
            .function_parts(&source)
            .expect("checked callable source");
        let actual_output = actual_output.clone();
        let make_id = |name: &str| ResolvedId {
            name: name.into(),
            qualified_name: None,
            symbol_info: None,
            unique_id: Self::next_synthetic_range_uid(),
            compiler_generated: true,
            span: typed.span.clone(),
        };
        let function = make_id("__error_transport_function");
        let params = inputs
            .iter()
            .enumerate()
            .map(|(index, ty)| TypedClosureParam {
                id: make_id(&format!("__error_transport_input_{index}")),
                ty: self.resolve_ty(ty),
            })
            .collect::<Vec<_>>();
        let function_ref = TypedNode {
            ty: source.clone(),
            span: typed.span.clone(),
            node: TypedInner::Var(function.clone()),
        };
        let args = params
            .iter()
            .map(|param| TypedNode {
                ty: param.ty.clone(),
                span: typed.span.clone(),
                node: TypedInner::Var(param.id.clone()),
            })
            .collect();
        let mut body = TypedNode {
            ty: actual_output,
            span: typed.span.clone(),
            node: TypedInner::App(Box::new(function_ref), args),
        };
        self.coerce_error_value_receiver(&mut body, &output, None)?;
        let closure = TypedNode {
            ty: target.clone(),
            span: typed.span.clone(),
            node: TypedInner::Closure(params, vec![function.clone()], Box::new(body)),
        };
        let binding = TypedNode {
            ty: Ty::Unit,
            span: typed.span.clone(),
            node: TypedInner::Bind(TypedPattern::Var(source, function), Box::new(typed.clone())),
        };
        *typed = TypedNode {
            ty: target,
            span: typed.span.clone(),
            node: TypedInner::Block(vec![binding, closure]),
        };
        Ok(true)
    }
}
