//! 関数定義・DO と PL/pgSQL の本体のレイアウト。
//!
//! - `CREATE FUNCTION name(args)` の後ろの RETURNS / LANGUAGE / AS などのオプションは 1 行ずつ
//! - ドル引用符の本体の中身は、CREATE と同じ深さから書き始める
//! - DECLARE / BEGIN / EXCEPTION / END / ELSIF / ELSE はブロックの深さ、文は 1 段深く
//! - CASE 文の WHEN と EXCEPTION の WHEN はさらに 1 段深く、その中の文はもう 1 段深く
//! - `FOR r IN` / `RETURN QUERY` / `OPEN c FOR` の後ろの問い合わせは、次の行から 1 段深く

use super::{Formatter, as_node, children, is_statement};
use crate::lexer::TokenKind;
use crate::syntax::{Element, Node, NodeKind};

/// ブロックの深さの行から始めるキーワード
const BLOCK_KEYWORDS: &[&str] = &[
    "declare",
    "begin",
    "exception",
    "elsif",
    "elseif",
    "else",
    "end",
];

fn is_pl_statement(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::PlBlock
            | NodeKind::PlDecl
            | NodeKind::PlAssign
            | NodeKind::PlIf
            | NodeKind::PlCase
            | NodeKind::PlLoop
            | NodeKind::PlExit
            | NodeKind::PlReturn
            | NodeKind::PlRaise
            | NodeKind::PlAssert
            | NodeKind::PlPerform
            | NodeKind::PlExecute
            | NodeKind::PlGetDiagnostics
            | NodeKind::PlOpen
            | NodeKind::PlNull
            | NodeKind::PlSimpleStmt
            | NodeKind::PlSqlStmt
            | NodeKind::Error
    )
}

/// 親の中で、改行して始める部分（中にさらに文を持つ）
fn is_pl_part(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::PlDeclareSection
            | NodeKind::PlExceptionSection
            | NodeKind::PlExceptionHandler
            | NodeKind::PlElsif
            | NodeKind::PlElse
            | NodeKind::PlCaseWhen
    )
}

fn is_keyword(element: &Element, words: &[&str]) -> bool {
    matches!(element, Element::Token(t)
        if t.kind == TokenKind::Keyword && words.iter().any(|w| t.text.eq_ignore_ascii_case(w)))
}

impl<'a> Formatter<'a> {
    /// `CREATE FUNCTION name(args)` と、1 行ずつのオプション
    pub(super) fn create_function(&mut self, stmt: &Node<'a>, base: usize) {
        for element in children(stmt) {
            match as_node(element).map(|n| (n, n.kind)) {
                Some((n, NodeKind::ParamList)) => {
                    self.w.glue();
                    self.node(n);
                }
                Some((n, NodeKind::ReturnsClause | NodeKind::FunctionOption)) => {
                    self.w.newline(base);
                    self.with_function_body(n, base);
                }
                Some((n, NodeKind::AtomicBody)) => {
                    self.w.newline(base);
                    self.atomic_body(n, base);
                }
                _ => self.element(element),
            }
        }
    }

    pub(super) fn do_stmt(&mut self, stmt: &Node<'a>, base: usize) {
        self.with_function_body(stmt, base);
    }

    fn with_function_body(&mut self, node: &Node<'a>, base: usize) {
        for element in children(node) {
            match as_node(element) {
                Some(body) if body.kind == NodeKind::FunctionBody => self.function_body(body, base),
                _ => self.element(element),
            }
        }
    }

    /// `$tag$` の後ろで改行し、中身を `base` の深さから書いて、閉じる `$tag$` を独立した行に置く
    fn function_body(&mut self, body: &Node<'a>, base: usize) {
        let mut contents = 0;
        for element in children(body) {
            match element {
                Element::Token(t) if t.kind == TokenKind::DollarDelimiter => {
                    if contents > 0 {
                        self.w.newline(base);
                    }
                    self.w.token(t);
                }
                Element::Node(n) => {
                    self.w.newline(base);
                    self.blank_line_if_separated(n, contents == 0);
                    contents += 1;
                    if n.kind == NodeKind::PlBlock {
                        self.pl_children(n, base);
                    } else {
                        self.statement(n, base);
                    }
                }
                Element::Token(t) => self.w.token(t),
            }
        }
    }

