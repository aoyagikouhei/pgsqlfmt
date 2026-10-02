//! DDL: `CREATE TABLE` / `CREATE INDEX` / `CREATE [MATERIALIZED] VIEW` / `ALTER TABLE` / `DROP`.
//!
//! Column definitions, constraints, and options come in many forms, so only the positions where an
//! expression appears (DEFAULT, CHECK, generated-column expressions, type names, and so on) are
//! parsed; everything else is read as a list of known keywords (upper-cased), names, and
//! parentheses.

use super::Parser;
use crate::lexer::TokenKind;
use crate::syntax::NodeKind;

/// Words treated as keywords inside DDL
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

/// Object types that follow DROP
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

/// Words that start a sequence option (CREATE / ALTER SEQUENCE)
const SEQUENCE_OPTION_STARTS: &[&str] = &[
    "as",
    "increment",
    "minvalue",
    "maxvalue",
    "no",
    "start",
    "restart",
    "cache",
    "cycle",
    "owned",
];

/// Object types that follow ALTER (ALTER TABLE is read separately). Matched longest first
const ALTER_OBJECT_TYPES: &[&[&str]] = &[
    &["text", "search", "configuration"],
    &["text", "search", "dictionary"],
    &["text", "search", "parser"],
    &["text", "search", "template"],
    &["foreign", "data", "wrapper"],
    &["default", "privileges"],
    &["event", "trigger"],
    &["foreign", "table"],
    &["large", "object"],
    &["materialized", "view"],
    &["operator", "class"],
    &["operator", "family"],
    &["procedural", "language"],
    &["user", "mapping"],
    &["aggregate"],
    &["collation"],
    &["conversion"],
    &["database"],
    &["domain"],
    &["extension"],
    &["function"],
    &["group"],
    &["index"],
    &["language"],
    &["operator"],
    &["policy"],
    &["procedure"],
    &["publication"],
    &["role"],
    &["routine"],
    &["rule"],
    &["schema"],
    &["sequence"],
    &["server"],
    &["statistics"],
    &["subscription"],
    &["system"],
    &["tablespace"],
    &["trigger"],
    &["type"],
    &["user"],
    &["view"],
];

/// Words treated as keywords in ALTER actions
const ALTER_KEYWORDS: &[&str] = &[
    "add",
    "admin",
    "after",
    "all",
    "alter",
    "always",
    "attach",
    "attribute",
    "before",
    "by",
    "bypassrls",
    "called",
    "cascade",
    "cast",
    "column",
    "connection",
    "constraint",
    "cost",
    "createdb",
    "createrole",
    "data",
    "database",
    "default",
    "definer",
    "depends",
    "detach",
    "disable",
    "drop",
    "enable",
    "encrypted",
    "exists",
    "extension",
    "for",
    "if",
    "immutable",
    "in",
    "inherit",
    "input",
    "invoker",
    "leakproof",
    "limit",
    "logged",
    "login",
    "no",
    "nobypassrls",
    "nocreatedb",
    "nocreaterole",
    "noinherit",
    "nologin",
    "noreplication",
    "nosuperuser",
    "not",
    "null",
    "on",
    "options",
    "owner",
    "parallel",
    "password",
    "rename",
    "replica",
    "replication",
    "reset",
    "restrict",
    "restricted",
    "returns",
    "rows",
    "safe",
    "schema",
    "security",
    "set",
    "skip",
    "stable",
    "strict",
    "superuser",
    "support",
    "table",
    "tablespace",
    "to",
    "unlogged",
    "unsafe",
    "until",
    "update",
    "using",
    "valid",
    "validate",
    "value",
    "version",
    "volatile",
    "with",
    "without",
];

/// ALTER keywords that are followed by a name
/// (`RENAME TO x` / `SET SCHEMA x` / `RENAME COLUMN a TO b`, etc.)
const ALTER_NAME_BEFORE: &[&str] = &[
    "to",
    "schema",
    "column",
    "attribute",
    "constraint",
    "tablespace",
];

/// GRANT / REVOKE privileges
const PRIVILEGES: &[&str] = &[
    "all",
    "alter",
    "connect",
    "create",
    "delete",
    "execute",
    "insert",
    "maintain",
    "privileges",
    "references",
    "select",
    "set",
    "system",
    "temp",
    "temporary",
    "trigger",
    "truncate",
    "update",
    "usage",
];

