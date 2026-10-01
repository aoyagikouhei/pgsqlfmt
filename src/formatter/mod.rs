//! 構文木を整形して文字列にする。
//!
//! スタイル:
//! - 句ごとに改行する。並びの項目が 2 つ以上なら、項目を 1 行ずつ字下げし、カンマは行頭に置く
//! - WHERE / HAVING / ON の最上位の AND・OR は改行して 1 段深くする
//! - JOIN は FROM の行より 1 段、ON はさらに 1 段深くする
//! - 副問い合わせと CASE は複数行にする。それ以外の式は 1 行で、行幅に収まらなければ折り返す（`wrap.rs`）
//! - キーワードは大文字にする。識別子・関数名・型名は入力のまま
//!
//! `RawStatement` / `Error` は元のテキストのまま出す。

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
    /// 行幅。式がこれを超えるときに折り返す（全角文字は 2 桁と数える）
    pub max_width: usize,
    /// 1 段の字下げの幅。行頭カンマは項目より 2 桁左に置くので、2 以上にする
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

/// キーワードの大文字・小文字
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordCase {
    Upper,
    Lower,
    /// 入力のまま
    Preserve,
}

/// 項目を 1 行ずつ並べるときのカンマの位置
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
    // 先頭の BOM は入力のまま残す（字句解析器は空白として読むので、ここで付け直す）
    if src.starts_with(BOM) {
        return format!("{BOM}{out}");
    }
    out
}

/// 中身を細かく解釈していない句で、大文字にするキーワード
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
    /// 同じ行の後ろに続く別名（`AS name`）の幅と、その行。折り返しの判定で行幅から差し引く
    reserve: Option<(usize, usize)>,
    indent_width: usize,
    comma_style: CommaStyle,
}

/// 空白・コメントを除いた子
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

/// AND / OR の BinaryExpr なら、その演算子（小文字）
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
                // COPY のデータは `;` の直後から入力のまま続ける（改行を足すとデータの始まりが変わる）
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

    // ---- 式・インライン ----

    fn element(&mut self, element: &Element<'a>) {
        match element {
            Element::Node(n) => self.node(n),
            Element::Token(t) => self.w.token(t),
        }
    }

    /// ノードを書く。副問い合わせ・CASE・JOIN 以外は 1 行にする。
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
            // 別名の列の並び `AS g(n, i)` は名前に続ける
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

    /// `glued` に当たる子の前には空白を入れない（`f(x)` / `numeric(10, 2)` / `a[1]`）。
    /// 最後の子が別名なら、その前を書く間は別名の幅を同じ行に取っておく
    /// （`f(a, b) AS name` の括弧の中を、別名まで含めた長さで折り返す）
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
            // 別名を持つノード（項目・表・副問い合わせ）は、入れ子になると改行が入るので、同じ行で重ならない
            let saved = self.reserve;
            if alias_width > 0 && i + 1 < elements.len() {
                self.reserve = Some((alias_width, self.w.line_id()));
            }
            self.element(element);
            self.reserve = saved;
        }
    }

    /// ノードを行の途中に 1 行で書いたときの幅（前の空白を含み、字下げとコメントは含まない）。外側を測っている途中なら 0。
    /// コメントは、入力によって前後どちらのトークンに付くかが変わるので数えない（数えると 2 回目の整形で変わる）
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

    /// 改行でつないだ文字列（`'a'` 改行 `'b'`）は、改行を残さないと意味が変わるので、続きを次の行に書く
    fn literal(&mut self, node: &Node<'a>) {
        let base = self.w.indent();
        for (i, element) in children(node).into_iter().enumerate() {
            if i > 0 {
                self.w.newline(base + self.indent_width);
            }
            self.element(element);
        }
    }

    /// 並びの項目の間のカンマを書き、次の項目の行（字下げ `item_indent`）に移る。
    /// 行頭カンマは項目より `outdent` 桁左に置く。行末カンマは前の項目の行の最後に付ける。
    fn list_separator(
        &mut self,
        comma: &Token<'a>,
        next_item: Option<Token<'a>>,
        item_indent: usize,
        outdent: usize,
    ) {
        match self.comma_style {
            CommaStyle::Leading => {
                // カンマの後ろの行末コメントは前の項目の行に残し、次の項目の前のコメントはカンマより前に出す
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

    /// `numeric(10, 2)` / `int[]` / `tbl.col%TYPE` は空白を入れない
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

    /// `-a` のような記号の前置演算子は空白を入れない。`NOT a` は入れる。
    /// 被演算子が演算子で始まるとき（`- -a`）は、つなげると別のトークン（`--` はコメント）になるので空ける。
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

    /// 中身を細かく解釈していない句。既知のキーワードだけ大文字にする。
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

    /// `(` と `)` の間の文やノードを、1 段深くした別の行に書く
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

    /// `left JOIN right ON cond` を、JOIN を `base` より 1 段、ON を 2 段深くして書く。
    /// `base` は左側を書き始める行の字下げなので、左側の JOIN も同じ深さに並ぶ。
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
