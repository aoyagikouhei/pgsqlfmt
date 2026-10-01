//! DDL: `CREATE TABLE` / `CREATE INDEX` / `CREATE [MATERIALIZED] VIEW` / `ALTER TABLE` / `DROP`。
//!
//! 列の定義や制約・オプションは種類が多いので、既定値（DEFAULT）・CHECK・生成列の式・型名など
//! 式が来る位置だけを解釈し、それ以外は既知のキーワード（大文字にする）と名前・括弧の並びとして読む。

use super::Parser;
use crate::lexer::TokenKind;
use crate::syntax::NodeKind;

/// DDL の中でキーワードとして扱う語
const DDL_KEYWORDS: &[&str] = &[
    "action",
    "add",
    "all",
    "always",
    "as",
    "asc",
    "attach",
    "by",
    "cascade",
    "cascaded",
    "check",
    "column",
    "comments",
    "commit",
    "compression",
    "concurrently",
    "constraint",
    "constraints",
    "data",
    "defaults",
    "deferrable",
    "deferred",
    "delete",
    "desc",
    "detach",
    "disable",
    "distinct",
    "drop",
    "enable",
    "exclude",
    "excluding",
    "exists",
    "first",
    "for",
    "foreign",
    "from",
    "full",
    "generated",
    "hash",
    "identity",
    "if",
    "immediate",
    "in",
    "include",
    "including",
    "increment",
    "indexes",
    "inherit",
    "inherits",
    "initially",
    "key",
    "last",
    "like",
    "list",
    "local",
    "match",
    "maxvalue",
    "minvalue",
    "modulus",
    "no",
    "not",
    "null",
    "nulls",
    "of",
    "on",
    "only",
    "option",
    "owner",
    "partial",
    "partition",
    "preserve",
    "primary",
    "range",
    "references",
    "remainder",
    "rename",
    "restrict",
    "rows",
    "schema",
    "set",
    "simple",
    "start",
    "statistics",
    "storage",
    "stored",
    "table",
    "tablespace",
    "to",
    "trigger",
    "type",
    "unique",
    "update",
    "valid",
    "validate",
    "values",
    "virtual",
    "with",
    "without",
];

/// DROP の後ろに来るオブジェクトの種類
const OBJECT_TYPES: &[&str] = &[
    "aggregate",
    "collation",
    "domain",
    "extension",
    "foreign",
    "function",
    "index",
    "materialized",
    "policy",
    "procedure",
    "role",
    "routine",
    "rule",
    "schema",
    "sequence",
    "server",
    "table",
    "trigger",
    "type",
    "view",
];

/// 表制約の始まり
const TABLE_CONSTRAINT_STARTS: &[&str] = &[
    "constraint",
    "check",
    "unique",
    "primary",
    "foreign",
    "exclude",
];