/// Object types after ON in GRANT / REVOKE. Matched longest first
/// (includes the plural forms used by ALTER DEFAULT PRIVILEGES)
const GRANT_OBJECT_TYPES: &[&[&str]] = &[
    &["all", "functions", "in", "schema"],
    &["all", "procedures", "in", "schema"],
    &["all", "routines", "in", "schema"],
    &["all", "sequences", "in", "schema"],
    &["all", "tables", "in", "schema"],
    &["foreign", "data", "wrapper"],
    &["foreign", "server"],
    &["large", "object"],
    &["large", "objects"],
    &["database"],
    &["domain"],
    &["function"],
    &["functions"],
    &["language"],
    &["parameter"],
    &["procedure"],
    &["routine"],
    &["routines"],
    &["schema"],
    &["schemas"],
    &["sequence"],
    &["sequences"],
    &["table"],
    &["tables"],
    &["tablespace"],
    &["type"],
    &["types"],
];

/// Words treated as keywords after TO / FROM in GRANT / REVOKE
const GRANTEE_KEYWORDS: &[&str] = &[
    "group",
    "public",
    "current_role",
    "current_user",
    "session_user",
];

/// ALTER TABLE keywords that are followed by a name
const ALTER_TABLE_NAME_BEFORE: &[&str] = &[
    "index",
    "inherit",
    "partition",
    "schema",
    "tablespace",
    "to",
    "trigger",
];

/// Words treated as keywords where a role is specified
const ROLE_SPEC_KEYWORDS: &[&str] = &["current_role", "current_user", "session_user"];

/// Words that follow the grantee role list in GRANT / REVOKE
const GRANTEE_LIST_ENDS: &[&str] = &["with", "granted", "cascade", "restrict"];

/// Trailing options of GRANT / REVOKE
const GRANT_OPTION_WORDS: &[&str] = &[
    "with", "grant", "admin", "inherit", "set", "option", "true", "false", "granted", "by",
    "cascade", "restrict",
];

/// Options after the table list in TRUNCATE
const TRUNCATE_OPTIONS: &[&str] = &["restart", "continue", "identity", "cascade", "restrict"];

/// Object types after COMMENT ON (the list from the PostgreSQL 18 docs). Matched longest first
const COMMENT_OBJECT_TYPES: &[&[&str]] = &[
    &["text", "search", "configuration"],
    &["text", "search", "dictionary"],
    &["text", "search", "parser"],
    &["text", "search", "template"],
    &["foreign", "data", "wrapper"],
    &["access", "method"],
    &["event", "trigger"],
    &["foreign", "table"],
    &["large", "object"],
    &["materialized", "view"],
    &["operator", "class"],
    &["operator", "family"],
    &["procedural", "language"],
    &["transform", "for"],
    &["aggregate"],
    &["cast"],
    &["collation"],
    &["column"],
    &["constraint"],
    &["conversion"],
    &["database"],
    &["domain"],
    &["extension"],
    &["function"],
    &["index"],
    &["language"],
    &["operator"],
    &["policy"],
    &["procedure"],
    &["publication"],
    &["role"],
    &["routine"],
    &["rule"],
    &["schema"],
    &["sequence"],
    &["server"],
    &["statistics"],
    &["subscription"],
    &["table"],
    &["tablespace"],
    &["trigger"],
    &["type"],
    &["view"],
];

/// Words treated as keywords after the name in COMMENT ON
/// (`ON [DOMAIN] table` / `OPERATOR CLASS c USING btree` / `TRANSFORM FOR t LANGUAGE l`)
const COMMENT_NAME_KEYWORDS: &[&str] = &["on", "domain", "using", "language"];

/// Words that start a CREATE TRIGGER clause
const TRIGGER_CLAUSE_STARTS: &[&str] = &[
    "before",
    "after",
    "instead",
    "from",
    "not",
    "deferrable",
    "initially",
    "referencing",
    "for",
    "when",
    "execute",
];

