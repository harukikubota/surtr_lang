use super::*;
use sindr::derive::{derive_trait_meta, DeriveGenerator, DeriveTraitMeta, FieldTraitRequirement};
use spire::ast::{
    AstMatchArm, AstPath, AstPattern, AstTy, DeclAttrs, EnumVariant, FunParam, Lit, RecordLitArg,
    Span, TypeParam, WhereClause, WhereConstraint, WhereConstraintRhs,
};

pub(super) fn expand_derive_annotations(stmts: Vec<Ast>) -> Result<Vec<Ast>, ResolveError> {
    let mut expanded = Vec::with_capacity(stmts.len());
    for stmt in stmts {
        let (base, derives) = match &stmt {
            Ast::StructDef(_, _, _, _, attrs)
            | Ast::RecordDef(_, _, _, attrs)
            | Ast::EnumDef(_, _, _, _, attrs) => (stmt.clone(), attrs.derives.clone()),
            Ast::DeferrorDef(span, _, _, _, attrs) if !attrs.derives.is_empty() => {
                return Err(ResolveError {
                    message: "DeriveNotAllowed: @derive is not allowed on deferror".into(),
                    span: span.clone(),
                    related_labels: Vec::new(),
                })
            }
            _ => {
                let has_derive = match &stmt {
                    Ast::Def(_, _, _, _, _, _, _, attrs)
                    | Ast::Defmod(_, _, _, attrs)
                    | Ast::TraitDef(_, _, _, _, _, attrs)
                    | Ast::ImplDef(_, _, _, attrs)
                    | Ast::BuiltinTypeDecl(_, _, attrs)
                    | Ast::TraitImplDef(_, _, _, _, _, _, attrs) => !attrs.derives.is_empty(),
                    _ => false,
                };
                if has_derive {
                    return Err(ResolveError {
                        message: "DeriveNotAllowed: @derive is only allowed on data declarations"
                            .into(),
                        span: stmt.span().clone(),
                        related_labels: Vec::new(),
                    });
                }
                (stmt.clone(), Vec::new())
            }
        };
        expanded.push(base.clone());
        if derives.is_empty() {
            continue;
        }
        let (name, type_params, fields, variants, span) = match &base {
            Ast::StructDef(span, name, params, fields, _) => (
                name.clone(),
                params.clone(),
                fields
                    .iter()
                    .map(|field| (field.name.clone(), field.ty.clone()))
                    .collect(),
                Vec::new(),
                span.clone(),
            ),
            Ast::RecordDef(span, name, fields, _) => (
                name.clone(),
                Vec::new(),
                fields
                    .iter()
                    .map(|field| (field.name.clone(), field.ty.clone()))
                    .collect(),
                Vec::new(),
                span.clone(),
            ),
            Ast::EnumDef(span, name, params, variants, _) => (
                name.clone(),
                params.clone(),
                Vec::new(),
                variants.clone(),
                span.clone(),
            ),
            _ => unreachable!(),
        };
        let mut metas = Vec::new();
        let mut names = Vec::new();
        for derive_name in derives {
            let meta = derive_trait_meta(&derive_name).ok_or_else(|| ResolveError {
                message: format!("UnknownDerivedTrait: {}", derive_name),
                span: span.clone(),
                related_labels: Vec::new(),
            })?;
            if names
                .iter()
                .any(|name: &String| name == meta.trait_name.as_str())
            {
                return Err(ResolveError {
                    message: format!("DuplicateDerivedTrait: {}", derive_name),
                    span: span.clone(),
                    related_labels: Vec::new(),
                });
            }
            names.push(meta.trait_name.as_str().to_string());
            metas.push(meta);
        }
        for meta in metas {
            if meta.default_variant.is_some() && !matches!(meta.generator, DeriveGenerator::Default) {
                return Err(ResolveError {
                    message: format!("DeriveVariantNotAllowed: {}", meta.trait_name.as_str()),
                    span: span.clone(),
                    related_labels: Vec::new(),
                });
            }
            if let Some(variant) = &meta.default_variant {
                if variants.iter().all(|candidate| candidate.name != *variant) {
                    return Err(ResolveError {
                        message: format!("UnknownDefaultVariant: {}", variant),
                        span: span.clone(),
                        related_labels: Vec::new(),
                    });
                }
            }
            expanded.push(make_derived_impl(
                &name,
                &type_params,
                &fields,
                &variants,
                &span,
                &meta,
            ));
        }
    }
    Ok(expanded)
}

fn named(span: &Span, name: &str) -> AstTy {
    AstTy::Named(span.clone(), name.into())
}

fn path(span: &Span, segments: &[&str]) -> Ast {
    Ast::Path(
        span.clone(),
        AstPath {
            span: span.clone(),
            segments: segments.iter().map(|segment| (*segment).into()).collect(),
        },
    )
}

fn var(span: &Span, name: &str) -> Ast {
    Ast::Var(span.clone(), name.into())
}

fn call(span: &Span, segments: &[&str], args: Vec<Ast>) -> Ast {
    Ast::App(
        span.clone(),
        Box::new(path(span, segments)),
        args.into_iter().map(RecordLitArg::Positional).collect(),
    )
}

