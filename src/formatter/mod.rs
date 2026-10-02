//! Formats a syntax tree into a string.
//!
//! Style:
//! - One clause per line. When a list has two or more items, each item goes on its own indented
//!   line with a leading comma
//! - Top-level AND / OR in WHERE / HAVING / ON break onto new lines one level deeper
//! - JOIN is one level deeper than the FROM line, and ON one level deeper still
//! - Subqueries and CASE span multiple lines. Every other expression stays on one line and is
//!   wrapped only when it does not fit the line width (`wrap.rs`)
//! - Keywords are upper-cased. Identifiers, function names and type names are kept as in the input
//!
//! `RawStatement` / `Error` are emitted verbatim.

mod ddl;
mod plpgsql;
mod stmt;
#[cfg(test)]
mod tests;
mod wrap;
mod writer;

use crate::lexer::{BOM, Token, TokenKind};
use crate::parser::parse;
use crate::syntax::{Element, Node, NodeKind};
use writer::{Writer, is_opaque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatOptions {
    /// Line width. An expression that exceeds it is wrapped (a full-width character counts as
    /// 2 columns)
    pub max_width: usize,
    /// Width of one indentation level. Use 2 or more, since a leading comma is placed 2 columns
    /// to the left of its item
    pub indent_width: usize,
    pub keyword_case: KeywordCase,
    pub comma_style: CommaStyle,
}

impl Default for FormatOptions {
    fn default() -> Self {
        FormatOptions {
            max_width: 80,
            indent_width: 4,
            keyword_case: KeywordCase::Upper,
            comma_style: CommaStyle::Leading,
        }
    }
}

/// Case of keywords
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordCase {
    Upper,
    Lower,
    /// As in the input
    Preserve,
}

/// Position of the comma when items are listed one per line
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommaStyle {
    /// `    a` / `  , b`
    Leading,
    /// `    a,` / `    b`
    Trailing,
}

pub fn format(src: &str) -> String {
    format_with_options(src, &FormatOptions::default())
}

pub fn format_with_options(src: &str, options: &FormatOptions) -> String {
    let root = parse(src);
    let mut f = Formatter {
        w: Writer::new(src, &root, options.keyword_case),
        max_width: options.max_width,
        reserve: None,
        indent_width: options.indent_width,
        comma_style: options.comma_style,
    };
    f.root(&root);
    let out = f.w.finish();
    // Keep the leading BOM as in the input (the lexer reads it as whitespace, so re-add it here)
    if src.starts_with(BOM) {
        return format!("{BOM}{out}");
    }
    out
}

/// Keywords to upper-case in clauses whose contents are not parsed in detail
const LOOSE_KEYWORDS: &[&str] = &[
    "absolute",
    "all",
    "and",
    "backward",
    "chain",
    "forward",
    "from",
    "in",
    "into",
    "last",
    "prior",
    "relative",
    "between",
    "current",
    "exclude",
    "first",
    "following",
    "groups",
    "key",
    "locked",
    "next",
    "no",
    "nowait",
    "of",
    "only",
    "others",
    "percent",
    "preceding",
    "range",
    "row",
    "rows",
    "share",
    "skip",
    "ties",
    "unbounded",
    "update",
    "with",
];

struct Formatter<'a> {
    w: Writer<'a>,
    max_width: usize,
    /// Width of the alias (`AS name`) that follows later on the same line, and that line.
    /// Subtracted from the line width when deciding whether to wrap
    reserve: Option<(usize, usize)>,
    indent_width: usize,
    comma_style: CommaStyle,
}

/// Children excluding whitespace and comments
fn children<'n, 'a>(node: &'n Node<'a>) -> Vec<&'n Element<'a>> {
    node.children
        .iter()
        .filter(|c| !matches!(c, Element::Token(t) if t.kind.is_trivia()))
        .collect()
}

fn as_node<'n, 'a>(element: &'n Element<'a>) -> Option<&'n Node<'a>> {
    match element {
        Element::Node(n) => Some(n),
        Element::Token(_) => None,
    }
}

fn as_token<'n, 'a>(element: &'n Element<'a>) -> Option<&'n Token<'a>> {
    match element {
        Element::Token(t) => Some(t),
        Element::Node(_) => None,
    }
}

fn is_node(element: &Element, kind: NodeKind) -> bool {
    as_node(element).is_some_and(|n| n.kind == kind)
}

fn is_token(element: &Element, kind: TokenKind) -> bool {
    as_token(element).is_some_and(|t| t.kind == kind)
}