/// Words that start a table constraint
const TABLE_CONSTRAINT_STARTS: &[&str] = &[
    "constraint",
    "check",
    "unique",
    "primary",
    "foreign",
    "exclude",
];

impl Parser<'_> {
    /// Whether `words` follow from the current token. Each element lists the alternatives for one
    /// position; an empty string `""` among them means the word at that position may be omitted
    pub(super) fn at_words(&self, words: &[&[&str]]) -> bool {
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

    /// Options up to the end of the statement. When a query follows `AS`, it is read with
    /// `as_query`.
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

    /// `AS query`. The query ends before a trailing `WITH [NO] DATA` / `WITH CHECK OPTION`.
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

    /// A column definition, a table constraint, or `LIKE source`
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

    /// A list of constraints or options up to a comma, a closing parenthesis, the end of the
    /// statement, or `stop`
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
                // `USING btree` / `USING expr` (ALTER COLUMN ... TYPE ... USING).
                // The INDEX of REPLICA IDENTITY USING INDEX is read by the caller
                if !self.at(TokenKind::LParen) && !self.at_kw("index") {
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

    /// Parenthesized column names or options: `(a, b)` / `(fillfactor = 70)` / `(START WITH 1)`
    pub(super) fn ddl_paren(&mut self) {
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
                    // COLLATE, operator class, ASC / DESC, NULLS FIRST / LAST after the expression
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

    pub(super) fn at_create_trigger(&self) -> bool {
        self.at_words(&[
            &["create"],
            &["or", ""],
            &["replace", ""],
            &["constraint", ""],
            &["trigger"],
        ])
    }

    /// `CREATE [OR REPLACE] [CONSTRAINT] TRIGGER name {BEFORE | AFTER | INSTEAD OF} event [OR ...] ON table
    ///  [FROM ref] [deferrable] [REFERENCING ...] [FOR [EACH] {ROW | STATEMENT}] [WHEN (cond)]
    ///  EXECUTE {FUNCTION | PROCEDURE} f(args)`.
    /// After the name, each clause becomes a `TriggerClause`
    pub(super) fn create_trigger_stmt(&mut self) {
        self.start_node(NodeKind::CreateTriggerStmt);
        while !self.at_kw("trigger") {
            self.bump_kw();
        }
        self.bump_kw();
        if self.at_name() {
            self.bump();
        }
        while !self.at_statement_end() {
            if !self.at_any_kw(TRIGGER_CLAUSE_STARTS) {
                // Anything that cannot be parsed is grouped into one node up to the next clause
                // and written verbatim on the same line as the previous clause (psql variables
                // such as `ON :tbl` are not split)
                self.start_node(NodeKind::Error);
                while !self.at_statement_end() && !self.at_any_kw(TRIGGER_CLAUSE_STARTS) {
                    self.bump_balanced();
                }
                self.finish_node();
                continue;
            }
            self.start_node(NodeKind::TriggerClause);
            if self.at_any_kw(&["before", "after", "instead"]) {
                self.trigger_events();
            } else if self.eat_kw("from") {
                self.name_path();
            } else if self.at_any_kw(&["not", "deferrable", "initially"]) {
                // NOT DEFERRABLE / DEFERRABLE / INITIALLY {IMMEDIATE | DEFERRED}
                while self.at_any_kw(&["not", "deferrable", "initially", "immediate", "deferred"]) {
                    self.bump_kw();
                }
            } else if self.eat_kw("referencing") {
                // A list of `{OLD | NEW} TABLE [AS] name`
                while self.at_any_kw(&["old", "new"]) {
                    self.bump_kw();
                    self.eat_kw("table");
                    self.eat_kw("as");
                    if self.at_name() {
                        self.bump();
                    }
                }
            } else if self.eat_kw("for") {
                self.eat_kw("each");
                self.eat_kw("row");
                self.eat_kw("statement");
            } else if self.eat_kw("when") {
                self.expr();
            } else {
                self.bump_kw();
                if !self.eat_kw("function") {
                    self.eat_kw("procedure");
                }
                self.expr();
            }
            self.finish_node();
        }
        self.finish_node();
    }

    /// `{BEFORE | AFTER | INSTEAD OF} {INSERT | UPDATE [OF col, ...] | DELETE | TRUNCATE} [OR ...] ON table`
    fn trigger_events(&mut self) {
        self.bump_kw();
        self.eat_kw("of");
        while self.at_any_kw(&["insert", "update", "delete", "truncate"]) {
            self.bump_kw();
            if self.eat_kw("of") {
                while self.at_name() && !self.at_any_kw(&["on", "or"]) {
                    self.bump();
                    self.eat(TokenKind::Comma);
                }
            }
            if !self.eat_kw("or") {
                break;
            }
        }
        if self.eat_kw("on") {
            self.name_path();
        }
    }

    pub(super) fn at_create_sequence(&self) -> bool {
        self.at_words(&[
            &["create"],
            &["temp", "temporary", "unlogged", ""],
            &["sequence"],
        ])
    }

    /// `CREATE [TEMP | UNLOGGED] SEQUENCE [IF NOT EXISTS] name [option ...]`
    pub(super) fn create_sequence_stmt(&mut self) {
        self.start_node(NodeKind::CreateSequenceStmt);
        while !self.at_kw("sequence") {
            self.bump_kw();
        }
        self.bump_kw();
        self.if_exists();
        self.name_path();
        self.sequence_options();
        self.finish_node();
    }

    /// Sequence options up to the end of the statement. Each one becomes a `SequenceOption`
    pub(super) fn sequence_options(&mut self) {
        while !self.at_statement_end() {
            if !self.at_any_kw(SEQUENCE_OPTION_STARTS) {
                // Anything that cannot be parsed is written verbatim up to the next option
                self.raw_until(|p| p.at_any_kw(SEQUENCE_OPTION_STARTS));
                continue;
            }
            self.start_node(NodeKind::SequenceOption);
            let word = self.current().unwrap().text.to_ascii_lowercase();
            self.bump_kw();
            match word.as_str() {
                "as" => self.type_name(),
                "no" => {
                    // Exactly one of NO MINVALUE / NO MAXVALUE / NO CYCLE
                    let _ =
                        self.eat_kw("minvalue") || self.eat_kw("maxvalue") || self.eat_kw("cycle");
                }
                "owned" => {
                    self.eat_kw("by");
                    if !self.eat_kw("none") {
                        self.name_path();
                    }
                }
                "cycle" => {}
                _ => {
                    // INCREMENT [BY] n / START [WITH] n / RESTART [[WITH] n] / MINVALUE n / CACHE n
                    if !self.eat_kw("by") {
                        self.eat_kw("with");
                    }
                    if !self.at_statement_end() && !self.at_any_kw(SEQUENCE_OPTION_STARTS) {
                        self.expr();
                    }
                }
            }
            self.finish_node();
        }
    }

    /// `CREATE TYPE name [AS ENUM (labels) | AS (column type, ...) | AS RANGE (options) | (options)]`
    pub(super) fn create_type_stmt(&mut self) {
        self.start_node(NodeKind::CreateTypeStmt);
        self.bump_kw();
        self.bump_kw();
        self.name_path();
        if self.eat_kw("as") {
            if self.at(TokenKind::LParen) {
                // The columns of a composite type are read like CREATE TABLE columns
                self.table_element_list();
            } else if (self.eat_kw("enum") || self.eat_kw("range")) && self.at(TokenKind::LParen) {
                self.ddl_paren();
            }
        } else if self.at(TokenKind::LParen) {
            self.ddl_paren();
        }
        self.raw_until(|_| false);
        self.finish_node();
    }

    /// `CREATE SCHEMA [IF NOT EXISTS] [name] [AUTHORIZATION role]`.
    /// A statement with CREATE TABLE etc. inside is written verbatim as a whole so that the line
    /// breaks are preserved
    pub(super) fn create_schema_stmt(&mut self) {
        let mut n = 2;
        if self.nth_kw(n, "if") {
            n += 3;
        }
        if !self.nth_kw(n, "authorization") && self.nth_is_any_name(n) {
            n += 1;
        }
        if self.nth_kw(n, "authorization") {
            n += 2;
        }
        if self.nth(n).is_some_and(|t| t.kind != TokenKind::Semicolon) {
            self.raw_statement();
            return;
        }
        self.start_node(NodeKind::CreateSchemaStmt);
        self.bump_kw();
        self.bump_kw();
        self.if_exists();
        if !self.at_kw("authorization") {
            self.name_path();
        }
        if self.eat_kw("authorization") {
            self.name_path();
        }
        self.finish_node();
    }

    /// `CREATE EXTENSION [IF NOT EXISTS] name [WITH] [SCHEMA s] [VERSION v] [CASCADE]`
    pub(super) fn create_extension_stmt(&mut self) {
        self.start_node(NodeKind::CreateExtensionStmt);
        self.bump_kw();
        self.bump_kw();
        self.if_exists();
        self.name_path();
        while !self.at_statement_end() {
            // The VERSION value (whether 'x.y' or a name) is written verbatim on the next iteration
            if self.eat_kw("with") || self.eat_kw("cascade") || self.eat_kw("version") {
                continue;
            }
            if self.eat_kw("schema") {
                self.name_path();
            } else {
                self.raw_until(|p| p.at_any_kw(&["with", "schema", "version", "cascade"]));
            }
        }
        self.finish_node();
    }

    /// `ALTER object_type [IF EXISTS] name [(args)] [ON table] action ...` (except ALTER TABLE).
    /// ALTER SEQUENCE options become one `SequenceOption` each, and what follows
    /// ALTER DEFAULT PRIVILEGES is read as GRANT / REVOKE
    pub(super) fn alter_stmt(&mut self) {
        self.start_node(NodeKind::AlterStmt);
        self.bump_kw();
        let object_type = ALTER_OBJECT_TYPES
            .iter()
            .find(|words| words.iter().enumerate().all(|(i, w)| self.nth_kw(i, w)))
            .copied()
            .unwrap_or(&[]);
        for _ in 0..object_type.len() {
            self.bump_kw();
        }
        match object_type {
            ["default", "privileges"] => {
                // [FOR {ROLE | USER} r, ...] [IN SCHEMA s, ...] {GRANT | REVOKE} ...
                while self.at_any_kw(&["for", "in"]) {
                    self.bump_kw();
                    let _ = self.eat_kw("role") || self.eat_kw("user") || self.eat_kw("schema");
                    while !self.at_statement_end() && !self.at_any_kw(&["in", "grant", "revoke"]) {
                        if !self.eat(TokenKind::Comma) && self.name_path() == 0 {
                            self.raw_until(|p| p.at_any_kw(&["in", "grant", "revoke"]));
                        }
                    }
                }
                if self.at_any_kw(&["grant", "revoke"]) {
                    self.grant_rest();
                }
            }
            ["system"] => {}
            _ => {
                self.if_exists();
                self.name_path();
                if self.at(TokenKind::LParen) {
                    self.ddl_paren();
                }
                if self.eat_kw("on") {
                    self.name_path();
                }
                if object_type == ["sequence"] && self.at_any_kw(SEQUENCE_OPTION_STARTS) {
                    self.sequence_options();
                }
            }
        }
        self.alter_actions();
        self.finish_node();
    }

    /// ALTER actions. Known words become keywords; names, expressions, and parentheses are each
    /// read as such. Written on one line
    fn alter_actions(&mut self) {
        // The word last read as a keyword (used to recognize SET DEFAULT)
        let mut previous = String::new();
        while !self.at_statement_end() {
            if self.eat(TokenKind::Comma) {
                previous.clear();
                continue;
            }
            if self.at_any_kw(ALTER_KEYWORDS) {
                let word = self.current().unwrap().text.to_ascii_lowercase();
                self.bump_kw();
                match word.as_str() {
                    // SET DEFAULT expr (the DEFAULT in `SET x = DEFAULT` / `SET x TO DEFAULT` is
                    // a value)
                    "default" if previous == "set" && !self.at_statement_end() => {
                        self.expr();
                    }
                    _ if ALTER_NAME_BEFORE.contains(&word.as_str()) => {
                        self.if_exists();
                        // A name even when spelled like a keyword (`RENAME TO data` /
                        // `SET SCHEMA public`), except `SET x TO DEFAULT`, CURRENT_USER, etc.
                        if self.at_any_kw(ROLE_SPEC_KEYWORDS) {
                            self.bump_kw();
                        } else if !self.at_kw("default") {
                            self.name_path();
                        }
                    }
                    _ => {}
                }
                previous = word;
                continue;
            } else if self.at_kw("check") && self.nth_is(1, TokenKind::LParen) {
                self.bump_kw();
                self.expr();
            } else if self.eat_kw("type") {
                self.type_name();
            } else if self.at(TokenKind::LParen) {
                self.ddl_paren();
            } else if self.name_path() == 0 {
                self.raw_until(|p| p.at_any_kw(ALTER_KEYWORDS) || p.at(TokenKind::Comma));
            }
            previous.clear();
        }
    }

    /// `GRANT privileges ON [type] objects TO grantees [WITH GRANT OPTION] [GRANTED BY role]`
    /// / `REVOKE [GRANT OPTION FOR] privileges ON ... FROM grantees [CASCADE | RESTRICT]`
    /// / `GRANT role, ... TO role, ... [WITH ADMIN OPTION]`
    pub(super) fn grant_stmt(&mut self) {
        self.start_node(NodeKind::GrantStmt);
        self.grant_rest();
        self.finish_node();
    }

    /// The body of GRANT / REVOKE (also used after ALTER DEFAULT PRIVILEGES)
    pub(super) fn grant_rest(&mut self) {
        let at_list_end = |p: &Self| p.at_statement_end() || p.at_any_kw(&["on", "to", "from"]);
        self.bump_kw();
        // REVOKE [GRANT | ADMIN | INHERIT | SET] OPTION FOR
        if self.nth_kw(1, "option") && self.nth_kw(2, "for") {
            for _ in 0..3 {
                self.bump_kw();
            }
        }
        // Either the privilege list (including column lists such as `SELECT (a, b)`) or the list
        // of roles being granted
        while !at_list_end(self) {
            if self.at_any_kw(PRIVILEGES) {
                self.bump_kw();
            } else if self.at(TokenKind::LParen) {
                self.expr_list();
            } else if !self.eat(TokenKind::Comma) && self.name_path() == 0 {
                self.raw_until(|p| at_list_end(p) || p.at(TokenKind::Comma));
            }
        }
        if self.eat_kw("on") {
            if let Some(words) = GRANT_OBJECT_TYPES
                .iter()
                .find(|words| words.iter().enumerate().all(|(i, w)| self.nth_kw(i, w)))
            {
                for _ in 0..words.len() {
                    self.bump_kw();
                }
            }
            // The object list. A function's argument parentheses stay attached to the name
            while !self.at_statement_end() && !self.at_any_kw(&["to", "from"]) {
                if self.at(TokenKind::LParen) {
                    self.ddl_paren();
                } else if !self.eat(TokenKind::Comma) && self.name_path() == 0 {
                    self.raw_until(|p| p.at_any_kw(&["to", "from"]) || p.at(TokenKind::Comma));
                }
            }
        }
        if self.eat_kw("to") || self.eat_kw("from") {
            // The grantee role list. Roles named `admin` or `option` are kept as they are
            while !self.at_statement_end() && !self.at_any_kw(GRANTEE_LIST_ENDS) {
                if self.at_any_kw(GRANTEE_KEYWORDS) {
                    self.bump_kw();
                } else if !self.eat(TokenKind::Comma) && self.name_path() == 0 {
                    self.raw_until(|p| p.at_any_kw(GRANTEE_LIST_ENDS) || p.at(TokenKind::Comma));
                }
            }
        }
        // WITH GRANT OPTION / WITH ADMIN TRUE / GRANTED BY role / CASCADE
        while !self.at_statement_end() {
            if self.eat_kw("by") {
                if self.at_any_kw(GRANTEE_KEYWORDS) {
                    self.bump_kw();
                } else {
                    self.name_path();
                }
            } else if self.at_any_kw(GRANT_OPTION_WORDS) {
                self.bump_kw();
            } else if !self.eat(TokenKind::Comma) {
                self.raw_until(|p| p.at_any_kw(GRANT_OPTION_WORDS) || p.at(TokenKind::Comma));
            }
        }
    }

    /// `TRUNCATE [TABLE] [ONLY] name [*], ... [RESTART | CONTINUE IDENTITY] [CASCADE | RESTRICT]`
    pub(super) fn truncate_stmt(&mut self) {
        self.start_node(NodeKind::TruncateStmt);
        self.bump_kw();
        self.eat_kw("table");
        // The table list. Option words become keywords only after a table name has been read
        // (`TRUNCATE identity`)
        loop {
            self.eat_kw("only");
            if self.name_path() == 0 {
                // psql variables such as `:tbl` are written verbatim without being split
                self.raw_until(|p| p.at(TokenKind::Comma) || p.at_any_kw(TRUNCATE_OPTIONS));
            }
            if self.at_op("*") {
                self.bump();
            }
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        while !self.at_statement_end() {
            if self.at_any_kw(TRUNCATE_OPTIONS) {
                self.bump_kw();
            } else {
                self.raw_until(|p| p.at_any_kw(TRUNCATE_OPTIONS));
            }
        }
        self.finish_node();
    }

    /// Make an `Error` node, written verbatim, up to the end of the statement or `stop`. Always
    /// consumes at least one token unless at the end of the statement
    pub(super) fn raw_until(&mut self, stop: impl Fn(&Self) -> bool) {
        if self.at_statement_end() {
            return;
        }
        self.start_node(NodeKind::Error);
        loop {
            self.bump_balanced();
            if self.at_statement_end() || stop(self) {
                break;
            }
        }
        self.finish_node();
    }

    /// `COMMENT ON object_type name [(args)] [ON table] IS {'text' | NULL}`
    pub(super) fn comment_stmt(&mut self) {
        self.start_node(NodeKind::CommentStmt);
        self.bump_kw();
        self.bump_kw();
        // Only the word sequence from the list becomes keywords for the object type (the names in
        // `TABLE data` / `FAMILY text` are kept)
        if let Some(words) = COMMENT_OBJECT_TYPES
            .iter()
            .find(|words| words.iter().enumerate().all(|(i, w)| self.nth_kw(i, w)))
        {
            for _ in 0..words.len() {
                self.bump_kw();
            }
        }
        let at_name_end = |p: &Self| p.at_statement_end() || p.at_kw("is");
        while !at_name_end(self) {
            if self.at_any_kw(COMMENT_NAME_KEYWORDS) {
                self.bump_kw();
            } else if self.at(TokenKind::LParen) {
                self.ddl_paren();
            } else if self.name_path() == 0 {
                // psql variables such as `:tbl` and operators are written verbatim without being
                // split
                self.start_node(NodeKind::Error);
                while !at_name_end(self)
                    && !self.at_any_kw(COMMENT_NAME_KEYWORDS)
                    && !self.at(TokenKind::LParen)
                {
                    self.bump_balanced();
                }
                self.finish_node();
            }
        }
        if self.eat_kw("is") {
            self.expr();
        }
        // Whatever could not be read as an expression (`:'v'` / `UESCAPE '!'`, etc.) is also written
        // verbatim on the same line
        if !self.at_statement_end() {
            self.start_node(NodeKind::Error);
            while !self.at_statement_end() {
                self.bump_balanced();
            }
            self.finish_node();
        }
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
                // The column or constraint name after DROP / ALTER / RENAME
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
        // Actions where a name follows the first word, such as `INHERIT parent`
        if ALTER_TABLE_NAME_BEFORE.contains(&first.as_str()) {
            self.name_path();
        }
        self.alter_table_words();
        self.finish_node();
        true
    }

    /// The rest of an ALTER TABLE action. The name after `RENAME TO x` / `SET SCHEMA x` /
    /// `ATTACH PARTITION x`, etc. is read as a name even when spelled like a keyword
    fn alter_table_words(&mut self) {
        loop {
            self.ddl_words(|p| p.at_any_kw(ALTER_TABLE_NAME_BEFORE));
            if !self.at_any_kw(ALTER_TABLE_NAME_BEFORE) {
                break;
            }
            self.bump_kw();
            if self.at_any_kw(ROLE_SPEC_KEYWORDS) {
                self.bump_kw();
            } else if !self.at_any_kw(&["all", "user", "always", "replica"]) {
                // Keywords such as ENABLE TRIGGER ALL / USER are read by the next ddl_words
                self.name_path();
            }
        }
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
