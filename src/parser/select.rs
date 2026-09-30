//! SELECT 文（WITH・集合演算・VALUES・TABLE を含む問い合わせ）。

use super::Parser;
use super::keywords::{CLAUSE_KEYWORDS, JOIN_KEYWORDS, is_join_keyword, is_reserved};
use crate::lexer::TokenKind;
use crate::syntax::NodeKind;

impl Parser<'_> {
    /// n 番目から問い合わせ（SELECT / WITH / VALUES / TABLE、またはそれを括弧で囲んだもの）が始まるか
    pub(super) fn at_query_start(&self, n: usize) -> bool {
        let mut n = n;
        while self.nth_is(n, TokenKind::LParen) {
            n += 1;
        }
        ["select", "with", "values", "table"]
            .iter()
            .any(|kw| self.nth_kw(n, kw))
    }

    pub(super) fn at_clause_keyword(&self) -> bool {
        self.at_any_kw(CLAUSE_KEYWORDS)
    }

    pub(super) fn select_stmt(&mut self) {
        let cp = self.checkpoint();
        if self.at_kw("with") {
            self.with_clause();
        }
        self.select_rest();
        self.wrap(cp, NodeKind::SelectStmt);
    }

    /// WITH より後ろの問い合わせ本体。呼び出し側で `SelectStmt` に包む。
    pub(super) fn select_rest(&mut self) {
        self.select_body(0);
        loop {
            if self.at_kw("order") && self.nth_kw(1, "by") {
                self.order_by_clause(Self::at_clause_keyword);
            } else if self.at_kw("limit") {
                self.start_node(NodeKind::LimitClause);
                self.bump();
                if !self.eat_kw("all") {
                    self.expr();
                }
                self.finish_node();
            } else if self.at_kw("offset") {
                self.start_node(NodeKind::OffsetClause);
                self.bump();
                self.expr();
                if !self.eat_kw("rows") {
                    self.eat_kw("row");
                }
                self.finish_node();
            } else if self.at_kw("fetch") {
                self.keyword_clause(NodeKind::FetchClause);
            } else if self.at_kw("for") {
                self.keyword_clause(NodeKind::LockingClause);
            } else {
                break;
            }
        }
    }

    /// 中身を細かく解釈しない句（`FETCH FIRST ...` / `FOR UPDATE ...`）。次の句の手前までを取り込む。
    fn keyword_clause(&mut self, kind: NodeKind) {
        self.start_node(kind);
        self.bump();
        while !self.at_statement_end() && !self.at(TokenKind::RParen) && !self.at_clause_keyword() {
            self.bump_balanced();
        }
        self.finish_node();
    }

    /// 集合演算。`INTERSECT` は `UNION` / `EXCEPT` より強く結びつく。
    fn select_body(&mut self, min_bp: u8) {
        let cp = self.checkpoint();
        self.select_primary();
        loop {
            let bp = if self.at_any_kw(&["union", "except"]) {
                1
            } else if self.at_kw("intersect") {
                2
            } else {
                break;
            };
            if bp < min_bp {
                break;
            }
            self.start_node_at(cp, NodeKind::SetOperation);
            self.bump();
            if !self.eat_kw("all") {
                self.eat_kw("distinct");
            }
            self.select_body(bp + 1);
            self.finish_node();
        }
    }

    fn select_primary(&mut self) {
        if self.at_kw("select") {
            self.simple_select();
        } else if self.at_kw("values") {
            self.start_node(NodeKind::ValuesClause);
            self.bump();
            self.comma_list(Self::at_clause_keyword, |p| {
                let found = p.at(TokenKind::LParen);
                if found {
                    p.expr_list();
                }
                found
            });
            self.finish_node();
        } else if self.at_kw("table") {
            self.start_node(NodeKind::TableClause);
            self.bump();
            self.eat_kw("only");
            self.name_path();
            self.finish_node();
        } else if self.at(TokenKind::LParen) {
            self.start_node(NodeKind::ParenSelect);
            self.bump();
            self.select_stmt();
            self.expect_closing(TokenKind::RParen);
            self.finish_node();
        } else {
            // `WITH ... INSERT` など、問い合わせ以外が続く場合
            self.error_until(|_| false);
        }
    }

    pub(super) fn with_clause(&mut self) {
        self.start_node(NodeKind::WithClause);
        self.bump();
        self.eat_kw("recursive");
        self.comma_list(
            |p| p.at_query_start(0) || p.at_any_kw(&["insert", "update", "delete", "merge"]),
            Self::cte,
        );
        self.finish_node();
    }

    /// `name [(col, ...)] AS [NOT] [MATERIALIZED] (...)`
    fn cte(&mut self) -> bool {
        if !self.at_name() {
            return false;
        }
        self.start_node(NodeKind::Cte);
        self.bump();
        if self.at(TokenKind::LParen) {
            self.expr_list();
        }
        self.eat_kw("as");
        self.eat_kw("not");
        self.eat_kw("materialized");
        if self.at(TokenKind::LParen) {
            self.subquery_expr();
        }
        self.finish_node();
        true
    }

    fn simple_select(&mut self) {
        self.start_node(NodeKind::SimpleSelect);

        self.start_node(NodeKind::SelectClause);
        self.bump();
        if self.eat_kw("distinct") {
            if self.eat_kw("on") && self.at(TokenKind::LParen) {
                self.expr_list();
            }
        } else {
            self.eat_kw("all");
        }
        self.comma_list(Self::at_clause_keyword, Self::target_item);
        self.finish_node();

        if self.at_kw("into") {
            self.start_node(NodeKind::IntoClause);
            self.bump();
            if !self.eat_kw("temporary") && !self.eat_kw("temp") {
                self.eat_kw("unlogged");
            }
            self.eat_kw("table");
            self.name_path();
            self.finish_node();
        }
        if self.at_kw("from") {
            self.table_source_clause();
        }
        if self.at_kw("where") {
            self.where_clause();
        }
        if self.at_kw("group") && self.nth_kw(1, "by") {
            self.start_node(NodeKind::GroupByClause);
            self.bump();
            self.bump();
            if !self.eat_kw("all") {
                self.eat_kw("distinct");
            }
            self.comma_list(Self::at_clause_keyword, Self::group_item);
            self.finish_node();
        }
        if self.at_kw("having") {
            self.start_node(NodeKind::HavingClause);
            self.bump();
            self.expr();
            self.finish_node();
        }
        if self.at_kw("window") {
            self.start_node(NodeKind::WindowClause);
            self.bump();
            self.comma_list(Self::at_clause_keyword, Self::window_def);
            self.finish_node();
        }

        self.finish_node();
    }

    pub(super) fn table_source_clause(&mut self) {
        self.start_node(NodeKind::FromClause);
        self.bump();
        self.comma_list(Self::at_clause_keyword, Self::table_expr);
        self.finish_node();
    }

    pub(super) fn target_item(&mut self) -> bool {
        let cp = self.checkpoint();
        if !self.expr() {
            return false;
        }
        self.opt_alias(false);
        self.wrap(cp, NodeKind::TargetItem);
        true
    }

    pub(super) fn opt_alias(&mut self, table_alias: bool) {
        self.opt_alias_except(table_alias, &[]);
    }

    /// 別名。`AS` なしの別名は、予約語でも `not_bare` でもない名前だけを受け付ける。
    /// 表の別名（`table_alias`）では、結合のキーワードも別名にしない。列名の並びを付けられる。
    pub(super) fn opt_alias_except(&mut self, table_alias: bool, not_bare: &[&str]) {
        let bare = self.current().is_some_and(|t| match t.kind {
            TokenKind::QuotedIdent { .. } => true,
            TokenKind::Ident => {
                !is_reserved(t.text)
                    && !not_bare.iter().any(|kw| t.text.eq_ignore_ascii_case(kw))
                    && !(table_alias
                        && (is_join_keyword(t.text) || t.text.eq_ignore_ascii_case("tablesample")))
            }
            _ => false,
        });
        if !bare && !self.at_kw("as") {
            return;
        }
        self.start_node(NodeKind::Alias);
        let has_as = self.eat_kw("as");
        if if has_as { self.at_name() } else { bare } {
            self.bump();
        }
        if table_alias && self.at(TokenKind::LParen) {
            self.expr_list();
        }
        self.finish_node();
    }

    pub(super) fn table_expr(&mut self) -> bool {
        let cp = self.checkpoint();
        if !self.table_primary() {
            return false;
        }
        while self.at_join_start() {
            self.start_node_at(cp, NodeKind::JoinExpr);
            while !self.at_kw("join") {
                self.bump();
            }
            self.bump();
            if !self.table_primary() {
                self.error_until(|p| {
                    p.at_clause_keyword()
                        || p.at_join_start()
                        || p.at_any_kw(&["on", "using"])
                        || p.at(TokenKind::Comma)
                });
            }
            if self.at_kw("on") {
                self.start_node(NodeKind::JoinCondition);
                self.bump();
                self.expr();
                self.finish_node();
            } else if self.at_kw("using") {
                self.start_node(NodeKind::JoinCondition);
                self.bump();
                if self.at(TokenKind::LParen) {
                    self.expr_list();
                }
                // `USING (...) AS j` の別名は AS が必須
                if self.at_kw("as") {
                    self.opt_alias(false);
                }
                self.finish_node();
            }
            self.finish_node();
        }
        true
    }

    /// `[NATURAL] [INNER | CROSS | LEFT | RIGHT | FULL] [OUTER] JOIN`
    fn at_join_start(&self) -> bool {
        let mut n = 0;
        while n < JOIN_KEYWORDS.len() && !self.nth_kw(n, "join") {
            if !self
                .nth(n)
                .is_some_and(|t| t.kind == TokenKind::Ident && is_join_keyword(t.text))
            {
                return false;
            }
            n += 1;
        }
        self.nth_kw(n, "join")
    }

    /// FROM 句の結合以外の要素: 表、副問い合わせ、関数、括弧で囲んだ結合
    fn table_primary(&mut self) -> bool {
        let cp = self.checkpoint();
        let lateral = self.eat_kw("lateral");
        if self.at(TokenKind::LParen) {
            if self.at_query_start(1) {
                self.subquery_expr();
                self.opt_alias(true);
                self.wrap(cp, NodeKind::DerivedTable);
            } else {
                self.bump();
                if !self.table_expr() {
                    self.error_until(|_| false);
                }
                self.expect_closing(TokenKind::RParen);
                self.opt_alias(true);
                self.wrap(cp, NodeKind::ParenJoin);
            }
            return true;
        }
        let only = self.eat_kw("only");
        let is_name = self.current().is_some_and(|t| match t.kind {
            TokenKind::QuotedIdent { .. } => true,
            TokenKind::Ident => !is_reserved(t.text),
            _ => false,
        });
        if !is_name {
            return lateral || only;
        }
        self.name_path();
        if self.at(TokenKind::LParen) {
            self.arg_list(0);
            if self.at_kw("with") && self.nth_kw(1, "ordinality") {
                self.bump();
                self.bump();
            }
            self.opt_alias(true);
            self.wrap(cp, NodeKind::FunctionTable);
        } else {
            if self.at_op("*") {
                self.bump();
            }
            self.opt_alias(true);
            self.wrap(cp, NodeKind::TableRef);
        }
        true
    }

    /// `WHERE expr` / `WHERE CURRENT OF cursor`
    pub(super) fn where_clause(&mut self) {
        self.start_node(NodeKind::WhereClause);
        self.bump();
        if self.at_kw("current") && self.nth_kw(1, "of") {
            self.bump();
            self.bump();
            self.name_path();
        } else {
            self.expr();
        }
        self.finish_node();
    }

    fn group_item(&mut self) -> bool {
        if self.at_kw("grouping") && self.nth_kw(1, "sets") {
            self.start_node(NodeKind::GroupingSets);
            self.bump();
            self.bump();
            if self.at(TokenKind::LParen) {
                self.expr_list();
            }
            self.finish_node();
            return true;
        }
        self.expr()
    }

    /// WINDOW 句の `name AS (...)`
    fn window_def(&mut self) -> bool {
        if !self.at_name() {
            return false;
        }
        self.start_node(NodeKind::WindowDef);
        self.bump();
        self.eat_kw("as");
        if self.at(TokenKind::LParen) {
            self.window_spec();
        }
        self.finish_node();
        true
    }

    /// `ORDER BY item, ...`。`is_end` は並びの終わり（窓関数では `ROWS` なども終わりになる）。
    pub(super) fn order_by_clause(&mut self, is_end: fn(&Self) -> bool) {
        self.start_node(NodeKind::OrderByClause);
        self.bump();
        self.eat_kw("by");
        self.comma_list(is_end, Self::sort_item);
        self.finish_node();
    }

    /// `expr [ASC | DESC | USING op] [NULLS FIRST | NULLS LAST]`
    fn sort_item(&mut self) -> bool {
        let cp = self.checkpoint();
        if !self.expr() {
            return false;
        }
        if !self.eat_kw("asc") && !self.eat_kw("desc") && self.eat_kw("using") {
            self.eat(TokenKind::Operator);
        }
        if self.eat_kw("nulls") && !self.eat_kw("first") {
            self.eat_kw("last");
        }
        self.wrap(cp, NodeKind::SortItem);
        true
    }
}
