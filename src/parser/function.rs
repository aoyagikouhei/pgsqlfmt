//! `CREATE FUNCTION` / `CREATE PROCEDURE` / `DO` / `CALL`.
//!
//! A dollar-quoted body is re-lexed and parsed by a separate parser, as PL/pgSQL for
//! `LANGUAGE plpgsql` and as a list of SQL statements for `LANGUAGE sql`, and embedded in the tree
//! as a `FunctionBody` node. Bodies in any other language are kept as their original tokens.

use super::Parser;
use crate::lexer::{Token, TokenKind, tokenize_with_offset};
use crate::syntax::{Element, Node, NodeKind};

/// Words that start a function option
const OPTION_STARTS: &[&str] = &[
    "language",
    "as",
    "immutable",
    "stable",
    "volatile",
    "not",
    "leakproof",
    "called",
    "returns",
    "strict",
    "security",
    "external",
    "parallel",
    "cost",
    "rows",
    "support",
    "set",
    "reset",
    "return",
    "window",
    "transform",
];

/// Option values that are treated as keywords
const OPTION_WORDS: &[&str] = &[
    "null",
    "on",
    "input",
    "safe",
    "unsafe",
    "restricted",
    "definer",
    "invoker",
    "to",
    "from",
    "current",
    "for",
    "type",
    "leakproof",
    "security",
];

