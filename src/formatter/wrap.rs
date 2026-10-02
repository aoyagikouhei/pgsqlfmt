//! Wrapping by line width.
//!
//! For each wrappable spot (a group), measure whether it fits the line width when written on one
//! line from the current position, and wrap only when it does not. Outer groups are decided
//! first, so if an inner group fits once the outer one has been wrapped, the inner one stays on
//! one line.
//!
//! - Lists in parentheses (arguments, `IN (...)`, column names, ...): one item per line with
//!   leading commas, and the closing parenthesis on its own line
//! - Chains of binary operations (`a || b || c`): break before the operator, one level deeper
//! - Window specifications `OVER (...)`: PARTITION BY / ORDER BY / the frame each on its own line
//! - RAISE / EXECUTE: break before `USING` / `INTO`

use super::stmt::split_binary;
use super::{Formatter, as_node, as_token, children};
use crate::lexer::{Token, TokenKind};
use crate::syntax::{Element, Node, NodeKind};

impl<'a> Formatter<'a> {
    /// Whether what `render` writes fits the line width on one line from the current position.
    /// While an outer group is being measured, every inner group is assumed to fit on one line.
    fn fits(&mut self, render: impl FnOnce(&mut Self)) -> bool {
        if self.w.measuring() {
            return true;
        }
        let reserve = match self.reserve {
            Some((width, line)) if line == self.w.line_id() => width,
            _ => 0,
        };
        let saved = self.w.begin_measure(self.max_width.saturating_sub(reserve));
        render(self);
        self.w.end_measure(saved)
    }

    /// `( item, item, ... )`
    /// A single item is never wrapped (`lower(\n    name\n)` is no easier to read. If the inner
    /// expression is long, that expression wraps instead)
    pub(super) fn paren_list(&mut self, node: &Node<'a>) {
        let single_item = !children(node)
            .iter()
            .any(|e| as_token(e).is_some_and(|t| t.kind == TokenKind::Comma));
        if single_item || self.fits(|f| f.inline(node)) {
            self.inline(node);
        } else {
            self.paren_list_broken(node);
        }
    }

    /// Breaks after `(`, lists the items one per line with leading commas, and puts the closing
    /// parenthesis on its own line
    pub(super) fn paren_list_broken(&mut self, node: &Node<'a>) {
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
                    // An empty item (`f(a, , b)`)
                    self.leading_comma(previous, None, base);
                }
                continue;
            }
            if first || pending_comma.is_some() {
                match pending_comma.take() {
                    Some(comma) => self.leading_comma(comma, Some(element), base),
                    None => self.w.newline(base + self.indent_width),
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

    /// Writes the comma between items and moves to the next item's line
    fn leading_comma(&mut self, comma: &Token<'a>, item: Option<&Element<'a>>, base: usize) {
        let next = item.and_then(first_token);
        self.list_separator(comma, next, base + self.indent_width, 2);
    }

    /// `a op b op c`. Lays out a chain of the same operator, breaking before each operator.
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
        // Wrapping inside an operand goes deeper than the operator line, to make it easier to
        // tell which operator the operand belongs to
        self.w.set_indent(base + self.indent_width);
        for (i, operand) in operands.into_iter().enumerate() {
            if i > 0 {
                self.w.newline(base + self.indent_width);
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
                    self.w.newline(base + self.indent_width);
                    self.element(element);
                }
            }
        }
    }

    /// RAISE / EXECUTE. If it does not fit, breaks before `USING` / `INTO`.
    pub(super) fn statement_with_options(&mut self, node: &Node<'a>) {
        if self.fits(|f| f.inline(node)) {
            self.inline(node);
            return;
        }
        let base = self.w.indent();
        for element in children(node) {
            match as_node(element) {
                Some(n) if matches!(n.kind, NodeKind::PlUsing | NodeKind::IntoClause) => {
                    self.w.newline(base + self.indent_width);
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

/// The operator (lower-cased) if the node has the form `left op right`
fn binary_op(node: &Node) -> Option<String> {
    split_binary(node).map(|(_, op, _)| op.text.to_ascii_lowercase())
}

/// Flattens the left-associative `a op b op c` into lists of operands and operators
fn flatten_same_op<'n, 'a>(
    node: &'n Node<'a>,
    op: &str,
    operands: &mut Vec<&'n Node<'a>>,
    operators: &mut Vec<&'n Token<'a>>,
) {
    let (left, operator, right) = split_binary(node).expect("split_binary succeeds");
    if left.kind == NodeKind::BinaryExpr && binary_op(left).as_deref() == Some(op) {
        flatten_same_op(left, op, operands, operators);
    } else {
        operands.push(left);
    }
    operators.push(operator);
    operands.push(right);
}
