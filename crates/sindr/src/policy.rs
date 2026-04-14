#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileUnitKind {
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
