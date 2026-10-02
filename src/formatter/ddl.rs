//! Layout of DDL and MERGE.
//!
//! - CREATE TABLE columns and constraints always go one per line with leading commas, even when
//!   there is only one
//! - The query of CREATE TABLE ... AS / CREATE VIEW ... AS starts on the next line
//! - The WHERE of CREATE INDEX goes on the next line
//! - Two or more ALTER TABLE actions go one per line with leading commas
//! - CREATE TRIGGER puts the clauses after the name, and CREATE / ALTER SEQUENCE its options,
//!   one per line
//! - Columns of a CREATE TYPE composite type go one per line, like CREATE TABLE columns
//! - COMMENT ON / GRANT / REVOKE stay on one line
//! - MERGE puts USING / WHEN at the start of a line, with ON and each WHEN's action one level
//!   deeper

use super::{Formatter, as_node, children, is_statement};
use crate::lexer::TokenKind;
use crate::syntax::{Element, Node, NodeKind};

impl<'a> Formatter<'a> {
    /// CREATE TABLE / CREATE VIEW / CREATE INDEX
    pub(super) fn create_object(&mut self, stmt: &Node<'a>, base: usize) {
        let mut after_query = false;
        for element in children(stmt) {
            match as_node(element) {
                Some(n) if n.kind == NodeKind::TableElementList => self.paren_list_broken(n),
                Some(n) if is_statement(n.kind) => {
                    self.w.newline(base);
                    self.statement(n, base);
                    after_query = true;
                }
                Some(n) if n.kind == NodeKind::WhereClause => {
                    self.w.newline(base);
                    self.condition_clause(n, base);
                }
                _ => {
                    // `WITH [NO] DATA` / `WITH CHECK OPTION` after the query
                    if after_query {
                        self.w.newline(base);
                        after_query = false;
                    }
                    self.element(element);
                }
            }
        }
    }

    /// CREATE TRIGGER clauses and sequence options go one per line without indentation (like
    /// CREATE FUNCTION options).
    /// Everything else continues on the same line, and the argument parentheses right after the
    /// name are glued to it (`f(int)`; `CAST (a AS b)` and `SELECT (a, b)` keep the space).
    /// COMMENT ON / GRANT / ALTER and the like are also written on one line through this
    pub(super) fn clause_per_line(&mut self, stmt: &Node<'a>, base: usize) {
        let mut after_name = false;
        for element in children(stmt) {
            match as_node(element) {
                Some(n) if matches!(n.kind, NodeKind::TriggerClause | NodeKind::SequenceOption) => {
                    self.w.newline(base);
                    self.node(n);
                }
                Some(n) if after_name && n.kind == NodeKind::ExprList => {
                    self.w.glue();
                    self.node(n);
                }
                _ => self.element(element),
            }
            after_name = matches!(
                element,
                Element::Token(t) if matches!(
                    t.kind,
                    TokenKind::Ident | TokenKind::QuotedIdent { .. } | TokenKind::PsqlVariable
                )
            );
        }
    }

    pub(super) fn merge_stmt(&mut self, stmt: &Node<'a>, base: usize) {
        for element in children(stmt) {
            match element {
                Element::Node(n) => match n.kind {
                    NodeKind::WithClause => {
                        self.query_part(n, base);
                        self.w.newline(base);
                    }
                    NodeKind::JoinCondition => {
                        self.w.newline(base + self.indent_width);
                        self.condition_clause(n, base + self.indent_width);
                    }
                    NodeKind::MergeWhenClause => {
                        self.w.newline(base);
                        self.merge_when_clause(n, base);
                    }
                    NodeKind::ReturningClause | NodeKind::IntoClause => {
                        self.w.newline(base);
                        self.query_part(n, base);
                    }
                    _ => self.node(n),
                },
                Element::Token(t)
                    if t.kind == TokenKind::Keyword && t.text.eq_ignore_ascii_case("using") =>
                {
                    self.w.newline(base);
                    self.w.token(t);
                }
                Element::Token(t) => self.w.token(t),
            }
        }
    }

    /// The action after `WHEN ... THEN` goes on the next line, one level deeper. The VALUES of an
    /// INSERT goes on the line after that.
    fn merge_when_clause(&mut self, node: &Node<'a>, base: usize) {
        let action = base + self.indent_width;
        let mut after_then = false;
        let mut action_started = false;
        for element in children(node) {
            if after_then && !action_started {
                self.w.newline(action);
                action_started = true;
            }
            match element {
                Element::Node(n) if n.kind == NodeKind::SetClause => self.query_part(n, action),
                Element::Node(n) if n.kind == NodeKind::ValuesClause => {
                    self.w.newline(action);
                    self.query_part(n, action);
                }
                Element::Token(t)
                    if t.kind == TokenKind::Keyword && t.text.eq_ignore_ascii_case("then") =>
                {
                    self.w.token(t);
                    after_then = true;
                }
                _ => self.element(element),
            }
        }
    }
}
