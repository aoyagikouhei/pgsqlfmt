//! 文と句のレイアウト。句を行頭に置き、並びや条件を字下げして並べる。

use super::{Formatter, INDENT, as_node, as_token, children, is_node, logical_op};
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

    /// 問い合わせを構成する句（WITH・本体・集合演算・ORDER BY・LIMIT など）
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

    /// 行頭に置く句
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

    /// `left UNION ALL right`。演算子を独立した行に置く。
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

    /// `WITH a AS (...)` の 2 つ目以降の CTE は、行頭のカンマから始める
    fn with_clause(&mut self, node: &Node<'a>, base: usize) {
        let elements = children(node);
        for (i, element) in elements.iter().enumerate() {
            if let Some(comma) = as_token(element).filter(|t| t.kind == TokenKind::Comma) {
                self.w.flush_trailing_comments(comma);
                if let Some(first) = first_token(&elements[i + 1..]) {
                    self.w.move_leading_comments(&first, comma);
                }
                self.w.newline(base);
            }
            self.element(element);
        }
    }

    /// `KEYWORD item` / `KEYWORD\n    item\n  , item` の形の句。
    /// 項目が 1 つなら句と同じ行に、2 つ以上なら 1 行ずつ字下げして行頭カンマで並べる。
    pub(super) fn list_clause(&mut self, node: &Node<'a>, base: usize) {
        let elements = children(node);
        // 先頭のキーワード（SELECT DISTINCT ON (...) の括弧を含む）
        let header_len = elements
            .iter()
            .position(|e| {
                let header_token = as_token(e).is_some_and(|t| t.kind != TokenKind::Comma);
                let distinct_on =
                    node.kind == NodeKind::SelectClause && is_node(e, NodeKind::ExprList);
                !(header_token || distinct_on)
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
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.w.flush_trailing_comments(commas[i - 1]);
            }
            self.w.newline(base + INDENT);
            if i > 0 {
                if let Some(first) = first_token(item) {
                    self.w.move_leading_comments(&first, commas[i - 1]);
                }
                self.w.token_outdented(commas[i - 1], 2);
            }
            for element in item {
                self.element(element);
            }
        }
    }

    /// `WHERE cond` / `HAVING cond` / `ON cond`。
    /// 条件の中の複数行の式（副問い合わせなど）は、AND / OR の行と同じ 1 段深い位置を基準にする。
    pub(super) fn condition_clause(&mut self, node: &Node<'a>, base: usize) {
        for element in children(node) {
            match element {
                Element::Node(n) => {
                    self.w.set_indent(base + INDENT);
                    self.condition(n, base);
                }
                Element::Token(t) => self.w.token(t),
            }
        }
    }

    /// 最上位の AND / OR の並びを、演算子ごとに改行して 1 段深く並べる
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
                self.w.newline(base + INDENT);
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

    /// `ON CONFLICT (...) DO UPDATE` の SET / WHERE は 1 段深くする
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
                    self.w.newline(base + INDENT);
                    self.list_clause(n, base + INDENT);
                }
                Element::Node(n) if after_do && n.kind == NodeKind::WhereClause => {
                    self.w.newline(base + INDENT);
                    self.condition_clause(n, base + INDENT);
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

/// 並びの最初の意味のあるトークン
fn first_token<'a>(elements: &[&Element<'a>]) -> Option<Token<'a>> {
    elements.first().and_then(|e| match e {
        Element::Token(t) => Some(*t),
        Element::Node(n) => super::writer::token_range(n).map(|(first, _)| first),
    })
}

/// `left op right` の形なら、その 3 つ
fn split_binary<'n, 'a>(node: &'n Node<'a>) -> Option<(&'n Node<'a>, &'n Token<'a>, &'n Node<'a>)> {
    match children(node).as_slice() {
        [left, op, right] => Some((as_node(left)?, as_token(op)?, as_node(right)?)),
        _ => None,
    }
}

/// `a AND b AND c`（左結合の木）を、被演算子と演算子の並びにする。
/// `node` は `split_binary` できること。左辺が同じ演算子でも形が崩れていれば、1 つの被演算子として扱う。
fn flatten_chain<'n, 'a>(
    node: &'n Node<'a>,
    op: &str,
    operands: &mut Vec<&'n Node<'a>>,
    operators: &mut Vec<&'n Token<'a>>,
) {
    let (left, operator, right) = split_binary(node).expect("split_binary できる");
    if logical_op(left).as_deref() == Some(op) && split_binary(left).is_some() {
        flatten_chain(left, op, operands, operators);
    } else {
        operands.push(left);
    }
    operators.push(operator);
    operands.push(right);
}