/// The first word of a multi-word type name and the word that follows it
const MULTIWORD_TYPES: &[(&str, &str)] = &[
    ("double", "precision"),
    ("character", "varying"),
    ("char", "varying"),
    ("bit", "varying"),
    ("national", "character"),
    ("national", "char"),
    ("timestamp", "with"),
    ("timestamp", "without"),
    ("time", "with"),
    ("time", "without"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyLanguage {
    PlPgSql,
    Sql,
    Other,
}

impl BodyLanguage {
    fn from_name(name: &str) -> Self {
        let name = name.trim_matches(|c| c == '\'' || c == '"');
        if name.eq_ignore_ascii_case("plpgsql") {
            BodyLanguage::PlPgSql
        } else if name.eq_ignore_ascii_case("sql") {
            BodyLanguage::Sql
        } else {
            BodyLanguage::Other
        }
    }
}

impl<'a> Parser<'a> {
    pub(super) fn at_create_function(&self) -> bool {
        let n = if self.nth_kw(1, "or") && self.nth_kw(2, "replace") {
            3
        } else {
            1
        };
        self.at_kw("create") && (self.nth_kw(n, "function") || self.nth_kw(n, "procedure"))
    }

    /// `CREATE [OR REPLACE] {FUNCTION | PROCEDURE} name (params) [RETURNS ...] option ...`
    pub(super) fn create_function_stmt(&mut self) {
        // LANGUAGE does not necessarily come before the body, so find it first
        let language = self.find_language().unwrap_or(BodyLanguage::Sql);
        self.start_node(NodeKind::CreateFunctionStmt);
        self.bump_kw();
        if self.eat_kw("or") {
            self.eat_kw("replace");
        }
        self.bump_kw();
        self.name_path();
        if self.at(TokenKind::LParen) {
            self.param_list();
        }
        while !self.at_statement_end() {
            if self.at_kw("returns") && !self.nth_kw(1, "null") {
                self.returns_clause();
            } else if self.at_kw("as") {
                self.start_node(NodeKind::FunctionOption);
                self.bump_kw();
                if !self.at_statement_end() {
                    self.body_string(language);
                }
                // `AS 'obj_file', 'link_symbol'` of a C-language function
                if self.eat(TokenKind::Comma) && !self.at_statement_end() {
                    self.bump();
                }
                self.finish_node();
            } else if self.at_kw("begin") && self.nth_kw(1, "atomic") {
                self.atomic_body();
            } else if self.at_kw("return") {
                // The SQL-standard body `RETURN expr`
                self.start_node(NodeKind::FunctionOption);
                self.bump_kw();
                self.expr();
                self.finish_node();
            } else if self.at_any_kw(OPTION_STARTS) {
                self.function_option();
            } else {
                self.error_token();
            }
        }
        self.finish_node();
    }

    /// Find `LANGUAGE name` before the end of the statement
    fn find_language(&self) -> Option<BodyLanguage> {
        let mut n = 0;
        while let Some(token) = self.nth(n) {
            if token.kind == TokenKind::Semicolon {
                return None;
            }
            if self.nth_kw(n, "language") {
                return self.nth(n + 1).map(|t| BodyLanguage::from_name(t.text));
            }
            n += 1;
        }
        None
    }

    /// `(name type, ...)`. Also used for the columns of `RETURNS TABLE (...)`.
    fn param_list(&mut self) {
        self.start_node(NodeKind::ParamList);
        self.bump();
        self.comma_list(|_| false, Self::param);
        self.expect_closing(TokenKind::RParen);
        self.finish_node();
    }

    /// `[IN | OUT | INOUT | VARIADIC] [name] type [{DEFAULT | =} expr]`
    fn param(&mut self) -> bool {
        if !self.at_name() {
            return false;
        }
        self.start_node(NodeKind::Param);
        if self.at_any_kw(&["in", "out", "inout", "variadic"]) {
            self.bump_kw();
        }
        if self.at_param_name() {
            self.bump();
        }
        self.type_name();
        if self.at_op("=") {
            self.bump();
            self.expr();
        } else if self.eat_kw("default") {
            self.expr();
        }
        self.finish_node();
        true
    }

    pub(super) fn nth_is_name(&self, n: usize) -> bool {
        self.nth(n)
            .is_some_and(|t| matches!(t.kind, TokenKind::Ident | TokenKind::QuotedIdent { .. }))
    }

    /// Whether the leading name of a parameter is a parameter name rather than a type name. It is
    /// taken as a parameter name when a type name follows.
    fn at_param_name(&self) -> bool {
        let Some(current) = self.current().filter(|_| self.at_name()) else {
            return false;
        };
        if !self.nth_is_name(1) || self.nth_kw(1, "default") {
            return false;
        }
        let next = self.nth(1).unwrap().text;
        !MULTIWORD_TYPES.iter().any(|(first, second)| {
            current.text.eq_ignore_ascii_case(first) && next.eq_ignore_ascii_case(second)
        })
    }

    /// `RETURNS [SETOF] type` / `RETURNS TABLE (col type, ...)`
    fn returns_clause(&mut self) {
        self.start_node(NodeKind::ReturnsClause);
        self.bump_kw();
        if self.at_kw("table") && self.nth_is(1, TokenKind::LParen) {
            self.bump_kw();
            self.param_list();
        } else {
            self.eat_kw("setof");
            self.type_name();
        }
        self.finish_node();
    }

    /// An option such as `IMMUTABLE` / `SECURITY DEFINER` / `SET search_path = ...`, up to the next
    /// option.
    fn function_option(&mut self) {
        self.start_node(NodeKind::FunctionOption);
        let two_words = self.at_kw("not") || self.at_kw("external");
        self.bump_kw();
        if two_words && !self.at_statement_end() {
            self.bump_kw();
        }
        while !self.at_statement_end()
            && !self.at_any_kw(OPTION_STARTS)
            && !(self.at_kw("begin") && self.nth_kw(1, "atomic"))
        {
            if self.at_any_kw(OPTION_WORDS) {
                self.bump_kw();
            } else {
                self.bump_balanced();
            }
        }
        self.finish_node();
    }

    /// `BEGIN ATOMIC stmt; ... END`
    fn atomic_body(&mut self) {
        self.start_node(NodeKind::AtomicBody);
        self.bump_kw();
        self.bump_kw();
        while !self.at_eof() && !self.at_kw("end") {
            if self.eat(TokenKind::Semicolon) {
                continue;
            }
            let before = self.current().map(|t| t.offset);
            self.statement();
            if self.current().map(|t| t.offset) == before {
                self.error_token();
            }
        }
        self.eat_kw("end");
        self.finish_node();
    }

    /// `DO [LANGUAGE name] body` (LANGUAGE may also be written after the body)
    pub(super) fn do_stmt(&mut self) {
        let language = self.find_language().unwrap_or(BodyLanguage::PlPgSql);
        self.start_node(NodeKind::DoStmt);
        self.bump_kw();
        while !self.at_statement_end() {
            if self.eat_kw("language") {
                if !self.at_statement_end() {
                    self.bump();
                }
            } else if self.current().is_some_and(|t| {
                matches!(
                    t.kind,
                    TokenKind::DollarString { .. } | TokenKind::String { .. }
                )
            }) {
                self.body_string(language);
            } else {
                self.error_token();
            }
        }
        self.finish_node();
    }

    /// `CALL proc(args)`
    pub(super) fn call_stmt(&mut self) {
        self.start_node(NodeKind::CallStmt);
        self.bump_kw();
        self.expr();
        self.finish_node();
    }

    /// The function body string. If it is a terminated dollar-quoted string and the language is
    /// PL/pgSQL or SQL, its contents are parsed and embedded.
    fn body_string(&mut self, language: BodyLanguage) {
        let Some(token) = self.current() else {
            return;
        };
        if token.kind != (TokenKind::DollarString { terminated: true })
            || language == BodyLanguage::Other
        {
            self.bump();
            return;
        }
        let body = parse_dollar_body(token, language);
        self.eat_trivia();
        self.pos += 1;
        self.stack.last_mut().unwrap().1.push(Element::Node(body));
    }
}

/// Split `$tag$ ... $tag$` into the delimiters and the contents parsed by a separate parser
fn parse_dollar_body<'a>(token: Token<'a>, language: BodyLanguage) -> Node<'a> {
    let text = token.text;
    let delimiter_len = text[1..].find('$').expect("dollar-quote delimiter") + 2;
    let open = Token {
        kind: TokenKind::DollarDelimiter,
        text: &text[..delimiter_len],
        offset: token.offset,
    };
    let close_start = text.len() - delimiter_len;
    let close = Token {
        kind: TokenKind::DollarDelimiter,
        text: &text[close_start..],
        offset: token.offset + close_start,
    };

    let tokens = tokenize_with_offset(
        &text[delimiter_len..close_start],
        token.offset + delimiter_len,
    );
    let mut inner = Parser::new(tokens);
    match language {
        BodyLanguage::PlPgSql => inner.pl_body(),
        _ => inner.statements(),
    }
    let root = inner.finish();

    let mut children = vec![Element::Token(open)];
    children.extend(root.children);
    children.push(Element::Token(close));
    Node {
        kind: NodeKind::FunctionBody,
        children,
    }
}
