use crate::names::{builtin_type_name, surface_path_name, TypeIdentity, TypeName};
pub use crate::signature::BuiltinId;
use crate::signature::{
    CallableDeclarationKind, CallableIdentity, CallableSignature, CanonicalConstraint,
    CanonicalConstraintSet, CanonicalReturnTypeArgument, CanonicalTypeOccurrence,
    CanonicalValueParameter, RuntimeTarget, SignatureOrigin, ValueParameterMode,
};

/// Built-in function metadata shared across Sigil / Scar / Forge / Eldr.
///
/// Surtr source files under `lib/*.srt` may declare these builtins with
/// `@builtin`, but the canonical definition order and ids live here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinMeta {
    pub name: &'static str,
    pub arity: u8,
    /// Type signature string used by type checker bootstrap and validation.
    pub sig_str: &'static str,
    /// Canonical source-level callable identities backed by this runtime
    /// builtin. An empty list means the runtime entry has no source surface.
    pub surfaces: &'static [BuiltinSurfaceSpec],
    /// Exact compiler-generated declaration identities backed by this runtime
    /// builtin. These are not source surfaces and therefore carry no callable
    /// signature of their own.
    pub compiler_generated_surfaces: &'static [BuiltinGeneratedSurfaceSpec],
}

/// A compiler-generated declaration identity stored on canonical runtime
/// metadata. Sigil accepts these declarations only when the AST provenance is
/// compiler-generated and this exact owner/name pair is registered here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinGeneratedSurfaceSpec {
    pub owner: &'static str,
    pub name: &'static str,
}

/// A canonical source-level callable identity and signature stored on its
/// runtime metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinSurfaceSpec {
    pub owner: Option<&'static str>,
    pub name: &'static str,
    pub return_type_arguments: &'static [&'static str],
    pub value_parameters: &'static [BuiltinSurfaceParameterSpec],
    pub return_type: &'static str,
    pub where_constraints: &'static [BuiltinSurfaceConstraintSpec],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinSurfaceParameterSpec {
    pub name: &'static str,
    pub mode: ValueParameterMode,
    pub ty: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinSurfaceConstraintSpec {
    pub subject: &'static str,
    pub trait_name: &'static str,
}

const fn builtin_surface_spec(
    owner: Option<&'static str>,
    name: &'static str,
    return_type_arguments: &'static [&'static str],
    value_parameters: &'static [BuiltinSurfaceParameterSpec],
    return_type: &'static str,
    where_constraints: &'static [BuiltinSurfaceConstraintSpec],
) -> BuiltinSurfaceSpec {
    BuiltinSurfaceSpec {
        owner,
        name,
        return_type_arguments,
        value_parameters,
        return_type,
        where_constraints,
    }
}

const fn builtin_generated_surface_spec(
    owner: &'static str,
    name: &'static str,
) -> BuiltinGeneratedSurfaceSpec {
    BuiltinGeneratedSurfaceSpec { owner, name }
}

const fn builtin_surface_parameter(
    name: &'static str,
    ty: &'static str,
) -> BuiltinSurfaceParameterSpec {
    BuiltinSurfaceParameterSpec {
        name,
        mode: ValueParameterMode::PositionalOrNamed,
        ty,
    }
}

const fn builtin_surface_constraint(
    subject: &'static str,
    trait_name: &'static str,
) -> BuiltinSurfaceConstraintSpec {
    BuiltinSurfaceConstraintSpec {
        subject,
        trait_name,
    }
}

/// Runtime builtin function metadata. `builtin_id` is still derived from
/// definition order to preserve existing bytecode encoding.
pub type BuiltinFunctionMeta = BuiltinMeta;

/// A complete surface callable view of one runtime builtin.
///
/// One runtime entry may expose multiple values in the language surface. A
/// surface variant owns its callable identity and signature; only the
/// `runtime_target` is shared by those variants.
pub type BuiltinSurfaceSignatureMeta = CallableSignature<String>;

/// A canonical Trait implementation surface of a runtime entry. Source
/// declarations resolve this metadata once; candidate selection keeps the id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinTraitMethodMeta {
    pub trait_name: &'static str,
    pub method_name: &'static str,
    pub targets: &'static [TypeName],
    pub builtin_id: BuiltinId,
}

impl BuiltinMeta {
    /// Optional direct lowering for a runtime primitive. The runtime entry
    /// remains its canonical dispatch identity even when Forge emits an opcode.
    pub fn primitive_opcode(&self) -> Option<crate::ir::Opcode> {
        use crate::ir::Opcode;
        Some(match self.name {
            "__operator_int_add" => Opcode::AddInt,
            "__operator_int_sub" => Opcode::SubInt,
            "__operator_int_mul" => Opcode::MulInt,
            "__operator_float_add" => Opcode::AddFloat,
            "__operator_float_sub" => Opcode::SubFloat,
            "__operator_float_mul" => Opcode::MulFloat,
            "__operator_int_eq" => Opcode::EqInt,
            "__operator_int_neq" => Opcode::NeqInt,
            "__operator_int_lt" => Opcode::LtInt,
            "__operator_int_lte" => Opcode::LteInt,
            "__operator_int_gt" => Opcode::GtInt,
            "__operator_int_gte" => Opcode::GteInt,
            "__operator_float_eq" => Opcode::EqFloat,
            "__operator_float_neq" => Opcode::NeqFloat,
            "__operator_float_lt" => Opcode::LtFloat,
            "__operator_float_lte" => Opcode::LteFloat,
            "__operator_float_gt" => Opcode::GtFloat,
            "__operator_float_gte" => Opcode::GteFloat,
            "__operator_string_eq" => Opcode::EqStr,
            "__operator_string_neq" => Opcode::NeqStr,
            "__operator_boolean_eq" => Opcode::EqBool,
            "__operator_boolean_neq" => Opcode::NeqBool,
            "__operator_string_concat" => Opcode::ConcatStr,
            _ => return None,
        })
    }

    pub fn trait_method(&self) -> Option<BuiltinTraitMethodMeta> {
        let (trait_name, method_name, targets): (&str, &str, &'static [TypeName]) = match self.name
        {
            "to_string" => (
                "Show",
                "to_string",
                &[
                    TypeName::Int,
                    TypeName::Float,
                    TypeName::String,
                    TypeName::Boolean,
                    TypeName::Unit,
                    TypeName::Error,
                ],
            ),
            "__operator_int_add" => ("Add", "add", &[TypeName::Int]),
            "__operator_float_add" => ("Add", "add", &[TypeName::Float]),
            "__operator_int_sub" => ("Sub", "sub", &[TypeName::Int]),
            "__operator_float_sub" => ("Sub", "sub", &[TypeName::Float]),
            "__operator_int_mul" => ("Mul", "mul", &[TypeName::Int]),
            "__operator_float_mul" => ("Mul", "mul", &[TypeName::Float]),
            "__operator_int_eq" => ("Eq", "eq", &[TypeName::Int]),
            "__operator_float_eq" => ("Eq", "eq", &[TypeName::Float]),
            "__operator_string_eq" => ("Eq", "eq", &[TypeName::String]),
            "__operator_boolean_eq" => ("Eq", "eq", &[TypeName::Boolean]),
            "__operator_int_neq" => ("Eq", "neq", &[TypeName::Int]),
            "__operator_float_neq" => ("Eq", "neq", &[TypeName::Float]),
            "__operator_string_neq" => ("Eq", "neq", &[TypeName::String]),
            "__operator_boolean_neq" => ("Eq", "neq", &[TypeName::Boolean]),
            "__compare_int" => ("Compare", "compare", &[TypeName::Int]),
            "__compare_float" => ("Compare", "compare", &[TypeName::Float]),
            "__operator_int_lt" => ("Compare", "lt", &[TypeName::Int]),
            "__operator_float_lt" => ("Compare", "lt", &[TypeName::Float]),
            "__operator_int_lte" => ("Compare", "lte", &[TypeName::Int]),
            "__operator_float_lte" => ("Compare", "lte", &[TypeName::Float]),
            "__operator_int_gt" => ("Compare", "gt", &[TypeName::Int]),
            "__operator_float_gt" => ("Compare", "gt", &[TypeName::Float]),
            "__operator_int_gte" => ("Compare", "gte", &[TypeName::Int]),
            "__operator_float_gte" => ("Compare", "gte", &[TypeName::Float]),
            "__operator_string_concat" => ("Concat", "concat", &[TypeName::String]),
            "__facet_chain" => ("Compose", "compose", &[TypeName::Facet]),
            _ => return None,
        };
        Some(BuiltinTraitMethodMeta {
            trait_name,
            method_name,
            targets,
            builtin_id: self.builtin_id(),
        })
    }
}

impl BuiltinMeta {
    /// Arity at the VM boundary. Kept as a method so callers do not need to
    /// infer it from a surface variant (variants may be qualified aliases).
    pub const fn runtime_arity(&self) -> u8 {
        self.arity
    }

    /// Runtime target shared by every surface variant of this entry.
    pub fn runtime_target(&self) -> RuntimeTarget {
        RuntimeTarget::Builtin(BuiltinId(self.runtime_id()))
    }

    /// Definition-order id as a typed internal identifier.
    pub fn builtin_id(&self) -> BuiltinId {
        BuiltinId(self.runtime_id())
    }

    /// Definition-order id used by the runtime implementation table.
    pub fn runtime_id(&self) -> u16 {
        BUILTIN_METAS
            .iter()
            .position(|candidate| std::ptr::eq(candidate, self))
            .and_then(|index| u16::try_from(index).ok())
            .unwrap_or_else(|| {
                // This branch is only reachable for a caller that constructed
                // a standalone BuiltinMeta. Metadata in BUILTIN_METAS always
                // has a stable table identity.
                builtin_id_by_name(self.name).unwrap_or(u16::MAX)
            })
    }

    /// Build all callable identities exposed by this runtime entry.
    ///
    /// The returned values are owned because Scar enriches them with source
    /// provenance when it validates a `@builtin def` declaration. This is
    /// intentionally derived from this table rather than from declaration
    /// order in `lib/*.srt`.
    pub fn surface_variants(&self) -> Vec<BuiltinSurfaceSignatureMeta> {
        self.surfaces
            .iter()
            .map(|surface| self.surface_signature(surface))
            .collect()
    }

    /// Find a surface variant by canonical owner and surface name.
    pub fn surface_variant(&self, owner: &str, name: &str) -> Option<BuiltinSurfaceSignatureMeta> {
        self.surface_variants().into_iter().find(|variant| {
            variant.identity.owner.as_deref().unwrap_or("") == owner
                && variant.identity.name == name
        })
    }

