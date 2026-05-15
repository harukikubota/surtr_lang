#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorFamily {
    OpenSameTypeSelf,
    OpenSameTypeBoolean,
    OpenDedicatedReturn,
    OpenHeterogeneousCompose,
    CloseFunction,
    OpenContextual,
    CloseBooleanSpecial,
    CloseBindingSpecial,
    OpenConversionHelper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleLabelKind {
    Op,
    Bind,
    SafeBind,
    Match,
    Cond,
    Boolean,
}

impl RuleLabelKind {
    pub const fn caption(self) -> &'static str {
        match self {
            Self::Op => "OP rule",
            Self::Bind => "Bind rule",
            Self::SafeBind => "SafeBind rule",
            Self::Match => "Match rule",
            Self::Cond => "Cond rule",
            Self::Boolean => "Boolean rule",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportSummaryPolicy {
    None,
    VisibleImplementations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatorProfile {
    pub name: &'static str,
    pub symbol: &'static str,
    pub family: OperatorFamily,
    pub label_kind: RuleLabelKind,
    pub canonical_rule: &'static str,
    pub trait_name: Option<&'static str>,
    pub support_summary_policy: SupportSummaryPolicy,
}

pub const ADD_PROFILE: OperatorProfile = OperatorProfile {
    name: "Add",
    symbol: "+",
    family: OperatorFamily::OpenSameTypeSelf,
    label_kind: RuleLabelKind::Op,
    canonical_rule: "A + A -> A",
    trait_name: Some("Add"),
    support_summary_policy: SupportSummaryPolicy::VisibleImplementations,
};

pub const SUB_PROFILE: OperatorProfile = OperatorProfile {
    name: "Sub",
    symbol: "-",
    family: OperatorFamily::OpenSameTypeSelf,
    label_kind: RuleLabelKind::Op,
    canonical_rule: "A - A -> A",
    trait_name: Some("Sub"),
    support_summary_policy: SupportSummaryPolicy::VisibleImplementations,
};

pub const MUL_PROFILE: OperatorProfile = OperatorProfile {
    name: "Mul",
    symbol: "*",
    family: OperatorFamily::OpenSameTypeSelf,
    label_kind: RuleLabelKind::Op,
    canonical_rule: "A * A -> A",
    trait_name: Some("Mul"),
    support_summary_policy: SupportSummaryPolicy::VisibleImplementations,
};

pub const EQ_PROFILE: OperatorProfile = OperatorProfile {
    name: "Eq",
    symbol: "==",
    family: OperatorFamily::OpenSameTypeBoolean,
    label_kind: RuleLabelKind::Op,
    canonical_rule: "A == A -> Boolean",
    trait_name: Some("Eq"),
    support_summary_policy: SupportSummaryPolicy::VisibleImplementations,
};

pub const NEQ_PROFILE: OperatorProfile = OperatorProfile {
    name: "Neq",
    symbol: "!=",
    family: OperatorFamily::OpenSameTypeBoolean,
    label_kind: RuleLabelKind::Op,
    canonical_rule: "A != A -> Boolean",
    trait_name: Some("Neq"),
    support_summary_policy: SupportSummaryPolicy::VisibleImplementations,
};

pub const LT_PROFILE: OperatorProfile = OperatorProfile {
    name: "Lt",
    symbol: "<",
    family: OperatorFamily::OpenSameTypeBoolean,
    label_kind: RuleLabelKind::Op,
    canonical_rule: "A < A -> Boolean",
    trait_name: Some("Compare"),
    support_summary_policy: SupportSummaryPolicy::VisibleImplementations,
};

pub const LTE_PROFILE: OperatorProfile = OperatorProfile {
    name: "Lte",
    symbol: "<=",
    family: OperatorFamily::OpenSameTypeBoolean,
    label_kind: RuleLabelKind::Op,
    canonical_rule: "A <= A -> Boolean",
    trait_name: Some("Compare"),
    support_summary_policy: SupportSummaryPolicy::VisibleImplementations,
};

pub const GT_PROFILE: OperatorProfile = OperatorProfile {
    name: "Gt",
    symbol: ">",
    family: OperatorFamily::OpenSameTypeBoolean,
    label_kind: RuleLabelKind::Op,
    canonical_rule: "A > A -> Boolean",
    trait_name: Some("Compare"),
    support_summary_policy: SupportSummaryPolicy::VisibleImplementations,
};

pub const GTE_PROFILE: OperatorProfile = OperatorProfile {
    name: "Gte",
    symbol: ">=",
    family: OperatorFamily::OpenSameTypeBoolean,
    label_kind: RuleLabelKind::Op,
    canonical_rule: "A >= A -> Boolean",
    trait_name: Some("Compare"),
    support_summary_policy: SupportSummaryPolicy::VisibleImplementations,
};

pub const CONCAT_PROFILE: OperatorProfile = OperatorProfile {
    name: "Concat",
    symbol: "++",
    family: OperatorFamily::OpenDedicatedReturn,
    label_kind: RuleLabelKind::Op,
    canonical_rule: "String ++ String -> String",
    trait_name: Some("Concat"),
    support_summary_policy: SupportSummaryPolicy::VisibleImplementations,
};

pub const BIND_RULE_TEXT: &str = "Bind rule: `=` accepts only total MatchBlock patterns.";

pub fn operator_profile_by_name(name: &str) -> Option<&'static OperatorProfile> {
    match name {
        "Add" => Some(&ADD_PROFILE),
        "Sub" => Some(&SUB_PROFILE),
        "Mul" => Some(&MUL_PROFILE),
        "Eq" => Some(&EQ_PROFILE),
        "Neq" => Some(&NEQ_PROFILE),
        "Lt" => Some(&LT_PROFILE),
        "Lte" => Some(&LTE_PROFILE),
        "Gt" => Some(&GT_PROFILE),
        "Gte" => Some(&GTE_PROFILE),
        "Concat" => Some(&CONCAT_PROFILE),
        _ => None,
    }
}

pub fn operator_profile_by_symbol(symbol: &str) -> Option<&'static OperatorProfile> {
    match symbol {
        "+" => Some(&ADD_PROFILE),
        "-" => Some(&SUB_PROFILE),
        "*" => Some(&MUL_PROFILE),
        "==" => Some(&EQ_PROFILE),
        "!=" => Some(&NEQ_PROFILE),
        "<" => Some(&LT_PROFILE),
        "<=" => Some(&LTE_PROFILE),
        ">" => Some(&GT_PROFILE),
        ">=" => Some(&GTE_PROFILE),
        "++" => Some(&CONCAT_PROFILE),
        _ => None,
    }
}