/// The operator (lower-cased) if the node is an AND / OR BinaryExpr
fn logical_op(node: &Node) -> Option<String> {
    if node.kind != NodeKind::BinaryExpr {
        return None;
    }
    children(node).iter().find_map(|c| {
        as_token(c)
            .filter(|t| t.kind == TokenKind::Keyword)
            .map(|t| t.text.to_ascii_lowercase())
            .filter(|op| op == "and" || op == "or")
    })
}

impl<'a> Formatter<'a> {
    fn root(&mut self, root: &Node<'a>) {
        let mut first = true;
        for element in children(root) {
            match element {
                // COPY data continues verbatim right after the `;` (adding a newline would
                // change where the data starts)
                Element::Node(stmt)
                    if writer::token_range(stmt)
                        .is_some_and(|(t, _)| t.kind == TokenKind::CopyData) =>
                {
                    self.w.glue();
                    self.statement(stmt, 0);
                    self.w.keep_output();
                }
                Element::Node(stmt) => {
                    if !first {
                        self.w.newline(0);
                        if let Some((token, _)) = writer::token_range(stmt)
                            && self.w.blank_line_before(&token)
                        {
                            self.w.blank_line();
                        }
                    }
                    first = false;
                    self.statement(stmt, 0);
                }
                Element::Token(t) => {
                    self.w.set_indent(0);
                    self.w.token(t);
                }
            }
        }
    }

    fn statement(&mut self, stmt: &Node<'a>, base: usize) {
        match stmt.kind {
            NodeKind::SelectStmt => self.select_stmt(stmt, base),
            NodeKind::InsertStmt => self.insert_stmt(stmt, base),
            NodeKind::UpdateStmt | NodeKind::DeleteStmt => self.update_or_delete(stmt, base),
            NodeKind::CreateFunctionStmt => self.create_function(stmt, base),
            NodeKind::CreateTableStmt
            | NodeKind::CreateViewStmt
            | NodeKind::CreateIndexStmt
            | NodeKind::CreateTypeStmt
            | NodeKind::ExplainStmt => self.create_object(stmt, base),
            NodeKind::AlterTableStmt => self.list_clause(stmt, base),
            // `DROP FUNCTION f(int, text)`
            NodeKind::DropStmt => self.inline_glued(stmt, |e| is_node(e, NodeKind::ExprList)),
            NodeKind::MergeStmt => self.merge_stmt(stmt, base),
            NodeKind::CreateTriggerStmt
            | NodeKind::CreateSequenceStmt
            | NodeKind::AlterStmt
            | NodeKind::CommentStmt
            | NodeKind::GrantStmt => self.clause_per_line(stmt, base),
            NodeKind::DoStmt => self.do_stmt(stmt, base),
            _ => self.node(stmt),
        }
    }

    // ---- Expressions and inline ----

    fn element(&mut self, element: &Element<'a>) {
        match element {
            Element::Node(n) => self.node(n),
            Element::Token(t) => self.w.token(t),
        }
    }

    /// Writes a node. Everything except subqueries, CASE and JOIN goes on one line.
    fn node(&mut self, node: &Node<'a>) {
        match node.kind {
            kind if is_opaque(kind) => self.w.raw(node),
            NodeKind::SubqueryExpr | NodeKind::ParenSelect | NodeKind::ParenJoin => {
                self.paren_block(node)
            }
            NodeKind::CaseExpr => self.case_expr(node),
            NodeKind::JoinExpr => {
                let base = self.w.indent();
                self.join_expr(node, base);
            }
            NodeKind::FuncCall | NodeKind::FunctionTable => {
                self.inline_glued(node, |e| is_node(e, NodeKind::ArgList))
            }
            NodeKind::TypeName => self.type_name(node),
            NodeKind::SubscriptExpr => {
                self.inline_glued(node, |e| is_token(e, TokenKind::LBracket))
            }
            NodeKind::CastCall => self.inline_glued(node, |e| is_token(e, TokenKind::LParen)),
            // The alias column list `AS g(n, i)` follows the name directly
            NodeKind::Alias => self.inline_glued(node, |e| is_node(e, NodeKind::ExprList)),
            NodeKind::ArrayExpr | NodeKind::RowExpr => self.inline_glued(node, |e| {
                is_token(e, TokenKind::LBracket)
                    || is_node(e, NodeKind::ExprList)
                    || is_node(e, NodeKind::SubqueryExpr)
            }),
            NodeKind::PrefixExpr => self.prefix_expr(node),
            NodeKind::Literal => self.literal(node),
            NodeKind::ArgList | NodeKind::ExprList | NodeKind::ParamList => self.paren_list(node),
            NodeKind::BinaryExpr => self.binary_expr(node),
            NodeKind::WindowSpec => self.window_spec(node),
            NodeKind::PlRaise | NodeKind::PlExecute => self.statement_with_options(node),
            NodeKind::FetchClause | NodeKind::LockingClause | NodeKind::FrameClause => {
                self.loose(node)
            }
            NodeKind::FunctionBody => {
                let base = self.w.indent();
                self.inline(node);
                self.w.set_indent(base);
            }
            _ => self.inline(node),
        }
    }

