//! 整形結果を書き出す。字下げ・トークン間の空白・コメントの配置を受け持つ。
//!
//! コメントは書き出す前に、意味のあるトークンへ結び付けておく。
//! - 直前のトークンと同じ行にあるコメントは、そのトークンの後ろ（trailing）
//! - それ以外は、次のトークンの前（leading）に独立した行として出す
//!
//! `RawStatement` / `Error` ノードは元のテキストのまま出すので、中のコメントは結び付けない。

use std::collections::HashMap;

use crate::lexer::{Token, TokenKind};
use crate::syntax::{Element, Node, NodeKind};

pub(super) const INDENT: usize = 4;

struct Comment<'a> {
    token: Token<'a>,
    /// 元のテキストで、直前に改行があった（入力の先頭を含む）
    newline_before: bool,
    /// 元のテキストで、直前に空行があった
    blank_before: bool,
    /// 元のテキストで、直後に改行があった
    newline_after: bool,
    /// 元のテキストで、直後に空行があった
    blank_after: bool,
}

#[derive(Default)]
struct Attached<'a> {
    leading: Vec<Comment<'a>>,
    trailing: Vec<Comment<'a>>,
}

impl Comment<'_> {
    /// 元のテキストで行末にあった（後ろに同じ行のトークンがない）
    fn ends_line(&self) -> bool {
        self.newline_after || self.token.kind == TokenKind::LineComment
    }
}

pub(super) struct Writer<'a> {
    src: &'a str,
    out: String,
    /// 意味のあるトークンの位置 → そのトークンに結び付けたコメント
    comments: HashMap<usize, Attached<'a>>,
    /// 意味のあるトークンの位置 → 元のテキストで直前にあった空白
    whitespace_before: HashMap<usize, &'a str>,
    /// 最後のトークンより後ろにあるコメント
    tail: Vec<Comment<'a>>,
    /// 現在の行の字下げ（論理的な深さ。行頭カンマの行でも項目の位置を指す）
    indent: usize,
    at_line_start: bool,
    /// 次のトークンを空白なしで続ける
    glue_next: bool,
    /// 行コメントを書いたので、次のトークンは改行してから書く
    must_break: bool,
}

/// 元のテキストのまま出すノード
pub(super) fn is_opaque(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::RawStatement | NodeKind::Error)
}

/// 空白・コメントを除いた最初と最後のトークン
pub(super) fn token_range<'a>(node: &Node<'a>) -> Option<(Token<'a>, Token<'a>)> {
    let mut first = None;
    let mut last = None;
    node.for_each_token(&mut |t| {
        if !t.kind.is_trivia() {
            first.get_or_insert(*t);
            last = Some(*t);
        }
    });
    Some((first?, last?))
}

impl<'a> Writer<'a> {
    pub(super) fn new(src: &'a str, root: &Node<'a>) -> Self {
        let mut w = Writer {
            src,
            out: String::new(),
            comments: HashMap::new(),
            whitespace_before: HashMap::new(),
            tail: Vec::new(),
            indent: 0,
            at_line_start: true,
            glue_next: false,
            must_break: false,
        };
        w.attach_comments(root);
        w
    }

    fn attach_comments(&mut self, root: &Node<'a>) {
        let mut units = Vec::new();
        flatten(root, &mut units);

        let mut prev: Option<Token<'a>> = None;
        let mut whitespace = "";
        let mut leading: Vec<Comment<'a>> = Vec::new();
        for (i, unit) in units.iter().enumerate() {
            let token = *unit;
            match token.kind {
                TokenKind::Whitespace => whitespace = token.text,
                TokenKind::LineComment | TokenKind::BlockComment { .. } => {
                    // 入力の終わりも行末とみなす（整形結果の最後には改行が付くので）
                    let next = units.get(i + 1);
                    let newlines_after = next
                        .filter(|t| t.kind == TokenKind::Whitespace)
                        .map_or(0, |t| count_newlines(t.text));
                    let comment = Comment {
                        token,
                        newline_before: prev.is_none() || whitespace.contains('\n'),
                        blank_before: prev.is_some() && count_newlines(whitespace) >= 2,
                        newline_after: next.is_none() || newlines_after >= 1,
                        blank_after: next.is_some() && newlines_after >= 2,
                    };
                    match prev {
                        Some(p) if leading.is_empty() && !whitespace.contains('\n') => {
                            self.comments
                                .entry(p.offset)
                                .or_default()
                                .trailing
                                .push(comment);
                        }
                        _ => leading.push(comment),
                    }
                    whitespace = "";
                }
                _ => {
                    self.whitespace_before.insert(token.offset, whitespace);
                    if !leading.is_empty() {
                        self.comments.entry(token.offset).or_default().leading =
                            std::mem::take(&mut leading);
                    }
                    prev = Some(token);
                    whitespace = "";
                }
            }
        }
        self.tail = leading;
    }

