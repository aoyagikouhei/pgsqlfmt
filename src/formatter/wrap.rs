//! 行幅による折り返し。
//!
//! 折り返せる箇所（グループ）ごとに、いまの位置から 1 行で書いたときに行幅に収まるかを測り、
//! 収まらないときだけ折り返す。外側のグループから順に決めるので、外側を折り返したあとで
//! 内側のグループが収まれば、内側は 1 行のまま残る。
//!
//! - 括弧の中の並び（引数・`IN (...)`・列名など）: 項目を 1 行ずつ並べて行頭カンマにし、閉じ括弧を独立した行に置く
//! - 二項演算の連なり（`a || b || c`）: 演算子の前で改行して 1 段深くする
//! - 窓の指定 `OVER (...)`: PARTITION BY / ORDER BY / フレームを 1 行ずつにする
//! - RAISE / EXECUTE: `USING` / `INTO` の前で改行する

use super::stmt::split_binary;
use super::{Formatter, INDENT, as_node, as_token, children};
use crate::lexer::{Token, TokenKind};
use crate::syntax::{Element, Node, NodeKind};

impl<'a> Formatter<'a> {
    /// `render` で書いたものが、いまの位置から 1 行で行幅に収まるか。
    /// 外側のグループを測っている途中なら、内側のグループはすべて 1 行とみなす。
    fn fits(&mut self, render: impl FnOnce(&mut Self)) -> bool {
        if self.w.measuring() {
            return true;
        }
        let saved = self.w.begin_measure(self.max_width);
        render(self);
        self.w.end_measure(saved)
    }

    /// `( item, item, ... )`
    pub(super) fn paren_list(&mut self, node: &Node<'a>) {
        if self.fits(|f| f.inline(node)) {
            self.inline(node);
            return;
        }
        let base = self.w.indent();
        let elements = children(node);
        let open = elements
            .iter()
            .position(|e| as_token(e).is_some_and(|t| t.kind == TokenKind::LParen));
        let close = elements
            .iter()
            .rposition(|e| as_token(e).is_some_and(|t| t.kind == TokenKind::RParen));
        let (Some(open), Some(close)) = (open, close) else {
            self.inline(node);
            return;
        };

        for element in &elements[..=open] {
            self.element(element);
        }
        let mut first = true;
        let mut pending_comma: Option<&Token<'a>> = None;
        for element in &elements[open + 1..close] {
            if let Some(comma) = as_token(element).filter(|t| t.kind == TokenKind::Comma) {
                if let Some(previous) = pending_comma.replace(comma) {
                    // 空の項目（`f(a, , b)`）
                    self.leading_comma(previous, None, base);
                }
                continue;
            }
            if first || pending_comma.is_some() {
                match pending_comma.take() {
                    Some(comma) => self.leading_comma(comma, Some(element), base),
                    None => self.w.newline(base + INDENT),
                }
                first = false;
            }
            self.element(element);
        }
        if let Some(comma) = pending_comma {
            self.leading_comma(comma, None, base);
        }
        self.w.newline(base);
        for element in &elements[close..] {
            self.element(element);
        }
    }

    /// 改行して、項目の前の行頭カンマを書く
    fn leading_comma(&mut self, comma: &Token<'a>, item: Option<&Element<'a>>, base: usize) {
        self.w.flush_trailing_comments(comma);
        self.w.newline(base + INDENT);
        if let Some(first) = item.and_then(first_token) {
            self.w.move_leading_comments(&first, comma);
        }
        self.w.token_outdented(comma, 2);
    }

    /// `a op b op c`。同じ演算子の連なりを、演算子の前で改行して並べる。
    pub(super) fn binary_expr(&mut self, node: &Node<'a>) {
        let op = binary_op(node);
        if op.is_none() || self.fits(|f| f.inline(node)) {
            self.inline(node);
            return;
        }
        let op = op.unwrap();
        let mut operands = Vec::new();
        let mut operators = Vec::new();
        flatten_same_op(node, &op, &mut operands, &mut operators);
        let base = self.w.indent();
        // 被演算子の中の折り返しは演算子の行より深くして、どの演算子の被演算子かを見分けやすくする
        self.w.set_indent(base + INDENT);
        for (i, operand) in operands.into_iter().enumerate() {
            if i > 0 {
                self.w.newline(base + INDENT);
                self.w.token(operators[i - 1]);
            }
            self.node(operand);
        }
    }

    /// `(name PARTITION BY ... ORDER BY ... ROWS ...)`
    pub(super) fn window_spec(&mut self, node: &Node<'a>) {
        if self.fits(|f| f.inline(node)) {
            self.inline(node);
            return;
        }
        let base = self.w.indent();
        for element in children(node) {
            match element {
                Element::Token(t) if t.kind == TokenKind::LParen => self.w.token(t),
                Element::Token(t) if t.kind == TokenKind::RParen => {
                    self.w.newline(base);
                    self.w.token(t);
                }
                _ => {
                    self.w.newline(base + INDENT);
                    self.element(element);
                }
            }
        }
    }

    /// RAISE / EXECUTE。収まらなければ `USING` / `INTO` の前で改行する。
    pub(super) fn statement_with_options(&mut self, node: &Node<'a>) {
        if self.fits(|f| f.inline(node)) {
            self.inline(node);
            return;
        }
        let base = self.w.indent();
        for element in children(node) {
            match as_node(element) {
                Some(n) if matches!(n.kind, NodeKind::PlUsing | NodeKind::IntoClause) => {
                    self.w.newline(base + INDENT);
                    self.node(n);
                }
                _ => self.element(element),
            }
        }
    }
}

fn first_token<'a>(element: &Element<'a>) -> Option<Token<'a>> {
    match element {
        Element::Token(t) => Some(*t),
        Element::Node(n) => super::writer::token_range(n).map(|(first, _)| first),
    }
}

/// `left op right` の形なら、演算子（小文字）
fn binary_op(node: &Node) -> Option<String> {
    split_binary(node).map(|(_, op, _)| op.text.to_ascii_lowercase())
}

/// 左結合の `a op b op c` を、被演算子と演算子の並びにする
fn flatten_same_op<'n, 'a>(
    node: &'n Node<'a>,
    op: &str,
    operands: &mut Vec<&'n Node<'a>>,
    operators: &mut Vec<&'n Token<'a>>,
) {
    let (left, operator, right) = split_binary(node).expect("split_binary できる");
    if left.kind == NodeKind::BinaryExpr && binary_op(left).as_deref() == Some(op) {
        flatten_same_op(left, op, operands, operators);
    } else {
        operands.push(left);
    }
    operators.push(operator);
    operands.push(right);
}
