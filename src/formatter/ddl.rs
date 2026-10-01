//! DDL と MERGE のレイアウト。
//!
//! - CREATE TABLE の列と制約は、1 つでも必ず 1 行ずつ行頭カンマで並べる
//! - CREATE TABLE ... AS / CREATE VIEW ... AS の問い合わせは、次の行から書く
//! - CREATE INDEX の WHERE は次の行に置く
//! - ALTER TABLE の操作が 2 つ以上なら、1 行ずつ行頭カンマで並べる
//! - CREATE TRIGGER は名前の後ろの句を、CREATE / ALTER SEQUENCE はオプションを 1 行ずつ並べる
//! - CREATE TYPE の複合型の列は、CREATE TABLE の列と同じく 1 行ずつ並べる
//! - COMMENT ON / GRANT / REVOKE は 1 行に書く
//! - MERGE は USING / WHEN を行頭に置き、ON と各 WHEN の処理を 1 段深くする

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
                    // 問い合わせの後ろの `WITH [NO] DATA` / `WITH CHECK OPTION`
                    if after_query {
                        self.w.newline(base);
                        after_query = false;
                    }
                    self.element(element);
                }
            }
        }
    }

    /// CREATE TRIGGER の句・シーケンスのオプションは、1 行ずつ字下げせずに並べる（CREATE FUNCTION のオプションと同じ）。
    /// それ以外は同じ行に続け、名前の直後の引数の括弧は続けて書く（`f(int)`。`CAST (a AS b)` や `SELECT (a, b)` は離す）。
    /// COMMENT ON / GRANT / ALTER などもこれで 1 行に書く
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

    /// `WHEN ... THEN` の後ろの処理を 1 段深い次の行に書く。INSERT の VALUES はさらに次の行。
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