    /// 元のテキストで、このトークン（またはその前のコメント）の直前に空行があったか
    pub(super) fn blank_line_before(&self, token: &Token<'a>) -> bool {
        if let Some(first) = self
            .comments
            .get(&token.offset)
            .and_then(|a| a.leading.first())
        {
            return first.blank_before;
        }
        self.whitespace_before
            .get(&token.offset)
            .is_some_and(|ws| count_newlines(ws) >= 2)
    }

    /// 現在の行の字下げ
    pub(super) fn indent(&self) -> usize {
        self.indent
    }

    /// 改行して、次の行を `indent` の字下げで始める。行頭なら字下げだけ変える。
    pub(super) fn newline(&mut self, indent: usize) {
        if !self.at_line_start {
            self.out.push('\n');
            self.at_line_start = true;
        }
        self.indent = indent;
        self.must_break = false;
        self.glue_next = false;
    }

    /// 改行せずに、現在の行の字下げ（以降の複数行の式や折り返しの基準）を変える
    pub(super) fn set_indent(&mut self, indent: usize) {
        self.indent = indent;
    }

    /// `token` の後ろの行末コメントを、いまの位置に書く（行頭カンマにする前の行末に残すため）。
    /// いまの行がすでに行コメントで終わっているとき（書くと行コメントに飲み込まれる）と、
    /// まだ何も書いていない行のときは動かさない。
    pub(super) fn flush_trailing_comments(&mut self, token: &Token<'a>) {
        if self.must_break || self.at_line_start {
            return;
        }
        let Some(attached) = self.comments.get_mut(&token.offset) else {
            return;
        };
        if !attached.trailing.last().is_some_and(Comment::ends_line) {
            return;
        }
        let trailing = std::mem::take(&mut attached.trailing);
        for comment in trailing {
            self.trailing_comment(&comment);
        }
    }

    /// `from` の前の、独立した行にあるコメントを `to` の前に移す
    /// （行頭カンマの項目の前のコメントを、カンマより前に出すため）。
    /// `from` と同じ行にあるブロックコメント（`/* x */ b`）は `from` に残す。
    pub(super) fn move_leading_comments(&mut self, from: &Token<'a>, to: &Token<'a>) {
        let Some(attached) = self.comments.get_mut(&from.offset) else {
            return;
        };
        let own_lines = attached
            .leading
            .iter()
            .position(|c| !c.ends_line())
            .unwrap_or(attached.leading.len());
        if own_lines == 0 {
            return;
        }
        let moved: Vec<_> = attached.leading.drain(..own_lines).collect();
        self.comments
            .entry(to.offset)
            .or_default()
            .leading
            .extend(moved);
    }

