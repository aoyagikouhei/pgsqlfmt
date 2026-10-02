//! INSERT / UPDATE / DELETE statements, and the statement entry point (which dispatches on the
//! kind of statement that follows WITH).

use super::Parser;
use crate::lexer::TokenKind;
use crate::syntax::NodeKind;

impl Parser<'_> {
    /// Reads one SELECT / INSERT / UPDATE / DELETE (with or without WITH).
    /// Returns false, consuming nothing, if it is none of these.
    pub(super) fn statement_body(&mut self) -> bool {
        if !self.at_query_start(0) && !self.at_any_kw(&["insert", "update", "delete", "merge"]) {
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
        } else if self.at_kw("merge") {
            self.merge_rest();
            NodeKind::MergeStmt
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
        self.bump_kw();
        self.eat_kw("into");
        if self.at_name() {
            let cp = self.checkpoint();
            self.name_path();
            // VALUES and the like are not reserved words, so an alias without AS is not accepted
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
            self.bump_kw();
            self.bump_kw();
        } else if self.at_query_start(0) {
            self.select_stmt();
        }
        if self.at_kw("on") && self.nth_kw(1, "conflict") {
            self.on_conflict_clause();
        }
        if self.at_kw("returning") {
            self.returning_clause();
        }
        if self.at_kw("into") {
            self.result_into_clause();
        }
    }

    /// `ON CONFLICT [(target, ...) [WHERE ...] | ON CONSTRAINT name]
    ///  DO {NOTHING | UPDATE SET ... [WHERE ...]}`
    fn on_conflict_clause(&mut self) {
        self.start_node(NodeKind::OnConflictClause);
        self.bump_kw();
        self.bump_kw();
        if self.at(TokenKind::LParen) {
            self.expr_list();
            if self.at_kw("where") {
                self.where_clause();
            }
        } else if self.at_kw("on") && self.nth_kw(1, "constraint") {
            self.bump_kw();
            self.bump_kw();
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
        self.bump_kw();
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
        if self.at_kw("into") {
            self.result_into_clause();
        }
    }

    /// `DELETE FROM [ONLY] table [*] [[AS] alias] [USING ...] [WHERE ...] [RETURNING ...]`
    fn delete_rest(&mut self) {
        self.bump_kw();
        self.eat_kw("from");
        self.dml_target(&[]);
        if self.at_kw("using") {
            self.start_node(NodeKind::UsingClause);
            self.bump_kw();
            self.comma_list(Self::at_clause_keyword, Self::table_expr);
            self.finish_node();
        }
        if self.at_kw("where") {
            self.where_clause();
        }
        if self.at_kw("returning") {
            self.returning_clause();
        }
        if self.at_kw("into") {
            self.result_into_clause();
        }
    }

    /// `MERGE INTO target [[AS] alias] USING source ON cond WHEN ... [RETURNING ...]`
    fn merge_rest(&mut self) {
        self.bump_kw();
        self.eat_kw("into");
        self.dml_target(&[]);
        if self.eat_kw("using") {
            self.table_expr();
        }
        if self.at_kw("on") {
            self.start_node(NodeKind::JoinCondition);
            self.bump_kw();
            self.expr();
            self.finish_node();
        }
        // SET and conditions inside a WHEN clause end before the next WHEN
        self.with_stops(&["when"], |p| {
            while p.at_kw("when") {
                p.merge_when_clause();
            }
        });
        if self.at_kw("returning") {
            self.returning_clause();
        }
        if self.at_kw("into") {
            self.result_into_clause();
        }
    }

    /// `WHEN [NOT] MATCHED [BY SOURCE | BY TARGET] [AND cond] THEN
    ///  {UPDATE SET ... | DELETE | DO NOTHING
    ///   | INSERT [(cols)] [OVERRIDING ...] {VALUES (...) | DEFAULT VALUES}}`
    fn merge_when_clause(&mut self) {
        self.start_node(NodeKind::MergeWhenClause);
        self.bump_kw();
        self.eat_kw("not");
        self.eat_kw("matched");
        if self.eat_kw("by") && !self.eat_kw("source") {
            self.eat_kw("target");
        }
        if self.eat_kw("and") {
            self.expr();
        }
        self.eat_kw("then");
        if self.eat_kw("update") {
            if self.at_kw("set") {
                self.set_clause();
            }
        } else if self.eat_kw("do") {
            self.eat_kw("nothing");
        } else if self.eat_kw("insert") {
            if self.at(TokenKind::LParen) {
                self.expr_list();
            }
            if self.eat_kw("overriding") {
                if !self.eat_kw("system") {
                    self.eat_kw("user");
                }
                self.eat_kw("value");
            }
            if self.at_kw("default") && self.nth_kw(1, "values") {
                self.bump_kw();
                self.bump_kw();
            } else if self.at_kw("values") {
                self.start_node(NodeKind::ValuesClause);
                self.bump_kw();
                if self.at(TokenKind::LParen) {
                    self.expr_list();
                }
                self.finish_node();
            }
        } else {
            self.eat_kw("delete");
        }
        self.finish_node();
    }

    /// The target table of UPDATE / DELETE. `not_bare` lists keywords that are not taken as an
    /// alias without AS.
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
        self.bump_kw();
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
        self.bump_kw();
        if self.at_kw("with") && self.nth_is(1, TokenKind::LParen) {
            self.bump_kw();
            self.bump_balanced();
        }
        self.comma_list(Self::at_clause_keyword, Self::target_item);
        self.finish_node();
    }
}