    fn surface_signature(&self, surface: &BuiltinSurfaceSpec) -> BuiltinSurfaceSignatureMeta {
        let value_parameters = surface
            .value_parameters
            .iter()
            .enumerate()
            .map(|(ordinal, parameter)| CanonicalValueParameter {
                ordinal: ordinal as u32,
                name: parameter.name.into(),
                mode: parameter.mode,
                ty: parameter.ty.into(),
                origin: SignatureOrigin::new(format!(
                    "builtin {} parameter {}",
                    surface.name, ordinal
                )),
            })
            .collect::<Vec<_>>();
        let identity = CallableIdentity {
            owner: surface.owner.map(str::to_string),
            name: surface.name.to_string(),
            declaration_kind: CallableDeclarationKind::Builtin,
        };
        let return_type_arguments = surface
            .return_type_arguments
            .iter()
            .enumerate()
            .map(|(ordinal, ty)| CanonicalReturnTypeArgument {
                ordinal: ordinal as u32,
                ty: (*ty).into(),
                origin: SignatureOrigin::new(format!(
                    "builtin {} return type argument {}",
                    surface.name, ordinal
                )),
            })
            .collect();
        let where_constraints = CanonicalConstraintSet {
            constraints: surface
                .where_constraints
                .iter()
                .map(|constraint| CanonicalConstraint {
                    subject: constraint.subject.into(),
                    trait_name: constraint.trait_name.into(),
                    origin: SignatureOrigin::new(format!(
                        "builtin {} where constraint",
                        surface.name
                    )),
                })
                .collect(),
        };
        CallableSignature {
            identity,
            return_type_arguments,
            value_parameters,
            return_type: CanonicalTypeOccurrence {
                ty: surface.return_type.into(),
                origin: SignatureOrigin::new(format!("builtin {} return", surface.name)),
            },
            where_constraints,
            runtime_target: self.runtime_target(),
            declaration_origins: vec![SignatureOrigin::new(format!(
                "builtin metadata {}",
                self.name
            ))],
        }
    }
}

#[cfg(test)]
fn parse_surface_signature(signature: &str) -> Option<(Vec<String>, String)> {
    let arrow = find_top_level_arrow(signature)?;
    let (params, return_type) = signature.split_at(arrow);
    let return_type = return_type.strip_prefix("->")?;
    let params = params.trim().strip_prefix('(')?.strip_suffix(')')?;
    let parameters = split_top_level(params, ',');
    Some((parameters, return_type.trim().to_string()))
}

#[cfg(test)]
fn find_top_level_arrow(input: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut characters = input.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        match character {
            '<' | '(' => depth += 1,
            '>' | ')' => depth = depth.saturating_sub(1),
            '-' if depth == 0 && characters.peek().is_some_and(|(_, next)| *next == '>') => {
                return Some(index);
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
fn split_top_level(input: &str, separator: char) -> Vec<String> {
    if input.trim().is_empty() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for (index, character) in input.char_indices() {
        match character {
            '<' | '(' => depth += 1,
            '>' | ')' => depth = depth.saturating_sub(1),
            _ if character == separator && depth == 0 => {
                result.push(input[start..index].trim().to_string());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    result.push(input[start..].trim().to_string());
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinTypeMeta {
    /// Canonical builtin type head that std-module `@builtin type`
    /// declarations must match exactly.
    pub name: &'static str,
    pub params: &'static [&'static str],
    pub identity: TypeIdentity,
}

/// Builtin type declaration-head metadata accepted by standard sources.
pub type BuiltinTypeHeadMeta = BuiltinTypeMeta;

/// Standard-library owners whose identities are not declared with `@builtin type`.
///
/// This is separate from [`BUILTIN_TYPE_METAS`], which validates only compiler
/// builtin type declaration heads. Resolver owner registration uses this table
/// for standard declarations such as `defenum Option<$T>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandardOwnerIdentityMeta {
    pub name: &'static str,
    pub identity: TypeIdentity,
}

pub const STANDARD_OWNER_IDENTITY_METAS: &[StandardOwnerIdentityMeta] =
    &[StandardOwnerIdentityMeta {
        name: "Option",
        identity: TypeIdentity::TypeConstructor,
    }];

/// Builtin unique ids start after the first two scope-reserved ids.
pub const BUILTIN_UID_BASE: u32 = 2;

pub const BUILTIN_METAS: &[BuiltinMeta] = &[
    BuiltinMeta {
        name: "print",
        arity: 1,
        sig_str: "(String) -> Unit",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Kernel"),
                "print",
                &[],
                &[
                    builtin_surface_parameter("a", "String"),
                ],
                "Unit",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "to_string",
        arity: 1,
        sig_str: "($A) -> String",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "inspect",
        arity: 1,
        sig_str: "($A) -> String",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Kernel"),
                "inspect",
                &[],
                &[
                    builtin_surface_parameter("a", "$A"),
                ],
                "String",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "safe_div",
        arity: 2,
        sig_str: "($A, $A) -> Result<$A, ZeroDivisionError>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Int"),
                "safe_div",
                &[],
                &[
                    builtin_surface_parameter("a", "Int"),
                    builtin_surface_parameter("b", "Int"),
                ],
                "Result<Int, ZeroDivisionError>",
                &[],
            ),
            builtin_surface_spec(
                Some("Float"),
                "safe_div",
                &[],
                &[
                    builtin_surface_parameter("a", "Float"),
                    builtin_surface_parameter("b", "Float"),
                ],
                "Result<Float, ZeroDivisionError>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "safe_mod",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, ZeroDivisionError>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Int"),
                "safe_mod",
                &[],
                &[
                    builtin_surface_parameter("a", "Int"),
                    builtin_surface_parameter("b", "Int"),
                ],
                "Result<Int, ZeroDivisionError>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "eprint",
        arity: 1,
        sig_str: "(Error) -> Unit",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Kernel"),
                "eprint",
                &[],
                &[
                    builtin_surface_parameter("err", "Error"),
                ],
                "Unit",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "set_exit_code",
        arity: 1,
        sig_str: "(Int) -> Unit",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Kernel"),
                "set_exit_code",
                &[],
                &[
                    builtin_surface_parameter("code", "Int"),
                ],
                "Unit",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "shl",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeShiftCount>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Int"),
                "shl",
                &[],
                &[
                    builtin_surface_parameter("value", "Int"),
                    builtin_surface_parameter("bits", "Int"),
                ],
                "Result<Int, NegativeShiftCount>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "shr",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeShiftCount>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Int"),
                "shr",
                &[],
                &[
                    builtin_surface_parameter("value", "Int"),
                    builtin_surface_parameter("bits", "Int"),
                ],
                "Result<Int, NegativeShiftCount>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "len",
        arity: 1,
        sig_str: "(List<$A>) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("List"),
                "len",
                &[],
                &[
                    builtin_surface_parameter("values", "List<$A>"),
                ],
                "Int",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "gen_make",
        arity: 2,
        sig_str: "(Int, List<$Item>) -> Generator<$State, $Item>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Generator"),
                "gen_make",
                &["$State"],
                &[
                    builtin_surface_parameter("idx", "Int"),
                    builtin_surface_parameter("items", "List<$Item>"),
                ],
                "Generator<$State, $Item>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "gen_idx",
        arity: 1,
        sig_str: "(Generator<$State, $Item>) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Generator"),
                "gen_idx",
                &[],
                &[
                    builtin_surface_parameter("gen", "Generator<$State, $Item>"),
                ],
                "Int",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "gen_items",
        arity: 1,
        sig_str: "(Generator<$State, $Item>) -> List<$Item>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Generator"),
                "gen_items",
                &[],
                &[
                    builtin_surface_parameter("gen", "Generator<$State, $Item>"),
                ],
                "List<$Item>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "bit_and",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Int"),
                "bit_and",
                &[],
                &[
                    builtin_surface_parameter("a", "Int"),
                    builtin_surface_parameter("b", "Int"),
                ],
                "Int",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "bit_or",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Int"),
                "bit_or",
                &[],
                &[
                    builtin_surface_parameter("a", "Int"),
                    builtin_surface_parameter("b", "Int"),
                ],
                "Int",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "bit_xor",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Int"),
                "bit_xor",
                &[],
                &[
                    builtin_surface_parameter("a", "Int"),
                    builtin_surface_parameter("b", "Int"),
                ],
                "Int",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "bit_not",
        arity: 1,
        sig_str: "(Int) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Int"),
                "bit_not",
                &[],
                &[
                    builtin_surface_parameter("value", "Int"),
                ],
                "Int",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "test_bit",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Boolean, NegativeBitIndex>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Int"),
                "test_bit",
                &[],
                &[
                    builtin_surface_parameter("value", "Int"),
                    builtin_surface_parameter("index", "Int"),
                ],
                "Result<Boolean, NegativeBitIndex>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "set_bit",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeBitIndex>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Int"),
                "set_bit",
                &[],
                &[
                    builtin_surface_parameter("value", "Int"),
                    builtin_surface_parameter("index", "Int"),
                ],
                "Result<Int, NegativeBitIndex>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "clear_bit",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeBitIndex>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Int"),
                "clear_bit",
                &[],
                &[
                    builtin_surface_parameter("value", "Int"),
                    builtin_surface_parameter("index", "Int"),
                ],
                "Result<Int, NegativeBitIndex>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "toggle_bit",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeBitIndex>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Int"),
                "toggle_bit",
                &[],
                &[
                    builtin_surface_parameter("value", "Int"),
                    builtin_surface_parameter("index", "Int"),
                ],
                "Result<Int, NegativeBitIndex>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "codepoints",
        arity: 2,
        sig_str: "(String, StringEncoding) -> Result<List<Int>, InvalidStringEncoding>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("String"),
                "codepoints",
                &[],
                &[
                    builtin_surface_parameter("value", "String"),
                    builtin_surface_parameter("encoding", "StringEncoding"),
                ],
                "Result<List<Int>, InvalidStringEncoding>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "from_codepoints",
        arity: 2,
        sig_str: "(List<Int>, StringEncoding) -> Result<String, InvalidStringEncoding>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("String"),
                "from_codepoints",
                &[],
                &[
                    builtin_surface_parameter("values", "List<Int>"),
                    builtin_surface_parameter("encoding", "StringEncoding"),
                ],
                "Result<String, InvalidStringEncoding>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "map_err",
        arity: 2,
        sig_str: "(Result<$T>, Lazy<Error>) -> Result<$T>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "cause",
        arity: 2,
        sig_str: "(Result<$T>, Lazy<Error>) -> Result<$T>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "chain",
        arity: 2,
        sig_str: "(Result<$T>, Result<()>) -> Result<$T>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Result"),
                "chain",
                &[],
                &[
                    builtin_surface_parameter("head", "Result<$T>"),
                    builtin_surface_parameter("tail", "Result<()>"),
                ],
                "Result<$T>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__recover_kind",
        arity: 3,
        sig_str: "(Result<$T>, String, (Error -> Result<$T>)) -> Result<$T>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__test_push",
        arity: 2,
        sig_str: "(String, String) -> Unit",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Test"),
                "__test_push",
                &[],
                &[
                    builtin_surface_parameter("kind", "String"),
                    builtin_surface_parameter("name", "String"),
                ],
                "Unit",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__test_pop",
        arity: 0,
        sig_str: "() -> Unit",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Test"),
                "__test_pop",
                &[],
                &[],
                "Unit",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__test_pass",
        arity: 1,
        sig_str: "(String) -> Unit",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Test"),
                "__test_pass",
                &[],
                &[
                    builtin_surface_parameter("name", "String"),
                ],
                "Unit",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__test_fail",
        arity: 2,
        sig_str: "(String, String) -> Unit",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Test"),
                "__test_fail",
                &[],
                &[
                    builtin_surface_parameter("name", "String"),
                    builtin_surface_parameter("detail", "String"),
                ],
                "Unit",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__test_fail_error",
        arity: 2,
        sig_str: "(String, Error) -> Unit",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Test"),
                "__test_fail_error",
                &[],
                &[
                    builtin_surface_parameter("name", "String"),
                    builtin_surface_parameter("err", "Error"),
                ],
                "Unit",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__test_fail_current",
        arity: 1,
        sig_str: "(String) -> Unit",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Test"),
                "__test_fail_current",
                &[],
                &[
                    builtin_surface_parameter("detail", "String"),
                ],
                "Unit",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "group_count",
        arity: 1,
        sig_str: "(List<$A>) -> List<($A, Int)>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("List"),
                "group_count",
                &[],
                &[
                    builtin_surface_parameter("values", "List<$A>"),
                ],
                "List<($A, Int)>",
                &[
                    builtin_surface_constraint("$A", "Eq"),
                ],
            ),
        ],
    },
    BuiltinMeta {
        name: "zip",
        arity: 2,
        sig_str: "(List<$A>, List<$B>) -> List<($A, $B)>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("List"),
                "zip",
                &[],
                &[
                    builtin_surface_parameter("left", "List<$A>"),
                    builtin_surface_parameter("right", "List<$B>"),
                ],
                "List<($A, $B)>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "empty_map",
        arity: 0,
        sig_str: "() -> HashMap<$V>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("HashMap"),
                "empty_map",
                &["$V"],
                &[],
                "HashMap<$V>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "map_from_entries",
        arity: 1,
        sig_str: "(List<(String, $V)>) -> HashMap<$V>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("HashMap"),
                "map_from_entries",
                &[],
                &[
                    builtin_surface_parameter("entries", "List<(String, $V)>"),
                ],
                "HashMap<$V>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "map_len",
        arity: 1,
        sig_str: "(HashMap<$V>) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("HashMap"),
                "map_len",
                &[],
                &[
                    builtin_surface_parameter("map", "HashMap<$V>"),
                ],
                "Int",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "map_contains_key",
        arity: 2,
        sig_str: "(HashMap<$V>, String) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("HashMap"),
                "map_contains_key",
                &[],
                &[
                    builtin_surface_parameter("map", "HashMap<$V>"),
                    builtin_surface_parameter("key", "String"),
                ],
                "Boolean",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "map_get",
        arity: 2,
        sig_str: "(HashMap<$V>, String) -> Result<$V, NoneError>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("HashMap"),
                "map_get",
                &[],
                &[
                    builtin_surface_parameter("map", "HashMap<$V>"),
                    builtin_surface_parameter("key", "String"),
                ],
                "Result<$V, NoneError>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "map_insert",
        arity: 3,
        sig_str: "(HashMap<$V>, String, $V) -> HashMap<$V>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("HashMap"),
                "map_insert",
                &[],
                &[
                    builtin_surface_parameter("map", "HashMap<$V>"),
                    builtin_surface_parameter("key", "String"),
                    builtin_surface_parameter("value", "$V"),
                ],
                "HashMap<$V>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "map_remove",
        arity: 2,
        sig_str: "(HashMap<$V>, String) -> HashMap<$V>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("HashMap"),
                "map_remove",
                &[],
                &[
                    builtin_surface_parameter("map", "HashMap<$V>"),
                    builtin_surface_parameter("key", "String"),
                ],
                "HashMap<$V>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "map_keys",
        arity: 1,
        sig_str: "(HashMap<$V>) -> List<String>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("HashMap"),
                "map_keys",
                &[],
                &[
                    builtin_surface_parameter("map", "HashMap<$V>"),
                ],
                "List<String>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "map_values_list",
        arity: 1,
        sig_str: "(HashMap<$V>) -> List<$V>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("HashMap"),
                "map_values_list",
                &[],
                &[
                    builtin_surface_parameter("map", "HashMap<$V>"),
                ],
                "List<$V>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "view",
        arity: 2,
        sig_str: "(Facet<ReadablePath, $S, $A, _, _>, $S) -> Result<$A>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Facet"),
                "view",
                &[],
                &[
                    builtin_surface_parameter("facet", "Facet<ReadablePath, $S, $A, _, _>"),
                    builtin_surface_parameter("source", "$S"),
                ],
                "Result<$A>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "preview",
        arity: 2,
        sig_str: "(Facet<PreviewPath, $S, $A, _, _>, $S) -> Result<$A>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Facet"),
                "preview",
                &[],
                &[
                    builtin_surface_parameter("facet", "Facet<PreviewPath, $S, $A, _, _>"),
                    builtin_surface_parameter("source", "$S"),
                ],
                "Result<$A>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__facet_chain",
        arity: 2,
        sig_str: "(Facet<$K, $S, $A, _, _>, Facet<$L, $A, $B, _, _>) -> Facet<$K, $S, $B, _, _>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Facet"),
                "chain",
                &[],
                &[
                    builtin_surface_parameter("outer", "Facet<$K, $S, $A, _, _>"),
                    builtin_surface_parameter("inner", "Facet<$L, $A, $B, _, _>"),
                ],
                "Facet<$K, $S, $B, _, _>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__facet_put",
        arity: 3,
        sig_str: "(Facet<PutPath, $S, $A, $T, $B>, $S, $B) -> $T",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Facet"),
                "put",
                &[],
                &[
                    builtin_surface_parameter("facet", "Facet<PutPath, $S, $A, $T, $B>"),
                    builtin_surface_parameter("source", "$S"),
                    builtin_surface_parameter("value", "$B"),
                ],
                "$T",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "set",
        arity: 3,
        sig_str: "(Facet<WritablePath, $S, $A, $T, $B>, $S, $B) -> Result<$T>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Facet"),
                "set",
                &[],
                &[
                    builtin_surface_parameter("facet", "Facet<WritablePath, $S, $A, $T, $B>"),
                    builtin_surface_parameter("source", "$S"),
                    builtin_surface_parameter("value", "$B"),
                ],
                "Result<$T>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "over",
        arity: 3,
        sig_str: "(Facet<WritablePath, $S, $A, $T, $B>, $S, ($A -> Result<$B>)) -> Result<$T>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Facet"),
                "over",
                &[],
                &[
                    builtin_surface_parameter("facet", "Facet<WritablePath, $S, $A, $T, $B>"),
                    builtin_surface_parameter("source", "$S"),
                    builtin_surface_parameter("update_fun", "($A -> Result<$B>)"),
                ],
                "Result<$T>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "over_result",
        arity: 3,
        sig_str: "(Facet<WritablePath, $S, Result<$A>, $T, Result<$B>>, $S, (Result<$A> -> Result<Result<$B>>)) -> Result<$T>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Facet"),
                "over_result",
                &[],
                &[
                    builtin_surface_parameter("facet", "Facet<WritablePath, $S, Result<$A>, $T, Result<$B>>"),
                    builtin_surface_parameter("source", "$S"),
                    builtin_surface_parameter("update_fun", "(Result<$A> -> Result<Result<$B>>)"),
                ],
                "Result<$T>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "case_set",
        arity: 3,
        sig_str: "(Facet<CasePath, $S, $A, $T, $B>, $S, $B) -> Result<$T>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Facet"),
                "case_set",
                &[],
                &[
                    builtin_surface_parameter("facet", "Facet<CasePath, $S, $A, $T, $B>"),
                    builtin_surface_parameter("source", "$S"),
                    builtin_surface_parameter("value", "$B"),
                ],
                "Result<$T>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "case_over",
        arity: 3,
        sig_str: "(Facet<CasePath, $S, $A, $T, $B>, $S, ($A -> Result<$B>)) -> Result<$T>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Facet"),
                "case_over",
                &[],
                &[
                    builtin_surface_parameter("facet", "Facet<CasePath, $S, $A, $T, $B>"),
                    builtin_surface_parameter("source", "$S"),
                    builtin_surface_parameter("update_fun", "($A -> Result<$B>)"),
                ],
                "Result<$T>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__facet_list_get",
        arity: 2,
        sig_str: "(List<$A>, Int) -> Result<$A>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__facet_list_set",
        arity: 3,
        sig_str: "(List<$A>, Int, $A) -> Result<List<$A>>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__facet_list_slice_get",
        arity: 3,
        sig_str: "(List<$A>, Int, Int) -> Result<List<$A>>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__facet_list_slice_set",
        arity: 4,
        sig_str: "(List<$A>, Int, Int, List<$A>) -> Result<List<$A>>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__facet_map_get",
        arity: 2,
        sig_str: "(HashMap<$A>, String) -> Result<$A>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__facet_map_set_existing",
        arity: 3,
        sig_str: "(HashMap<$A>, String, $A) -> Result<HashMap<$A>>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__test_capture_stdout",
        arity: 0,
        sig_str: "() -> List<String>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Test"),
                "__test_capture_stdout",
                &[],
                &[],
                "List<String>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__test_capture_stderr",
        arity: 0,
        sig_str: "() -> List<String>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Test"),
                "__test_capture_stderr",
                &[],
                &[],
                "List<String>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__test_push_stdin",
        arity: 1,
        sig_str: "(String) -> Unit",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Test"),
                "__test_push_stdin",
                &[],
                &[
                    builtin_surface_parameter("input", "String"),
                ],
                "Unit",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__test_begin_it",
        arity: 0,
        sig_str: "() -> Unit",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Test"),
                "__test_begin_it",
                &[],
                &[],
                "Unit",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "compile",
        arity: 1,
        sig_str: "(String) -> Result<Regex, RegexCompileError>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Regex"),
                "compile",
                &[],
                &[
                    builtin_surface_parameter("pattern", "String"),
                ],
                "Result<Regex, RegexCompileError>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "is_match",
        arity: 2,
        sig_str: "(Regex, String) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Regex"),
                "is_match",
                &[],
                &[
                    builtin_surface_parameter("re", "Regex"),
                    builtin_surface_parameter("input", "String"),
                ],
                "Boolean",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "captures",
        arity: 2,
        sig_str: "(Regex, String) -> Result<RegexCaptures, NoneError>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Regex"),
                "captures",
                &[],
                &[
                    builtin_surface_parameter("re", "Regex"),
                    builtin_surface_parameter("input", "String"),
                ],
                "Result<RegexCaptures, NoneError>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "whole",
        arity: 1,
        sig_str: "(RegexCaptures) -> String",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("RegexCaptures"),
                "whole",
                &[],
                &[
                    builtin_surface_parameter("caps", "RegexCaptures"),
                ],
                "String",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "capture_count",
        arity: 1,
        sig_str: "(RegexCaptures) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("RegexCaptures"),
                "capture_count",
                &[],
                &[
                    builtin_surface_parameter("caps", "RegexCaptures"),
                ],
                "Int",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "get",
        arity: 2,
        sig_str: "(RegexCaptures, Int) -> Result<String, NoneError>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("RegexCaptures"),
                "get",
                &[],
                &[
                    builtin_surface_parameter("caps", "RegexCaptures"),
                    builtin_surface_parameter("idx", "Int"),
                ],
                "Result<String, NoneError>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "get_name",
        arity: 2,
        sig_str: "(RegexCaptures, String) -> Result<String, NoneError>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("RegexCaptures"),
                "get_name",
                &[],
                &[
                    builtin_surface_parameter("caps", "RegexCaptures"),
                    builtin_surface_parameter("name", "String"),
                ],
                "Result<String, NoneError>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "find",
        arity: 2,
        sig_str: "(Regex, String) -> Result<RegexMatch, NoneError>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Regex"),
                "find",
                &[],
                &[
                    builtin_surface_parameter("re", "Regex"),
                    builtin_surface_parameter("input", "String"),
                ],
                "Result<RegexMatch, NoneError>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "find_all",
        arity: 2,
        sig_str: "(Regex, String) -> List<RegexMatch>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Regex"),
                "find_all",
                &[],
                &[
                    builtin_surface_parameter("re", "Regex"),
                    builtin_surface_parameter("input", "String"),
                ],
                "List<RegexMatch>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "split",
        arity: 2,
        sig_str: "(Regex, String) -> List<String>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Regex"),
                "split",
                &[],
                &[
                    builtin_surface_parameter("re", "Regex"),
                    builtin_surface_parameter("input", "String"),
                ],
                "List<String>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__regex_replace",
        arity: 3,
        sig_str: "(Regex, String, String) -> String",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Regex"),
                "replace",
                &[],
                &[
                    builtin_surface_parameter("re", "Regex"),
                    builtin_surface_parameter("input", "String"),
                    builtin_surface_parameter("replacement", "String"),
                ],
                "String",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "replace_all",
        arity: 3,
        sig_str: "(Regex, String, String) -> String",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Regex"),
                "replace_all",
                &[],
                &[
                    builtin_surface_parameter("re", "Regex"),
                    builtin_surface_parameter("input", "String"),
                    builtin_surface_parameter("replacement", "String"),
                ],
                "String",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "escape",
        arity: 1,
        sig_str: "(String) -> String",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Regex"),
                "escape",
                &[],
                &[
                    builtin_surface_parameter("text", "String"),
                ],
                "String",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "group_names",
        arity: 1,
        sig_str: "(Regex) -> List<String>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Regex"),
                "group_names",
                &[],
                &[
                    builtin_surface_parameter("re", "Regex"),
                ],
                "List<String>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "text",
        arity: 1,
        sig_str: "(RegexMatch) -> String",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("RegexMatch"),
                "text",
                &[],
                &[
                    builtin_surface_parameter("m", "RegexMatch"),
                ],
                "String",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "start",
        arity: 1,
        sig_str: "(RegexMatch) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("RegexMatch"),
                "start",
                &[],
                &[
                    builtin_surface_parameter("m", "RegexMatch"),
                ],
                "Int",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "end",
        arity: 1,
        sig_str: "(RegexMatch) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("RegexMatch"),
                "end",
                &[],
                &[
                    builtin_surface_parameter("m", "RegexMatch"),
                ],
                "Int",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "project_args",
        arity: 0,
        sig_str: "() -> List<String>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Kernel"),
                "project_args",
                &[],
                &[],
                "List<String>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "io_get",
        arity: 1,
        sig_str: "(String) -> Result<String, InputError>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("IO"),
                "get",
                &[],
                &[
                    builtin_surface_parameter("prompt", "String"),
                ],
                "Result<String, InputError>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "io_get_line",
        arity: 1,
        sig_str: "(String) -> Result<String, InputError>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("IO"),
                "get_line",
                &[],
                &[
                    builtin_surface_parameter("prompt", "String"),
                ],
                "Result<String, InputError>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "file_read",
        arity: 1,
        sig_str: "(String) -> Result<String>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("File"),
                "read",
                &[],
                &[
                    builtin_surface_parameter("path", "String"),
                ],
                "Result<String>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "file_write",
        arity: 2,
        sig_str: "(String, String) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("File"),
                "write",
                &[],
                &[
                    builtin_surface_parameter("path", "String"),
                    builtin_surface_parameter("text", "String"),
                ],
                "Result<Unit>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "file_append",
        arity: 2,
        sig_str: "(String, String) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("File"),
                "append",
                &[],
                &[
                    builtin_surface_parameter("path", "String"),
                    builtin_surface_parameter("text", "String"),
                ],
                "Result<Unit>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "file_exists",
        arity: 1,
        sig_str: "(String) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("File"),
                "exists",
                &[],
                &[
                    builtin_surface_parameter("path", "String"),
                ],
                "Boolean",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "file_delete",
        arity: 1,
        sig_str: "(String) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("File"),
                "delete",
                &[],
                &[
                    builtin_surface_parameter("path", "String"),
                ],
                "Result<Unit>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "file_with_open",
        arity: 3,
        sig_str: "(String, FileMode, (FileHandle -> Result<$A>)) -> Result<$A>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("File"),
                "with_open",
                &[],
                &[
                    builtin_surface_parameter("path", "String"),
                    builtin_surface_parameter("mode", "FileMode"),
                    builtin_surface_parameter("body", "(FileHandle -> Result<$A>)"),
                ],
                "Result<$A>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "file_read_chunk",
        arity: 2,
        sig_str: "(FileHandle, Int) -> Result<String>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("File"),
                "read_chunk",
                &[],
                &[
                    builtin_surface_parameter("file", "FileHandle"),
                    builtin_surface_parameter("max_chars", "Int"),
                ],
                "Result<String>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "file_write_chunk",
        arity: 2,
        sig_str: "(FileHandle, String) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("File"),
                "write_chunk",
                &[],
                &[
                    builtin_surface_parameter("file", "FileHandle"),
                    builtin_surface_parameter("text", "String"),
                ],
                "Result<Unit>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "file_flush",
        arity: 1,
        sig_str: "(FileHandle) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("File"),
                "flush",
                &[],
                &[
                    builtin_surface_parameter("file", "FileHandle"),
                ],
                "Result<Unit>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "filesystem_path",
        arity: 1,
        sig_str: "(String) -> Result<FilePath, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("FS"),
                "path",
                &[],
                &[
                    builtin_surface_parameter("raw", "String"),
                ],
                "Result<FilePath, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "filesystem_join",
        arity: 2,
        sig_str: "(FilePath, String) -> Result<FilePath, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("FS"),
                "join",
                &[],
                &[
                    builtin_surface_parameter("base", "FilePath"),
                    builtin_surface_parameter("child", "String"),
                ],
                "Result<FilePath, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "filesystem_parent",
        arity: 1,
        sig_str: "(FilePath) -> Result<FilePath, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("FS"),
                "parent",
                &[],
                &[
                    builtin_surface_parameter("path", "FilePath"),
                ],
                "Result<FilePath, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "filesystem_name",
        arity: 1,
        sig_str: "(FilePath) -> Result<String, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("FS"),
                "name",
                &[],
                &[
                    builtin_surface_parameter("path", "FilePath"),
                ],
                "Result<String, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "filesystem_extension",
        arity: 1,
        sig_str: "(FilePath) -> Option<String>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("FS"),
                "extension",
                &[],
                &[
                    builtin_surface_parameter("path", "FilePath"),
                ],
                "Option<String>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "filesystem_exists",
        arity: 1,
        sig_str: "(FilePath) -> Result<Boolean, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("FS"),
                "exists",
                &[],
                &[
                    builtin_surface_parameter("path", "FilePath"),
                ],
                "Result<Boolean, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "filesystem_stat",
        arity: 1,
        sig_str: "(FilePath) -> Result<FileSystemEntry, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("FS"),
                "stat",
                &[],
                &[
                    builtin_surface_parameter("path", "FilePath"),
                ],
                "Result<FileSystemEntry, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "filesystem_ls",
        arity: 1,
        sig_str: "(FilePath) -> Result<FileSystemSnapshot, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("FS"),
                "ls",
                &[],
                &[
                    builtin_surface_parameter("path", "FilePath"),
                ],
                "Result<FileSystemSnapshot, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "filesystem_tree_depth",
        arity: 2,
        sig_str: "(FilePath, Int) -> Result<FileSystemSnapshot, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("FS"),
                "tree_depth",
                &[],
                &[
                    builtin_surface_parameter("path", "FilePath"),
                    builtin_surface_parameter("depth", "Int"),
                ],
                "Result<FileSystemSnapshot, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "filesystem_mkdir",
        arity: 1,
        sig_str: "(FilePath) -> Result<Unit, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("FS"),
                "mkdir",
                &[],
                &[
                    builtin_surface_parameter("path", "FilePath"),
                ],
                "Result<Unit, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "filesystem_mkdir_all",
        arity: 1,
        sig_str: "(FilePath) -> Result<Unit, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("FS"),
                "mkdir_all",
                &[],
                &[
                    builtin_surface_parameter("path", "FilePath"),
                ],
                "Result<Unit, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "filesystem_rm",
        arity: 1,
        sig_str: "(FilePath) -> Result<Unit, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("FS"),
                "rm",
                &[],
                &[
                    builtin_surface_parameter("path", "FilePath"),
                ],
                "Result<Unit, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "filesystem_mv",
        arity: 2,
        sig_str: "(FilePath, FilePath) -> Result<Unit, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("FS"),
                "mv",
                &[],
                &[
                    builtin_surface_parameter("from", "FilePath"),
                    builtin_surface_parameter("to", "FilePath"),
                ],
                "Result<Unit, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "filesystem_cp",
        arity: 2,
        sig_str: "(FilePath, FilePath) -> Result<Unit, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("FS"),
                "cp",
                &[],
                &[
                    builtin_surface_parameter("from", "FilePath"),
                    builtin_surface_parameter("to", "FilePath"),
                ],
                "Result<Unit, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "shell_pwd",
        arity: 0,
        sig_str: "() -> Result<FilePath, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Shell"),
                "pwd",
                &[],
                &[],
                "Result<FilePath, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "shell_cd",
        arity: 1,
        sig_str: "(FilePath) -> Result<Unit, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Shell"),
                "cd",
                &[],
                &[
                    builtin_surface_parameter("path", "FilePath"),
                ],
                "Result<Unit, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "shell_exec",
        arity: 2,
        sig_str: "(String, List<String>) -> Result<CommandResult, Error>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Shell"),
                "exec",
                &[],
                &[
                    builtin_surface_parameter("command", "String"),
                    builtin_surface_parameter("args", "List<String>"),
                ],
                "Result<CommandResult, Error>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "seed",
        arity: 1,
        sig_str: "(Int) -> RandomGenerator",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Random"),
                "seed",
                &[],
                &[
                    builtin_surface_parameter("seed", "Int"),
                ],
                "RandomGenerator",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "int_until",
        arity: 1,
        sig_str: "(Int) -> Result<Int, InvalidRandomRange>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Random"),
                "int_until",
                &[],
                &[
                    builtin_surface_parameter("end", "Int"),
                ],
                "Result<Int, InvalidRandomRange>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "int_range",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, InvalidRandomRange>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Random"),
                "int_range",
                &[],
                &[
                    builtin_surface_parameter("start", "Int"),
                    builtin_surface_parameter("end", "Int"),
                ],
                "Result<Int, InvalidRandomRange>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "next_int_until",
        arity: 2,
        sig_str: "(RandomGenerator, Int) -> Result<(Int, RandomGenerator), InvalidRandomRange>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Random"),
                "next_int_until",
                &[],
                &[
                    builtin_surface_parameter("rng", "RandomGenerator"),
                    builtin_surface_parameter("end", "Int"),
                ],
                "Result<(Int, RandomGenerator), InvalidRandomRange>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "next_int_range",
        arity: 3,
        sig_str:
            "(RandomGenerator, Int, Int) -> Result<(Int, RandomGenerator), InvalidRandomRange>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Random"),
                "next_int_range",
                &[],
                &[
                    builtin_surface_parameter("rng", "RandomGenerator"),
                    builtin_surface_parameter("start", "Int"),
                    builtin_surface_parameter("end", "Int"),
                ],
                "Result<(Int, RandomGenerator), InvalidRandomRange>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "kind",
        arity: 1,
        sig_str: "(Error) -> String",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Error"),
                "kind",
                &[],
                &[
                    builtin_surface_parameter("err", "Error"),
                ],
                "String",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "message",
        arity: 1,
        sig_str: "(Error) -> String",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Error"),
                "message",
                &[],
                &[
                    builtin_surface_parameter("err", "Error"),
                ],
                "String",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "format",
        arity: 1,
        sig_str: "(Error) -> String",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Error"),
                "format",
                &[],
                &[
                    builtin_surface_parameter("err", "Error"),
                ],
                "String",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__process_pid",
        arity: 2,
        sig_str: "($Owner, (-> Result<$State>)) -> PID<$Process>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Agent"),
                "pid",
                &["$Process"],
                &[
                    builtin_surface_parameter("owner", "$Owner"),
                    builtin_surface_parameter("init", "(-> Result<$State>)"),
                ],
                "PID<$Process>",
                &[],
            ),
            builtin_surface_spec(
                Some("GenServer"),
                "pid",
                &["$Process"],
                &[
                    builtin_surface_parameter("owner", "$Owner"),
                    builtin_surface_parameter("init", "(-> Result<$State>)"),
                ],
                "PID<$Process>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__process_spawn",
        arity: 2,
        sig_str: "($Owner, (-> Result<$State>)) -> Result<PID<$Process>>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Agent"),
                "spawn",
                &["$Process"],
                &[
                    builtin_surface_parameter("owner", "$Owner"),
                    builtin_surface_parameter("init", "(-> Result<$State>)"),
                ],
                "Result<PID<$Process>>",
                &[],
            ),
            builtin_surface_spec(
                Some("GenServer"),
                "spawn",
                &["$Process"],
                &[
                    builtin_surface_parameter("owner", "$Owner"),
                    builtin_surface_parameter("init", "(-> Result<$State>)"),
                ],
                "Result<PID<$Process>>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__dynamic_supervisor_spawn",
        arity: 1,
        sig_str: "((-> Result<$State>)) -> Result<PID<$Process>>",
        compiler_generated_surfaces: &[builtin_generated_surface_spec(
            "DynamicSupervisor",
            "spawn",
        )],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__dynamic_supervisor_adopt",
        arity: 1,
        sig_str: "(PID<$Process>) -> Result<Unit>",
        compiler_generated_surfaces: &[builtin_generated_surface_spec(
            "DynamicSupervisor",
            "adopt",
        )],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__dynamic_supervisor_status",
        arity: 0,
        sig_str: "() -> Result<SupervisorStatus>",
        compiler_generated_surfaces: &[builtin_generated_surface_spec(
            "DynamicSupervisor",
            "status",
        )],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__supervisor_spawn",
        arity: 2,
        sig_str: "($Supervisor, (-> Result<$State>)) -> Result<PID<$Process>>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Supervisor"),
                "spawn",
                &["$Process"],
                &[
                    builtin_surface_parameter("supervisor", "$Supervisor"),
                    builtin_surface_parameter("worker_init", "(-> Result<$State>)"),
                ],
                "Result<PID<$Process>>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__supervisor_adopt",
        arity: 2,
        sig_str: "($Supervisor, PID<$Process>) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Supervisor"),
                "adopt",
                &[],
                &[
                    builtin_surface_parameter("supervisor", "$Supervisor"),
                    builtin_surface_parameter("pid", "PID<$Process>"),
                ],
                "Result<Unit>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__supervisor_status",
        arity: 1,
        sig_str: "($Supervisor) -> Result<SupervisorStatus>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Supervisor"),
                "status",
                &[],
                &[
                    builtin_surface_parameter("supervisor", "$Supervisor"),
                ],
                "Result<SupervisorStatus>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__supervisor_workers",
        arity: 3,
        sig_str: "($Supervisor, (-> Result<$State>), WorkerStrategy) -> Result<Workers<$Process>>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Supervisor"),
                "workers",
                &["$Process"],
                &[
                    builtin_surface_parameter("supervisor", "$Supervisor"),
                    builtin_surface_parameter("worker_init", "(-> Result<$State>)"),
                    builtin_surface_parameter("strategy", "WorkerStrategy"),
                ],
                "Result<Workers<$Process>>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__process_state",
        arity: 1,
        sig_str: "(PID<$Process>) -> Result<$State>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Agent"),
                "state",
                &["$State"],
                &[
                    builtin_surface_parameter("pid", "PID<$Process>"),
                ],
                "Result<$State>",
                &[],
            ),
            builtin_surface_spec(
                Some("GenServer"),
                "state",
                &["$State"],
                &[
                    builtin_surface_parameter("pid", "PID<$Process>"),
                ],
                "Result<$State>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__process_store",
        arity: 2,
        sig_str: "(PID<$Process>, $State) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Agent"),
                "store",
                &[],
                &[
                    builtin_surface_parameter("pid", "PID<$Process>"),
                    builtin_surface_parameter("state", "$State"),
                ],
                "Result<Unit>",
                &[],
            ),
            builtin_surface_spec(
                Some("GenServer"),
                "store",
                &[],
                &[
                    builtin_surface_parameter("pid", "PID<$Process>"),
                    builtin_surface_parameter("state", "$State"),
                ],
                "Result<Unit>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__genserver_call_reply",
        arity: 3,
        sig_str: "(PID<$Process>, $State, $Reply) -> Result<$Reply>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__genserver_call_reply_later",
        arity: 3,
        sig_str: "(PID<$Process>, $State, (-> Result<$Reply>)) -> Result<$Reply>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__genserver_call_stop_normal",
        arity: 2,
        sig_str: "(PID<$Process>, $Reply) -> Result<$Reply>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__genserver_call_stop_error",
        arity: 2,
        sig_str: "(PID<$Process>, Error) -> Result<$Reply>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__genserver_cast_next",
        arity: 2,
        sig_str: "(PID<$Process>, $State) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__genserver_cast_stop_normal",
        arity: 1,
        sig_str: "(PID<$Process>) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__genserver_cast_stop_error",
        arity: 2,
        sig_str: "(PID<$Process>, Error) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__process_self",
        arity: 0,
        sig_str: "() -> PID<$Process>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Process"),
                "self",
                &["$Process"],
                &[],
                "PID<$Process>",
                &[],
            ),
            builtin_surface_spec(
                Some("Agent"),
                "self",
                &["$Process"],
                &[],
                "PID<$Process>",
                &[],
            ),
            builtin_surface_spec(
                Some("GenServer"),
                "self",
                &["$Process"],
                &[],
                "PID<$Process>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__process_context_handler",
        arity: 2,
        sig_str: "($Owner, String) -> PID<$Handler>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Agent"),
                "context_handler",
                &["$Handler"],
                &[
                    builtin_surface_parameter("owner", "$Owner"),
                    builtin_surface_parameter("slot", "String"),
                ],
                "PID<$Handler>",
                &[],
            ),
            builtin_surface_spec(
                Some("GenServer"),
                "context_handler",
                &["$Handler"],
                &[
                    builtin_surface_parameter("owner", "$Owner"),
                    builtin_surface_parameter("slot", "String"),
                ],
                "PID<$Handler>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__out_handler_write",
        arity: 2,
        sig_str: "(PID<OutHandler>, String) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("OutHandler"),
                "write",
                &[],
                &[
                    builtin_surface_parameter("pid", "PID<OutHandler>"),
                    builtin_surface_parameter("text", "String"),
                ],
                "Result<Unit>",
                &[],
            ),
            builtin_surface_spec(
                Some("OutHandler"),
                "__out_handler_write",
                &[],
                &[
                    builtin_surface_parameter("pid", "PID<OutHandler>"),
                    builtin_surface_parameter("text", "String"),
                ],
                "Result<Unit>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__process_sleep",
        arity: 1,
        sig_str: "(Duration) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Process"),
                "sleep",
                &[],
                &[
                    builtin_surface_parameter("duration", "Duration"),
                ],
                "Result<Unit>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "Pending",
        arity: 0,
        sig_str: "() -> StandbyInit<$T>",
        compiler_generated_surfaces: &[],
        surfaces: &[builtin_surface_spec(
            None,
            "Pending",
            &["$T"],
            &[],
            "StandbyInit<$T>",
            &[],
        )],
    },
    BuiltinMeta {
        name: "PendingAfter",
        arity: 1,
        sig_str: "(Duration) -> StandbyInit<$T>",
        compiler_generated_surfaces: &[],
        surfaces: &[builtin_surface_spec(
            None,
            "PendingAfter",
            &["$T"],
            &[builtin_surface_parameter("duration", "Duration")],
            "StandbyInit<$T>",
            &[],
        )],
    },
    BuiltinMeta {
        name: "Ready",
        arity: 1,
        sig_str: "($T) -> StandbyInit<$T>",
        compiler_generated_surfaces: &[],
        surfaces: &[builtin_surface_spec(
            None,
            "Ready",
            &[],
            &[builtin_surface_parameter("value", "$T")],
            "StandbyInit<$T>",
            &[],
        )],
    },
    BuiltinMeta {
        name: "__task_call",
        arity: 1,
        sig_str: "((-> Result<$A>)) -> Result<$A>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Task"),
                "call",
                &[],
                &[
                    builtin_surface_parameter("body", "(-> Result<$A>)"),
                ],
                "Result<$A>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__task_async",
        arity: 1,
        sig_str: "((-> Result<$A>)) -> TaskHandle<$A>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Task"),
                "async",
                &[],
                &[
                    builtin_surface_parameter("body", "(-> Result<$A>)"),
                ],
                "TaskHandle<$A>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__task_await",
        arity: 1,
        sig_str: "(TaskHandle<$A>) -> Result<$A>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Task"),
                "await",
                &[],
                &[
                    builtin_surface_parameter("task", "TaskHandle<$A>"),
                ],
                "Result<$A>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__task_launch",
        arity: 1,
        sig_str: "((-> Result<Unit>)) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Task"),
                "launch",
                &[],
                &[
                    builtin_surface_parameter("body", "(-> Result<Unit>)"),
                ],
                "Result<Unit>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__task_cast",
        arity: 1,
        sig_str: "((-> Unit)) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Task"),
                "cast",
                &[],
                &[
                    builtin_surface_parameter("body", "(-> Unit)"),
                ],
                "Result<Unit>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__task_call_timeout",
        arity: 2,
        sig_str: "(Duration, (-> Result<$A>)) -> Result<$A>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__task_async_timeout",
        arity: 2,
        sig_str: "(Duration, (-> Result<$A>)) -> TaskHandle<$A>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__task_await_timeout",
        arity: 2,
        sig_str: "(Duration, TaskHandle<$A>) -> Result<$A>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__task_launch_timeout",
        arity: 2,
        sig_str: "(Duration, (-> Result<Unit>)) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__task_cast_timeout",
        arity: 2,
        sig_str: "(Duration, (-> Unit)) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__workers_submit",
        arity: 2,
        sig_str: "(Workers<$Worker>, (PID<$Worker> -> Result<Unit>)) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Workers"),
                "submit",
                &[],
                &[
                    builtin_surface_parameter("workers", "Workers<$Worker>"),
                    builtin_surface_parameter("message", "(PID<$Worker> -> Result<Unit>)"),
                ],
                "Result<Unit>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__workers_submit_timeout",
        arity: 3,
        sig_str: "(Duration, Workers<$Worker>, (PID<$Worker> -> Result<Unit>)) -> Result<Unit>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__workers_broadcast",
        arity: 2,
        sig_str: "(Workers<$Worker>, (PID<$Worker> -> Result<$A>)) -> List<Result<$A>>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Workers"),
                "broadcast",
                &[],
                &[
                    builtin_surface_parameter("workers", "Workers<$Worker>"),
                    builtin_surface_parameter("message", "(PID<$Worker> -> Result<$A>)"),
                ],
                "List<Result<$A>>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__workers_broadcast_timeout",
        arity: 3,
        sig_str: "(Duration, Workers<$Worker>, (PID<$Worker> -> Result<$A>)) -> List<Result<$A>>",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__workers_reserve",
        arity: 1,
        sig_str: "(Workers<$Worker>) -> Result<WorkerLease<$Worker>>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Workers"),
                "reserve",
                &[],
                &[
                    builtin_surface_parameter("workers", "Workers<$Worker>"),
                ],
                "Result<WorkerLease<$Worker>>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__workers_size",
        arity: 1,
        sig_str: "(Workers<$Worker>) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Workers"),
                "size",
                &[],
                &[
                    builtin_surface_parameter("workers", "Workers<$Worker>"),
                ],
                "Int",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__operator_int_add",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_int_sub",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_int_mul",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_float_add",
        arity: 2,
        sig_str: "(Float, Float) -> Float",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_float_sub",
        arity: 2,
        sig_str: "(Float, Float) -> Float",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_float_mul",
        arity: 2,
        sig_str: "(Float, Float) -> Float",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "floor",
        arity: 1,
        sig_str: "(Float) -> Float",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Float"),
                "floor",
                &[],
                &[
                    builtin_surface_parameter("value", "Float"),
                ],
                "Float",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "ceil",
        arity: 1,
        sig_str: "(Float) -> Float",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Float"),
                "ceil",
                &[],
                &[
                    builtin_surface_parameter("value", "Float"),
                ],
                "Float",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "round",
        arity: 1,
        sig_str: "(Float) -> Float",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Float"),
                "round",
                &[],
                &[
                    builtin_surface_parameter("value", "Float"),
                ],
                "Float",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "trunc",
        arity: 1,
        sig_str: "(Float) -> Float",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Float"),
                "trunc",
                &[],
                &[
                    builtin_surface_parameter("value", "Float"),
                ],
                "Float",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "pi",
        arity: 0,
        sig_str: "() -> Float",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Float"),
                "pi",
                &[],
                &[],
                "Float",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "e",
        arity: 0,
        sig_str: "() -> Float",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Float"),
                "e",
                &[],
                &[],
                "Float",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "__operator_int_eq",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_int_neq",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_int_lt",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_int_lte",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_int_gt",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_int_gte",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_float_eq",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_float_neq",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_float_lt",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_float_lte",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_float_gt",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_float_gte",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__compare_int",
        arity: 2,
        sig_str: "(Int, Int) -> Ordering",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__compare_float",
        arity: 2,
        sig_str: "(Float, Float) -> Ordering",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__ordering_is_lt",
        arity: 1,
        sig_str: "(Ordering) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__ordering_is_lte",
        arity: 1,
        sig_str: "(Ordering) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__ordering_is_gt",
        arity: 1,
        sig_str: "(Ordering) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__ordering_is_gte",
        arity: 1,
        sig_str: "(Ordering) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_string_eq",
        arity: 2,
        sig_str: "(String, String) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_string_neq",
        arity: 2,
        sig_str: "(String, String) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_boolean_eq",
        arity: 2,
        sig_str: "(Boolean, Boolean) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_boolean_neq",
        arity: 2,
        sig_str: "(Boolean, Boolean) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "__operator_string_concat",
        arity: 2,
        sig_str: "(String, String) -> String",
        compiler_generated_surfaces: &[],
        surfaces: &[],
    },
    BuiltinMeta {
        name: "json_parse",
        arity: 1,
        sig_str: "(String) -> Result<JsonValue, JsonParseError>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Json"),
                "parse",
                &[],
                &[
                    builtin_surface_parameter("text", "String"),
                ],
                "Result<JsonValue, JsonParseError>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "json_stringify",
        arity: 1,
        sig_str: "(JsonValue) -> Result<String, JsonEncodeError>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Json"),
                "stringify",
                &[],
                &[
                    builtin_surface_parameter("value", "JsonValue"),
                ],
                "Result<String, JsonEncodeError>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "string_len",
        arity: 1,
        sig_str: "(String) -> Int",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("String"),
                "len",
                &[],
                &[
                    builtin_surface_parameter("value", "String"),
                ],
                "Int",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "string_contains",
        arity: 2,
        sig_str: "(String, String) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("String"),
                "contains",
                &[],
                &[
                    builtin_surface_parameter("value", "String"),
                    builtin_surface_parameter("needle", "String"),
                ],
                "Boolean",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "string_starts_with",
        arity: 2,
        sig_str: "(String, String) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("String"),
                "starts_with",
                &[],
                &[
                    builtin_surface_parameter("value", "String"),
                    builtin_surface_parameter("prefix", "String"),
                ],
                "Boolean",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "string_ends_with",
        arity: 2,
        sig_str: "(String, String) -> Boolean",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("String"),
                "ends_with",
                &[],
                &[
                    builtin_surface_parameter("value", "String"),
                    builtin_surface_parameter("suffix", "String"),
                ],
                "Boolean",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "string_split",
        arity: 2,
        sig_str: "(String, String) -> List<String>",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("String"),
                "split",
                &[],
                &[
                    builtin_surface_parameter("value", "String"),
                    builtin_surface_parameter("separator", "String"),
                ],
                "List<String>",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "string_replace",
        arity: 3,
        sig_str: "(String, String, String) -> String",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("String"),
                "replace",
                &[],
                &[
                    builtin_surface_parameter("value", "String"),
                    builtin_surface_parameter("from", "String"),
                    builtin_surface_parameter("to", "String"),
                ],
                "String",
                &[],
            ),
        ],
    },
    BuiltinMeta {
        name: "curry",
        arity: 1,
        sig_str: "($A) -> $B",
        compiler_generated_surfaces: &[],
        surfaces: &[
            builtin_surface_spec(
                Some("Function"),
                "curry",
                &["$Curried"],
                &[
                    builtin_surface_parameter("fun", "$Fun"),
                ],
                "$Curried",
                &[],
            ),
        ],
    },
];

