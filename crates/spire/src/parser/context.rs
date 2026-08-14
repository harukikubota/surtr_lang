#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeclLevel {
    Top,
    Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParseUnitKind {
    Script,
    Module,
    Project,
    Repl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TopLevelDeclKind {
    Def,
    ExtractorDef,
    Defmod,
    Defagent,
    Defgenserver,
    Defsupervisor,
    DefdynamicSupervisor,
    Namespace,
    ImplDef,
    TraitDef,
    TraitImplDef,
    Import,
    Include,
    StructDef,
    RecordDef,
    DeferrorDef,
    EnumDef,
    ConstDef,
    SupervisorInit,
    BuiltinDecl,
    BuiltinExtractorDecl,
    BuiltinTypeDecl,
    TypeAlias,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TopLevelDeclPolicy {
    Any,
    Only(Vec<TopLevelDeclKind>),
}

impl TopLevelDeclPolicy {
    fn only(allowed: &[TopLevelDeclKind]) -> Self {
        Self::Only(allowed.to_vec())
    }

    pub(crate) fn allows(&self, kind: TopLevelDeclKind) -> bool {
        match self {
            Self::Any => true,
            Self::Only(allowed) => allowed.contains(&kind),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseRules {
    pub(crate) allow_top_level_expr: bool,
    pub(crate) allowed_top_level_decl_kinds: TopLevelDeclPolicy,
}

const SCRIPT_TOP_LEVEL_DECLS: &[TopLevelDeclKind] = &[
    TopLevelDeclKind::Def,
    TopLevelDeclKind::Defagent,
    TopLevelDeclKind::Defgenserver,
    TopLevelDeclKind::Defsupervisor,
    TopLevelDeclKind::DefdynamicSupervisor,
    TopLevelDeclKind::Namespace,
    TopLevelDeclKind::ImplDef,
    TopLevelDeclKind::TraitDef,
    TopLevelDeclKind::TraitImplDef,
    TopLevelDeclKind::ConstDef,
    TopLevelDeclKind::SupervisorInit,
    TopLevelDeclKind::Import,
    TopLevelDeclKind::Include,
    TopLevelDeclKind::StructDef,
    TopLevelDeclKind::RecordDef,
    TopLevelDeclKind::DeferrorDef,
    TopLevelDeclKind::EnumDef,
    TopLevelDeclKind::TypeAlias,
];

const MODULE_TOP_LEVEL_DECLS: &[TopLevelDeclKind] = &[
    TopLevelDeclKind::Defmod,
    TopLevelDeclKind::Defagent,
    TopLevelDeclKind::Defgenserver,
    TopLevelDeclKind::Defsupervisor,
    TopLevelDeclKind::DefdynamicSupervisor,
    TopLevelDeclKind::Namespace,
    TopLevelDeclKind::ImplDef,
    TopLevelDeclKind::TraitDef,
    TopLevelDeclKind::TraitImplDef,
    TopLevelDeclKind::Import,
    TopLevelDeclKind::StructDef,
    TopLevelDeclKind::RecordDef,
    TopLevelDeclKind::DeferrorDef,
    TopLevelDeclKind::EnumDef,
    TopLevelDeclKind::TypeAlias,
    TopLevelDeclKind::ConstDef,
    TopLevelDeclKind::SupervisorInit,
];

const PROJECT_TOP_LEVEL_DECLS: &[TopLevelDeclKind] = &[
    TopLevelDeclKind::Def,
    TopLevelDeclKind::Defmod,
    TopLevelDeclKind::Defagent,
    TopLevelDeclKind::Defgenserver,
    TopLevelDeclKind::Defsupervisor,
    TopLevelDeclKind::DefdynamicSupervisor,
    TopLevelDeclKind::Namespace,
    TopLevelDeclKind::ImplDef,
    TopLevelDeclKind::TraitDef,
    TopLevelDeclKind::TraitImplDef,
    TopLevelDeclKind::Import,
    TopLevelDeclKind::StructDef,
    TopLevelDeclKind::RecordDef,
    TopLevelDeclKind::DeferrorDef,
    TopLevelDeclKind::EnumDef,
    TopLevelDeclKind::TypeAlias,
    TopLevelDeclKind::ConstDef,
    TopLevelDeclKind::SupervisorInit,
];

const STD_MODULE_TOP_LEVEL_DECLS: &[TopLevelDeclKind] = &[
    TopLevelDeclKind::Defmod,
    TopLevelDeclKind::Defagent,
    TopLevelDeclKind::Defgenserver,
    TopLevelDeclKind::Defsupervisor,
    TopLevelDeclKind::DefdynamicSupervisor,
    TopLevelDeclKind::Namespace,
    TopLevelDeclKind::ImplDef,
    TopLevelDeclKind::TraitDef,
    TopLevelDeclKind::TraitImplDef,
    TopLevelDeclKind::Import,
    TopLevelDeclKind::StructDef,
    TopLevelDeclKind::RecordDef,
    TopLevelDeclKind::DeferrorDef,
    TopLevelDeclKind::EnumDef,
    TopLevelDeclKind::TypeAlias,
    TopLevelDeclKind::ConstDef,
    TopLevelDeclKind::BuiltinDecl,
    TopLevelDeclKind::BuiltinExtractorDecl,
    TopLevelDeclKind::BuiltinTypeDecl,
    TopLevelDeclKind::TypeAlias,
];

const MODULE_MEMBER_DECLS: &[TopLevelDeclKind] = &[
    TopLevelDeclKind::Import,
    TopLevelDeclKind::Def,
    TopLevelDeclKind::ExtractorDef,
    TopLevelDeclKind::TraitDef,
    TopLevelDeclKind::TraitImplDef,
    TopLevelDeclKind::TypeAlias,
];

const STD_MODULE_MEMBER_DECLS: &[TopLevelDeclKind] = &[
    TopLevelDeclKind::Import,
    TopLevelDeclKind::Def,
    TopLevelDeclKind::ExtractorDef,
    TopLevelDeclKind::TraitDef,
    TopLevelDeclKind::TraitImplDef,
    TopLevelDeclKind::BuiltinDecl,
    TopLevelDeclKind::BuiltinExtractorDecl,
    TopLevelDeclKind::BuiltinTypeDecl,
    TopLevelDeclKind::TypeAlias,
];

const REPL_CHUNK_DECLS: &[TopLevelDeclKind] = &[TopLevelDeclKind::Def, TopLevelDeclKind::Import];

impl ParseRules {
    pub fn script() -> Self {
        Self {
            allow_top_level_expr: true,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::only(SCRIPT_TOP_LEVEL_DECLS),
        }
    }

    pub fn module() -> Self {
        Self::module_source()
    }

    pub fn module_source() -> Self {
        Self::module_source_without_builtin()
    }

    pub fn module_source_without_builtin() -> Self {
        Self {
            allow_top_level_expr: false,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::only(MODULE_TOP_LEVEL_DECLS),
        }
    }

    pub fn std_module() -> Self {
        Self {
            allow_top_level_expr: false,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::only(STD_MODULE_TOP_LEVEL_DECLS),
        }
    }

    pub fn module_member() -> Self {
        Self {
            allow_top_level_expr: false,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::only(MODULE_MEMBER_DECLS),
        }
    }

    pub fn std_module_member() -> Self {
        Self {
            allow_top_level_expr: false,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::only(STD_MODULE_MEMBER_DECLS),
        }
    }

    pub fn repl_chunk() -> Self {
        Self {
            allow_top_level_expr: true,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::only(REPL_CHUNK_DECLS),
        }
    }

    pub fn project() -> Self {
        Self {
            allow_top_level_expr: true,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::only(PROJECT_TOP_LEVEL_DECLS),
        }
    }

    pub fn permissive_for_tests() -> Self {
        Self {
            allow_top_level_expr: true,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::Any,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParserContext {
    pub(crate) level: DeclLevel,
    pub(crate) unit_kind: ParseUnitKind,
    pub(crate) source_id: u32,
    pub(crate) module_path: Option<String>,
    pub(crate) parse_rules: ParseRules,
}

impl Default for ParserContext {
    fn default() -> Self {
        Self::script(0)
    }
}

impl ParserContext {
    pub fn script(source_id: u32) -> Self {
        Self {
            level: DeclLevel::Top,
            unit_kind: ParseUnitKind::Script,
            source_id,
            module_path: None,
            parse_rules: ParseRules::script(),
        }
    }

    pub fn module(source_id: u32, module_path: Option<String>) -> Self {
        Self {
            level: DeclLevel::Top,
            unit_kind: ParseUnitKind::Module,
            source_id,
            module_path,
            parse_rules: ParseRules::module_source(),
        }
    }

    pub fn repl(source_id: u32) -> Self {
        Self {
            level: DeclLevel::Top,
            unit_kind: ParseUnitKind::Repl,
            source_id,
            module_path: None,
            parse_rules: ParseRules::repl_chunk(),
        }
    }

    pub fn project(source_id: u32) -> Self {
        Self {
            level: DeclLevel::Top,
            unit_kind: ParseUnitKind::Project,
            source_id,
            module_path: None,
            parse_rules: ParseRules::project(),
        }
    }

    pub fn with_rules(mut self, parse_rules: ParseRules) -> Self {
        self.parse_rules = parse_rules;
        self
    }
}
