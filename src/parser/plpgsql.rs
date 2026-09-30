//! PL/pgSQL の関数本体（PostgreSQL の `src/pl/plpgsql/src/pl_gram.y` に合わせた範囲）。
//!
//! 文はすべて `;` で終わる。文の途中で解釈できなくなったら、`;` かブロックの区切り
//! （`END` / `ELSE` / `ELSIF` / `WHEN` / `EXCEPTION`）までを `Error` にして次の文へ進む。

use super::Parser;
use crate::lexer::TokenKind;
use crate::syntax::NodeKind;

/// 文の並びを終わらせるキーワード
const BLOCK_ENDS: &[&str] = &["end", "else", "elsif", "elseif", "when", "exception"];

/// PL/pgSQL の文としては解釈せず、`;` までをそのまま保持する SQL のコマンド
const RAW_SQL_COMMANDS: &[&str] = &[
    "alter",
    "analyze",
    "checkpoint",
    "cluster",
    "comment",
    "copy",
    "create",
    "deallocate",
    "discard",
    "drop",
    "explain",
    "grant",
    "import",
    "listen",
    "load",
    "lock",
    "merge",
    "notify",
    "prepare",
    "refresh",
    "reindex",
    "release",
    "reset",
    "revoke",
    "savepoint",
    "security",
    "set",
    "show",
    "start",
    "truncate",
    "unlisten",
    "vacuum",
];

const RAISE_LEVELS: &[&str] = &["debug", "log", "info", "notice", "warning", "exception"];