/// Function metadata view. Prefer this name when the caller needs runtime
/// builtin dispatch/signature information, not type-head or surface policy.
pub const BUILTIN_FUNCTION_METAS: &[BuiltinFunctionMeta] = BUILTIN_METAS;

pub fn builtin_function_metas() -> &'static [BuiltinFunctionMeta] {
    BUILTIN_FUNCTION_METAS
}

/// Canonical builtin type declarations accepted from standard definition sources.
///
/// These entries define the exact source-level heads the compiler accepts,
/// including generic parameter names such as `List<$A>` and `Result<$T>`.
pub const BUILTIN_TYPE_METAS: &[BuiltinTypeMeta] = &[
    BuiltinTypeMeta {
        name: TypeName::Int.as_str(),
        params: &[],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::Float.as_str(),
        params: &[],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::String.as_str(),
        params: &[],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::Boolean.as_str(),
        params: &[],
        identity: TypeIdentity::Enum,
    },
    BuiltinTypeMeta {
        name: TypeName::Unit.as_str(),
        params: &[],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::Closure.as_str(),
        params: &[],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::MatchArms.as_str(),
        params: &["$Scrutinee", "$Result"],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::CondClauses.as_str(),
        params: &["$Result"],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::BulkUpdateEntries.as_str(),
        params: &["$State"],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::Error.as_str(),
        params: &[],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::Regex.as_str(),
        params: &[],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::RegexCaptures.as_str(),
        params: &[],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::RegexMatch.as_str(),
        params: &[],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::RandomGenerator.as_str(),
        params: &[],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::FileHandle.as_str(),
        params: &[],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::List.as_str(),
        params: &["$A"],
        identity: TypeIdentity::TypeConstructor,
    },
    BuiltinTypeMeta {
        name: TypeName::HashMap.as_str(),
        params: &["$V"],
        identity: TypeIdentity::TypeConstructor,
    },
    BuiltinTypeMeta {
        name: TypeName::Generator.as_str(),
        params: &["$State", "$Item"],
        identity: TypeIdentity::TypeConstructor,
    },
    BuiltinTypeMeta {
        name: TypeName::Result.as_str(),
        params: &["$T"],
        identity: TypeIdentity::TypeConstructor,
    },
    BuiltinTypeMeta {
        name: TypeName::StandbyInit.as_str(),
        params: &["$T"],
        identity: TypeIdentity::TypeConstructor,
    },
    BuiltinTypeMeta {
        name: TypeName::Lazy.as_str(),
        params: &["$T"],
        identity: TypeIdentity::TypeConstructor,
    },
    BuiltinTypeMeta {
        name: TypeName::Hole.as_str(),
        params: &[],
        identity: TypeIdentity::Type,
    },
    BuiltinTypeMeta {
        name: TypeName::Facet.as_str(),
        params: &["$K", "$S", "$A", "$T", "$B"],
        identity: TypeIdentity::TypeConstructor,
    },
    BuiltinTypeMeta {
        name: TypeName::Workers.as_str(),
        params: &["$Worker"],
        identity: TypeIdentity::TypeConstructor,
    },
    BuiltinTypeMeta {
        name: TypeName::WorkerLease.as_str(),
        params: &["$Worker"],
        identity: TypeIdentity::TypeConstructor,
    },
    BuiltinTypeMeta {
        name: TypeName::TaskHandle.as_str(),
        params: &["$T"],
        identity: TypeIdentity::TypeConstructor,
    },
];

