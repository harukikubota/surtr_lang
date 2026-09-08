//! Compile-time value views follow source projections, independently of nominal type resolution.
use super::*;

type Source = (ConstructorCapabilityProvenance, Ty);
type Bindings = HashMap<u32, Source>;
use ConstructorCapabilityProvenance as Provenance;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) enum Projection {
    Field { index: usize, tag: Option<u32> },
    Element,
    TypeArgument(usize),
}

impl Checker {
    pub(super) fn constructor_capability_for_node(&self, node: &TypedNode) -> Provenance {
        self.value_provenance(node, &Bindings::new())
    }

    fn source_provenance(&self, node: &TypedNode, bindings: &Bindings) -> Source {
        (self.value_provenance(node, bindings), node.ty.clone())
    }

    fn value_provenance(&self, node: &TypedNode, bindings: &Bindings) -> Provenance {
        match &node.node {
            TypedInner::Var(id) => bindings
                .get(&id.unique_id)
                .map(|source| source.0.clone())
                .or_else(|| self.constructor_capabilities.get(&id.unique_id).cloned())
                .unwrap_or_else(|| {
                    if self.callable_signatures.contains_key(&id.unique_id) {
                        Provenance::DeclaredCallable(id.unique_id)
                    } else {
                        Provenance::RequiresProof
                    }
                }),
            TypedInner::TupleLiteral(items) | TypedInner::StructLit(_, items) => {
                Provenance::Fields(
                    items
                        .iter()
                        .map(|item| self.source_provenance(item, bindings))
                        .collect(),
                )
            }
            TypedInner::ConstructorCall(tag, items) => Provenance::Variants(vec![(
                *tag,
                items
                    .iter()
                    .map(|item| self.source_provenance(item, bindings))
                    .collect(),
            )]),
            TypedInner::ListNil => Provenance::Sequence(Vec::new()),
            TypedInner::ListLiteral(items) => Provenance::Sequence(
                items
                    .iter()
                    .map(|item| self.source_provenance(item, bindings))
                    .collect(),
            ),
            TypedInner::HashMapLiteral(items) => Provenance::Sequence(
                items
                    .iter()
                    .map(|(_, item)| self.source_provenance(item, bindings))
                    .collect(),
            ),
            TypedInner::ListCons(head, tail) => {
                let mut elements = vec![self.source_provenance(head, bindings)];
                let tail_source = self.source_provenance(tail, bindings);
                if !matches!(&tail_source.0, Provenance::Sequence(items) if items.is_empty()) {
                    elements.push(self.project_provenance(
                        &tail_source,
                        &Projection::Element,
                        &head.ty,
                    ));
                }
                Provenance::Sequence(elements)
            }
            TypedInner::FieldAccess(value, index) => {
                self.project_provenance(
                    &self.source_provenance(value, bindings),
                    &Projection::Field {
                        index: *index as usize,
                        tag: None,
                    },
                    &node.ty,
                )
                .0
            }
            TypedInner::FacetView {
                source,
                path,
                source_is_result,
            } => {
                let mut source = self.source_provenance(source, bindings);
                if *source_is_result {
                    source = self.project_provenance(
                        &source,
                        &Projection::Field {
                            index: 0,
                            tag: Some(0),
                        },
                        &path.source_ty,
                    );
                }
                for segment in &path.segments {
                    let projection = match segment {
                        TypedFacetSegment::Field { field_index, .. }
                        | TypedFacetSegment::Tuple { field_index, .. } => Projection::Field {
                            index: *field_index as usize,
                            tag: None,
                        },
                        TypedFacetSegment::ListIndex { .. } | TypedFacetSegment::MapKey { .. } => {
                            Projection::Element
                        }
                        TypedFacetSegment::Variant {
                            variant_tag,
                            payload_arity: 1,
                            ..
                        } => Projection::Field {
                            index: 1,
                            tag: Some(*variant_tag),
                        },
                        TypedFacetSegment::ListRange { .. } => continue,
                        _ => return Provenance::Intersection(Vec::new()),
                    };
                    source = self.project_provenance(&source, &projection, &path.focus_ty);
                }
                source.0
            }
            TypedInner::Closure(parameters, _, body) => {
                let mut local = bindings.clone();
                for parameter in parameters {
                    local.insert(
                        parameter.id.unique_id,
                        (
                            Provenance::Parameter(parameter.id.unique_id),
                            parameter.ty.clone(),
                        ),
                    );
                }
                Provenance::Callable {
                    parameters: parameters
                        .iter()
                        .map(|parameter| parameter.id.unique_id)
                        .collect(),
                    result: Box::new(self.source_provenance(body, &local)),
                }
            }
            TypedInner::App(function, arguments) => {
                let arguments = arguments
                    .iter()
                    .map(|argument| self.source_provenance(argument, bindings))
                    .collect::<Vec<_>>();
                let function_source = self.source_provenance(function, bindings);
                self.invoke_provenance(&function_source, &arguments, &node.ty)
                    .0
            }
            TypedInner::InjectCall(function, arguments) => Provenance::Injected {
                function: Box::new(self.source_provenance(function, bindings)),
                arguments: arguments
                    .iter()
                    .map(|argument| self.source_provenance(argument, bindings))
                    .collect(),
            },
            TypedInner::MapErr(value, error) => {
                let source = self.source_provenance(value, bindings);
                let success = self.project_provenance(
                    &source,
                    &Projection::Field {
                        index: 0,
                        tag: Some(0),
                    },
                    &Ty::Hole,
                );
                Provenance::Variants(vec![
                    (0, vec![success]),
                    (1, vec![self.source_provenance(error, bindings)]),
                ])
            }
            TypedInner::Capture(function, arguments) if arguments.is_empty() => {
                self.value_provenance(function, bindings)
            }
            TypedInner::TraitCall {
                obligation,
                method_name,
                dispatch,
                args,
                ..
            } => {
                let Some(method) = self
                    .traits
                    .get(&obligation.trait_id)
                    .and_then(|info| info.methods.get(method_name))
                else {
                    return Provenance::Intersection(Vec::new());
                };
                let arguments = args
                    .iter()
                    .map(|argument| self.source_provenance(argument, bindings))
                    .collect::<Vec<_>>();
                if self.traits[&obligation.trait_id]
                    .constructor_slots
                    .is_empty()
                {
                    if let TraitDispatch::Selected(selected) = dispatch {
                        if let TraitImplementationId::Declared { declaration, .. } =
                            &selected.implementation
                        {
                            let Some(signature) = self
                                .trait_impls
                                .values()
                                .find(|info| &info.declaration_key == declaration)
                                .and_then(|info| info.method_signature_lists.get(method_name))
                            else {
                                return Provenance::Intersection(Vec::new());
                            };
                            let mut parameters = Vec::new();
                            let mut result = None;
                            for entry in &signature.entries {
                                let Ok(ty) = self.canonical_to_ty(&entry.ty) else {
                                    return Provenance::Intersection(Vec::new());
                                };
                                match entry.role {
                                    TypeListRole::ValueParameter => parameters.push(ty),
                                    TypeListRole::ReturnType => result = Some(ty),
                                    _ => {}
                                }
                            }
                            let Some(result) = result else {
                                return Provenance::Intersection(Vec::new());
                            };
                            let mut variables = HashMap::new();
                            self.collect_callable_provenance_variables(
                                &parameters,
                                &arguments,
                                &mut variables,
                            );
                            return self
                                .template_provenance(
                                    &result,
                                    &node.ty,
                                    &self.merged_provenance_variables(&variables),
                                )
                                .0;
                        }
                    }
                }
                self.invoke_provenance(
                    &(Provenance::DeclaredCallable(method.id.unique_id), Ty::Hole),
                    &arguments,
                    &node.ty,
                )
                .0
            }
            TypedInner::EagerBoundary(inner) => self.value_provenance(inner, bindings),
            TypedInner::If(_, then_branch, Some(else_branch)) => self
                .common_constructor_provenance(&[
                    self.source_provenance(then_branch, bindings),
                    self.source_provenance(else_branch, bindings),
                ]),
            TypedInner::Match(scrutinee, arms) => {
                let source = self.source_provenance(scrutinee, bindings);
                self.common_constructor_provenance(
                    &arms
                        .iter()
                        .map(|arm| {
                            let mut local = bindings.clone();
                            self.match_provenance_bindings(&arm.pattern, &source, &mut local);
                            self.source_provenance(&arm.body, &local)
                        })
                        .collect::<Vec<_>>(),
                )
            }
            TypedInner::Block(statements) => {
                let mut local = bindings.clone();
                let mut result = Provenance::RequiresProof;
                for statement in statements {
                    result = self.value_provenance(statement, &local);
                    let inner = match &statement.node {
                        TypedInner::Semi(inner) => inner.as_ref(),
                        _ => statement,
                    };
                    match &inner.node {
                        TypedInner::Bind(pattern, value) => {
                            let source = self.source_provenance(value, &local);
                            self.pattern_provenance_bindings(pattern, &source, &mut local);
                        }
                        TypedInner::SafeBind(pattern, value) => {
                            let source = self.source_provenance(value, &local);
                            let success = self.safebind_source_provenance(&source);
                            self.pattern_provenance_bindings(pattern, &success, &mut local);
                        }
                        _ => {}
                    }
                }
                result
            }
            _ => Provenance::RequiresProof,
        }
    }

