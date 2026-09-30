//! `CREATE FUNCTION` / `CREATE PROCEDURE` / `DO` / `CALL`。
//!
//! ドル引用符の本体は、`LANGUAGE plpgsql` なら PL/pgSQL として、`LANGUAGE sql` なら SQL の文の並びとして、
//! 中身を字句解析し直して別のパーサーで解析し、`FunctionBody` ノードとして木に埋め込む。
//! それ以外の言語の本体は、元のトークンのまま残す。

use super::Parser;
use crate::lexer::{Token, TokenKind, tokenize_with_offset};
use crate::syntax::{Element, Node, NodeKind};

/// 関数のオプションの始まり
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
    "window",
    "transform",
];

/// オプションの値のうちキーワードとして扱う語
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

/// 2 語以上の型名の最初の語と、その次に続く語
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
        // 本体の前に LANGUAGE が来るとは限らないので、先に探しておく
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
                // C 言語の関数の `AS 'obj_file', 'link_symbol'`
                if self.eat(TokenKind::Comma) && !self.at_statement_end() {
                    self.bump();
                }
                self.finish_node();
            } else if self.at_kw("begin") && self.nth_kw(1, "atomic") {
                self.atomic_body();
            } else if self.at_any_kw(OPTION_STARTS) {
                self.function_option();
            } else {
                self.error_token();
            }
        }
        self.finish_node();
    }

    /// 文の終わりまでの `LANGUAGE name` を探す
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

    /// `(name type, ...)`。`RETURNS TABLE (...)` の列にも使う。
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

    /// 引数の先頭の名前が、型名ではなく引数名か。次に型名が続くなら引数名とみなす。
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

    /// `IMMUTABLE` / `SECURITY DEFINER` / `SET search_path = ...` などのオプション。次のオプションの手前まで。
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

    /// `DO [LANGUAGE name] body`（本体の後ろに LANGUAGE を書いてもよい）
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

    /// 関数本体の文字列。閉じたドル引用符で、言語が PL/pgSQL か SQL なら中身を解析して埋め込む。
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

/// `$tag$ ... $tag$` を、区切りと、別のパーサーで解析した中身に分ける
fn parse_dollar_body<'a>(token: Token<'a>, language: BodyLanguage) -> Node<'a> {
    let text = token.text;
    let delimiter_len = text[1..].find('$').expect("ドル引用符の区切り") + 2;
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
