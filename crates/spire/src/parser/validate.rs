use crate::ast::Ast;
use crate::error::ParseError;

use super::context::{DeclLevel, ParseUnitKind, ParserContext, TopLevelDeclKind};

pub(crate) fn validate_stmt_by_context(
    context: &ParserContext,
    stmt: &Ast,
) -> Result<(), ParseError> {
    if context.level == DeclLevel::Top {
        if let Some(kind) = top_level_decl_kind(stmt) {
            if !context
                .parse_rules
                .allowed_top_level_decl_kinds
                .allows(kind)
            {
                let message = match (context.unit_kind, kind) {
                    (ParseUnitKind::Script, TopLevelDeclKind::Defmod) => {
                        "defmod is not allowed at script top-level"
                    }
                    (ParseUnitKind::Repl, TopLevelDeclKind::ConstDef) => {
                        "REPL chunks only allow top-level def and import declarations"
                    }
                    (ParseUnitKind::Repl, _) => {
                        "This top-level declaration is not allowed in REPL chunks"
                    }
                    _ => "This top-level declaration is not allowed in the current source policy",
                };
                return Err(ParseError::syntax(message, stmt.span().clone()));
            }
        } else if !context.parse_rules.allow_top_level_expr {
            let message = if context.unit_kind == ParseUnitKind::Module {
                "Top-level expressions are not allowed in module compile units"
            } else {
                "Top-level expressions are not allowed in this source context"
            };
            return Err(ParseError::syntax(message, stmt.span().clone()));
        }
    }
    Ok(())
}

pub(crate) fn validate_program_by_context(
    context: &ParserContext,
    ast: &[Ast],
) -> Result<(), ParseError> {
    if context.unit_kind != ParseUnitKind::Script || context.level != DeclLevel::Top {
        return Ok(());
    }

    let mut seen_non_include = false;
    let mut seen_expr = false;

    for stmt in ast {
        match top_level_decl_kind(stmt) {
            Some(TopLevelDeclKind::Include) => {
                if seen_non_include {
                    return Err(ParseError::syntax(
                        "include directive must appear before declarations and top-level expressions",
                        stmt.span().clone(),
                    ));
                }
            }
            Some(_) => {
                seen_non_include = true;
                if seen_expr {
                    return Err(ParseError::syntax(
                        "top-level definition cannot appear after top-level expression",
                        stmt.span().clone(),
                    ));
                }
            }
            None => {
                seen_non_include = true;
                seen_expr = true;
            }
        }
    }

    Ok(())
}

fn top_level_decl_kind(ast: &Ast) -> Option<TopLevelDeclKind> {
    match ast {
        Ast::Def(_, _, _, _, _, _, _) => Some(TopLevelDeclKind::Def),
        Ast::ExtractorDef(_, _, _, _, _, _, _) => Some(TopLevelDeclKind::ExtractorDef),
        Ast::Defmod(_, _, _, _) => Some(TopLevelDeclKind::Defmod),
        Ast::Defagent(_, _, _, _, _) => Some(TopLevelDeclKind::Defagent),
        Ast::Defgenserver(_, _, _, _, _) => Some(TopLevelDeclKind::Defgenserver),
        Ast::Defsupervisor(_, _, _, _, _) => Some(TopLevelDeclKind::Defsupervisor),
        Ast::DefdynamicSupervisor(_, _, _, _, _) => Some(TopLevelDeclKind::DefdynamicSupervisor),
        Ast::Namespace(_, _, _) => Some(TopLevelDeclKind::Namespace),
        Ast::ImplDef(_, _, _, _) => Some(TopLevelDeclKind::ImplDef),
        Ast::TraitDef(_, _, _, _, _) => Some(TopLevelDeclKind::TraitDef),
        Ast::TraitImplDef(_, _, _, _, _, _) => Some(TopLevelDeclKind::TraitImplDef),
        Ast::Import(_, _, _) => Some(TopLevelDeclKind::Import),
        Ast::Include(_, _) => Some(TopLevelDeclKind::Include),
        Ast::StructDef(..) => Some(TopLevelDeclKind::StructDef),
        Ast::RecordDef(_, _, _, _) => Some(TopLevelDeclKind::RecordDef),
        Ast::DeferrorDef(_, _, _, _, _) => Some(TopLevelDeclKind::DeferrorDef),
        Ast::EnumDef(_, _, _, _, _) => Some(TopLevelDeclKind::EnumDef),
        Ast::ConstDef(_, _, _, _, _) => Some(TopLevelDeclKind::ConstDef),
        Ast::SupervisorInit(_, _) => Some(TopLevelDeclKind::SupervisorInit),
        Ast::BuiltinDecl(_, _, _, _, _) => Some(TopLevelDeclKind::BuiltinDecl),
        Ast::IntrinsicDecl(_, _, _, _) => Some(TopLevelDeclKind::BuiltinDecl),
        Ast::BuiltinExtractorDecl(_, _, _, _, _) => Some(TopLevelDeclKind::BuiltinExtractorDecl),
        Ast::BuiltinTypeDecl(_, _, _) => Some(TopLevelDeclKind::BuiltinTypeDecl),
        Ast::ResultCtorDecl(_, _, _, _, _) => Some(TopLevelDeclKind::BuiltinDecl),
        _ => None,
    }
}
