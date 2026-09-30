//! 手書きの再帰下降パーサー。
//!
//! トークン列からロスレスな構文木（[`crate::syntax::Node`]）を作る。
//! 不正な入力や未対応の構文でも失敗せず、解釈できない部分は
//! `Error` / `RawStatement` ノードとして元のトークンのまま木に残す。
//!
//! 空白・コメントは、次の意味のあるトークンを取り込む時点で開いているノードに入る。
//! ノードを開くときは先に空白・コメントを親に流すので、ノードは必ず意味のあるトークンから始まる。

mod dml;
mod expr;
mod keywords;
mod select;
#[cfg(test)]
mod tests;

use std::cell::Cell;

use crate::lexer::{Token, TokenKind, tokenize};
use crate::syntax::{Element, Node, NodeKind};

pub fn parse(src: &str) -> Node<'_> {
    let mut p = Parser::new(src);
    while let Some(token) = p.current() {
        if token.kind == TokenKind::Semicolon {
            p.bump();
            continue;
        }
        p.statement();
        if !p.at_statement_end() {
            p.start_node(NodeKind::Error);
            while !p.at_statement_end() {
                p.bump();
            }
            p.finish_node();
        }
    }
    p.finish()
}

/// `start_node_at` で、あとから包むノードの開始位置
#[derive(Clone, Copy)]
struct Checkpoint {
    depth: usize,
    index: usize,
}

struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    /// 次に木へ取り込むトークンの位置（空白・コメントを指すこともある）
    pos: usize,
    /// 開いているノードの種類と、そこまでに取り込んだ子
    stack: Vec<(NodeKind, Vec<Element<'a>>)>,
    /// 無限ループの検出用
    steps: Cell<u32>,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Parser {
            tokens: tokenize(src),
            pos: 0,
            stack: vec![(NodeKind::Root, Vec::new())],
            steps: Cell::new(0),
        }
    }

    fn finish(mut self) -> Node<'a> {
        while self.pos < self.tokens.len() {
            self.push_token();
        }
        assert_eq!(self.stack.len(), 1, "閉じていないノードがある");
        let (kind, children) = self.stack.pop().unwrap();
        Node { kind, children }
    }

    // ---- 先読み ----

    /// 空白・コメントを飛ばした n 番目のトークン
    fn nth(&self, n: usize) -> Option<Token<'a>> {
        let steps = self.steps.get() + 1;
        assert!(steps < 10_000_000, "パーサーが先に進んでいない");
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

    /// n 番目のトークンがキーワード `kw`（小文字で渡す）か
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

    // ---- 木の組み立て ----

    fn push_token(&mut self) {
        let token = self.tokens[self.pos];
        self.pos += 1;
        self.stack.last_mut().unwrap().1.push(Element::Token(token));
    }

    fn eat_trivia(&mut self) {
        while self.pos < self.tokens.len() && self.tokens[self.pos].kind.is_trivia() {
            self.push_token();
        }
    }

    /// 次の意味のあるトークンを、手前の空白・コメントとともに現在のノードへ取り込む
    fn bump(&mut self) {
        self.eat_trivia();
        assert!(self.pos < self.tokens.len(), "入力の終わりで bump した");
        self.push_token();
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        let found = self.at(kind);
        if found {
            self.bump();
        }
        found
    }

    /// `bump` と同じだが、識別子をキーワードとして取り込む（整形で大文字にする対象になる）
    fn bump_kw(&mut self) {
        self.eat_trivia();
        assert!(self.pos < self.tokens.len(), "入力の終わりで bump した");
        let mut token = self.tokens[self.pos];
        if token.kind == TokenKind::Ident {
            token.kind = TokenKind::Keyword;
        }
        self.pos += 1;
        self.stack.last_mut().unwrap().1.push(Element::Token(token));
    }

    /// キーワード `kw` があればキーワードとして取り込む
    fn eat_kw(&mut self, kw: &str) -> bool {
        let found = self.at_kw(kw);
        if found {
            self.bump_kw();
        }
        found
    }

    /// `eat_kw` と同じだが、名前の一部（型名の `precision` など）として取り込む
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
            .expect("Root を閉じようとした")
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

    /// checkpoint 以降に取り込んだ子を、新しいノードの子として開き直す
    fn start_node_at(&mut self, cp: Checkpoint, kind: NodeKind) {
        assert_eq!(cp.depth, self.stack.len(), "checkpoint と深さが違う");
        let children = self.stack.last_mut().unwrap().1.split_off(cp.index);
        self.stack.push((kind, children));
    }

    /// checkpoint 以降に取り込んだ子を 1 つのノードにまとめる
    fn wrap(&mut self, cp: Checkpoint, kind: NodeKind) {
        self.start_node_at(cp, kind);
        self.finish_node();
    }

    // ---- エラーからの回復 ----

    /// 括弧の中身ごとトークンを取り込む。閉じ括弧がなければ文の終わりで止まる。
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

    /// 閉じ括弧・文の終わり・`stop` のどれかに来るまでを `Error` ノードにする
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

    /// 次のトークン 1 つ（括弧なら中身ごと）を `Error` ノードにする
    fn error_token(&mut self) {
        self.start_node(NodeKind::Error);
        self.bump_balanced();
        self.finish_node();
    }

    /// `kind` があれば取り込み、なければ閉じ括弧・文の終わりまでを `Error` にしてから探す
    fn expect_closing(&mut self, kind: TokenKind) {
        if !self.at(kind) {
            self.error_until(|p| p.at(kind));
        }
        self.eat(kind);
    }

    /// カンマ区切りの並び。`item` が読めなかった部分は次のカンマか終わりまでを `Error` にする。
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

    // ---- 文 ----

    fn statement(&mut self) {
        if !self.statement_body() {
            self.raw_statement();
        }
    }

    fn raw_statement(&mut self) {
        self.start_node(NodeKind::RawStatement);
        while !self.at_statement_end() {
            self.bump();
        }
        self.finish_node();
    }
}