/// Type-head metadata view. Prefer this name when validating `@builtin type`
/// declarations, not runtime builtin dispatch or surface capabilities.
pub const BUILTIN_TYPE_HEAD_METAS: &[BuiltinTypeHeadMeta] = BUILTIN_TYPE_METAS;

pub fn builtin_type_head_metas() -> &'static [BuiltinTypeHeadMeta] {
    BUILTIN_TYPE_HEAD_METAS
}

pub fn builtin_function_meta_by_name(name: &str) -> Option<&'static BuiltinFunctionMeta> {
    BUILTIN_FUNCTION_METAS.iter().find(|meta| meta.name == name)
}

pub fn builtin_meta_by_name(name: &str) -> Option<&'static BuiltinMeta> {
    builtin_function_meta_by_name(name)
}

/// Look up a runtime entry by its canonical runtime name.
///
/// This name is deliberately explicit at call sites: a surface callable
/// identity is resolved through [`BuiltinMeta::surface_variant`], while this
/// helper only selects the shared runtime entry.
pub fn builtin_meta_by_runtime_name(name: &str) -> Option<&'static BuiltinMeta> {
    builtin_meta_by_name(name)
}

/// Resolve the surface variant for a declaration such as `Int::safe_div`.
/// The owner is taken from the qualified source identity and is not inferred
/// from the order of declarations in the standard library.
pub fn builtin_surface_variant_for_decl(
    declared_name: &str,
    qualified_name: Option<&str>,
) -> Option<BuiltinSurfaceSignatureMeta> {
    let Some(qualified_name) = qualified_name else {
        return builtin_meta_by_runtime_name(declared_name)?.surface_variant("", declared_name);
    };
    let qualified_name = surface_path_name(qualified_name);
    let Some((owner, surface_name)) = qualified_name.rsplit_once("::") else {
        return (qualified_name == declared_name)
            .then(|| builtin_meta_for_surface(None, declared_name))
            .flatten()?
            .surface_variant("", declared_name);
    };
    if surface_name != declared_name {
        return None;
    }
    builtin_meta_for_qualified_surface(owner, declared_name)?.surface_variant(owner, declared_name)
}