    /// 空行を入れる（出力の先頭では何もしない）
    pub(super) fn blank_line(&mut self) {
        if self.out.is_empty() {
            return;
        }
        let indent = self.indent;
        self.newline(indent);
        if !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    /// 次のトークンを空白なしで続ける
    pub(super) fn glue(&mut self) {
        self.glue_next = true;
    }

    pub(super) fn token(&mut self, token: &Token<'a>) {
        let text = match token.kind {
            TokenKind::Keyword => token.text.to_ascii_uppercase(),
            _ => token.text.to_string(),
        };
        self.token_as(token, &text, 0);
    }

    /// 字下げより `outdent` だけ左から書き始める（行頭カンマ用）
    pub(super) fn token_outdented(&mut self, token: &Token<'a>, outdent: usize) {
        self.token_as(token, token.text, outdent);
    }

    /// トークンを `text` として書く。前後のコメントも書く。
    pub(super) fn token_as(&mut self, token: &Token<'a>, text: &str, outdent: usize) {
        let attached = self.comments.remove(&token.offset).unwrap_or_default();
        for comment in attached.leading {
            self.leading_comment(&comment);
        }
        self.word(text, token.kind, outdent);
        for comment in attached.trailing {
            self.trailing_comment(&comment);
        }
    }

    /// `RawStatement` / `Error` を元のテキストのまま書く
    pub(super) fn raw(&mut self, node: &Node<'a>) {
        let Some((first, last)) = token_range(node) else {
            return;
        };
        let attached = self.comments.remove(&first.offset).unwrap_or_default();
        for comment in attached.leading {
            self.leading_comment(&comment);
        }
        let end = last.offset + last.text.len();
        self.word(&self.src[first.offset..end], TokenKind::Unknown, 0);
        let trailing = if first.offset == last.offset {
            attached.trailing
        } else {
            self.comments
                .remove(&last.offset)
                .unwrap_or_default()
                .trailing
        };
        for comment in trailing {
            self.trailing_comment(&comment);
        }
    }

    fn word(&mut self, text: &str, kind: TokenKind, outdent: usize) {
        if self.must_break {
            let indent = self.indent;
            self.newline(indent);
        }
        if self.at_line_start {
            let width = self.indent.saturating_sub(outdent);
            self.out.extend(std::iter::repeat_n(' ', width));
            self.at_line_start = false;
        } else if !self.glue_next && !no_space_before(kind) {
            self.out.push(' ');
        }
        self.out.push_str(text);
        self.glue_next = no_space_after(kind);
    }

    fn leading_comment(&mut self, comment: &Comment<'a>) {
        // 元のテキストで前のコメントと同じ行にあったか、次のトークンと同じ行にあったブロックコメントは、
        // 行の途中でもそのまま続けて書く
        let inline = !comment.newline_before
            || (comment.token.kind != TokenKind::LineComment && !comment.newline_after);
        if comment.blank_before {
            self.blank_line();
        } else if self.must_break || (!self.at_line_start && !inline) {
            let indent = self.indent;
            self.newline(indent);
        }
        if self.at_line_start {
            self.word(comment.token.text, comment.token.kind, 0);
        } else {
            // 行の途中のコメントは、直前のトークンによらず空白を 1 つ空ける（行末のコメントと同じ形にする）
            self.out.push(' ');
            self.out.push_str(comment.token.text);
            self.glue_next = false;
        }
        if comment.newline_after || comment.token.kind == TokenKind::LineComment {
            let indent = self.indent;
            self.newline(indent);
        }
        if comment.blank_after {
            self.blank_line();
        }
    }

    fn trailing_comment(&mut self, comment: &Comment<'a>) {
        self.out.push(' ');
        self.out.push_str(comment.token.text);
        self.glue_next = false;
        if comment.token.kind == TokenKind::LineComment {
            self.must_break = true;
        }
    }

    pub(super) fn finish(mut self) -> String {
        let tail = std::mem::take(&mut self.tail);
        for comment in &tail {
            if comment.blank_before {
                self.blank_line();
            } else {
                self.newline(0);
            }
            self.word(comment.token.text, comment.token.kind, 0);
        }
        let trimmed = self.out.trim_end().len();
        self.out.truncate(trimmed);
        if !self.out.is_empty() {
            self.out.push('\n');
        }
        self.out
    }
}

/// 木のトークンを順に並べる。`RawStatement` / `Error` は最初と最後のトークンだけにする。
fn flatten<'a>(node: &Node<'a>, out: &mut Vec<Token<'a>>) {
    for child in &node.children {
        match child {
            Element::Node(n) if is_opaque(n.kind) => {
                if let Some((first, last)) = token_range(n) {
                    out.push(first);
                    if last.offset != first.offset {
                        out.push(last);
                    }
                }
            }
            Element::Node(n) => flatten(n, out),
            Element::Token(t) => out.push(*t),
        }
    }
}

fn count_newlines(text: &str) -> usize {
    text.bytes().filter(|&b| b == b'\n').count()
}

fn no_space_before(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::RParen
            | TokenKind::RBracket
            | TokenKind::Comma
            | TokenKind::Semicolon
            | TokenKind::Dot
            | TokenKind::DoubleColon
            | TokenKind::Colon
    )
}

fn no_space_after(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::LParen
            | TokenKind::LBracket
            | TokenKind::Dot
            | TokenKind::DoubleColon
            | TokenKind::Colon
    )
}
