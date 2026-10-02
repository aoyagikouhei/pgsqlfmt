//! Hand-written recursive-descent parser.
//!
//! Builds a lossless syntax tree ([`crate::syntax::Node`]) from the token stream.
//! It never fails on invalid input or unsupported syntax: whatever cannot be parsed stays in
//! the tree as `Error` / `RawStatement` nodes holding the original tokens verbatim.
//!
//! Whitespace and comments (trivia) go into whichever node is open at the moment the next
//! significant token is consumed. Opening a node first flushes trivia into the parent, so a
//! node always starts with a significant token.

mod ddl;
mod dml;
mod expr;
mod function;
mod keywords;
mod plpgsql;
mod select;
#[cfg(test)]
mod tests;
mod utility;

use std::cell::Cell;

use crate::lexer::{Token, TokenKind, tokenize};
use crate::syntax::{Element, Node, NodeKind};

pub fn parse(src: &str) -> Node<'_> {
    let mut p = Parser::new(tokenize(src));
    p.statements();
    p.finish()
}

/// The start position of a node that `start_node_at` wraps around later
#[derive(Clone, Copy)]
struct Checkpoint {
    depth: usize,
    index: usize,
}

struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    /// Position of the next token to consume into the tree (may point at trivia)
    pos: usize,
    /// The open nodes: each one's kind and the children consumed so far
    stack: Vec<(NodeKind, Vec<Element<'a>>)>,
    /// For infinite-loop detection: the number of lookaheads since a token was last consumed
    steps: Cell<u32>,
    /// Extra keywords treated as the end of a clause (e.g. `loop` in `FOR r IN SELECT ... LOOP`)
    stops: Vec<&'static str>,
}

impl<'a> Parser<'a> {
    fn new(tokens: Vec<Token<'a>>) -> Self {
        Parser {
            tokens,
            pos: 0,
            stack: vec![(NodeKind::Root, Vec::new())],
            steps: Cell::new(0),
            stops: Vec::new(),
        }
    }

    /// Runs `f` with `stops` added to the keywords that end a clause
    fn with_stops<R>(&mut self, stops: &[&'static str], f: impl FnOnce(&mut Self) -> R) -> R {
        let saved = self.stops.len();
        self.stops.extend_from_slice(stops);
        let result = f(self);
        self.stops.truncate(saved);
        result
    }

    fn at_stop_keyword(&self) -> bool {
        self.stops.iter().any(|kw| self.at_kw(kw))
    }

    fn finish(mut self) -> Node<'a> {
        while self.pos < self.tokens.len() {
            self.push_token();
        }
        assert_eq!(self.stack.len(), 1, "there is an unclosed node");
        let (kind, children) = self.stack.pop().unwrap();
        Node { kind, children }
    }

    // ---- Lookahead ----