fn builtin_meta_for_qualified_surface(owner: &str, name: &str) -> Option<&'static BuiltinMeta> {
    builtin_meta_for_surface(Some(owner), name)
}

fn builtin_meta_for_surface(owner: Option<&str>, name: &str) -> Option<&'static BuiltinMeta> {
    BUILTIN_METAS.iter().find(|meta| {
        meta.surfaces
            .iter()
            .any(|surface| surface.owner == owner && surface.name == name)
    })
}

pub fn builtin_runtime_name<'a>(
    declared_name: &'a str,
    qualified_name: Option<&'a str>,
) -> &'a str {
    let Some(qualified_name) = qualified_name else {
        return declared_name;
    };
    let qualified_name = surface_path_name(qualified_name);
    let Some((owner, surface_name)) = qualified_name.rsplit_once("::") else {
        return builtin_meta_for_surface(None, declared_name)
            .map(|meta| meta.name)
            .unwrap_or(qualified_name);
    };
    if surface_name != declared_name {
        return qualified_name;
    }
    builtin_meta_for_qualified_surface(owner, declared_name)
        .map(|meta| meta.name)
        .unwrap_or(qualified_name)
}

pub fn builtin_meta_for_decl(
    declared_name: &str,
    qualified_name: Option<&str>,
) -> Option<&'static BuiltinMeta> {
    match qualified_name {
        Some(qualified_name) => {
            let qualified_name = surface_path_name(qualified_name);
            let Some((owner, surface_name)) = qualified_name.rsplit_once("::") else {
                return (qualified_name == declared_name)
                    .then(|| builtin_meta_for_surface(None, declared_name))
                    .flatten();
            };
            if surface_name != declared_name {
                return None;
            }
            builtin_meta_for_qualified_surface(owner, declared_name)
        }
        None => builtin_meta_by_name(declared_name),
    }
}

