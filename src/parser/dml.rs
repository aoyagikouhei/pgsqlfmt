//! INSERT / UPDATE / DELETE 文と、文の入口（WITH の後ろで種類を振り分ける）。

use super::Parser;
use crate::lexer::TokenKind;
use crate::syntax::NodeKind;

impl Parser<'_> {
    /// SELECT / INSERT / UPDATE / DELETE（WITH 付きを含む）を 1 つ読む。
    /// どれでもなければ何も取り込まずに false を返す。
    pub(super) fn statement_body(&mut self) -> bool {
        if !self.at_query_start(0) && !self.at_any_kw(&["insert", "update", "delete"]) {
            return false;
        }
        let cp = self.checkpoint();
        if self.at_kw("with") {
            self.with_clause();
        }
        let kind = if self.at_kw("insert") {
            self.insert_rest();
            NodeKind::InsertStmt
        } else if self.at_kw("update") {
            self.update_rest();
            NodeKind::UpdateStmt
        } else if self.at_kw("delete") {
            self.delete_rest();
            NodeKind::DeleteStmt
        } else {
            self.select_rest();
            NodeKind::SelectStmt
        };
        self.wrap(cp, kind);
        true
    }

    /// `INSERT INTO table [AS alias] [(col, ...)] [OVERRIDING ... VALUE]
    ///  {DEFAULT VALUES | query} [ON CONFLICT ...] [RETURNING ...]`
    fn insert_rest(&mut self) {
        self.bump();
        self.eat_kw("into");
        if self.at_name() {
            let cp = self.checkpoint();
            self.name_path();
            // VALUES なども予約語ではないので、AS なしの別名は受け付けない
            if self.at_kw("as") {
                self.opt_alias(false);
            }
            self.wrap(cp, NodeKind::TableRef);
        }
        if self.at(TokenKind::LParen) && !self.at_query_start(1) {
            self.expr_list();
        }
        if self.eat_kw("overriding") {
            if !self.eat_kw("system") {
                self.eat_kw("user");
            }
            self.eat_kw("value");
        }
        if self.at_kw("default") && self.nth_kw(1, "values") {
            self.bump();
            self.bump();
        } else if self.at_query_start(0) {
            self.select_stmt();
        }
        if self.at_kw("on") && self.nth_kw(1, "conflict") {
            self.on_conflict_clause();
        }
        if self.at_kw("returning") {
            self.returning_clause();
        }
    }

    /// `ON CONFLICT [(target, ...) [WHERE ...] | ON CONSTRAINT name]
    ///  DO {NOTHING | UPDATE SET ... [WHERE ...]}`
    fn on_conflict_clause(&mut self) {
        self.start_node(NodeKind::OnConflictClause);
        self.bump();
        self.bump();
        if self.at(TokenKind::LParen) {
            self.expr_list();
            if self.at_kw("where") {
                self.where_clause();
            }
        } else if self.at_kw("on") && self.nth_kw(1, "constraint") {
            self.bump();
            self.bump();
            self.name_path();
        }
        if self.eat_kw("do") && !self.eat_kw("nothing") && self.eat_kw("update") {
            if self.at_kw("set") {
                self.set_clause();
            }
            if self.at_kw("where") {
                self.where_clause();
            }
        }
        self.finish_node();
    }

    /// `UPDATE [ONLY] table [*] [[AS] alias] SET ... [FROM ...] [WHERE ...] [RETURNING ...]`
    fn update_rest(&mut self) {
        self.bump();
        self.dml_target(&["set"]);
        if self.at_kw("set") {
            self.set_clause();
        }
        if self.at_kw("from") {
            self.table_source_clause();
        }
        if self.at_kw("where") {
            self.where_clause();
        }
        if self.at_kw("returning") {
            self.returning_clause();
        }
    }

    /// `DELETE FROM [ONLY] table [*] [[AS] alias] [USING ...] [WHERE ...] [RETURNING ...]`
    fn delete_rest(&mut self) {
        self.bump();
        self.eat_kw("from");
        self.dml_target(&[]);
        if self.at_kw("using") {
            self.start_node(NodeKind::UsingClause);
            self.bump();
            self.comma_list(Self::at_clause_keyword, Self::table_expr);
            self.finish_node();
        }
        if self.at_kw("where") {
            self.where_clause();
        }
        if self.at_kw("returning") {
            self.returning_clause();
        }
    }

    /// UPDATE / DELETE の対象の表。`not_bare` は AS なしの別名にしないキーワード。
    fn dml_target(&mut self, not_bare: &[&str]) {
        if !self.at_name() {
            return;
        }
        let cp = self.checkpoint();
        self.eat_kw("only");
        self.name_path();
        if self.at_op("*") {
            self.bump();
        }
        self.opt_alias_except(false, not_bare);
        self.wrap(cp, NodeKind::TableRef);
    }

    /// `SET col = expr, (a, b) = (...), ...`
    fn set_clause(&mut self) {
        self.start_node(NodeKind::SetClause);
        self.bump();
        self.comma_list(Self::at_clause_keyword, Self::set_item);
        self.finish_node();
    }

    fn set_item(&mut self) -> bool {
        let cp = self.checkpoint();
        if self.at(TokenKind::LParen) {
            self.expr_list();
        } else if !self.set_target() {
            return false;
        }
        if self.at_op("=") {
            self.bump();
            self.expr();
        }
        self.wrap(cp, NodeKind::SetItem);
        true
    }

    /// `RETURNING [WITH (OLD AS o, NEW AS n)] item, ...`
    fn returning_clause(&mut self) {
        self.start_node(NodeKind::ReturningClause);
        self.bump();
        if self.at_kw("with") && self.nth_is(1, TokenKind::LParen) {
            self.bump();
            self.bump_balanced();
        }
        self.comma_list(Self::at_clause_keyword, Self::target_item);
        self.finish_node();
    }
}
