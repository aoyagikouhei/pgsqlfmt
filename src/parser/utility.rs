//! Utility statements: `COPY` / `SET` / `RESET` / `SHOW` / `EXPLAIN` / transaction control.
//!
//! All of these are written on one line, so known words become keywords and names, expressions,
//! and parentheses are each read as such. Anything that cannot be parsed is kept verbatim.

use super::Parser;
use crate::lexer::TokenKind;
use crate::syntax::NodeKind;

/// COPY words (including option names inside `WITH (...)` and the legacy option syntax)
const COPY_KEYWORDS: &[&str] = &[
    "binary",
    "csv",
    "default",
    "delimiter",
    "encoding",
    "escape",
    "force",
    "force_not_null",
    "force_null",
    "force_quote",
    "format",
    "freeze",
    "from",
    "header",
    "log_verbosity",
    "not",
    "null",
    "on_error",
    "program",
    "quote",
    "reject_limit",
    "stdin",
    "stdout",
    "to",
    "with",
];

/// Option names inside EXPLAIN's `(...)`
const EXPLAIN_OPTIONS: &[&str] = &[
    "analyse",
    "analyze",
    "buffers",
    "costs",
    "format",
    "generic_plan",
    "memory",
    "serialize",
    "settings",
    "summary",
    "timing",
    "verbose",
    "wal",
];

/// Transaction control and the transaction modes of SET TRANSACTION etc.
const TRANSACTION_KEYWORDS: &[&str] = &[
    "abort",
    "and",
    "as",
    "begin",
    "chain",
    "characteristics",
    "commit",
    "committed",
    "deferrable",
    "end",
    "isolation",
    "level",
    "no",
    "not",
    "only",
    "prepare",
    "prepared",
    "read",
    "release",
    "repeatable",
    "rollback",
    "savepoint",
    "serializable",
    "session",
    "snapshot",
    "start",
    "to",
    "transaction",
    "uncommitted",
    "work",
    "write",
];

/// Transaction-control words followed by a savepoint name
const SAVEPOINT_BEFORE: &[&str] = &["savepoint", "release", "to"];