/// Resolve an exact compiler-generated declaration identity to its canonical
/// runtime metadata. Callers must separately prove compiler-generated AST
/// provenance; this lookup deliberately does not participate in source-surface
/// resolution.
pub fn builtin_meta_for_compiler_generated_decl(
    declared_name: &str,
    qualified_name: &str,
) -> Option<&'static BuiltinMeta> {
    let qualified_name = surface_path_name(qualified_name);
    let (owner, surface_name) = qualified_name.rsplit_once("::")?;
    if surface_name != declared_name {
        return None;
    }
    BUILTIN_METAS.iter().find(|meta| {
        meta.compiler_generated_surfaces
            .iter()
            .any(|surface| surface.owner == owner && surface.name == declared_name)
    })
}

pub fn builtin_id_by_name(name: &str) -> Option<u16> {
    builtin_function_metas()
        .iter()
        .position(|meta| meta.name == name)
        .and_then(|idx| (idx <= u16::MAX as usize).then_some(idx as u16))
}

pub fn builtin_meta_by_id(builtin_id: u16) -> Option<&'static BuiltinMeta> {
    builtin_function_metas().get(builtin_id as usize)
}

pub fn builtin_type_head_meta_by_name(name: &str) -> Option<&'static BuiltinTypeHeadMeta> {
    BUILTIN_TYPE_HEAD_METAS
        .iter()
        .find(|meta| meta.name == name)
}

