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

/// シーケンスのオプションの始まり（CREATE / ALTER SEQUENCE）
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

/// ALTER の後ろのオブジェクトの種類（ALTER TABLE は別に読む）。長いものから順に当てる
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

/// ALTER の操作でキーワードにする語
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

/// ALTER で、後ろに名前が来るキーワード（`RENAME TO x` / `SET SCHEMA x` / `RENAME COLUMN a TO b` など）
const ALTER_NAME_BEFORE: &[&str] = &[
    "to",
    "schema",
    "column",
    "attribute",
    "constraint",
    "tablespace",
];

/// GRANT / REVOKE の権限
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

/// GRANT / REVOKE の ON の後ろのオブジェクトの種類。長いものから順に当てる
/// （ALTER DEFAULT PRIVILEGES の複数形も含む）
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

/// GRANT / REVOKE の TO / FROM の後ろでキーワードにする語
const GRANTEE_KEYWORDS: &[&str] = &[
    "group",
    "public",
    "current_role",
    "current_user",
    "session_user",
];

/// ALTER TABLE で、後ろに名前が来るキーワード
const ALTER_TABLE_NAME_BEFORE: &[&str] = &[
    "index",
    "inherit",
    "partition",
    "schema",
    "tablespace",
    "to",
    "trigger",
];

/// ロールを指定する位置でキーワードにする語
const ROLE_SPEC_KEYWORDS: &[&str] = &["current_role", "current_user", "session_user"];

/// GRANT / REVOKE の受け取るロールの並びの後ろに続く語
const GRANTEE_LIST_ENDS: &[&str] = &["with", "granted", "cascade", "restrict"];

/// GRANT / REVOKE の末尾のオプション
const GRANT_OPTION_WORDS: &[&str] = &[
    "with", "grant", "admin", "inherit", "set", "option", "true", "false", "granted", "by",
    "cascade", "restrict",
];

/// TRUNCATE の表の並びの後ろのオプション
const TRUNCATE_OPTIONS: &[&str] = &["restart", "continue", "identity", "cascade", "restrict"];

/// COMMENT ON の後ろのオブジェクトの種類（PostgreSQL 18 の文書の一覧）。長いものから順に当てる
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

/// COMMENT ON の名前の後ろでキーワードにする語
/// （`ON [DOMAIN] table` / `OPERATOR CLASS c USING btree` / `TRANSFORM FOR t LANGUAGE l`）
const COMMENT_NAME_KEYWORDS: &[&str] = &["on", "domain", "using", "language"];