    /// 元のテキストで、このノードの前に空行があれば空行を入れる。
    /// 並びの先頭（`first`）では、コメントの前の空行も含めて入れない。
    fn blank_line_if_separated(&mut self, node: &Node<'a>, first: bool) {
        let Some((token, _)) = super::writer::token_range(node) else {
            return;
        };
        if first {
            self.w.drop_blank_line_before(&token);
        } else if self.w.blank_line_before(&token) {
            self.w.blank_line();
        }
    }

    /// `BEGIN ATOMIC` の中の文は 1 段深く、`END` は `base` の深さ
    fn atomic_body(&mut self, node: &Node<'a>, base: usize) {
        for element in children(node) {
            match element {
                Element::Node(n) => {
                    self.w.newline(base + self.indent_width);
                    self.statement(n, base + self.indent_width);
                }
                _ if is_keyword(element, &["end"]) => {
                    self.w.newline(base);
                    self.element(element);
                }
                _ => self.element(element),
            }
        }
    }

    fn pl_statement(&mut self, node: &Node<'a>, base: usize) {
        match node.kind {
            NodeKind::PlSqlStmt | NodeKind::PlPerform => {
                for element in children(node) {
                    match as_node(element) {
                        Some(n) if is_statement(n.kind) => self.statement(n, base),
                        Some(n) if n.kind == NodeKind::SimpleSelect => self.query_part(n, base),
                        _ => self.element(element),
                    }
                }
            }
            NodeKind::PlSimpleStmt => self.loose(node),
            // 中に文や問い合わせを持つもの
            NodeKind::PlBlock
            | NodeKind::PlIf
            | NodeKind::PlCase
            | NodeKind::PlLoop
            | NodeKind::PlDecl
            | NodeKind::PlReturn
            | NodeKind::PlOpen => self.pl_children(node, base),
            // `RAISE EXCEPTION` の EXCEPTION などを改行しないよう、1 行で書く
            _ => self.node(node),
        }
    }

    /// PL/pgSQL の文やブロックの子を書く。`base` はこの文の行の深さ。
    fn pl_children(&mut self, node: &Node<'a>, base: usize) {
        let part_base = match node.kind {
            NodeKind::PlCase | NodeKind::PlExceptionSection => base + self.indent_width,
            _ => base,
        };
        let mut statements = 0;
        let mut after_query = false;
        for element in children(node) {
            match element {
                Element::Node(n) if is_pl_statement(n.kind) => {
                    self.w.newline(base + self.indent_width);
                    self.blank_line_if_separated(n, statements == 0);
                    statements += 1;
                    self.pl_statement(n, base + self.indent_width);
                }
                Element::Node(n) if is_pl_part(n.kind) => {
                    self.w.newline(part_base);
                    self.pl_children(n, part_base);
                }
                // `OPEN cursor(args)`
                Element::Node(n) if n.kind == NodeKind::ArgList => {
                    self.w.glue();
                    self.inline(n);
                }
                Element::Node(n) if n.kind == NodeKind::PlLabel => {
                    for (i, token) in children(n).into_iter().enumerate() {
                        if i > 0 {
                            self.w.glue();
                        }
                        self.element(token);
                    }
                    self.w.newline(base);
                }
                // `FOR r IN` / `RETURN QUERY` / `OPEN c FOR` / `CURSOR FOR` の後ろの問い合わせ
                Element::Node(n) if is_statement(n.kind) => {
                    self.w.newline(base + self.indent_width);
                    self.statement(n, base + self.indent_width);
                    after_query = true;
                }
                _ if is_keyword(element, BLOCK_KEYWORDS) => {
                    self.w.newline(base);
                    self.element(element);
                }
                _ if after_query && is_keyword(element, &["loop"]) => {
                    self.w.newline(base);
                    self.element(element);
                    after_query = false;
                }
                _ => self.element(element),
            }
        }
    }
}