impl Parser<'_> {
    /// n 番目から `words` が続くか（`None` はその位置の語を省略できる）
    fn at_words(&self, words: &[&[&str]]) -> bool {
        let mut n = 0;
        for alternatives in words {
            if alternatives.iter().any(|w| self.nth_kw(n, w)) {
                n += 1;
            } else if !alternatives.contains(&"") {
                return false;
            }
        }
        true
    }

    pub(super) fn at_create_table(&self) -> bool {
        self.at_words(&[
            &["create"],
            &["global", "local", ""],
            &["temp", "temporary", "unlogged", ""],
            &["table"],
        ])
    }

    pub(super) fn at_create_index(&self) -> bool {
        self.at_words(&[&["create"], &["unique", ""], &["index"]])
    }

    pub(super) fn at_create_view(&self) -> bool {
        self.at_words(&[
            &["create"],
            &["or", ""],
            &["replace", ""],
            &["temp", "temporary", ""],
            &["recursive", "materialized", ""],
            &["view"],
        ])
    }

    /// `CREATE [TEMP | UNLOGGED] TABLE [IF NOT EXISTS] name (column, constraint, ...) [options]`
    /// / `CREATE TABLE name PARTITION OF parent ...` / `CREATE TABLE name AS query`
    pub(super) fn create_table_stmt(&mut self) {
        self.start_node(NodeKind::CreateTableStmt);
        while !self.at_kw("table") {
            self.bump_kw();
        }
        self.bump_kw();
        self.if_exists();
        self.name_path();
        if self.eat_kw("partition") {
            self.eat_kw("of");
            self.name_path();
        } else if self.eat_kw("of") {
            self.type_name();
        }
        if self.at(TokenKind::LParen) {
            self.table_element_list();
        }
        self.ddl_rest(Self::as_query);
        self.finish_node();
    }

    /// 文の終わりまでのオプション。`AS` の後ろに問い合わせが来たら `as_query` で読む。
    fn ddl_rest(&mut self, as_query: fn(&mut Self)) {
        while !self.at_statement_end() {
            if self.at_kw("as") && !self.nth_is(1, TokenKind::LParen) {
                as_query(self);
                continue;
            }
            let before = self.current().map(|t| t.offset);
            self.ddl_words(|p| p.at_kw("as") && !p.nth_is(1, TokenKind::LParen));
            self.eat(TokenKind::Comma);
            if self.current().map(|t| t.offset) == before {
                self.error_token();
            }
        }
    }

    /// `AS query`。問い合わせは、後ろの `WITH [NO] DATA` / `WITH CHECK OPTION` の手前で終わる。
    fn as_query(&mut self) {
        self.bump_kw();
        self.with_stops(&["with"], |p| {
            p.statement_body();
        });
    }

    /// `IF [NOT] EXISTS`
    fn if_exists(&mut self) {
        if self.at_kw("if") && (self.nth_kw(1, "exists") || self.nth_kw(2, "exists")) {
            self.bump_kw();
            self.eat_kw("not");
            self.bump_kw();
        }
    }

    fn table_element_list(&mut self) {
        self.start_node(NodeKind::TableElementList);
        self.bump();
        self.comma_list(|_| false, Self::table_element);
        self.expect_closing(TokenKind::RParen);
        self.finish_node();
    }

    /// 列の定義・表制約・`LIKE source`
    fn table_element(&mut self) -> bool {
        if self.at_any_kw(TABLE_CONSTRAINT_STARTS) || self.at_kw("like") {
            self.start_node(NodeKind::TableConstraint);
            self.ddl_words(|_| false);
            self.finish_node();
            return true;
        }
        if !self.at_name() {
            return false;
        }
        self.column_def();
        true
    }

    /// `name type [COLLATE c] [constraint ...]`
    fn column_def(&mut self) {
        self.start_node(NodeKind::ColumnDef);
        self.bump();
        self.type_name();
        self.ddl_words(|_| false);
        self.finish_node();
    }

    /// カンマ・閉じ括弧・文の終わり・`stop` の手前までの、制約やオプションの並び
    fn ddl_words(&mut self, stop: fn(&Self) -> bool) {
        while !self.at_statement_end()
            && !self.at(TokenKind::RParen)
            && !self.at(TokenKind::Comma)
            && !stop(self)
        {
            if self.eat_kw("default") {
                self.expr();
            } else if (self.at_kw("check") || self.at_kw("as")) && self.nth_is(1, TokenKind::LParen)
            {
                // CHECK (expr) / GENERATED ALWAYS AS (expr)
                self.bump_kw();
                self.expr();
            } else if self.eat_kw("constraint") || self.eat_kw("collate") {
                self.name_path();
            } else if self.eat_kw("type") {
                self.type_name();
            } else if self.eat_kw("using") {
                // `USING btree` / `USING expr`（ALTER COLUMN ... TYPE ... USING）
                if !self.at(TokenKind::LParen) {
                    self.expr();
                }
            } else if self.at_kw("where") {
                self.where_clause();
            } else if self.at(TokenKind::LParen) {
                self.ddl_paren();
            } else if self.at_any_kw(DDL_KEYWORDS) {
                self.bump_kw();
            } else if self.at_name() {
                self.bump();
            } else {
                self.bump_balanced();
            }
        }
    }

    /// 列名やオプションの括弧 `(a, b)` / `(fillfactor = 70)` / `(START WITH 1)`
    fn ddl_paren(&mut self) {
        self.start_node(NodeKind::ExprList);
        self.bump();
        while !self.at(TokenKind::RParen) && !self.at_statement_end() {
            if self.eat(TokenKind::Comma) {
                continue;
            }
            let before = self.current().map(|t| t.offset);
            self.ddl_words(|_| false);
            if self.current().map(|t| t.offset) == before {
                self.error_token();
            }
        }
        self.expect_closing(TokenKind::RParen);
        self.finish_node();
    }

    /// `CREATE [UNIQUE] INDEX [CONCURRENTLY] [IF NOT EXISTS] [name] ON [ONLY] table [USING method]
    ///  (elem, ...) [INCLUDE (...)] [NULLS [NOT] DISTINCT] [WITH (...)] [TABLESPACE t] [WHERE pred]`
    pub(super) fn create_index_stmt(&mut self) {
        self.start_node(NodeKind::CreateIndexStmt);
        self.bump_kw();
        self.eat_kw("unique");
        self.bump_kw();
        self.eat_kw("concurrently");
        self.if_exists();
        if self.at_name() && !self.at_kw("on") {
            self.bump();
        }
        if self.eat_kw("on") {
            self.eat_kw("only");
            self.name_path();
        }
        if self.eat_kw("using") && self.at_name() {
            self.bump();
        }
        if self.at(TokenKind::LParen) {
            self.start_node(NodeKind::ExprList);
            self.bump();
            self.comma_list(
                |_| false,
                |p| {
                    // 式の後ろの COLLATE・演算子クラス・ASC / DESC・NULLS FIRST / LAST
                    let found = p.expr();
                    if found {
                        p.ddl_words(|_| false);
                    }
                    found
                },
            );
            self.expect_closing(TokenKind::RParen);
            self.finish_node();
        }
        self.ddl_rest(|p| p.error_token());
        self.finish_node();
    }

    /// `CREATE [OR REPLACE] [TEMP] [RECURSIVE | MATERIALIZED] VIEW [IF NOT EXISTS] name [(cols)] [options]
    ///  AS query [WITH [CASCADED | LOCAL] CHECK OPTION | WITH [NO] DATA]`
    pub(super) fn create_view_stmt(&mut self) {
        self.start_node(NodeKind::CreateViewStmt);
        while !self.at_kw("view") {
            self.bump_kw();
        }
        self.bump_kw();
        self.if_exists();
        self.name_path();
        self.ddl_rest(Self::as_query);
        self.finish_node();
    }

    /// `ALTER TABLE [IF EXISTS] [ONLY] name [*] action, ...`
    pub(super) fn alter_table_stmt(&mut self) {
        self.start_node(NodeKind::AlterTableStmt);
        self.bump_kw();
        self.bump_kw();
        self.if_exists();
        self.eat_kw("only");
        self.name_path();
        if self.at_op("*") {
            self.bump();
        }
        self.comma_list(|_| false, Self::alter_table_action);
        self.finish_node();
    }

    fn alter_table_action(&mut self) -> bool {
        if !self.at_name() {
            return false;
        }
        self.start_node(NodeKind::AlterTableAction);
        let first = self.current().unwrap().text.to_ascii_lowercase();
        self.bump_kw();
        match first.as_str() {
            "add" => {
                let column = self.eat_kw("column");
                self.if_exists();
                if !column && self.at_any_kw(TABLE_CONSTRAINT_STARTS) {
                    self.ddl_words(|_| false);
                } else if self.at_name() {
                    self.bump();
                    self.type_name();
                }
            }
            "drop" | "alter" | "rename" => {
                // DROP / ALTER / RENAME の後ろの列名や制約名
                if !self.eat_kw("column") && self.eat_kw("constraint") {
                    self.if_exists();
                    if self.at_name() {
                        self.bump();
                    }
                } else {
                    self.if_exists();
                    let keyword_follows =
                        self.at_any_kw(&["default", "not", "identity", "expression", "to"]);
                    if self.at_name() && !keyword_follows {
                        self.bump();
                    }
                }
            }
            _ => {}
        }
        self.ddl_words(|_| false);
        self.finish_node();
        true
    }

    /// `DROP object_type [CONCURRENTLY] [IF EXISTS] name [(args)], ... [ON table] [CASCADE | RESTRICT]`
    pub(super) fn drop_stmt(&mut self) {
        self.start_node(NodeKind::DropStmt);
        self.bump_kw();
        while self.at_any_kw(OBJECT_TYPES) || self.at_any_kw(&["data", "wrapper", "view"]) {
            self.bump_kw();
        }
        self.eat_kw("concurrently");
        self.if_exists();
        while !self.at_statement_end() {
            if self.at_name() && !self.at_any_kw(&["on", "cascade", "restrict"]) {
                self.name_path();
                if self.at(TokenKind::LParen) {
                    self.ddl_paren();
                }
            } else if !self.eat(TokenKind::Comma) {
                let before = self.current().map(|t| t.offset);
                self.ddl_words(|p| p.at_name() && !p.at_any_kw(DDL_KEYWORDS));
                if self.current().map(|t| t.offset) == before {
                    self.error_token();
                }
            }
        }
        self.finish_node();
    }
}