pub fn builtin_type_meta_by_name(name: &str) -> Option<&'static BuiltinTypeMeta> {
    builtin_type_head_meta_by_name(name)
}

/// Look up a standard-library declaration's explicit owner identity.
pub fn standard_owner_identity_by_name(name: &str) -> Option<TypeIdentity> {
    STANDARD_OWNER_IDENTITY_METAS
        .iter()
        .find(|meta| meta.name == name)
        .map(|meta| meta.identity)
}

pub fn builtin_type_supports_inherent_impl(name: &str) -> bool {
    builtin_type_name(name).is_some_and(TypeName::supports_inherent_impl)
}

pub fn builtin_uid(builtin_id: u16) -> u32 {
    BUILTIN_UID_BASE + u32::from(builtin_id)
}

#[cfg(test)]
mod tests {
    use super::{
        builtin_function_metas, builtin_id_by_name, builtin_meta_by_id, builtin_meta_by_name,
        builtin_meta_by_runtime_name, builtin_meta_for_compiler_generated_decl,
        builtin_meta_for_decl, builtin_runtime_name, builtin_surface_variant_for_decl,
        builtin_type_head_metas, builtin_uid, parse_surface_signature,
        standard_owner_identity_by_name, BUILTIN_FUNCTION_METAS, BUILTIN_METAS,
        BUILTIN_TYPE_HEAD_METAS, BUILTIN_TYPE_METAS,
    };
    use crate::names::{TypeIdentity, TypeName};

    #[test]
    fn builtin_ids_match_definition_order() {
        for (idx, meta) in BUILTIN_METAS.iter().enumerate() {
            let id = idx as u16;
            assert_eq!(builtin_id_by_name(meta.name), Some(id));
            assert_eq!(builtin_uid(id), 2 + idx as u32);
        }
    }

    #[test]
    fn builtin_metadata_exposes_function_and_type_head_facets() {
        assert_eq!(BUILTIN_FUNCTION_METAS.len(), BUILTIN_METAS.len());
        assert_eq!(BUILTIN_FUNCTION_METAS[0].name, "print");
        assert_eq!(BUILTIN_TYPE_HEAD_METAS.len(), BUILTIN_TYPE_METAS.len());
        assert!(BUILTIN_TYPE_HEAD_METAS
            .iter()
            .any(|meta| meta.name == "StandbyInit"));
    }

    #[test]
    fn to_string_trait_metadata_lists_every_builtin_show_target() {
        let metadata = builtin_meta_by_name("to_string")
            .expect("to_string metadata")
            .trait_method()
            .expect("to_string Trait surface");
        assert_eq!(metadata.trait_name, "Show");
        assert_eq!(metadata.method_name, "to_string");
        assert_eq!(
            metadata.targets,
            &[
                TypeName::Int,
                TypeName::Float,
                TypeName::String,
                TypeName::Boolean,
                TypeName::Unit,
                TypeName::Error,
            ]
        );
        assert_eq!(
            metadata.builtin_id,
            builtin_meta_by_name("to_string")
                .expect("to_string metadata")
                .builtin_id()
        );
    }

    #[test]
    fn standard_owner_identity_metadata_marks_option_as_type_constructor() {
        assert_eq!(
            standard_owner_identity_by_name("Option"),
            Some(TypeIdentity::TypeConstructor)
        );
    }

    #[test]
    fn builtin_table_accessors_expose_split_metadata_views() {
        assert_eq!(builtin_function_metas()[0].name, "print");
        assert!(builtin_type_head_metas()
            .iter()
            .any(|meta| meta.name == "StandbyInit"));
    }

    #[test]
    fn builtin_lookup_returns_none_for_unknown_values() {
        assert!(builtin_meta_by_id(u16::MAX).is_none());
        assert!(builtin_meta_by_name("__missing__").is_none());
    }

    #[test]
    fn surface_variants_keep_distinct_identities_and_shared_runtime_target() {
        let meta = builtin_meta_by_runtime_name("safe_div").expect("safe_div metadata");
        let int = meta
            .surface_variant("Int", "safe_div")
            .expect("Int::safe_div surface variant");
        let float = meta
            .surface_variant("Float", "safe_div")
            .expect("Float::safe_div surface variant");

        let int_names = int
            .value_parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>();
        let float_names = float
            .value_parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(int_names, ["a", "b"]);
        assert_eq!(float_names, ["a", "b"]);
        assert_eq!(int.value_parameters.len(), meta.runtime_arity() as usize);
        assert_eq!(float.value_parameters.len(), meta.runtime_arity() as usize);
        assert_eq!(int.runtime_target, float.runtime_target);
        assert_ne!(int.identity, float.identity);
    }

    #[test]
    fn declaration_lookup_uses_canonical_owner_and_name() {
        let variant = builtin_surface_variant_for_decl("safe_div", Some("Global::Int::safe_div"))
            .expect("qualified builtin declaration should resolve");
        assert_eq!(variant.identity.owner.as_deref(), Some("Int"));
        assert_eq!(variant.identity.name, "safe_div");
    }

    #[test]
    fn declaration_lookup_rejects_an_unregistered_owner_alias() {
        assert!(
            builtin_surface_variant_for_decl("print", Some("Global::Unknown::print")).is_none()
        );
        assert!(builtin_meta_for_decl("print", Some("Global::Unknown::print")).is_none());
        assert_eq!(
            builtin_runtime_name("print", Some("Global::Unknown::print")),
            "Unknown::print"
        );
    }