fn constructor(span: &Span, name: &str, args: Vec<Ast>) -> Ast {
    Ast::ConstructorCall(
        span.clone(),
        name.into(),
        args.into_iter().map(RecordLitArg::Positional).collect(),
    )
}

fn field(span: &Span, receiver: &str, name: &str) -> Ast {
    Ast::FieldAccess(span.clone(), Box::new(var(span, receiver)), name.into())
}

fn fold_and(span: &Span, values: Vec<Ast>) -> Ast {
    values
        .into_iter()
        .reduce(|left, right| {
            Ast::App(
                span.clone(),
                Box::new(Ast::Var(span.clone(), "&&".into())),
                vec![
                    RecordLitArg::Positional(left),
                    RecordLitArg::Positional(right),
                ],
            )
        })
        .unwrap_or_else(|| Ast::Lit(span.clone(), Lit::Bool(true)))
}

fn lexicographic(span: &Span, comparisons: Vec<Ast>) -> Ast {
    comparisons
        .into_iter()
        .rev()
        .reduce(|next, comparison| {
            Ast::Match(
                span.clone(),
                Box::new(comparison),
                vec![
                    AstMatchArm {
                        pattern: AstPattern::Constructor(
                            span.clone(),
                            "Ordering::Equal".into(),
                            Vec::new(),
                        ),
                        guard: None,
                        body: next,
                    },
                    AstMatchArm {
                        pattern: AstPattern::Var(span.clone(), "__derive_order".into()),
                        guard: None,
                        body: var(span, "__derive_order"),
                    },
                ],
            )
        })
        .unwrap_or_else(|| path(span, &["Ordering", "Equal"]))
}

fn enum_body(
    span: &Span,
    type_name: &str,
    variants: &[EnumVariant],
    generator: DeriveGenerator,
) -> Ast {
    let mut arms = Vec::new();
    for (left_index, left_variant) in variants.iter().enumerate() {
        for (right_index, right_variant) in variants.iter().enumerate() {
            let left_names = (0..left_variant.payload.len())
                .map(|index| format!("__derive_l{}_{}", left_index, index))
                .collect::<Vec<_>>();
            let right_names = (0..right_variant.payload.len())
                .map(|index| format!("__derive_r{}_{}", right_index, index))
                .collect::<Vec<_>>();
            let left_pattern = AstPattern::Constructor(
                span.clone(),
                format!("{}::{}", type_name, left_variant.name),
                left_names
                    .iter()
                    .map(|name| AstPattern::Var(span.clone(), name.clone()))
                    .collect(),
            );
            let right_pattern = AstPattern::Constructor(
                span.clone(),
                format!("{}::{}", type_name, right_variant.name),
                right_names
                    .iter()
                    .map(|name| AstPattern::Var(span.clone(), name.clone()))
                    .collect(),
            );
            let body = if left_index != right_index {
                match generator {
                    DeriveGenerator::StructuralEq => Ast::Lit(span.clone(), Lit::Bool(false)),
                    DeriveGenerator::LexicographicCompare => path(
                        span,
                        if left_index < right_index {
                            &["Ordering", "Less"]
                        } else {
                            &["Ordering", "Greater"]
                        },
                    ),
                    DeriveGenerator::Default => constructor(
                        span,
                        &format!("{}::{}", type_name, left_variant.name),
                        left_names
                            .iter()
                            .map(|_| call(span, &["Default", "default"], Vec::new()))
                            .collect(),
                    ),
                    _ => Ast::Lit(span.clone(), Lit::Bool(true)),
                }
            } else {
                match generator {
                    DeriveGenerator::StructuralEq => fold_and(
                        span,
                        left_names
                            .iter()
                            .zip(right_names.iter())
                            .map(|(left, right)| {
                                call(span, &["Eq", "eq"], vec![var(span, left), var(span, right)])
                            })
                            .collect(),
                    ),
                    DeriveGenerator::LexicographicCompare => lexicographic(
                        span,
                        left_names
                            .iter()
                            .zip(right_names.iter())
                            .map(|(left, right)| {
                                call(
                                    span,
                                    &["Compare", "compare"],
                                    vec![var(span, left), var(span, right)],
                                )
                            })
                            .collect(),
                    ),
                    DeriveGenerator::Default => constructor(
                        span,
                        &format!("{}::{}", type_name, left_variant.name),
                        left_names
                            .iter()
                            .map(|_| call(span, &["Default", "default"], Vec::new()))
                            .collect(),
                    ),
                    _ => Ast::Lit(span.clone(), Lit::Bool(true)),
                }
            };
            arms.push(AstMatchArm {
                pattern: AstPattern::Tuple(span.clone(), vec![left_pattern, right_pattern]),
                guard: None,
                body,
            });
        }
    }
    arms.push(AstMatchArm {
        pattern: AstPattern::Wildcard(span.clone()),
        guard: None,
        body: match generator {
            DeriveGenerator::StructuralEq => Ast::Lit(span.clone(), Lit::Bool(false)),
            DeriveGenerator::LexicographicCompare => path(span, &["Ordering", "Equal"]),
            DeriveGenerator::Default => Ast::Lit(span.clone(), Lit::Bool(false)),
            _ => Ast::Lit(span.clone(), Lit::Bool(false)),
        },
    });
    Ast::Match(
        span.clone(),
        Box::new(Ast::TupleLiteral(
            span.clone(),
            vec![var(span, "self"), var(span, "rhs")],
        )),
        arms,
    )
}