/// CREATE TRIGGER の句の始まり
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
                // `USING btree` / `USING expr`（ALTER COLUMN ... TYPE ... USING）。
                // REPLICA IDENTITY USING INDEX の INDEX は呼び出し側で読む
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

    /// 列名やオプションの括弧 `(a, b)` / `(fillfactor = 70)` / `(START WITH 1)`
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
    ///  EXECUTE {FUNCTION | PROCEDURE} f(args)`。名前の後ろは句ごとに `TriggerClause` にする
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
                // 解釈できない部分は次の句まで 1 つにまとめ、前の句と同じ行に元のまま書く
                // （`ON :tbl` のような psql の変数を分けない）
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
                // `{OLD | NEW} TABLE [AS] name` の並び
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

    /// 文の終わりまでのシーケンスのオプション。1 つずつ `SequenceOption` にする
    pub(super) fn sequence_options(&mut self) {
        while !self.at_statement_end() {
            if !self.at_any_kw(SEQUENCE_OPTION_STARTS) {
                // 解釈できない部分は次のオプションまで元のまま書く
                self.raw_until(|p| p.at_any_kw(SEQUENCE_OPTION_STARTS));
                continue;
            }
            self.start_node(NodeKind::SequenceOption);
            let word = self.current().unwrap().text.to_ascii_lowercase();
            self.bump_kw();
            match word.as_str() {
                "as" => self.type_name(),
                "no" => {
                    // NO MINVALUE / NO MAXVALUE / NO CYCLE のどれか 1 つ
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
                // 複合型の列は CREATE TABLE の列と同じに読む
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

    /// `CREATE SCHEMA [IF NOT EXISTS] [name] [AUTHORIZATION role]`。
    /// 中に CREATE TABLE などを書いた文は、改行の位置を残すために全体を元のまま書く
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
            // VERSION の値（'x.y' でも名前でも）は、次の繰り返しで元のまま書く
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

    /// `ALTER object_type [IF EXISTS] name [(args)] [ON table] action ...`（ALTER TABLE 以外）。
    /// ALTER SEQUENCE のオプションは 1 つずつ `SequenceOption` にし、
    /// ALTER DEFAULT PRIVILEGES の後ろは GRANT / REVOKE として読む
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

    /// ALTER の操作。既知の語をキーワードにし、名前・式・括弧はそれぞれ読む。1 行に書く
    fn alter_actions(&mut self) {
        // 直前にキーワードとして読んだ語（SET DEFAULT の判定に使う）
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
                    // SET DEFAULT expr（`SET x = DEFAULT` / `SET x TO DEFAULT` の DEFAULT は値）
                    "default" if previous == "set" && !self.at_statement_end() => {
                        self.expr();
                    }
                    _ if ALTER_NAME_BEFORE.contains(&word.as_str()) => {
                        self.if_exists();
                        // キーワードと同じ綴りでも名前（`RENAME TO data` / `SET SCHEMA public`）。
                        // `SET x TO DEFAULT` と CURRENT_USER などは除く
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

    /// GRANT / REVOKE の本体（ALTER DEFAULT PRIVILEGES の後ろでも使う）
    pub(super) fn grant_rest(&mut self) {
        let at_list_end = |p: &Self| p.at_statement_end() || p.at_any_kw(&["on", "to", "from"]);
        self.bump_kw();
        // REVOKE [GRANT | ADMIN | INHERIT | SET] OPTION FOR
        if self.nth_kw(1, "option") && self.nth_kw(2, "for") {
            for _ in 0..3 {
                self.bump_kw();
            }
        }
        // 権限の並び（列の並び `SELECT (a, b)` を含む）か、付与するロールの並び
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
            // オブジェクトの並び。関数の引数の括弧は名前に続ける
            while !self.at_statement_end() && !self.at_any_kw(&["to", "from"]) {
                if self.at(TokenKind::LParen) {
                    self.ddl_paren();
                } else if !self.eat(TokenKind::Comma) && self.name_path() == 0 {
                    self.raw_until(|p| p.at_any_kw(&["to", "from"]) || p.at(TokenKind::Comma));
                }
            }
        }
        if self.eat_kw("to") || self.eat_kw("from") {
            // 受け取るロールの並び。`admin` や `option` という名前のロールもそのまま
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
        // 表の並び。オプションの語は、表の名前を読んだ後だけキーワードにする（`TRUNCATE identity`）
        loop {
            self.eat_kw("only");
            if self.name_path() == 0 {
                // `:tbl` のような psql の変数は、分けずに元のまま書く
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

    /// 文の終わりか `stop` の手前までを、元のまま書く `Error` にする。文の終わりでなければ必ず 1 つは読む
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
        // 種類は一覧の語の並びだけをキーワードにする（`TABLE data` / `FAMILY text` の名前は残す）
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
                // `:tbl` のような psql の変数や演算子は、分けずに元のまま書く
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
        // `:'v'` / `UESCAPE '!'` など、式として読めなかった残りも同じ行に元のまま書く
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
        // `INHERIT parent` のように先頭の語の後ろに名前が来る操作
        if ALTER_TABLE_NAME_BEFORE.contains(&first.as_str()) {
            self.name_path();
        }
        self.alter_table_words();
        self.finish_node();
        true
    }

    /// ALTER TABLE の操作の残り。`RENAME TO x` / `SET SCHEMA x` / `ATTACH PARTITION x` などの後ろの名前は、
    /// キーワードと同じ綴りでも名前として読む
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
                // ENABLE TRIGGER ALL / USER などのキーワードは次の ddl_words で読む
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