impl Parser<'_> {
    /// 本体全体。ブロックの後ろに残ったものは `Error` にする。
    pub(super) fn pl_body(&mut self) {
        if self.at_pl_block_start() {
            self.pl_block();
        }
        if !self.at_eof() {
            self.start_node(NodeKind::Error);
            while !self.at_eof() {
                self.bump();
            }
            self.finish_node();
        }
    }

    fn at_pl_label(&self) -> bool {
        let nth_op = |n, op| {
            self.nth(n)
                .is_some_and(|t| t.kind == TokenKind::Operator && t.text == op)
        };
        nth_op(0, "<<") && self.nth_is_name(1) && nth_op(2, ">>")
    }

    fn at_pl_block_start(&self) -> bool {
        let n = if self.at_pl_label() { 3 } else { 0 };
        self.nth_kw(n, "declare") || self.nth_kw(n, "begin")
    }

    fn pl_label(&mut self) {
        self.start_node(NodeKind::PlLabel);
        self.bump();
        self.bump();
        self.bump();
        self.finish_node();
    }

    /// `[<<label>>] [DECLARE decl ...] BEGIN stmt ... [EXCEPTION handler ...] END [label];`
    fn pl_block(&mut self) {
        self.start_node(NodeKind::PlBlock);
        if self.at_pl_label() {
            self.pl_label();
        }
        if self.at_kw("declare") {
            self.start_node(NodeKind::PlDeclareSection);
            self.bump_kw();
            while !self.at_eof() && !self.at_kw("begin") {
                if self.eat_kw("declare") {
                    continue;
                }
                if !self.pl_declaration() {
                    self.pl_skip_to_statement_end(&["begin"]);
                }
            }
            self.finish_node();
        }
        if self.eat_kw("begin") {
            self.pl_statements();
            if self.at_kw("exception") {
                self.pl_exception_section();
            }
        }
        // 対応のない ELSE / WHEN などが残っていたら、それを `Error` にして END を探し続ける
        while !self.at_eof() && !self.at_kw("end") {
            self.error_token();
            self.pl_statements();
        }
        if self.eat_kw("end") && self.at_name() {
            self.bump();
        }
        self.eat(TokenKind::Semicolon);
        self.finish_node();
    }

    /// `name [CONSTANT] type [COLLATE c] [NOT NULL] [{DEFAULT | := | =} expr];`
    /// / `name ALIAS FOR $1;` / `name [[NO] SCROLL] CURSOR [(args)] {FOR | IS} query;`
    fn pl_declaration(&mut self) -> bool {
        if !self.at_name() {
            return false;
        }
        self.start_node(NodeKind::PlDecl);
        self.bump();
        if self.eat_kw("alias") {
            self.eat_kw("for");
            if !self.at_statement_end() {
                self.bump();
            }
        } else if self.at_any_kw(&["no", "scroll", "cursor"]) {
            self.eat_kw("no");
            self.eat_kw("scroll");
            self.eat_kw("cursor");
            if self.at(TokenKind::LParen) {
                self.bump_balanced();
            }
            if self.eat_kw("for") || self.eat_kw("is") {
                self.statement_body();
            }
        } else {
            self.eat_kw("constant");
            self.type_name();
            if self.eat_kw("collate") {
                self.name_path();
            }
            if self.eat_kw("not") {
                self.eat_kw("null");
            }
            if self.eat_kw("default") || self.eat(TokenKind::ColonEquals) {
                self.expr();
            } else if self.at_op("=") {
                self.bump();
                self.expr();
            }
        }
        self.pl_end_statement();
        self.finish_node();
        true
    }

    /// 文の並び。ブロックの区切りのキーワードか本体の終わりで止まる。
    fn pl_statements(&mut self) {
        while !self.at_eof() && !self.at_any_kw(BLOCK_ENDS) {
            // 空の文
            if self.eat(TokenKind::Semicolon) {
                continue;
            }
            let before = self.current().map(|t| t.offset);
            self.pl_statement();
            if self.current().map(|t| t.offset) == before {
                self.error_token();
            }
        }
    }

    fn pl_statement(&mut self) {
        let Some(token) = self.current() else {
            return;
        };
        if self.at_pl_block_start() {
            return self.pl_block();
        }
        if self.at_pl_label() {
            // ラベルはブロックかループの前にだけ書ける
            return self.pl_loop();
        }
        let word = if token.kind == TokenKind::Ident {
            token.text.to_ascii_lowercase()
        } else {
            String::new()
        };
        match word.as_str() {
            "if" => self.pl_if(),
            "case" => self.pl_case(),
            "loop" | "while" | "for" | "foreach" => self.pl_loop(),
            "exit" | "continue" => self.pl_simple(NodeKind::PlExit, |p| {
                if p.at_name() && !p.at_kw("when") {
                    p.bump();
                }
                if p.eat_kw("when") {
                    p.expr();
                }
            }),
            "return" => self.pl_simple(NodeKind::PlReturn, Self::pl_return_rest),
            "raise" => self.pl_simple(NodeKind::PlRaise, Self::pl_raise_rest),
            "assert" => self.pl_simple(NodeKind::PlAssert, |p| {
                p.expr();
                if p.eat(TokenKind::Comma) {
                    p.expr();
                }
            }),
            "perform" => {
                self.start_node(NodeKind::PlPerform);
                self.simple_select();
                self.pl_end_statement();
                self.finish_node();
            }
            "execute" => self.pl_simple(NodeKind::PlExecute, |p| {
                p.expr();
                p.pl_into_using();
            }),
            "get" => self.pl_simple(NodeKind::PlGetDiagnostics, Self::pl_get_diagnostics_rest),
            "open" => self.pl_simple(NodeKind::PlOpen, Self::pl_open_rest),
            // 文の先頭の NULL は NULL 文しかない
            "null" => self.pl_simple(NodeKind::PlNull, |_| {}),
            "fetch" | "move" | "close" | "commit" | "rollback" => {
                self.pl_simple(NodeKind::PlSimpleStmt, |p| {
                    while !p.at_statement_end() {
                        p.bump_balanced();
                    }
                })
            }
            "call" => self.pl_sql(Self::call_stmt),
            _ if self.at_query_start(0) || self.at_any_kw(&["insert", "update", "delete"]) => self
                .pl_sql(|p| {
                    p.statement_body();
                }),
            _ if self.at_any_kw(RAW_SQL_COMMANDS) => self.pl_sql(Self::raw_statement),
            _ if self.at_name() => self.pl_assignment(),
            _ => {}
        }
    }

    /// 先頭のキーワードを取り込み、`rest` で残りを読んで `;` で閉じる文
    fn pl_simple(&mut self, kind: NodeKind, rest: impl FnOnce(&mut Self)) {
        self.start_node(kind);
        self.bump_kw();
        rest(self);
        self.pl_end_statement();
        self.finish_node();
    }

    /// 本体の中の SQL 文
    fn pl_sql(&mut self, statement: impl FnOnce(&mut Self)) {
        self.start_node(NodeKind::PlSqlStmt);
        statement(self);
        self.pl_end_statement();
        self.finish_node();
    }

    /// `target := expr;`。代入でなければ `;` までを 1 つの文として保持する。
    fn pl_assignment(&mut self) {
        let cp = self.checkpoint();
        self.set_target();
        let is_assignment = self.at(TokenKind::ColonEquals) || self.at_op("=");
        if is_assignment {
            self.bump();
            self.expr();
        }
        self.pl_end_statement();
        self.wrap(
            cp,
            if is_assignment {
                NodeKind::PlAssign
            } else {
                NodeKind::PlSqlStmt
            },
        );
    }

    /// `;` で文を閉じる。`;` の前に読み残しがあれば `Error` にする。
    fn pl_end_statement(&mut self) {
        if !self.at(TokenKind::Semicolon) {
            self.error_until(|p| p.at_any_kw(BLOCK_ENDS));
        }
        self.eat(TokenKind::Semicolon);
    }

    /// 解釈できなかった宣言などを、`;`（とその `;`）か `stops` の手前まで `Error` にする
    fn pl_skip_to_statement_end(&mut self, stops: &[&str]) {
        if self.at_eof() || self.at_any_kw(stops) {
            return;
        }
        self.start_node(NodeKind::Error);
        while !self.at_eof() && !self.at(TokenKind::Semicolon) && !self.at_any_kw(stops) {
            self.bump();
        }
        self.eat(TokenKind::Semicolon);
        self.finish_node();
    }

    /// `IF cond THEN stmt ... [ELSIF cond THEN ...] [ELSE ...] END IF;`
    fn pl_if(&mut self) {
        self.start_node(NodeKind::PlIf);
        self.bump_kw();
        self.expr();
        self.eat_kw("then");
        self.pl_statements();
        while self.at_kw("elsif") || self.at_kw("elseif") {
            self.start_node(NodeKind::PlElsif);
            self.bump_kw();
            self.expr();
            self.eat_kw("then");
            self.pl_statements();
            self.finish_node();
        }
        self.pl_else();
        if self.eat_kw("end") {
            self.eat_kw("if");
        }
        self.pl_end_statement();
        self.finish_node();
    }

    fn pl_else(&mut self) {
        if self.at_kw("else") {
            self.start_node(NodeKind::PlElse);
            self.bump_kw();
            self.pl_statements();
            self.finish_node();
        }
    }

    /// `CASE [expr] WHEN expr, ... THEN stmt ... [ELSE ...] END CASE;`
    fn pl_case(&mut self) {
        self.start_node(NodeKind::PlCase);
        self.bump_kw();
        if !self.at_kw("when") {
            self.expr();
        }
        while self.at_kw("when") {
            self.start_node(NodeKind::PlCaseWhen);
            self.bump_kw();
            while self.expr() && self.eat(TokenKind::Comma) {}
            self.eat_kw("then");
            self.pl_statements();
            self.finish_node();
        }
        self.pl_else();
        if self.eat_kw("end") {
            self.eat_kw("case");
        }
        self.pl_end_statement();
        self.finish_node();
    }

    /// `[<<label>>] {LOOP | WHILE cond LOOP | FOR ... LOOP | FOREACH ... LOOP} stmt ... END LOOP [label];`
    fn pl_loop(&mut self) {
        self.start_node(NodeKind::PlLoop);
        if self.at_pl_label() {
            self.pl_label();
        }
        if self.at_kw("while") {
            self.bump_kw();
            self.expr();
        } else if self.at_kw("for") {
            self.bump_kw();
            self.pl_for_header();
        } else if self.at_kw("foreach") {
            self.bump_kw();
            while self.name_path() > 0 && self.eat(TokenKind::Comma) {}
            if self.eat_kw("slice") {
                self.expr_without_in();
            }
            self.eat_kw("in");
            self.eat_kw("array");
            self.expr();
        }
        if self.eat_kw("loop") {
            self.pl_statements();
            if self.eat_kw("end") {
                self.eat_kw("loop");
                if self.at_name() {
                    self.bump();
                }
            }
        }
        self.pl_end_statement();
        self.finish_node();
    }

    /// `FOR` の後ろの `target IN ...`。整数の範囲・問い合わせ・EXECUTE・カーソルのいずれか。
    fn pl_for_header(&mut self) {
        while self.name_path() > 0 && self.eat(TokenKind::Comma) {}
        self.eat_kw("in");
        self.eat_kw("reverse");
        if self.at_query_start(0) {
            self.with_stops(&["loop"], |p| {
                p.statement_body();
            });
        } else if self.eat_kw("execute") {
            self.expr();
            if self.at_kw("using") {
                self.pl_using();
            }
        } else {
            self.expr();
            if self.eat(TokenKind::DotDot) {
                self.expr();
                if self.eat_kw("by") {
                    self.expr();
                }
            }
        }
    }

    /// `RETURN [expr]` / `RETURN NEXT [expr]` / `RETURN QUERY query` / `RETURN QUERY EXECUTE ...`
    fn pl_return_rest(&mut self) {
        if self.eat_kw("query") {
            if self.eat_kw("execute") {
                self.expr();
                if self.at_kw("using") {
                    self.pl_using();
                }
            } else {
                self.statement_body();
            }
            return;
        }
        self.eat_kw("next");
        if !self.at_statement_end() {
            self.expr();
        }
    }

    /// `RAISE [level] {'format' [, expr ...] | condition | SQLSTATE 'code'} [USING option = expr, ...]`
    fn pl_raise_rest(&mut self) {
        if self.at_any_kw(RAISE_LEVELS) {
            self.bump_kw();
        }
        if self.eat_kw("sqlstate") {
            self.expr();
        } else {
            // USING や `;` では式が始まらないので、何も読まずに終わる
            while self.expr() && self.eat(TokenKind::Comma) {}
        }
        if self.at_kw("using") {
            self.start_node(NodeKind::PlUsing);
            self.bump_kw();
            loop {
                if self.at_name() {
                    self.bump_kw();
                }
                if self.at_op("=") || self.at(TokenKind::ColonEquals) {
                    self.bump();
                }
                if !self.expr() || !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            self.finish_node();
        }
    }

    /// EXECUTE の `INTO [STRICT] target, ...` と `USING expr, ...`（順不同）
    fn pl_into_using(&mut self) {
        loop {
            if self.at_kw("into") {
                self.result_into_clause();
            } else if self.at_kw("using") {
                self.pl_using();
            } else {
                break;
            }
        }
    }

    fn pl_using(&mut self) {
        self.start_node(NodeKind::PlUsing);
        self.bump_kw();
        while self.expr() && self.eat(TokenKind::Comma) {}
        self.finish_node();
    }

    /// `GET [CURRENT | STACKED] DIAGNOSTICS target {= | :=} item, ...`
    fn pl_get_diagnostics_rest(&mut self) {
        if !self.eat_kw("current") {
            self.eat_kw("stacked");
        }
        self.eat_kw("diagnostics");
        loop {
            self.name_path();
            if self.at_op("=") || self.at(TokenKind::ColonEquals) {
                self.bump();
            }
            if self.at_name() {
                self.bump_kw();
            }
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
    }

    /// `OPEN cursor [[NO] SCROLL] [FOR query | FOR EXECUTE expr [USING ...]]` / `OPEN cursor [(args)]`
    fn pl_open_rest(&mut self) {
        self.name_path();
        self.eat_kw("no");
        self.eat_kw("scroll");
        if self.at(TokenKind::LParen) {
            self.arg_list(0);
        }
        if self.eat_kw("for") {
            if self.eat_kw("execute") {
                self.expr();
                if self.at_kw("using") {
                    self.pl_using();
                }
            } else {
                self.statement_body();
            }
        }
    }

    /// `EXCEPTION WHEN cond [OR cond ...] THEN stmt ... ...`
    fn pl_exception_section(&mut self) {
        self.start_node(NodeKind::PlExceptionSection);
        self.bump_kw();
        while self.at_kw("when") {
            self.start_node(NodeKind::PlExceptionHandler);
            self.bump_kw();
            loop {
                if self.eat_kw("sqlstate") {
                    self.expr();
                } else if self.at_name() && !self.at_kw("then") {
                    self.bump();
                }
                if !self.eat_kw("or") {
                    break;
                }
            }
            self.eat_kw("then");
            self.pl_statements();
            self.finish_node();
        }
        self.finish_node();
    }
}
