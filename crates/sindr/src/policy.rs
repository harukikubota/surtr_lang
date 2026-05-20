#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileUnitKind {
    Script,
    DefinitionCheck,
    Project,
    Repl,
}

/// Bump when source policy semantics change in a way that invalidates staged
/// semantic snapshots.
pub const SOURCE_POLICY_SCHEMA_VERSION: u32 = 1;

/// Logical source categories that drive parser/typechecker policy selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Script,
    DefinitionSource,
    StdDefinitionSource,
    ProjectConfigSource,
    ReplChunk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseProfile {
    Script,
    Module,
    StdModule,
    Project,
    ReplChunk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserContextKind {
    Script,
    Module,
    Project,
    Repl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPoint {
    pub qualified_symbol: String,
}

impl EntryPoint {
    pub fn qualified(qualified_symbol: impl Into<String>) -> Self {
        Self {
            qualified_symbol: qualified_symbol.into(),
        }
    }

    pub fn script_short_name(
        short_name: impl AsRef<str>,
        pseudo_module_path: impl AsRef<str>,
    ) -> Self {
        Self::qualified(format!(
            "{}::{}",
            pseudo_module_path.as_ref(),
            short_name.as_ref()
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCodePolicy {
    Forbidden,
    Anywhere,
    EntryOnly,
}

impl ExitCodePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Forbidden => "Forbidden",
            Self::Anywhere => "Anywhere",
            Self::EntryOnly => "EntryOnly",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSourcePolicy {
    pub exit_code_policy: ExitCodePolicy,
    pub normalized_entrypoint: Option<String>,
}

impl RuntimeSourcePolicy {
    pub fn script() -> Self {
        Self {
            exit_code_policy: ExitCodePolicy::Anywhere,
            normalized_entrypoint: Some("main".to_string()),
        }
    }

    pub fn module() -> Self {
        Self {
            exit_code_policy: ExitCodePolicy::Forbidden,
            normalized_entrypoint: None,
        }
    }

    pub fn std_module() -> Self {
        Self::module()
    }

    pub fn repl_chunk() -> Self {
        Self {
            exit_code_policy: ExitCodePolicy::Forbidden,
            normalized_entrypoint: None,
        }
    }

    pub fn project() -> Self {
        Self {
            exit_code_policy: ExitCodePolicy::Forbidden,
            normalized_entrypoint: None,
        }
    }

    pub fn with_exit_code_policy(
        mut self,
        policy: ExitCodePolicy,
        entrypoint: Option<&EntryPoint>,
    ) -> Self {
        self.exit_code_policy = policy;
        self.normalized_entrypoint = entrypoint.map(|entry| entry.qualified_symbol.clone());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePolicy {
    pub parse_profile: ParseProfile,
    pub parser_context: ParserContextKind,
    pub runtime_policy: RuntimeSourcePolicy,
    pub allows_builtin_decls: bool,
    pub allows_top_level_expr: bool,
}

impl SourceKind {
    pub fn parse_profile(self) -> ParseProfile {
        match self {
            Self::Script => ParseProfile::Script,
            Self::DefinitionSource => ParseProfile::Module,
            Self::StdDefinitionSource => ParseProfile::StdModule,
            Self::ProjectConfigSource => ParseProfile::Project,
            Self::ReplChunk => ParseProfile::ReplChunk,
        }
    }

    pub fn parser_context_kind(self, compile_unit_kind: CompileUnitKind) -> ParserContextKind {
        match (self, compile_unit_kind) {
            (Self::ProjectConfigSource, CompileUnitKind::Project) => ParserContextKind::Project,
            (Self::Script, _) => ParserContextKind::Script,
            (Self::ReplChunk, _) => ParserContextKind::Repl,
            _ => ParserContextKind::Module,
        }
    }

    pub fn runtime_policy(
        self,
        compile_unit_kind: CompileUnitKind,
        entrypoint: Option<&EntryPoint>,
    ) -> RuntimeSourcePolicy {
        let base = match self {
            Self::Script => RuntimeSourcePolicy::script(),
            Self::DefinitionSource => RuntimeSourcePolicy::module(),
            Self::StdDefinitionSource => RuntimeSourcePolicy::std_module(),
            Self::ProjectConfigSource => RuntimeSourcePolicy::project(),
            Self::ReplChunk => RuntimeSourcePolicy::repl_chunk(),
        };

        let exit_code_policy = match (self, compile_unit_kind) {
            (Self::Script, _) => ExitCodePolicy::Anywhere,
            (Self::ReplChunk, _) => ExitCodePolicy::Forbidden,
            (
                Self::DefinitionSource | Self::StdDefinitionSource | Self::ProjectConfigSource,
                CompileUnitKind::Project,
            ) => ExitCodePolicy::EntryOnly,
            (Self::DefinitionSource | Self::StdDefinitionSource | Self::ProjectConfigSource, _) => {
                ExitCodePolicy::Forbidden
            }
        };

        base.with_exit_code_policy(exit_code_policy, entrypoint)
    }

    pub fn allows_builtin_decls(self) -> bool {
        matches!(self, Self::StdDefinitionSource)
    }

    pub fn allows_top_level_expr(self) -> bool {
        matches!(
            self,
            Self::Script | Self::ProjectConfigSource | Self::ReplChunk
        )
    }

    pub fn policy(
        self,
        compile_unit_kind: CompileUnitKind,
        entrypoint: Option<&EntryPoint>,
    ) -> SourcePolicy {
        SourcePolicy {
            parse_profile: self.parse_profile(),
            parser_context: self.parser_context_kind(compile_unit_kind),
            runtime_policy: self.runtime_policy(compile_unit_kind, entrypoint),
            allows_builtin_decls: self.allows_builtin_decls(),
            allows_top_level_expr: self.allows_top_level_expr(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn std_definition_source_policy_enables_builtin_module_profile() {
        let policy = SourceKind::StdDefinitionSource.policy(CompileUnitKind::DefinitionCheck, None);

        assert_eq!(policy.parse_profile, ParseProfile::StdModule);
        assert_eq!(policy.parser_context, ParserContextKind::Module);
        assert!(!policy.allows_top_level_expr);
        assert!(policy.allows_builtin_decls);
        assert_eq!(
            policy.runtime_policy,
            RuntimeSourcePolicy::std_module()
                .with_exit_code_policy(ExitCodePolicy::Forbidden, None)
        );
    }

    #[test]
    fn project_config_policy_uses_project_parser_context_only_in_project_mode() {
        let project_policy = SourceKind::ProjectConfigSource.policy(CompileUnitKind::Project, None);
        let definition_policy =
            SourceKind::ProjectConfigSource.policy(CompileUnitKind::DefinitionCheck, None);

        assert_eq!(project_policy.parse_profile, ParseProfile::Project);
        assert_eq!(project_policy.parser_context, ParserContextKind::Project);
        assert!(project_policy.allows_top_level_expr);
        assert!(!project_policy.allows_builtin_decls);
        assert_eq!(
            project_policy.runtime_policy.exit_code_policy,
            ExitCodePolicy::EntryOnly
        );

        assert_eq!(definition_policy.parse_profile, ParseProfile::Project);
        assert_eq!(definition_policy.parser_context, ParserContextKind::Module);
        assert_eq!(
            definition_policy.runtime_policy.exit_code_policy,
            ExitCodePolicy::Forbidden
        );
    }

    #[test]
    fn script_policy_preserves_entrypoint_when_promoted_to_project_entry_only() {
        let entrypoint = EntryPoint::qualified("App::main");
        let policy = SourceKind::Script.policy(CompileUnitKind::Project, Some(&entrypoint));

        assert_eq!(policy.parse_profile, ParseProfile::Script);
        assert_eq!(policy.parser_context, ParserContextKind::Script);
        assert!(policy.allows_top_level_expr);
        assert!(!policy.allows_builtin_decls);
        assert_eq!(
            policy.runtime_policy.exit_code_policy,
            ExitCodePolicy::Anywhere
        );
        assert_eq!(
            policy.runtime_policy.normalized_entrypoint.as_deref(),
            Some("App::main")
        );
    }

    #[test]
    fn repl_chunk_policy_stays_repl_specific() {
        let policy = SourceKind::ReplChunk.policy(CompileUnitKind::Repl, None);

        assert_eq!(policy.parse_profile, ParseProfile::ReplChunk);
        assert_eq!(policy.parser_context, ParserContextKind::Repl);
        assert!(policy.allows_top_level_expr);
        assert!(!policy.allows_builtin_decls);
        assert_eq!(policy.runtime_policy, RuntimeSourcePolicy::repl_chunk());
    }
}