    fn inline(&mut self, node: &Node<'a>) {
        self.inline_glued(node, |_| false);
    }

    /// No space is written before a child matching `glued` (`f(x)` / `numeric(10, 2)` / `a[1]`).
    /// If the last child is an alias, its width is reserved on the current line while the
    /// preceding children are written (so the inside of the parentheses in `f(a, b) AS name` is
    /// wrapped based on the length including the alias)
    fn inline_glued(&mut self, node: &Node<'a>, glued: fn(&Element) -> bool) {
        let elements = children(node);
        let alias_width = match elements.last() {
            Some(Element::Node(alias)) if alias.kind == NodeKind::Alias => self.inline_width(alias),
            _ => 0,
        };
        for (i, element) in elements.iter().enumerate() {
            if i > 0 && glued(element) {
                self.w.glue();
            }
            // Nodes that carry an alias (items, tables, subqueries) get a line break when nested,
            // so two reservations never overlap on the same line
            let saved = self.reserve;
            if alias_width > 0 && i + 1 < elements.len() {
                self.reserve = Some((alias_width, self.w.line_id()));
            }
            self.element(element);
            self.reserve = saved;
        }
    }

    /// Width of the node when written on one line in the middle of a line (including the
    /// preceding space, excluding indentation and comments). 0 while an enclosing measurement is
    /// in progress.
    /// Comments are not counted, because which token they attach to (before or after) depends on
    /// the input (counting them would change the result on a second formatting pass)
    fn inline_width(&mut self, node: &Node<'a>) -> usize {
        if self.w.measuring() {
            return 0;
        }
        let saved = self.w.begin_measure_inline();
        let start = self.w.measure_column().unwrap_or(0);
        self.node(node);
        let end = self.w.measure_column().unwrap_or(start);
        self.w.end_measure(saved);
        end.saturating_sub(start)
    }

    /// Strings joined by a newline (`'a'` newline `'b'`) change meaning if the newline is
    /// dropped, so the continuation goes on the next line
    fn literal(&mut self, node: &Node<'a>) {
        let base = self.w.indent();
        for (i, element) in children(node).into_iter().enumerate() {
            if i > 0 {
                self.w.newline(base + self.indent_width);
            }
            self.element(element);
        }
    }

    /// Writes the comma between list items and moves to the next item's line (indented by
    /// `item_indent`). A leading comma is placed `outdent` columns to the left of the item. A
    /// trailing comma is appended to the end of the previous item's line.
    fn list_separator(
        &mut self,
        comma: &Token<'a>,
        next_item: Option<Token<'a>>,
        item_indent: usize,
        outdent: usize,
    ) {
        match self.comma_style {
            CommaStyle::Leading => {
                // Trailing comments after the comma stay on the previous item's line; comments
                // before the next item are emitted before the comma
                self.w.flush_trailing_comments(comma);
                self.w.newline(item_indent);
                if let Some(first) = next_item {
                    self.w.move_leading_comments(&first, comma);
                }
                self.w.token_outdented(comma, outdent);
            }
            CommaStyle::Trailing => {
                self.w.token(comma);
                self.w.newline(item_indent);
            }
        }
    }

    /// No spaces in `numeric(10, 2)` / `int[]` / `tbl.col%TYPE`
    fn type_name(&mut self, node: &Node<'a>) {
        let mut after_percent = false;
        for (i, element) in children(node).into_iter().enumerate() {
            let percent =
                as_token(element).is_some_and(|t| t.kind == TokenKind::Operator && t.text == "%");
            if i > 0
                && (after_percent
                    || percent
                    || is_token(element, TokenKind::LParen)
                    || is_token(element, TokenKind::LBracket))
            {
                self.w.glue();
            }
            self.element(element);
            after_percent = percent;
        }
    }

