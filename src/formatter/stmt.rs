//! Layout of statements and clauses. Each clause starts a line, with its list or condition
//! indented beneath it.

use super::{Formatter, as_node, as_token, children, is_node, logical_op};
use crate::lexer::{Token, TokenKind};
use crate::syntax::{Element, Node, NodeKind};

impl<'a> Formatter<'a> {
    pub(super) fn select_stmt(&mut self, stmt: &Node<'a>, base: usize) {
        for (i, element) in children(stmt).into_iter().enumerate() {
            if i > 0 {
                self.w.newline(base);
            }
            match element {
                Element::Node(n) => self.query_part(n, base),
                Element::Token(t) => self.w.token(t),
            }
        }
    }

    /// A part of a query (WITH, the body, a set operation, ORDER BY, LIMIT, ...)
    pub(super) fn query_part(&mut self, node: &Node<'a>, base: usize) {
        match node.kind {
            NodeKind::WithClause => self.with_clause(node, base),
            NodeKind::SimpleSelect => {
                for (i, element) in children(node).into_iter().enumerate() {
                    if i > 0 {
                        self.w.newline(base);
                    }
                    match element {
                        Element::Node(n) => self.clause(n, base),
                        Element::Token(t) => self.w.token(t),
                    }
                }
            }
            NodeKind::SetOperation => self.set_operation(node, base),
            _ => self.clause(node, base),
        }
    }

    /// A clause placed at the start of a line
    fn clause(&mut self, node: &Node<'a>, base: usize) {
        match node.kind {
            NodeKind::SelectClause
            | NodeKind::FromClause
            | NodeKind::GroupByClause
            | NodeKind::WindowClause
            | NodeKind::OrderByClause
            | NodeKind::ValuesClause
            | NodeKind::SetClause
            | NodeKind::UsingClause
            | NodeKind::ReturningClause => self.list_clause(node, base),
            NodeKind::WhereClause | NodeKind::HavingClause => self.condition_clause(node, base),
            _ => self.node(node),
        }
    }

    /// `left UNION ALL right`. The operator goes on its own line.
    fn set_operation(&mut self, node: &Node<'a>, base: usize) {
        let mut seen_left = false;
        let mut on_operator_line = false;
        for element in children(node) {
            match element {
                Element::Node(n) if !seen_left => {
                    seen_left = true;
                    self.query_part(n, base);
                }
                Element::Token(t) => {
                    if !on_operator_line {
                        self.w.newline(base);
                        on_operator_line = true;
                    }
                    self.w.token(t);
                }
                Element::Node(n) => {
                    self.w.newline(base);
                    on_operator_line = false;
                    self.query_part(n, base);
                }
            }
        }
    }

    /// `WITH a AS (...)`: the second and later CTEs start with a leading comma
    fn with_clause(&mut self, node: &Node<'a>, base: usize) {
        let elements = children(node);
        for (i, element) in elements.iter().enumerate() {
            if let Some(comma) = as_token(element).filter(|t| t.kind == TokenKind::Comma) {
                // The second and later CTEs start at the same column as the CTE name (the leading
                // comma is not pulled to the left of it)
                self.list_separator(comma, first_token(&elements[i + 1..]), base, 0);
                continue;
            }
            self.element(element);
        }
    }

    /// A clause of the form `KEYWORD item` / `KEYWORD\n    item\n  , item`.
    /// A single item stays on the keyword's line; two or more go one per line, indented, with
    /// leading commas.
    pub(super) fn list_clause(&mut self, node: &Node<'a>, base: usize) {
        let elements = children(node);
        // The leading keywords (including the parentheses of SELECT DISTINCT ON (...) and the
        // PL/pgSQL `SELECT INTO target`)
        let header_len = elements
            .iter()
            .position(|e| {
                let header_token = as_token(e).is_some_and(|t| t.kind != TokenKind::Comma);
                let select_header = node.kind == NodeKind::SelectClause
                    && (is_node(e, NodeKind::ExprList) || is_node(e, NodeKind::IntoClause));
                !(header_token || select_header)
            })
            .unwrap_or(elements.len());
        let (header, rest) = elements.split_at(header_len);
        for element in header {
            self.element(element);
        }
        if rest.is_empty() {
            return;
        }

        let mut items: Vec<Vec<&Element<'a>>> = vec![Vec::new()];
        let mut commas = Vec::new();
        for element in rest {
            match as_token(element) {
                Some(t) if t.kind == TokenKind::Comma => {
                    commas.push(t);
                    items.push(Vec::new());
                }
                _ => items.last_mut().unwrap().push(element),
            }
        }
        if commas.is_empty() {
            for element in &items[0] {
                self.element(element);
            }
            return;
        }
        let item_indent = base + self.indent_width;
        for (i, item) in items.iter().enumerate() {
            if i == 0 {
                self.w.newline(item_indent);
            } else {
                self.list_separator(commas[i - 1], first_token(item), item_indent, 2);
            }
            for element in item {
                self.element(element);
            }
        }
    }