    /// The n-th token, skipping trivia
    fn nth(&self, n: usize) -> Option<Token<'a>> {
        // A token takes a few dozen lookaheads at most, so reaching 10 million without advancing
        // means an infinite loop
        let steps = self.steps.get() + 1;
        assert!(steps < 10_000_000, "the parser is not advancing");
        self.steps.set(steps);
        self.tokens[self.pos..]
            .iter()
            .filter(|t| !t.kind.is_trivia())
            .nth(n)
            .copied()
    }

    fn current(&self) -> Option<Token<'a>> {
        self.nth(0)
    }

    fn nth_is(&self, n: usize, kind: TokenKind) -> bool {
        self.nth(n).is_some_and(|t| t.kind == kind)
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.nth_is(0, kind)
    }

    fn at_eof(&self) -> bool {
        self.current().is_none()
    }

    /// Whether the n-th token is the keyword `kw` (passed in lowercase)
    fn nth_kw(&self, n: usize, kw: &str) -> bool {
        self.nth(n)
            .is_some_and(|t| t.kind == TokenKind::Ident && t.text.eq_ignore_ascii_case(kw))
    }

    fn at_kw(&self, kw: &str) -> bool {
        self.nth_kw(0, kw)
    }

    fn at_any_kw(&self, kws: &[&str]) -> bool {
        kws.iter().any(|kw| self.at_kw(kw))
    }

    fn at_op(&self, op: &str) -> bool {
        self.current()
            .is_some_and(|t| t.kind == TokenKind::Operator && t.text == op)
    }

    fn at_statement_end(&self) -> bool {
        self.at_eof() || self.at(TokenKind::Semicolon)
    }

    // ---- Building the tree ----

    fn push_token(&mut self) {
        let token = self.tokens[self.pos];
        self.push_token_as(token);
    }

    /// Consumes the token at the current position into the current node as `token`, and
    /// advances
    fn push_token_as(&mut self, token: Token<'a>) {
        self.pos += 1;
        self.steps.set(0);
        self.stack.last_mut().unwrap().1.push(Element::Token(token));
    }

    fn eat_trivia(&mut self) {
        while self.pos < self.tokens.len() && self.tokens[self.pos].kind.is_trivia() {
            self.push_token();
        }
    }

    /// Consumes the next significant token, with the trivia before it, into the current node
    fn bump(&mut self) {
        self.eat_trivia();
        assert!(self.pos < self.tokens.len(), "bump at end of input");
        self.push_token();
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        let found = self.at(kind);
        if found {
            self.bump();
        }
        found
    }

    /// Same as `bump`, but consumes an identifier as a keyword (which formatting uppercases)
    fn bump_kw(&mut self) {
        self.eat_trivia();
        assert!(self.pos < self.tokens.len(), "bump at end of input");
        let mut token = self.tokens[self.pos];
        if token.kind == TokenKind::Ident {
            token.kind = TokenKind::Keyword;
        }
        self.push_token_as(token);
    }

    /// Consumes the keyword `kw` as a keyword, if present
    fn eat_kw(&mut self, kw: &str) -> bool {
        let found = self.at_kw(kw);
        if found {
            self.bump_kw();
        }
        found
    }

    /// Same as `eat_kw`, but consumes the word as part of a name (e.g. `precision` in a type
    /// name)
    fn eat_word(&mut self, word: &str) -> bool {
        let found = self.at_kw(word);
        if found {
            self.bump();
        }
        found
    }

    fn start_node(&mut self, kind: NodeKind) {
        self.eat_trivia();
        self.stack.push((kind, Vec::new()));
    }

    fn finish_node(&mut self) {
        let (kind, children) = self.stack.pop().unwrap();
        self.stack
            .last_mut()
            .expect("attempted to close Root")
            .1
            .push(Element::Node(Node { kind, children }));
    }

    fn checkpoint(&mut self) -> Checkpoint {
        self.eat_trivia();
        Checkpoint {
            depth: self.stack.len(),
            index: self.stack.last().unwrap().1.len(),
        }
    }

    /// Reopens the children consumed since the checkpoint as the children of a new node
    fn start_node_at(&mut self, cp: Checkpoint, kind: NodeKind) {
        assert_eq!(
            cp.depth,
            self.stack.len(),
            "depth differs from the checkpoint"
        );
        let children = self.stack.last_mut().unwrap().1.split_off(cp.index);
        self.stack.push((kind, children));
    }

    /// Wraps the children consumed since the checkpoint into a single node
    fn wrap(&mut self, cp: Checkpoint, kind: NodeKind) {
        self.start_node_at(cp, kind);
        self.finish_node();
    }

    // ---- Error recovery ----

    /// Consumes a token, or a bracketed group with its contents. Stops at the end of the
    /// statement if the closing bracket is missing.
    fn bump_balanced(&mut self) {
        let mut depth = 0usize;
        loop {
            match self.current().map(|t| t.kind) {
                Some(TokenKind::LParen | TokenKind::LBracket) => depth += 1,
                Some(TokenKind::RParen | TokenKind::RBracket) => depth = depth.saturating_sub(1),
                _ => {}
            }
            self.bump();
            if depth == 0 || self.at_statement_end() {
                break;
            }
        }
    }

    /// Wraps everything up to a closing bracket, the end of the statement, or `stop` in an
    /// `Error` node
    fn error_until(&mut self, stop: impl Fn(&Self) -> bool) {
        let at_stop = |p: &Self| {
            p.at_statement_end() || p.at(TokenKind::RParen) || p.at(TokenKind::RBracket) || stop(p)
        };
        if at_stop(self) {
            return;
        }
        self.start_node(NodeKind::Error);
        while !at_stop(self) {
            self.bump_balanced();
        }
        self.finish_node();
    }

    /// Wraps the next single token (or a bracketed group with its contents) in an `Error` node
    fn error_token(&mut self) {
        self.start_node(NodeKind::Error);
        self.bump_balanced();
        self.finish_node();
    }

    /// Consumes `kind` if present; otherwise turns everything up to a closing bracket or the end
    /// of the statement into an `Error` node first, then looks for it
    fn expect_closing(&mut self, kind: TokenKind) {
        if !self.at(kind) {
            self.error_until(|p| p.at(kind));
        }
        self.eat(kind);
    }

    /// A comma-separated list. Where `item` cannot be parsed, everything up to the next comma or
    /// the end becomes an `Error` node.
    fn comma_list(&mut self, is_end: fn(&Self) -> bool, item: fn(&mut Self) -> bool) {
        let at_end = |p: &Self| {
            p.at_statement_end()
                || p.at(TokenKind::RParen)
                || p.at(TokenKind::RBracket)
                || is_end(p)
        };
        loop {
            if at_end(self) {
                break;
            }
            if !item(self) || !(self.at(TokenKind::Comma) || at_end(self)) {
                self.error_until(|p| p.at(TokenKind::Comma) || is_end(p));
            }
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
    }

    // ---- Statements ----

    /// Statements separated by `;` (the whole input, or the body of a `LANGUAGE sql` function)
    fn statements(&mut self) {
        while let Some(token) = self.current() {
            if token.kind == TokenKind::Semicolon {
                self.bump();
                continue;
            }
            // COPY data does not end with a semicolon, so it forms a statement on its own
            if token.kind == TokenKind::CopyData {
                self.start_node(NodeKind::RawStatement);
                self.bump();
                self.finish_node();
                continue;
            }
            self.statement();
            if !self.at_statement_end() {
                self.start_node(NodeKind::Error);
                while !self.at_statement_end() {
                    self.bump();
                }
                self.finish_node();
            }
        }
    }

    fn statement(&mut self) {
        if self.at_create_function() {
            self.create_function_stmt();
        } else if self.at_kw("do") {
            self.do_stmt();
        } else if self.at_kw("call") {
            self.call_stmt();
        } else if self.at_create_table() {
            self.create_table_stmt();
        } else if self.at_create_index() {
            self.create_index_stmt();
        } else if self.at_create_view() {
            self.create_view_stmt();
        } else if self.at_create_trigger() {
            self.create_trigger_stmt();
        } else if self.at_create_sequence() {
            self.create_sequence_stmt();
        } else if self.at_words(&[&["create"], &["type"]]) {
            self.create_type_stmt();
        } else if self.at_words(&[&["create"], &["schema"]]) {
            self.create_schema_stmt();
        } else if self.at_words(&[&["create"], &["extension"]]) {
            self.create_extension_stmt();
        } else if self.at_kw("grant") || self.at_kw("revoke") {
            self.grant_stmt();
        } else if self.at_kw("truncate") {
            self.truncate_stmt();
        } else if self.at_kw("comment") && self.nth_kw(1, "on") {
            self.comment_stmt();
        } else if self.at_kw("alter") && self.nth_kw(1, "table") {
            self.alter_table_stmt();
        } else if self.at_kw("copy") {
            self.copy_stmt();
        } else if self.at_any_kw(&["set", "reset", "show"]) {
            self.set_stmt();
        } else if self.at_kw("explain") {
            self.explain_stmt();
        } else if self.at_transaction_stmt() {
            self.transaction_stmt();
        } else if self.at_kw("alter") {
            self.alter_stmt();
        } else if self.at_kw("drop") {
            self.drop_stmt();
        } else if !self.statement_body() {
            self.raw_statement();
        }
    }

    pub(super) fn raw_statement(&mut self) {
        self.start_node(NodeKind::RawStatement);
        while !self.at_statement_end() {
            self.bump();
        }
        self.finish_node();
    }
}