impl Parser<'_> {
    pub(super) fn at_transaction_stmt(&self) -> bool {
        self.at_any_kw(&[
            "begin",
            "commit",
            "end",
            "rollback",
            "abort",
            "savepoint",
            "release",
        ]) || self.at_words(&[&["start", "prepare"], &["transaction"]])
    }

    /// `BEGIN` / `START TRANSACTION` / `COMMIT` / `ROLLBACK [TO SAVEPOINT s]` / `SAVEPOINT s` / `RELEASE s` /
    /// `PREPARE TRANSACTION 'id'` / `COMMIT PREPARED 'id'`, etc.
    pub(super) fn transaction_stmt(&mut self) {
        self.start_node(NodeKind::TransactionStmt);
        self.keyword_words(TRANSACTION_KEYWORDS, SAVEPOINT_BEFORE);
        self.finish_node();
    }

    /// `SET [SESSION | LOCAL] name {TO | =} value, ...` / `SET TIME ZONE ...` / `SET ROLE ...` /
    /// `SET TRANSACTION ...` / `SET CONSTRAINTS ...` / `RESET name` / `SHOW name`
    pub(super) fn set_stmt(&mut self) {
        self.start_node(NodeKind::SetStmt);
        let set = self.at_kw("set");
        self.bump_kw();
        if !set {
            // RESET / SHOW: ALL / TIME ZONE / ROLE / SESSION AUTHORIZATION /
            // TRANSACTION ISOLATION LEVEL, or a setting name
            self.keyword_words(
                &[
                    "all",
                    "authorization",
                    "isolation",
                    "level",
                    "role",
                    "session",
                    "time",
                    "transaction",
                    "zone",
                ],
                &[],
            );
            self.finish_node();
            return;
        }
        // Some setting names are spelled like keywords, as in `SET local.x = 1` / `SET role = none`
        let plain_name = |p: &Self| {
            p.nth_is(1, TokenKind::Dot)
                || p.nth_kw(1, "to")
                || p.nth(1)
                    .is_some_and(|t| t.kind == TokenKind::Operator && t.text == "=")
        };
        let session_authorization = |p: &Self| p.at_kw("session") && p.nth_kw(1, "authorization");
        if (self.at_kw("local") || self.at_kw("session"))
            && !plain_name(self)
            && !session_authorization(self)
            && !self.nth_kw(1, "characteristics")
        {
            self.bump_kw();
        }
        if session_authorization(self) {
            self.bump_kw();
            self.bump_kw();
            self.set_values();
        } else if !plain_name(self) && self.at_any_kw(&["transaction", "session", "constraints"]) {
            // SET TRANSACTION ... / SET SESSION CHARACTERISTICS AS TRANSACTION ... / SET CONSTRAINTS c, ... DEFERRED
            let mut keywords = TRANSACTION_KEYWORDS.to_vec();
            keywords.extend(["all", "constraints", "deferred", "immediate"]);
            self.keyword_words(&keywords, &[]);
        } else if !plain_name(self) && self.at_kw("time") && self.nth_kw(1, "zone") {
            self.bump_kw();
            self.bump_kw();
            self.set_values();
        } else if !plain_name(self) && self.at_kw("role") {
            self.bump_kw();
            self.set_values();
        } else {
            self.name_path();
            if !self.eat_kw("to") && self.at_op("=") {
                self.bump();
            }
            self.set_values();
        }
        self.finish_node();
    }

    /// The list of SET values (`search_path = a, b`)
    fn set_values(&mut self) {
        while !self.at_statement_end() {
            if self.eat(TokenKind::Comma) {
                continue;
            }
            if self.at_any_kw(&["default", "local", "none"]) {
                self.bump_kw();
            } else if !self.expr() {
                self.raw_until(|p| p.at(TokenKind::Comma));
            }
        }
    }

    /// `EXPLAIN [ANALYZE] [VERBOSE] statement` / `EXPLAIN (option [value], ...) statement`
    pub(super) fn explain_stmt(&mut self) {
        self.start_node(NodeKind::ExplainStmt);
        self.bump_kw();
        // The parentheses in `EXPLAIN (SELECT 1) ...` are a query
        if self.at(TokenKind::LParen) && EXPLAIN_OPTIONS.iter().any(|o| self.nth_kw(1, o)) {
            self.option_list(EXPLAIN_OPTIONS);
        }
        while self.at_any_kw(&["analyze", "analyse", "verbose"]) {
            self.bump_kw();
        }
        if !self.at_statement_end() {
            self.statement();
        }
        self.finish_node();
    }

    /// `COPY table [(columns)] {FROM | TO} {'file' | PROGRAM 'cmd' | STDIN | STDOUT} [[WITH] (options)] [WHERE cond]`
    /// / `COPY (query) TO ...`
    pub(super) fn copy_stmt(&mut self) {
        self.start_node(NodeKind::CopyStmt);
        self.bump_kw();
        if self.at(TokenKind::LParen) {
            self.subquery_expr();
        } else {
            self.name_path();
            // The column list. Column names spelled like keywords (owner, etc.) are kept as is
            if self.at(TokenKind::LParen) {
                self.expr_list();
            }
        }
        while !self.at_statement_end() {
            if self.eat(TokenKind::Comma) {
                continue;
            }
            if self.at_kw("where") {
                self.where_clause();
            } else if self.at(TokenKind::LParen) {
                self.option_list(COPY_KEYWORDS);
            } else if self.at_any_kw(COPY_KEYWORDS) {
                self.bump_kw();
            } else {
                // Values such as file names and delimiters are kept verbatim
                self.raw_until(|p| {
                    p.at_any_kw(COPY_KEYWORDS) || p.at_kw("where") || p.at(TokenKind::LParen)
                });
            }
        }
        self.finish_node();
    }

    /// `(name [value], ...)`. The leading name of each option becomes a keyword; the value
    /// (the csv in `FORMAT csv`) is written verbatim
    fn option_list(&mut self, names: &[&str]) {
        self.start_node(NodeKind::ExprList);
        self.bump();
        let mut at_option_start = true;
        while !self.at(TokenKind::RParen) && !self.at_statement_end() {
            if self.eat(TokenKind::Comma) {
                at_option_start = true;
                continue;
            }
            if at_option_start && self.at_any_kw(names) {
                self.bump_kw();
            } else if self.at(TokenKind::LParen) {
                // The column list of `FORCE_QUOTE (a, b)`
                self.expr_list();
            } else {
                self.bump_balanced();
            }
            at_option_start = false;
        }
        self.expect_closing(TokenKind::RParen);
        self.finish_node();
    }

    /// Read everything up to the end of the statement as words on one line. `keywords` become
    /// keywords, and the name after a `name_before` word is read as a name (even when spelled like
    /// a keyword)
    fn keyword_words(&mut self, keywords: &[&str], name_before: &[&str]) {
        while !self.at_statement_end() {
            if self.eat(TokenKind::Comma) {
                continue;
            }
            // A setting name if a dot follows, as in `SHOW session.x`
            if self.at_any_kw(keywords) && !self.nth_is(1, TokenKind::Dot) {
                let word = self.current().unwrap().text.to_ascii_lowercase();
                self.bump_kw();
                // The SAVEPOINT after TO in `ROLLBACK TO SAVEPOINT s` is read as a keyword on the
                // next iteration
                if name_before.contains(&word.as_str()) && !self.at_any_kw(name_before) {
                    self.name_path();
                }
            } else if self.name_path() == 0 {
                self.raw_until(|p| p.at_any_kw(keywords) || p.at(TokenKind::Comma));
            }
        }
    }
}