    /// A symbolic prefix operator such as `-a` takes no space; `NOT a` does.
    /// When the operand itself starts with an operator (`- -a`), a space is kept: joined together
    /// they would form a different token (`--` is a comment).
    fn prefix_expr(&mut self, node: &Node<'a>) {
        let elements = children(node);
        for (i, element) in elements.iter().enumerate() {
            self.element(element);
            let operand_starts_with_operator = elements
                .get(i + 1)
                .and_then(|e| as_node(e))
                .and_then(writer::token_range)
                .is_some_and(|(first, _)| first.kind == TokenKind::Operator);
            if is_token(element, TokenKind::Operator) && !operand_starts_with_operator {
                self.w.glue();
            }
        }
    }

    /// A clause whose contents are not parsed in detail. Only known keywords are upper-cased.
    fn loose(&mut self, node: &Node<'a>) {
        for element in children(node) {
            match element {
                Element::Token(t)
                    if t.kind == TokenKind::Ident
                        && LOOSE_KEYWORDS
                            .iter()
                            .any(|k| t.text.eq_ignore_ascii_case(k)) =>
                {
                    let text = self.w.keyword_text(t.text);
                    self.w.token_as(t, &text, 0);
                }
                _ => self.element(element),
            }
        }
    }

    /// Writes the statement or node between `(` and `)` on its own lines, one level deeper
    fn paren_block(&mut self, node: &Node<'a>) {
        let base = self.w.indent();
        let elements = children(node);
        let has_body = elements.iter().any(|e| as_node(e).is_some());
        for element in elements {
            match element {
                Element::Token(t) if t.kind == TokenKind::LParen => self.w.token(t),
                Element::Token(t) if t.kind == TokenKind::RParen && has_body => {
                    self.w.newline(base);
                    self.w.token(t);
                }
                Element::Node(inner) if is_statement(inner.kind) => {
                    self.w.newline(base + self.indent_width);
                    self.statement(inner, base + self.indent_width);
                }
                Element::Node(inner) if inner.kind == NodeKind::JoinExpr => {
                    self.w.newline(base + self.indent_width);
                    self.join_expr(inner, base + self.indent_width);
                }
                _ => self.element(element),
            }
        }
    }

    fn case_expr(&mut self, node: &Node<'a>) {
        let base = self.w.indent();
        for element in children(node) {
            match element {
                Element::Node(n)
                    if matches!(n.kind, NodeKind::WhenClause | NodeKind::ElseClause) =>
                {
                    self.w.newline(base + self.indent_width);
                    self.inline(n);
                }
                Element::Token(t)
                    if t.kind == TokenKind::Keyword && t.text.eq_ignore_ascii_case("end") =>
                {
                    self.w.newline(base);
                    self.w.token(t);
                }
                _ => self.element(element),
            }
        }
    }

    /// Writes `left JOIN right ON cond` with JOIN one level deeper than `base` and ON two levels
    /// deeper. `base` is the indentation of the line where the left side starts, so a JOIN inside
    /// the left side lines up at the same depth.
    fn join_expr(&mut self, node: &Node<'a>, base: usize) {
        let mut seen_left = false;
        let mut on_join_line = false;
        for element in children(node) {
            match element {
                Element::Node(n) if !seen_left => {
                    seen_left = true;
                    self.node(n);
                }
                Element::Token(t) if seen_left && !on_join_line => {
                    self.w.newline(base + self.indent_width);
                    on_join_line = true;
                    self.w.token(t);
                }
                Element::Node(n) if n.kind == NodeKind::JoinCondition => {
                    self.w.newline(base + 2 * self.indent_width);
                    self.condition_clause(n, base + 2 * self.indent_width);
                }
                _ => self.element(element),
            }
        }
    }
}

fn is_statement(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::SelectStmt
            | NodeKind::InsertStmt
            | NodeKind::UpdateStmt
            | NodeKind::DeleteStmt
            | NodeKind::MergeStmt
            | NodeKind::CreateTableStmt
            | NodeKind::CreateIndexStmt
            | NodeKind::CreateViewStmt
            | NodeKind::AlterTableStmt
            | NodeKind::DropStmt
            | NodeKind::CreateTriggerStmt
            | NodeKind::CommentStmt
            | NodeKind::TruncateStmt
            | NodeKind::GrantStmt
            | NodeKind::AlterStmt
            | NodeKind::CopyStmt
            | NodeKind::SetStmt
            | NodeKind::ExplainStmt
            | NodeKind::TransactionStmt
            | NodeKind::CreateSequenceStmt
            | NodeKind::CreateTypeStmt
            | NodeKind::CreateSchemaStmt
            | NodeKind::CreateExtensionStmt
            | NodeKind::CreateFunctionStmt
            | NodeKind::DoStmt
            | NodeKind::CallStmt
            | NodeKind::RawStatement
    )
}
