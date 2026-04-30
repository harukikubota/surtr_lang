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
                return Err(ParseError::syntax(
                    "This top-level declaration is not allowed in the current source policy",
                    stmt.span().clone(),
                ));
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

fn top_level_decl_kind(ast: &Ast) -> Option<TopLevelDeclKind> {
    match ast {
        Ast::Def(_, _, _, _, _, _, _) => Some(TopLevelDeclKind::Def),
        Ast::ExtractorDef(_, _, _, _, _, _, _) => Some(TopLevelDeclKind::ExtractorDef),
        Ast::Defmod(_, _, _, _) => Some(TopLevelDeclKind::Defmod),
        Ast::Namespace(_, _, _) => Some(TopLevelDeclKind::Namespace),
        Ast::ImplDef(_, _, _, _) => Some(TopLevelDeclKind::ImplDef),
        Ast::TraitDef(_, _, _, _, _) => Some(TopLevelDeclKind::TraitDef),
        Ast::TraitImplDef(_, _, _, _, _, _) => Some(TopLevelDeclKind::TraitImplDef),
        Ast::Import(_, _, _) => Some(TopLevelDeclKind::Import),
        Ast::Include(_, _) => Some(TopLevelDeclKind::Include),
        Ast::StructDef(_, _, _) => Some(TopLevelDeclKind::StructDef),
        Ast::RecordDef(_, _, _) => Some(TopLevelDeclKind::RecordDef),
        Ast::DeferrorDef(_, _, _, _, _) => Some(TopLevelDeclKind::DeferrorDef),
        Ast::EnumDef(_, _, _, _, _) => Some(TopLevelDeclKind::EnumDef),
        Ast::BuiltinDecl(_, _, _, _, _) => Some(TopLevelDeclKind::BuiltinDecl),
        Ast::BuiltinExtractorDecl(_, _, _, _, _) => Some(TopLevelDeclKind::BuiltinExtractorDecl),
        Ast::BuiltinTypeDecl(_, _, _) => Some(TopLevelDeclKind::BuiltinTypeDecl),
        Ast::ResultCtorDecl(_, _, _, _, _) => Some(TopLevelDeclKind::BuiltinDecl),
        _ => None,
    }
}