fn make_derived_impl(
    name: &str,
    type_params: &[TypeParam],
    fields: &[(String, AstTy)],
    variants: &[EnumVariant],
    span: &Span,
    meta: &DeriveTraitMeta,
) -> Ast {
    let generator = meta.generator;
    let (method_name, return_type, body) = match generator {
        DeriveGenerator::StructuralEq => (
            "eq",
            "Boolean",
            if variants.is_empty() {
                fold_and(
                    span,
                    fields
                        .iter()
                        .map(|(name, _)| {
                            call(
                                span,
                                &["Eq", "eq"],
                                vec![field(span, "self", name), field(span, "rhs", name)],
                            )
                        })
                        .collect(),
                )
            } else {
                enum_body(span, name, variants, generator)
            },
        ),
        DeriveGenerator::LexicographicCompare => (
            "compare",
            "Ordering",
            if variants.is_empty() {
                lexicographic(
                    span,
                    fields
                        .iter()
                        .map(|(name, _)| {
                            call(
                                span,
                                &["Compare", "compare"],
                                vec![field(span, "self", name), field(span, "rhs", name)],
                            )
                        })
                        .collect(),
                )
            } else {
                enum_body(span, name, variants, generator)
            },
        ),
        DeriveGenerator::InspectShow => (
            "to_string",
            "String",
            call(span, &["inspect"], vec![var(span, "self")]),
        ),
        DeriveGenerator::Default => {
            let body = if variants.is_empty() {
                constructor(
                    span,
                    name,
                    fields
                        .iter()
                        .map(|_| call(span, &["Default", "default"], Vec::new()))
                        .collect(),
                )
            } else {
                let variant = meta
                    .default_variant
                    .as_deref()
                    .and_then(|wanted| variants.iter().find(|candidate| candidate.name == wanted))
                    .unwrap_or(&variants[0]);
                constructor(
                    span,
                    &format!("{}::{}", name, variant.name),
                    variant
                        .payload
                        .iter()
                        .map(|_| call(span, &["Default", "default"], Vec::new()))
                        .collect(),
                )
            };
            ("default", "Self", body)
        }
    };
    let target = if type_params.is_empty() {
        named(span, name)
    } else {
        AstTy::Generic(
            span.clone(),
            name.into(),
            type_params
                .iter()
                .map(|param| named(span, &param.name))
                .collect(),
        )
    };
    let where_clause = match &meta.field_requirement {
        FieldTraitRequirement::None => None,
        FieldTraitRequirement::RequiresTrait(trait_name) => Some(WhereClause {
            constraints: type_params
                .iter()
                .map(|param| WhereConstraint {
                    subject: named(span, &param.name),
                    bounds: vec![WhereConstraintRhs::Trait(
                        span.clone(),
                        trait_name.as_str().into(),
                    )],
                    span: span.clone(),
                })
                .collect(),
            span: span.clone(),
        }),
    };
    let mut params = Vec::new();
    if !matches!(generator, DeriveGenerator::InspectShow | DeriveGenerator::Default) {
        params.push(FunParam {
            name: "self".into(),
            ty: named(span, "Self"),
            span: span.clone(),
        });
        params.push(FunParam {
            name: "rhs".into(),
            ty: named(span, "Self"),
            span: span.clone(),
        });
    } else if generator == DeriveGenerator::InspectShow {
        params.push(FunParam {
            name: "self".into(),
            ty: named(span, "Self"),
            span: span.clone(),
        });
    }
    let mut generated_attrs = DeclAttrs::default();
    generated_attrs.fun_params = vec![named(span, "Self")];
    let mut methods = vec![Ast::Def(
        span.clone(),
        method_name.into(),
        Vec::new(),
        params.clone(),
        Some(named(span, return_type)),
        None,
        Box::new(Ast::Block(span.clone(), vec![body.clone()])),
        generated_attrs.clone(),
    )];
    if generator == DeriveGenerator::StructuralEq {
        let neq_body = call(
            span,
            &["if"],
            vec![
                body.clone(),
                Ast::Lit(span.clone(), Lit::Bool(false)),
                Ast::Lit(span.clone(), Lit::Bool(true)),
            ],
        );
        methods.push(Ast::Def(
            span.clone(),
            "neq".into(),
            Vec::new(),
            params,
            Some(named(span, "Boolean")),
            None,
            Box::new(Ast::Block(span.clone(), vec![neq_body])),
            generated_attrs,
        ));
    }
    Ast::TraitImplDef(
        span.clone(),
        meta.trait_name.as_str().into(),
        Vec::new(),
        target,
        where_clause,
        methods,
        DeclAttrs {
            compiler_generated: true,
            ..DeclAttrs::default()
        },
    )
}
