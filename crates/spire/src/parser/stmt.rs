use crate::ast::*;
use crate::error::ParseError;
use crate::token::Token;

use super::context::{DeclLevel, ParseUnitKind, TopLevelDeclKind};
use super::{validate, ParseRules, Parser};

impl Parser<'_> {
    pub(super) fn parse_stmt(&mut self) -> Result<Ast, ParseError> {
        self.skip_newlines();

        if self.context.level == DeclLevel::Expr
            && matches!(
                self.peek(),
                Token::Annotator(_)
                    | Token::Def
                    | Token::Defp
                    | Token::Defagent
                    | Token::Defgenserver
                    | Token::Defsupervisor
                    | Token::DefdynamicSupervisor
                    | Token::SupervisorInit
                    | Token::Defmod
                    | Token::Namespace
                    | Token::Deftrait
                    | Token::Impl
                    | Token::Import
                    | Token::Include
                    | Token::Private
                    | Token::Public
                    | Token::Const
                    | Token::Defstruct
                    | Token::Defrecord
                    | Token::Deferror
                    | Token::Defenum
                    | Token::Defextractor
            )
        {
            return Err(ParseError::syntax(
                "Declarations are only allowed at the top level",
                self.peek_span(),
            ));
        }

        // Data definitions
        let stmt = match self.peek() {
            Token::Annotator(_) => self.parse_annotated_decl()?,
            Token::Def | Token::Defp => self.parse_def()?,
            Token::Defagent => self.parse_defagent_without_legacy_meta()?,
            Token::Defgenserver => self.parse_defgenserver()?,
            Token::Defsupervisor => self.parse_defsupervisor(false)?,
            Token::DefdynamicSupervisor => self.parse_defsupervisor(true)?,
            Token::SupervisorInit => self.parse_supervisor_init()?,
            Token::Defmod => self.parse_defmod()?,
            Token::Namespace => self.parse_namespace()?,
            Token::Deftrait => self.parse_trait_def()?,
            Token::Impl => self.parse_impl_def()?,
            Token::Import => self.parse_import()?,
            Token::Include => self.parse_include()?,
            Token::Private | Token::Public | Token::Const => self.parse_const_def()?,
            Token::Defstruct => self.parse_struct_def()?,
            Token::Defrecord => self.parse_record_def()?,
            Token::Deferror => self.parse_deferror_def()?,
            Token::Defenum => self.parse_enum_def()?,
            Token::Defextractor => self.parse_extractor_def()?,
            _ => {
                if self.is_pattern_bind_stmt_start() {
                    let save = self.pos;
                    match self.parse_pattern_bind_stmt() {
                        Ok(stmt) => {
                            if matches!(self.peek(), Token::Semicolon) {
                                let semi = self.advance().span.clone();
                                let span = Span {
                                    start: stmt.span().start,
                                    end: semi.end,
                                };
                                let wrapped = Ast::Semi(span, Box::new(stmt));
                                self.validate_stmt_by_context(&wrapped)?;
                                return Ok(wrapped);
                            }
                            self.validate_stmt_by_context(&stmt)?;
                            return Ok(stmt);
                        }
                        Err(err) => {
                            let looks_like_bind = matches!(
                                self.tokens.get(save).map(|sp| &sp.token),
                                Some(Token::LParen | Token::LBrack)
                            ) && self
                                .stmt_has_top_level_assignment_from(save)
                                || matches!(
                                    self.tokens.get(save).map(|sp| &sp.token),
                                    Some(Token::Ident(_))
                                ) && self.stmt_has_top_level_assignment_from(save)
                                    && self.stmt_has_top_level_at_from(save);
                            self.pos = save;
                            if looks_like_bind {
                                return Err(err);
                            }
                        }
                    }
                }

                let expr = self.parse_expr()?;
                if matches!(self.peek(), Token::Semicolon) {
                    let semi = self.advance().span.clone();
                    let span = Span {
                        start: expr.span().start,
                        end: semi.end,
                    };
                    Ast::Semi(span, Box::new(expr))
                } else {
                    expr
                }
            }
        };

        self.validate_stmt_by_context(&stmt)?;

        Ok(stmt)
    }

    pub(super) fn validate_stmt_by_context(&self, stmt: &Ast) -> Result<(), ParseError> {
        validate::validate_stmt_by_context(&self.context, stmt)
    }

    pub(super) fn parse_module_body_stmts(
        &mut self,
        module_path: Option<String>,
    ) -> Result<Vec<Ast>, ParseError> {
        let prev_context = self.context.clone();
        self.context.level = DeclLevel::Top;
        self.context.unit_kind = ParseUnitKind::Module;
        self.context.module_path = module_path;
        self.context.parse_rules = if prev_context
            .parse_rules
            .allowed_top_level_decl_kinds
            .allows(TopLevelDeclKind::BuiltinDecl)
        {
            ParseRules::std_module_member()
        } else {
            ParseRules::module_member()
        };

        let result = (|| {
            let mut stmts = Vec::new();
            self.skip_newlines();

            while !matches!(self.peek(), Token::RBrace) {
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete("}", self.peek_span()));
                }
                let stmt = self.parse_stmt()?;
                self.ensure_stmt_boundary(&stmt, true)?;
                stmts.push(stmt);
                while matches!(self.peek(), Token::Newline) {
                    self.advance();
                }
            }

            Ok(stmts)
        })();

        self.context = prev_context;
        result
    }

    pub(super) fn parse_namespace_body_stmts(&mut self) -> Result<Vec<Ast>, ParseError> {
        let prev_context = self.context.clone();
        self.context.level = DeclLevel::Top;
        self.context.module_path = None;
        self.context.parse_rules =
            if prev_context.parse_rules == crate::parser::context::ParseRules::std_module() {
                crate::parser::context::ParseRules::std_module()
            } else {
                crate::parser::context::ParseRules::module_source_without_builtin()
            };

        let result = (|| {
            let mut stmts = Vec::new();
            self.skip_newlines();

            while !matches!(self.peek(), Token::RBrace) {
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete("}", self.peek_span()));
                }
                let stmt = self.parse_stmt()?;
                self.ensure_stmt_boundary(&stmt, true)?;
                stmts.push(stmt);
                while matches!(self.peek(), Token::Newline) {
                    self.advance();
                }
            }

            Ok(stmts)
        })();

        self.context = prev_context;
        result
    }

    pub(super) fn parse_block_stmts(&mut self) -> Result<Vec<Ast>, ParseError> {
        let prev_level = self.context.level;
        self.context.level = DeclLevel::Expr;
        let result = (|| {
            let mut stmts = Vec::new();
            self.skip_newlines();

            while !matches!(self.peek(), Token::RBrace) {
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete("}", self.peek_span()));
                }
                let stmt = self.parse_stmt()?;
                self.ensure_stmt_boundary(&stmt, true)?;
                stmts.push(stmt);
                while matches!(self.peek(), Token::Newline) {
                    self.advance();
                }
            }

            Ok(stmts)
        })();
        self.context.level = prev_level;
        result
    }
}