    #[test]
    fn declaration_lookup_accepts_explicit_owner_aliases() {
        let variant = builtin_surface_variant_for_decl("print", Some("Global::Kernel::print"))
            .expect("Kernel::print is an explicit standard-library surface");
        assert_eq!(variant.identity.owner.as_deref(), Some("Kernel"));
        assert_eq!(variant.identity.name, "print");

        let metadata = builtin_meta_by_name("print").expect("print metadata");
        assert!(metadata
            .surface_variants()
            .iter()
            .any(|surface| surface.identity == variant.identity));
    }

    #[test]
    fn declaration_lookup_accepts_explicit_unqualified_hidden_surfaces() {
        for (name, runtime_name) in [
            ("Pending", "Pending"),
            ("PendingAfter", "PendingAfter"),
            ("Ready", "Ready"),
        ] {
            let qualified = format!("Global::{name}");
            let variant = builtin_surface_variant_for_decl(name, Some(&qualified))
                .unwrap_or_else(|| panic!("{qualified} should be an explicit source surface"));
            assert_eq!(variant.identity.owner, None);
            assert_eq!(variant.identity.name, name);
            assert_eq!(
                builtin_meta_for_decl(name, Some(&qualified)).map(|meta| meta.name),
                Some(runtime_name)
            );
        }
    }

    #[test]
    fn compiler_generated_declaration_lookup_is_exact_and_not_a_source_surface() {
        for (declared_name, qualified_name, runtime_name) in [
            (
                "spawn",
                "Global::DynamicSupervisor::spawn",
                "__dynamic_supervisor_spawn",
            ),
            (
                "adopt",
                "Global::DynamicSupervisor::adopt",
                "__dynamic_supervisor_adopt",
            ),
            (
                "status",
                "Global::DynamicSupervisor::status",
                "__dynamic_supervisor_status",
            ),
        ] {
            let metadata = builtin_meta_for_compiler_generated_decl(declared_name, qualified_name)
                .unwrap_or_else(|| panic!("{qualified_name} should be registered"));
            assert_eq!(metadata.name, runtime_name);
            assert!(metadata.surface_variants().is_empty());
            assert!(builtin_meta_for_decl(declared_name, Some(qualified_name)).is_none());
        }

        assert!(
            builtin_meta_for_compiler_generated_decl("spawn", "Global::Unknown::spawn").is_none()
        );
        assert!(builtin_meta_for_compiler_generated_decl(
            "status",
            "Global::DynamicSupervisor::spawn"
        )
        .is_none());
    }

    #[test]
    fn every_builtin_signature_is_parseable_and_matches_runtime_arity() {
        for meta in BUILTIN_METAS {
            let (parameters, _) = parse_surface_signature(meta.sig_str)
                .unwrap_or_else(|| panic!("malformed signature for {}", meta.name));
            assert_eq!(
                parameters.len(),
                usize::from(meta.runtime_arity()),
                "signature arity drift for {}",
                meta.name
            );
        }
        assert!(parse_surface_signature("Int").is_none());
    }

    #[test]
    fn structured_surface_preserves_nested_type_commas() {
        let meta = builtin_meta_by_name("gen_make").expect("gen_make metadata");
        let variant = meta
            .surface_variant("Generator", "gen_make")
            .expect("Generator::gen_make surface variant");
        assert_eq!(variant.value_parameters.len(), 2);
        assert_eq!(variant.value_parameters[1].ty, "List<$Item>");
        assert_eq!(variant.return_type.ty, "Generator<$State, $Item>");
    }

    #[test]
    fn every_surface_variant_preserves_runtime_arity() {
        for meta in BUILTIN_METAS {
            for variant in meta.surface_variants() {
                assert_eq!(
                    variant.value_parameters.len(),
                    usize::from(meta.runtime_arity()),
                    "surface variant {:?} drifted from runtime arity",
                    variant.identity
                );
                assert!(matches!(
                    variant.runtime_target,
                    crate::signature::RuntimeTarget::Builtin(_)
                ));
            }
        }
    }

    #[test]
    fn surface_variant_preserves_parameter_and_where_metadata() {
        let meta = builtin_meta_by_runtime_name("group_count").expect("group_count metadata");
        let variant = meta
            .surface_variant("List", "group_count")
            .expect("List::group_count surface variant");
        assert_eq!(variant.value_parameters[0].ordinal, 0);
        assert_eq!(variant.value_parameters[0].name, "values");
        assert_eq!(variant.value_parameters[0].ty, "List<$A>");
        assert_eq!(variant.where_constraints.constraints.len(), 1);
        assert_eq!(variant.where_constraints.constraints[0].subject, "$A");
        assert_eq!(variant.where_constraints.constraints[0].trait_name, "Eq");
    }

    #[test]
    fn source_backed_internal_and_hidden_builtins_have_explicit_surfaces() {
        let test_surfaces = builtin_meta_by_name("__test_push")
            .expect("test push metadata")
            .surface_variants();
        assert_eq!(test_surfaces.len(), 1);
        assert_eq!(test_surfaces[0].identity.owner.as_deref(), Some("Test"));

        let out_surfaces = builtin_meta_by_name("__out_handler_write")
            .expect("out handler metadata")
            .surface_variants();
        assert!(out_surfaces.iter().any(|surface| {
            surface.identity.owner.as_deref() == Some("OutHandler")
                && surface.identity.name == "__out_handler_write"
        }));

        for runtime_only in [
            "__recover_kind",
            "__facet_list_get",
            "__dynamic_supervisor_spawn",
        ] {
            assert!(
                builtin_meta_by_name(runtime_only)
                    .expect("runtime-only builtin metadata")
                    .surface_variants()
                    .is_empty(),
                "{runtime_only} must not acquire a source surface"
            );
        }
    }

    #[test]
    fn qualified_put_builtins_resolve_to_distinct_runtime_names() {
        assert_eq!(
            builtin_runtime_name("chain", Some("Facet::chain")),
            "__facet_chain"
        );
        assert_eq!(
            builtin_runtime_name("replace", Some("String::replace")),
            "string_replace"
        );
        assert_eq!(
            builtin_runtime_name("put", Some("Facet::put")),
            "__facet_put"
        );
        assert_eq!(
            builtin_runtime_name("replace", Some("Regex::replace")),
            "__regex_replace"
        );
        assert_eq!(
            builtin_meta_for_decl("put", Some("Facet::put"))
                .expect("facet put builtin metadata")
                .sig_str,
            "(Facet<PutPath, $S, $A, $T, $B>, $S, $B) -> $T"
        );
        assert_eq!(
            builtin_meta_for_decl("replace", Some("Regex::replace"))
                .expect("regex replace builtin metadata")
                .sig_str,
            "(Regex, String, String) -> String"
        );
    }

    #[test]
    fn qualified_string_split_builtin_resolves_to_runtime_name() {
        assert_eq!(
            builtin_runtime_name("split", Some("String::split")),
            "string_split"
        );
        assert_eq!(
            builtin_meta_for_decl("split", Some("String::split"))
                .expect("string split builtin metadata")
                .sig_str,
            "(String, String) -> List<String>"
        );
    }

    #[test]
    fn qualified_json_builtins_resolve_to_runtime_names() {
        assert_eq!(
            builtin_runtime_name("parse", Some("Json::parse")),
            "json_parse"
        );
        assert_eq!(
            builtin_runtime_name("stringify", Some("Json::stringify")),
            "json_stringify"
        );
    }

    #[test]
    fn qualified_string_len_builtin_resolves_to_runtime_name() {
        assert_eq!(
            builtin_runtime_name("len", Some("String::len")),
            "string_len"
        );
        assert_eq!(
            builtin_meta_for_decl("len", Some("String::len"))
                .expect("string len builtin metadata")
                .sig_str,
            "(String) -> Int"
        );
    }

    #[test]
    fn qualified_string_predicate_builtins_resolve_to_runtime_names() {
        let cases = [
            ("contains", "String::contains", "string_contains"),
            ("starts_with", "String::starts_with", "string_starts_with"),
            ("ends_with", "String::ends_with", "string_ends_with"),
        ];

        for (declared, qualified, runtime) in cases {
            assert_eq!(builtin_runtime_name(declared, Some(qualified)), runtime);
            assert!(
                builtin_meta_for_decl(declared, Some(qualified)).is_some(),
                "{qualified} should have builtin metadata"
            );
        }
    }

    #[test]
    fn qualified_filesystem_and_shell_builtins_resolve_to_runtime_names() {
        let cases = [
            ("path", "FS::path", "filesystem_path"),
            ("join", "FS::join", "filesystem_join"),
            ("parent", "FS::parent", "filesystem_parent"),
            ("name", "FS::name", "filesystem_name"),
            ("extension", "FS::extension", "filesystem_extension"),
            ("exists", "FS::exists", "filesystem_exists"),
            ("stat", "FS::stat", "filesystem_stat"),
            ("ls", "FS::ls", "filesystem_ls"),
            ("tree_depth", "FS::tree_depth", "filesystem_tree_depth"),
            ("mkdir", "FS::mkdir", "filesystem_mkdir"),
            ("mkdir_all", "FS::mkdir_all", "filesystem_mkdir_all"),
            ("rm", "FS::rm", "filesystem_rm"),
            ("mv", "FS::mv", "filesystem_mv"),
            ("cp", "FS::cp", "filesystem_cp"),
            ("pwd", "Shell::pwd", "shell_pwd"),
            ("cd", "Shell::cd", "shell_cd"),
            ("exec", "Shell::exec", "shell_exec"),
        ];

        for (declared, qualified, runtime) in cases {
            assert_eq!(builtin_runtime_name(declared, Some(qualified)), runtime);
            assert!(
                builtin_meta_for_decl(declared, Some(qualified)).is_some(),
                "{qualified} should have builtin metadata"
            );
        }
    }

    #[test]
    fn supervisor_spawn_hidden_builtin_signature_matches_surface() {
        let meta = builtin_meta_by_name("__supervisor_spawn").expect("supervisor spawn builtin");
        assert_eq!(meta.arity, 2);
        assert_eq!(
            meta.sig_str,
            "($Supervisor, (-> Result<$State>)) -> Result<PID<$Process>>"
        );
    }

    #[test]
    fn supervisor_adopt_hidden_builtin_signature_matches_surface() {
        let meta = builtin_meta_by_name("__supervisor_adopt").expect("supervisor adopt builtin");
        assert_eq!(meta.arity, 2);
        assert_eq!(meta.sig_str, "($Supervisor, PID<$Process>) -> Result<Unit>");
    }

    #[test]
    fn supervisor_status_hidden_builtin_signature_matches_surface() {
        let meta = builtin_meta_by_name("__supervisor_status").expect("supervisor status builtin");
        assert_eq!(meta.arity, 1);
        assert_eq!(meta.sig_str, "($Supervisor) -> Result<SupervisorStatus>");
    }
}
