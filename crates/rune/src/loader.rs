use std::fs;
use std::path::{Path, PathBuf};

use spire::token::Token;

use crate::error::{ExecutionEnv, RuneError, RuneResult};
use crate::util::display_path;

pub(crate) fn collect_lib_root_sources(env: ExecutionEnv) -> RuneResult<Vec<(PathBuf, String)>> {
    let lib_dir = Path::new("lib");
    let entries = fs::read_dir(lib_dir).map_err(|e| {
        RuneError::message(
            1,
            format!(
                "{}: failed to read `{}`: {}",
                env.command_name(),
                lib_dir.display(),
                e
            ),
        )
    })?;
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == "srt")
        {
            files.push(path);
        }
    }
    files.sort();

    let mut sources = Vec::with_capacity(files.len());
    for path in files {
        let source = fs::read_to_string(&path).map_err(|e| {
            RuneError::message(
                1,
                format!(
                    "{}: failed to read `{}`: {}",
                    env.command_name(),
                    display_path(&path),
                    e
                ),
            )
        })?;
        sources.push((path, source));
    }
    Ok(sources)
}

pub(crate) fn collect_additional_std_module_inputs(
    env: ExecutionEnv,
) -> RuneResult<Vec<xldr::ModuleInput>> {
    if !Path::new("lib").exists() {
        return Ok(Vec::new());
    }
    let lib_sources = collect_lib_root_sources(env)?;
    let mut module_inputs = Vec::new();
    for (path, source) in lib_sources {
        let file_path = display_path(&path);
        let module_path = derive_primary_module_path(&source)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| module_path_from_file_name(&path));

        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !xldr::is_default_std_module_file_name(file_name)
            && !xldr::is_default_std_module_path(&module_path)
        {
            module_inputs.push(xldr::ModuleInput {
                file_name: file_path,
                source,
                module_path,
            });
        }
    }

    Ok(module_inputs)
}

pub(crate) fn module_path_from_file_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(ToString::to_string)
        .unwrap_or_default()
}

pub(crate) fn derive_primary_module_path(source: &str) -> Option<String> {
    if let Ok(ast) = spire::parse(source) {
        let lowered = xldr::lower_module_source_ast(ast, None);
        if let Some(module_path) = lowered
            .into_iter()
            .find(|module| module.declared_span.is_some() && !module.module_path.is_empty())
            .map(|module| module.module_path)
        {
            return Some(module_path);
        }
    }

    let tokens = spire::lexer::tokenize(source).ok()?;
    for (idx, spanned) in tokens.iter().enumerate() {
        if !matches!(spanned.token, Token::Defmod) {
            continue;
        }

        let mut j = idx + 1;
        while matches!(tokens.get(j).map(|t| &t.token), Some(Token::Newline)) {
            j += 1;
        }

        let mut segments = Vec::new();
        match tokens.get(j).map(|sp| &sp.token) {
            Some(Token::Ident(name)) => {
                segments.push(name.clone());
                j += 1;
            }
            _ => return None,
        }

        while matches!(tokens.get(j).map(|t| &t.token), Some(Token::Colon))
            && matches!(tokens.get(j + 1).map(|t| &t.token), Some(Token::Colon))
        {
            j += 2;
            match tokens.get(j).map(|sp| &sp.token) {
                Some(Token::Ident(name)) => {
                    segments.push(name.clone());
                    j += 1;
                }
                _ => return None,
            }
        }

        if !segments.is_empty() {
            return Some(segments.join("::"));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::derive_primary_module_path;

    #[test]
    fn derive_primary_module_path_reads_simple_defmod() {
        assert_eq!(
            derive_primary_module_path(
                "defmod Kernel {\n  def add(x: Int, y: Int) -> Int { x + y }\n}"
            ),
            Some("Kernel".to_string())
        );
    }

    #[test]
    fn derive_primary_module_path_reads_qualified_defmod() {
        assert_eq!(
            derive_primary_module_path("defmod A::B {\n  def ping() -> Int { 1 }\n}"),
            Some("A::B".to_string())
        );
    }

    #[test]
    fn derive_primary_module_path_skips_comments_and_blank_lines() {
        assert_eq!(
            derive_primary_module_path(
                "\n\n# comment\ndefmod Math {\n  def add(x: Int, y: Int) -> Int { x + y }\n}"
            ),
            Some("Math".to_string())
        );
    }

    #[test]
    fn derive_primary_module_path_returns_none_without_defmod() {
        assert_eq!(
            derive_primary_module_path("def add(x: Int, y: Int) -> Int { x + y }\n"),
            None
        );
    }
}