    /// `WHERE cond` / `HAVING cond` / `ON cond`.
    /// A multi-line expression inside the condition (a subquery, say) is based at the same
    /// position as the AND / OR lines, one level deeper.
    pub(super) fn condition_clause(&mut self, node: &Node<'a>, base: usize) {
        for element in children(node) {
            match element {
                Element::Node(n) => {
                    self.w.set_indent(base + self.indent_width);
                    self.condition(n, base);
                }
                Element::Token(t) => self.w.token(t),
            }
        }
    }

    /// Lays out the top-level AND / OR chain, breaking before each operator, one level deeper
    fn condition(&mut self, node: &Node<'a>, base: usize) {
        let Some(op) = logical_op(node).filter(|_| split_binary(node).is_some()) else {
            self.node(node);
            return;
        };
        let mut operands = Vec::new();
        let mut operators = Vec::new();
        flatten_chain(node, &op, &mut operands, &mut operators);
        for (i, operand) in operands.into_iter().enumerate() {
            if i > 0 {
                self.w.newline(base + self.indent_width);
                self.w.token(operators[i - 1]);
            }
            self.node(operand);
        }
    }

    pub(super) fn insert_stmt(&mut self, stmt: &Node<'a>, base: usize) {
        for element in children(stmt) {
            match as_node(element).map(|n| (n, n.kind)) {
                Some((n, NodeKind::WithClause)) => {
                    self.with_clause(n, base);
                    self.w.newline(base);
                }
                Some((n, NodeKind::SelectStmt)) => {
                    self.w.newline(base);
                    self.select_stmt(n, base);
                }
                Some((n, NodeKind::OnConflictClause)) => {
                    self.w.newline(base);
                    self.on_conflict(n, base);
                }
                Some((n, NodeKind::ReturningClause)) => {
                    self.w.newline(base);
                    self.list_clause(n, base);
                }
                Some((n, NodeKind::IntoClause)) => {
                    self.w.newline(base);
                    self.node(n);
                }
                _ => self.element(element),
            }
        }
    }

    /// SET / WHERE of `ON CONFLICT (...) DO UPDATE` go one level deeper
    fn on_conflict(&mut self, node: &Node<'a>, base: usize) {
        let mut after_do = false;
        for element in children(node) {
            match element {
                Element::Token(t)
                    if t.kind == TokenKind::Keyword && t.text.eq_ignore_ascii_case("do") =>
                {
                    after_do = true;
                    self.w.token(t);
                }
                Element::Node(n) if after_do && n.kind == NodeKind::SetClause => {
                    self.w.newline(base + self.indent_width);
                    self.list_clause(n, base + self.indent_width);
                }
                Element::Node(n) if after_do && n.kind == NodeKind::WhereClause => {
                    self.w.newline(base + self.indent_width);
                    self.condition_clause(n, base + self.indent_width);
                }
                _ => self.element(element),
            }
        }
    }

    pub(super) fn update_or_delete(&mut self, stmt: &Node<'a>, base: usize) {
        for element in children(stmt) {
            match as_node(element) {
                Some(n) if n.kind == NodeKind::WithClause => {
                    self.with_clause(n, base);
                    self.w.newline(base);
                }
                Some(n)
                    if matches!(
                        n.kind,
                        NodeKind::SetClause
                            | NodeKind::FromClause
                            | NodeKind::UsingClause
                            | NodeKind::WhereClause
                            | NodeKind::ReturningClause
                            | NodeKind::IntoClause
                    ) =>
                {
                    self.w.newline(base);
                    self.clause(n, base);
                }
                _ => self.element(element),
            }
        }
    }
}

/// The first significant token of the list
fn first_token<'a>(elements: &[&Element<'a>]) -> Option<Token<'a>> {
    elements.first().and_then(|e| match e {
        Element::Token(t) => Some(*t),
        Element::Node(n) => super::writer::token_range(n).map(|(first, _)| first),
    })
}

/// The three parts if the node has the form `left op right`
pub(super) fn split_binary<'n, 'a>(
    node: &'n Node<'a>,
) -> Option<(&'n Node<'a>, &'n Token<'a>, &'n Node<'a>)> {
    match children(node).as_slice() {
        [left, op, right] => Some((as_node(left)?, as_token(op)?, as_node(right)?)),
        _ => None,
    }
}

/// Flattens `a AND b AND c` (a left-associative tree) into lists of operands and operators.
/// `node` must be `split_binary`-able. A left side with the same operator but a broken shape is
/// treated as a single operand.
fn flatten_chain<'n, 'a>(
    node: &'n Node<'a>,
    op: &str,
    operands: &mut Vec<&'n Node<'a>>,
    operators: &mut Vec<&'n Token<'a>>,
) {
    let (left, operator, right) = split_binary(node).expect("split_binary succeeds");
    if logical_op(left).as_deref() == Some(op) && split_binary(left).is_some() {
        flatten_chain(left, op, operands, operators);
    } else {
        operands.push(left);
    }
    operators.push(operator);
    operands.push(right);
}