    fn trait_provenance_template(&self, ty: &Ty, context: Option<(&str, &Ty)>) -> Ty {
        let Some((capability, actual)) = context else {
            return ty.clone();
        };
        let recurse = |ty: &Ty| self.trait_provenance_template(ty, context);
        match ty {
            Ty::SelfApp(arguments) if Self::constructor_application_parts(arguments).is_none() => {
                self.expand_constructor_template(capability, actual, arguments)
                    .unwrap_or_else(|| ty.clone())
            }
            Ty::Func(parameters, result) => Ty::Func(
                parameters.iter().map(recurse).collect(),
                Box::new(recurse(result)),
            ),
            Ty::List(element) => Ty::List(Box::new(recurse(element))),
            Ty::Tuple(items) => Ty::Tuple(items.iter().map(recurse).collect()),
            Ty::Result(ok, error) => Ty::Result(Box::new(recurse(ok)), Box::new(recurse(error))),
            Ty::Enum(name, arguments) => {
                Ty::Enum(name.clone(), arguments.iter().map(recurse).collect())
            }
            Ty::Struct(name, fields) => Ty::Struct(
                name.clone(),
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), recurse(ty)))
                    .collect(),
            ),
            Ty::Record(name, fields) => Ty::Record(
                name.clone(),
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), recurse(ty)))
                    .collect(),
            ),
            _ => ty.clone(),
        }
    }

    fn merged_provenance_variables(&self, variables: &HashMap<u32, Vec<Source>>) -> Bindings {
        variables
            .iter()
            .map(|(variable, sources)| {
                (
                    *variable,
                    (
                        self.common_constructor_provenance(sources),
                        sources
                            .first()
                            .map(|source| source.1.clone())
                            .unwrap_or(Ty::Hole),
                    ),
                )
            })
            .collect()
    }

    fn collect_callable_provenance_variables(
        &self,
        parameters: &[Ty],
        arguments: &[Source],
        variables: &mut HashMap<u32, Vec<Source>>,
    ) {
        for (parameter, argument) in parameters.iter().zip(arguments) {
            if !matches!(parameter, Ty::Func(..)) {
                self.collect_provenance_variables(parameter, argument, variables);
            }
        }
        for (parameter, argument) in parameters.iter().zip(arguments) {
            if let Ty::Func(parameters, result) = parameter {
                let known = self.merged_provenance_variables(variables);
                let inputs = parameters
                    .iter()
                    .map(|ty| self.template_provenance(ty, ty, &known))
                    .collect::<Vec<_>>();
                let output = self.invoke_provenance(argument, &inputs, result);
                self.collect_provenance_variables(result, &output, variables);
            }
        }
    }

    fn invoke_provenance(&self, function: &Source, arguments: &[Source], actual: &Ty) -> Source {
        match &function.0 {
            Provenance::Injected {
                function,
                arguments: tail,
            } => {
                if arguments.len() != 1 {
                    return (Provenance::Intersection(Vec::new()), actual.clone());
                }
                let mut arguments = arguments.to_vec();
                arguments.extend(tail.iter().cloned());
                self.invoke_provenance(function, &arguments, actual)
            }
            Provenance::Intersection(functions) => {
                let results = functions
                    .iter()
                    .map(|function| self.invoke_provenance(function, arguments, actual))
                    .collect::<Vec<_>>();
                (self.common_constructor_provenance(&results), actual.clone())
            }
            Provenance::Callable { parameters, result } => {
                if parameters.len() != arguments.len() {
                    return (Provenance::Intersection(Vec::new()), actual.clone());
                }
                self.substitute_provenance(
                    result,
                    &parameters
                        .iter()
                        .copied()
                        .zip(arguments.iter().cloned())
                        .collect(),
                )
            }
            Provenance::DeclaredCallable(id) => {
                let Some(signature) = self.callable_signatures.get(id) else {
                    return (Provenance::Intersection(Vec::new()), actual.clone());
                };
                let context = self
                    .traits
                    .iter()
                    .find(|(_, info)| {
                        info.methods
                            .values()
                            .any(|method| method.id.unique_id == *id)
                    })
                    .and_then(|(name, _)| {
                        let receiver = signature.value_parameters.iter().zip(arguments).find_map(
                            |(parameter, source)| {
                                matches!(parameter.ty, Ty::SelfApp(_)).then_some(&source.1)
                            },
                        );
                        Some((name.as_str(), receiver.unwrap_or(actual)))
                    });
                let parameters = signature
                    .value_parameters
                    .iter()
                    .map(|parameter| self.trait_provenance_template(&parameter.ty, context))
                    .collect::<Vec<_>>();
                let return_type =
                    self.trait_provenance_template(&signature.return_type.ty, context);
                let mut variables = HashMap::<u32, Vec<Source>>::new();
                self.collect_callable_provenance_variables(&parameters, arguments, &mut variables);
                self.template_provenance(
                    &return_type,
                    actual,
                    &self.merged_provenance_variables(&variables),
                )
            }
            Provenance::Template { ty, variables } => {
                let Some((parameters, result)) = self.function_parts(ty) else {
                    return (Provenance::Intersection(Vec::new()), actual.clone());
                };
                if parameters.len() != arguments.len() {
                    return (Provenance::Intersection(Vec::new()), actual.clone());
                }
                let mut sources = variables
                    .iter()
                    .map(|(variable, source)| (*variable, vec![source.clone()]))
                    .collect::<HashMap<_, _>>();
                self.collect_callable_provenance_variables(parameters, arguments, &mut sources);
                self.template_provenance(
                    result,
                    actual,
                    &self.merged_provenance_variables(&sources),
                )
            }
            Provenance::Parameter(_) | Provenance::Projection { .. } => (
                Provenance::Call {
                    function: Box::new(function.clone()),
                    arguments: arguments.to_vec(),
                },
                actual.clone(),
            ),
            Provenance::RequiresProof => self
                .function_parts(&function.1)
                .map(|(_, result)| self.template_provenance(result, actual, &Bindings::new()))
                .unwrap_or_else(|| (Provenance::Intersection(Vec::new()), actual.clone())),
            _ => (Provenance::Intersection(Vec::new()), actual.clone()),
        }
    }

    pub(super) fn constructor_capability_for_type(&self, ty: &Ty) -> Option<String> {
        let Ty::SelfApp(items) = ty else {
            return None;
        };
        let (Ty::Var(variable), _) = Self::constructor_application_parts(items)? else {
            return None;
        };
        self.constructor_witness_traits.get(variable).cloned()
    }

    pub(super) fn common_constructor_provenance(&self, provenances: &[Source]) -> Provenance {
        let mut sources = Vec::new();
        for (provenance, ty) in provenances {
            let leaves = match provenance {
                Provenance::Intersection(nested) => nested.clone(),
                _ => vec![(provenance.clone(), ty.clone())],
            };
            for source in leaves {
                if !sources.contains(&source) {
                    sources.push(source);
                }
            }
        }
        Provenance::Intersection(sources)
    }

    pub(super) fn constructor_provenance_allows(
        &self,
        provenance: &Provenance,
        required: &str,
        actual_ty: &Ty,
    ) -> bool {
        match provenance {
            Provenance::Constrained(capabilities) => capabilities.iter().any(|actual| {
                self.constructor_capability_allows(actual, required, &mut HashSet::new())
            }),
            Provenance::Intersection(sources) => {
                !sources.is_empty()
                    && sources.iter().all(|(source, ty)| {
                        self.constructor_provenance_allows(source, required, ty)
                    })
            }
            Provenance::Template { ty, .. }
                if self.constructor_capability_for_type(ty).is_some() =>
            {
                let capability = self
                    .constructor_capability_for_type(ty)
                    .expect("checked constructor template");
                self.constructor_capability_allows(&capability, required, &mut HashSet::new())
            }
            Provenance::Template { ty, variables } => {
                let mapping = variables
                    .iter()
                    .map(|(variable, source)| (*variable, source.1.clone()))
                    .collect();
                let declared = self.substitute_ty_with_mapping(ty, &mapping);
                self.constructor_projection(required, &declared).is_some()
            }
            Provenance::Parameter(_) | Provenance::Projection { .. } | Provenance::Call { .. } => {
                false
            }
            _ => {
                if let Some(capability) = self.constructor_capability_for_type(actual_ty) {
                    self.constructor_capability_allows(&capability, required, &mut HashSet::new())
                } else {
                    self.constructor_projection(required, actual_ty).is_some()
                }
            }
        }
    }

    fn template_provenance(&self, template: &Ty, actual: &Ty, variables: &Bindings) -> Source {
        if let Ty::Var(variable) = template {
            if let Some(source) = variables.get(variable) {
                return source.clone();
            }
        }
        (
            Provenance::Template {
                ty: template.clone(),
                variables: variables.clone(),
            },
            actual.clone(),
        )
    }

    fn substitute_provenance(&self, source: &Source, bindings: &Bindings) -> Source {
        let substitute = |source: &Source| self.substitute_provenance(source, bindings);
        let provenance = match &source.0 {
            Provenance::Injected {
                function,
                arguments,
            } => Provenance::Injected {
                function: Box::new(substitute(function)),
                arguments: arguments.iter().map(substitute).collect(),
            },
            Provenance::Call {
                function,
                arguments,
            } => {
                return self.invoke_provenance(
                    &substitute(function),
                    &arguments.iter().map(substitute).collect::<Vec<_>>(),
                    &source.1,
                );
            }
            Provenance::Parameter(variable) => {
                return bindings
                    .get(variable)
                    .cloned()
                    .unwrap_or_else(|| source.clone());
            }
            Provenance::Projection {
                source: projected,
                projection,
            } => return self.project_provenance(&substitute(projected), projection, &source.1),
            Provenance::Intersection(sources) => self
                .common_constructor_provenance(&sources.iter().map(substitute).collect::<Vec<_>>()),
            Provenance::Fields(fields) => {
                Provenance::Fields(fields.iter().map(substitute).collect())
            }
            Provenance::Sequence(elements) => {
                Provenance::Sequence(elements.iter().map(substitute).collect())
            }
            Provenance::Variants(variants) => Provenance::Variants(
                variants
                    .iter()
                    .map(|(tag, fields)| (*tag, fields.iter().map(substitute).collect()))
                    .collect(),
            ),
            Provenance::Callable { parameters, result } => Provenance::Callable {
                parameters: parameters.clone(),
                result: Box::new(substitute(result)),
            },
            Provenance::Template { ty, variables } => Provenance::Template {
                ty: ty.clone(),
                variables: variables
                    .iter()
                    .map(|(variable, source)| (*variable, substitute(source)))
                    .collect(),
            },
            other => other.clone(),
        };
        (provenance, source.1.clone())
    }

    fn project_provenance(
        &self,
        source: &Source,
        projection: &Projection,
        expected: &Ty,
    ) -> Source {
        let actual = self
            .projection_type(&source.1, projection)
            .unwrap_or_else(|| expected.clone());
        match &source.0 {
            Provenance::Intersection(sources) => {
                let sources = sources
                    .iter()
                    .map(|source| self.project_provenance(source, projection, &actual))
                    .collect::<Vec<_>>();
                (self.common_constructor_provenance(&sources), actual)
            }
            Provenance::Fields(fields) => match projection {
                Projection::Field { index, .. } => fields
                    .get(*index)
                    .cloned()
                    .unwrap_or((Provenance::RequiresProof, actual)),
                _ => (Provenance::RequiresProof, actual),
            },
            Provenance::Variants(variants) => match projection {
                Projection::Field { index, tag } => {
                    let fields = variants
                        .iter()
                        .filter(|(candidate, _)| tag.is_none_or(|tag| tag == *candidate))
                        .filter_map(|(_, fields)| fields.get(*index).cloned())
                        .collect::<Vec<_>>();
                    if fields.is_empty() {
                        (Provenance::RequiresProof, actual)
                    } else {
                        (self.common_constructor_provenance(&fields), actual)
                    }
                }
                Projection::TypeArgument(index) => {
                    let Ty::Enum(name, _) = &source.1 else {
                        return (Provenance::RequiresProof, actual);
                    };
                    let Some(metadata) = self.env.enum_variants_of(name) else {
                        return (Provenance::RequiresProof, actual);
                    };
                    let mut collected = HashMap::<u32, Vec<Source>>::new();
                    let mut sources = Vec::new();
                    for (tag, fields) in variants {
                        let Some(variant) = metadata.iter().find(|variant| variant.tag == *tag)
                        else {
                            continue;
                        };
                        let Ty::Enum(_, arguments) = &variant.enum_ty else {
                            continue;
                        };
                        let Some(Ty::Var(variable)) = arguments.get(*index) else {
                            continue;
                        };
                        for (template, field) in variant.payload.iter().zip(fields.iter().skip(1)) {
                            self.collect_provenance_variables(template, field, &mut collected);
                        }
                        if let Some(found) = collected.remove(variable) {
                            sources.extend(found);
                        }
                    }
                    if sources.is_empty() {
                        (Provenance::RequiresProof, actual)
                    } else {
                        (self.common_constructor_provenance(&sources), actual)
                    }
                }
                _ => (Provenance::RequiresProof, actual),
            },
            Provenance::Sequence(elements)
                if matches!(
                    projection,
                    Projection::Element | Projection::TypeArgument(_)
                ) =>
            {
                if elements.is_empty() {
                    (Provenance::RequiresProof, actual)
                } else {
                    (self.common_constructor_provenance(elements), actual)
                }
            }
            Provenance::Template { ty, variables } => {
                let concrete = self.concrete_template(ty, &source.1);
                if let Some(template) =
                    self.projection_type(concrete.as_ref().unwrap_or(ty), projection)
                {
                    self.template_provenance(&template, &actual, variables)
                } else {
                    (Provenance::RequiresProof, actual)
                }
            }
            Provenance::Parameter(_) | Provenance::Projection { .. } => (
                Provenance::Projection {
                    source: Box::new(source.clone()),
                    projection: projection.clone(),
                },
                actual,
            ),
            _ => (Provenance::RequiresProof, actual),
        }
    }

    fn projection_type(&self, ty: &Ty, projection: &Projection) -> Option<Ty> {
        match (ty, projection) {
            (Ty::Tuple(items), Projection::Field { index, .. }) => items.get(*index).cloned(),
            (Ty::Struct(_, fields) | Ty::Record(_, fields), Projection::Field { index, .. }) => {
                fields.get(*index).map(|(_, ty)| ty.clone())
            }
            (Ty::List(element), Projection::Element) => Some(element.as_ref().clone()),
            (Ty::Result(ok, error), Projection::Field { index: 0, tag }) => {
                Some(if *tag == Some(1) { error } else { ok }.as_ref().clone())
            }
            (Ty::Enum(_, arguments), Projection::TypeArgument(index)) => {
                arguments.get(*index).cloned()
            }
            (
                Ty::Enum(name, arguments),
                Projection::Field {
                    index,
                    tag: Some(tag),
                },
            ) => {
                let variant = self
                    .env
                    .enum_variants_of(name)?
                    .iter()
                    .find(|variant| variant.tag == *tag)?;
                if *index == 0 {
                    return Some(Ty::Int);
                }
                let Ty::Enum(_, parameters) = &variant.enum_ty else {
                    return None;
                };
                let mapping = parameters
                    .iter()
                    .zip(arguments)
                    .filter_map(|(parameter, argument)| {
                        if let Ty::Var(variable) = parameter {
                            Some((*variable, argument.clone()))
                        } else {
                            None
                        }
                    })
                    .collect();
                variant
                    .payload
                    .get(index - 1)
                    .map(|payload| self.substitute_ty_with_mapping(payload, &mapping))
            }
            _ => None,
        }
    }

    fn concrete_template(&self, template: &Ty, actual: &Ty) -> Option<Ty> {
        let Ty::SelfApp(items) = template else {
            return None;
        };
        let (_, arguments) = Self::constructor_application_parts(items)?;
        let capability = self.constructor_capability_for_type(template)?;
        self.expand_constructor_template(&capability, actual, arguments)
    }

    fn expand_constructor_template(
        &self,
        capability: &str,
        actual: &Ty,
        arguments: &[Ty],
    ) -> Option<Ty> {
        let (implementation, mut mapping) =
            self.constructor_projection(capability, &self.resolve_ty(actual))?;
        if implementation.constructor_slot_vars.len() != arguments.len() {
            return None;
        }
        for (variable, argument) in implementation.constructor_slot_vars.iter().zip(arguments) {
            mapping.insert(*variable, argument.clone());
        }
        let target = implementation
            .head_type_list
            .entries
            .iter()
            .find(|entry| entry.role == TypeListRole::ImplTarget)?;
        let target = self.canonical_to_ty(&target.ty).ok()?;
        Some(self.substitute_ty_with_mapping(&target, &mapping))
    }

    fn collect_provenance_variables(
        &self,
        template: &Ty,
        source: &Source,
        variables: &mut HashMap<u32, Vec<Source>>,
    ) {
        match template {
            Ty::SelfApp(_) => {
                if let Some(concrete) = self.concrete_template(template, &source.1) {
                    self.collect_provenance_variables(&concrete, source, variables);
                }
            }
            Ty::Var(variable) => {
                variables.entry(*variable).or_default().push(source.clone());
            }
            Ty::List(element) => self.collect_provenance_variables(
                element,
                &self.project_provenance(source, &Projection::Element, element),
                variables,
            ),
            Ty::Tuple(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.collect_provenance_variables(
                        item,
                        &self.project_provenance(
                            source,
                            &Projection::Field { index, tag: None },
                            item,
                        ),
                        variables,
                    );
                }
            }
            Ty::Struct(_, fields) | Ty::Record(_, fields) => {
                for (index, (_, item)) in fields.iter().enumerate() {
                    self.collect_provenance_variables(
                        item,
                        &self.project_provenance(
                            source,
                            &Projection::Field { index, tag: None },
                            item,
                        ),
                        variables,
                    );
                }
            }
            Ty::Result(ok, error) => {
                for (tag, item) in [(0, ok.as_ref()), (1, error.as_ref())] {
                    self.collect_provenance_variables(
                        item,
                        &self.project_provenance(
                            source,
                            &Projection::Field {
                                index: 0,
                                tag: Some(tag),
                            },
                            item,
                        ),
                        variables,
                    );
                }
            }
            Ty::Enum(_, arguments) => {
                for (index, argument) in arguments.iter().enumerate() {
                    self.collect_provenance_variables(
                        argument,
                        &self.project_provenance(
                            source,
                            &Projection::TypeArgument(index),
                            argument,
                        ),
                        variables,
                    );
                }
            }
            _ => {}
        }
    }

    pub(super) fn bind_constructor_provenance(&mut self, pattern: &TypedPattern, source: Source) {
        let mut bindings = Bindings::new();
        self.pattern_provenance_bindings(pattern, &source, &mut bindings);
        for (id, (provenance, _)) in bindings {
            self.constructor_capabilities.insert(id, provenance);
        }
    }

    pub(super) fn bind_match_constructor_provenance(
        &mut self,
        pattern: &TypedMatchPattern,
        source: Source,
    ) {
        let mut bindings = Bindings::new();
        self.match_provenance_bindings(pattern, &source, &mut bindings);
        for (id, (provenance, _)) in bindings {
            self.constructor_capabilities.insert(id, provenance);
        }
    }

    pub(super) fn result_constructor_provenance(&self, node: &TypedNode) -> Source {
        self.safebind_source_provenance(&self.source_provenance(node, &Bindings::new()))
    }

    fn safebind_source_provenance(&self, source: &Source) -> Source {
        if matches!(self.resolve_ty(&source.1), Ty::Result(..)) {
            self.project_provenance(
                source,
                &Projection::Field {
                    index: 0,
                    tag: Some(0),
                },
                &Ty::Hole,
            )
        } else {
            source.clone()
        }
    }

    fn pattern_provenance_bindings(
        &self,
        pattern: &TypedPattern,
        source: &Source,
        bindings: &mut Bindings,
    ) {
        match pattern {
            TypedPattern::Var(_, id) => {
                bindings.insert(id.unique_id, source.clone());
            }
            TypedPattern::As(_, inner, id) => {
                bindings.insert(id.unique_id, source.clone());
                self.pattern_provenance_bindings(inner, source, bindings);
            }
            TypedPattern::Tuple(_, items) => {
                for (index, item) in items.iter().enumerate() {
                    self.pattern_provenance_bindings(
                        item,
                        &self.project_provenance(
                            source,
                            &Projection::Field { index, tag: None },
                            &Ty::Hole,
                        ),
                        bindings,
                    );
                }
            }
            TypedPattern::ListCons(_, head, tail) => {
                self.pattern_provenance_bindings(
                    head,
                    &self.project_provenance(source, &Projection::Element, &Ty::Hole),
                    bindings,
                );
                self.pattern_provenance_bindings(tail, source, bindings);
            }
            TypedPattern::ResultOk(_, inner) => self.pattern_provenance_bindings(
                inner,
                &self.project_provenance(
                    source,
                    &Projection::Field {
                        index: 0,
                        tag: Some(0),
                    },
                    &Ty::Hole,
                ),
                bindings,
            ),
            _ => {}
        }
    }

    fn match_provenance_bindings(
        &self,
        pattern: &TypedMatchPattern,
        source: &Source,
        bindings: &mut Bindings,
    ) {
        match pattern {
            TypedMatchPattern::Binding(id) => {
                bindings.insert(id.unique_id, source.clone());
            }
            TypedMatchPattern::As(inner, id) => {
                bindings.insert(id.unique_id, source.clone());
                self.match_provenance_bindings(inner, source, bindings);
            }
            TypedMatchPattern::Tuple(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.match_provenance_bindings(
                        item,
                        &self.project_provenance(
                            source,
                            &Projection::Field { index, tag: None },
                            &Ty::Hole,
                        ),
                        bindings,
                    );
                }
            }
            TypedMatchPattern::ListCons(head, tail) => {
                self.match_provenance_bindings(
                    head,
                    &self.project_provenance(source, &Projection::Element, &Ty::Hole),
                    bindings,
                );
                self.match_provenance_bindings(tail, source, bindings);
            }
            TypedMatchPattern::Constructor {
                tag,
                fields,
                field_offset,
            } => {
                for (index, field) in fields.iter().enumerate() {
                    self.match_provenance_bindings(
                        field,
                        &self.project_provenance(
                            source,
                            &Projection::Field {
                                index: index + *field_offset as usize,
                                tag: Some(*tag),
                            },
                            &Ty::Hole,
                        ),
                        bindings,
                    );
                }
            }
            _ => {}
        }
    }
}
