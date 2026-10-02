//! Writes out the formatted result. Handles indentation, spacing between tokens and comment
//! placement.
//!
//! Comments are attached to significant tokens before writing begins.
//! - A comment on the same line as the preceding token goes after that token (trailing)
//! - Any other comment goes before the next token (leading), on its own line
//!
//! `RawStatement` / `Error` nodes are emitted verbatim, so comments inside them are not attached.

use std::collections::HashMap;

use super::KeywordCase;
use crate::lexer::{Token, TokenKind};
use crate::syntax::{Element, Node, NodeKind};

#[derive(Clone)]
struct Comment<'a> {
    token: Token<'a>,
    /// Preceded by a newline in the original text (includes the start of the input)
    newline_before: bool,
    /// Preceded by a blank line in the original text
    blank_before: bool,
    /// Followed by a newline in the original text
    newline_after: bool,
    /// Followed by a blank line in the original text
    blank_after: bool,
}

#[derive(Default, Clone)]
struct Attached<'a> {
    leading: Vec<Comment<'a>>,
    trailing: Vec<Comment<'a>>,
}

impl Comment<'_> {
    /// Was at the end of a line in the original text (no token follows on the same line)
    fn ends_line(&self) -> bool {
        self.newline_after || self.token.kind == TokenKind::LineComment
    }
}

pub(super) struct Writer<'a> {
    src: &'a str,
    out: String,
    /// Offset of a significant token → the comments attached to that token
    comments: HashMap<usize, Attached<'a>>,
    /// Offset of a significant token → the whitespace that preceded it in the original text
    whitespace_before: HashMap<usize, &'a str>,
    /// Comments after the last token
    tail: Vec<Comment<'a>>,
    /// Indentation of the current line (the logical depth; on a leading-comma line it is still
    /// the item's position)
    indent: usize,
    at_line_start: bool,
    /// Write the next token without a preceding space
    glue_next: bool,
    /// A line comment was written, so the next token must start on a new line
    must_break: bool,
    /// While measuring, nothing is emitted; only the width is counted
    measure: Option<Measure>,
    keyword_case: KeywordCase,
    /// When trimming trailing whitespace from the output, nothing before this point is trimmed
    /// (trailing whitespace in COPY data is part of the value)
    keep_len: usize,
    /// If the previously written token was `:`, its end offset in the original input
    after_colon: Option<usize>,
    /// Newline string. Follows the first newline in the input (`\r\n` input stays `\r\n`)
    newline: &'static str,
    /// The last token of the input is an unterminated string or comment. To leave its contents
    /// untouched, trailing whitespace is not trimmed and no newline is appended
    ends_unterminated: bool,
}

/// Measurement of whether something fits the line width when written on one line
struct Measure {
    column: usize,
    max_width: usize,
    fits: bool,
    /// Whether comments are counted too
    comments: bool,
}

/// State from before a measurement (restored afterwards)
pub(super) struct Saved {
    indent: usize,
    at_line_start: bool,
    glue_next: bool,
    must_break: bool,
}

/// An unterminated string, quoted identifier or comment (runs to the end of the input)
fn is_unterminated(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::BlockComment { terminated: false }
            | TokenKind::QuotedIdent { terminated: false }
            | TokenKind::String {
                terminated: false,
                ..
            }
            | TokenKind::DollarString { terminated: false }
    )
}

/// Nodes emitted verbatim
pub(super) fn is_opaque(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::RawStatement | NodeKind::Error)
}

/// The first and last tokens, excluding whitespace and comments
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
    pub(super) fn new(src: &'a str, root: &Node<'a>, keyword_case: KeywordCase) -> Self {
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
            measure: None,
            keyword_case,
            keep_len: 0,
            after_colon: None,
            newline: newline_style(src),
            ends_unterminated: false,
        };
        w.attach_comments(root);
        w
    }

    fn attach_comments(&mut self, root: &Node<'a>) {
        let mut units = Vec::new();
        flatten(root, &mut units);

        self.ends_unterminated = units.last().is_some_and(|t| is_unterminated(t.kind));

        let mut prev: Option<Token<'a>> = None;
        let mut whitespace = "";
        let mut leading: Vec<Comment<'a>> = Vec::new();
        for (i, unit) in units.iter().enumerate() {
            let token = *unit;
            match token.kind {
                TokenKind::Whitespace => whitespace = token.text,
                TokenKind::LineComment | TokenKind::BlockComment { .. } => {
                    // The end of the input also counts as the end of a line (the formatted
                    // output ends with a newline)
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

    /// Whether a blank line preceded this token (or the comments before it) in the original text
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

    /// Suppresses the blank line before the comments preceding this token (to drop a blank line
    /// at the start of a block)
    pub(super) fn drop_blank_line_before(&mut self, token: &Token<'a>) {
        if let Some(first) = self
            .comments
            .get_mut(&token.offset)
            .and_then(|a| a.leading.first_mut())
        {
            first.blank_before = false;
        }
    }

    /// Indentation of the current line
    pub(super) fn indent(&self) -> usize {
        self.indent
    }

    /// Column of the position about to be written (0 at the start of a line; the indentation is
    /// emitted along with the next token)
    fn column(&self) -> usize {
        let line_start = self.out.rfind('\n').map_or(0, |i| i + 1);
        display_width(&self.out[line_start..])
    }

    pub(super) fn measuring(&self) -> bool {
        self.measure.is_some()
    }

    /// While measuring, the column of the measured position
    pub(super) fn measure_column(&self) -> Option<usize> {
        self.measure.as_ref().map(|m| m.column)
    }

    /// A value identifying the current line (the offset of its start). Used to detect that the
    /// line has changed
    pub(super) fn line_id(&self) -> usize {
        self.out.rfind('\n').map_or(0, |i| i + 1)
    }

    /// Starts a measurement. Subsequent output is discarded; it only checks whether the text fits
    /// within `max_width` from the current position.
    pub(super) fn begin_measure(&mut self, max_width: usize) -> Saved {
        let saved = Saved {
            indent: self.indent,
            at_line_start: self.at_line_start,
            glue_next: self.glue_next,
            must_break: self.must_break,
        };
        // After a line comment the text actually starts at the head of the next line, so measure
        // from there
        if self.must_break {
            self.must_break = false;
            self.at_line_start = true;
        }
        self.measure = Some(Measure {
            column: self.column(),
            max_width,
            fits: true,
            comments: true,
        });
        saved
    }

    /// Measures the width when written as a continuation in the middle of a line (the preceding
    /// space is counted; indentation and comments are not)
    pub(super) fn begin_measure_inline(&mut self) -> Saved {
        let saved = self.begin_measure(usize::MAX);
        if let Some(m) = &mut self.measure {
            m.comments = false;
        }
        // At the start of a line the indentation would be emitted, so measure as if mid-line
        self.at_line_start = false;
        self.glue_next = false;
        saved
    }

    /// Ends the measurement, restores the state and returns whether it fit on one line
    pub(super) fn end_measure(&mut self, saved: Saved) -> bool {
        let measure = self.measure.take().expect("not measuring");
        self.indent = saved.indent;
        self.at_line_start = saved.at_line_start;
        self.glue_next = saved.glue_next;
        self.must_break = saved.must_break;
        measure.fits
    }

    /// Writes a string. While measuring, only the width is counted.
    fn emit(&mut self, text: &str) {
        match &mut self.measure {
            Some(m) => {
                if text.contains('\n') {
                    m.fits = false;
                } else {
                    m.column += display_width(text);
                    m.fits &= m.column <= m.max_width;
                }
            }
            None => self.out.push_str(text),
        }
    }

    /// Breaks the line and starts the next one indented by `indent`. At the start of a line, only
    /// the indentation is changed.
    pub(super) fn newline(&mut self, indent: usize) {
        if !self.at_line_start {
            match &mut self.measure {
                Some(m) => m.fits = false,
                None => self.out.push_str(self.newline),
            }
            self.at_line_start = true;
        }
        self.indent = indent;
        self.must_break = false;
        self.glue_next = false;
    }

    /// Changes the current line's indentation (the base for subsequent multi-line expressions and
    /// wrapping) without breaking the line
    pub(super) fn set_indent(&mut self, indent: usize) {
        self.indent = indent;
    }

    /// Writes the trailing comments of `token` at the current position (to keep them at the end
    /// of the line before the comma is moved to the start of the next line).
    /// They are left in place when the current line already ends with a line comment (writing
    /// them would swallow them into it) and when nothing has been written on the line yet.
    /// They are also left in place while measuring (taking them out would lose the comments when
    /// writing for real).
    pub(super) fn flush_trailing_comments(&mut self, token: &Token<'a>) {
        if self.must_break || self.at_line_start || self.measuring() {
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

    /// Moves the comments on their own lines before `from` to before `to`
    /// (to emit the comments before a leading-comma item ahead of the comma).
    /// A block comment on the same line as `from` (`/* x */ b`) stays with `from`.
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

    /// Inserts a blank line (does nothing at the start of the output)
    pub(super) fn blank_line(&mut self) {
        if let Some(m) = &mut self.measure {
            m.fits = false;
            return;
        }
        if self.out.is_empty() {
            return;
        }
        let indent = self.indent;
        self.newline(indent);
        let blank = [self.newline, self.newline].concat();
        if !self.out.ends_with(&blank) {
            self.out.push_str(self.newline);
        }
    }

    /// Write the next token without a preceding space
    pub(super) fn glue(&mut self) {
        self.glue_next = true;
    }

    /// Excludes the output so far from trailing-whitespace trimming
    pub(super) fn keep_output(&mut self) {
        self.keep_len = self.out.len();
    }

    pub(super) fn token(&mut self, token: &Token<'a>) {
        let text = match token.kind {
            TokenKind::Keyword => self.keyword_text(token.text),
            _ => token.text.to_string(),
        };
        self.token_as(token, &text, 0);
    }

    /// Applies the configured case to a keyword
    pub(super) fn keyword_text(&self, text: &str) -> String {
        match self.keyword_case {
            KeywordCase::Upper => text.to_ascii_uppercase(),
            KeywordCase::Lower => text.to_ascii_lowercase(),
            KeywordCase::Preserve => text.to_string(),
        }
    }

    /// Starts writing `outdent` columns to the left of the indentation (for leading commas)
    pub(super) fn token_outdented(&mut self, token: &Token<'a>, outdent: usize) {
        self.token_as(token, token.text, outdent);
    }

    /// Writes the token as `text`, along with its leading and trailing comments.
    pub(super) fn token_as(&mut self, token: &Token<'a>, text: &str, outdent: usize) {
        let attached = if self.measure.as_ref().is_some_and(|m| !m.comments) {
            Attached::default()
        } else {
            self.take_comments(token)
        };
        for comment in attached.leading {
            self.leading_comment(&comment);
        }
        // Keep a space after `:` when the original input had one, as in `a[2: n]`. Closing the
        // gap would make psql substitute `:n` as a variable
        let forms_variable = text.bytes().next().is_some_and(|b| {
            b.is_ascii_alphabetic() || b == b'_' || b >= 0x80 || b == b'\'' || b == b'"'
        });
        if forms_variable && self.after_colon.is_some_and(|end| token.offset > end) {
            self.glue_next = false;
        }
        self.word(text, token.kind, outdent);
        self.after_colon =
            (token.kind == TokenKind::Colon).then(|| token.offset + token.text.len());
        for comment in attached.trailing {
            self.trailing_comment(&comment);
        }
    }

    /// Writes a `RawStatement` / `Error` verbatim
    pub(super) fn raw(&mut self, node: &Node<'a>) {
        let Some((first, last)) = token_range(node) else {
            return;
        };
        let attached = self.take_comments(&first);
        for comment in attached.leading {
            self.leading_comment(&comment);
        }
        let end = last.offset + last.text.len();
        self.word(&self.src[first.offset..end], TokenKind::Unknown, 0);
        let trailing = if first.offset == last.offset {
            attached.trailing
        } else {
            self.take_comments(&last).trailing
        };
        for comment in trailing {
            self.trailing_comment(&comment);
        }
    }

    /// Takes the comments attached to a token. While measuring, they are left in place and a
    /// copy is returned.
    fn take_comments(&mut self, token: &Token<'a>) -> Attached<'a> {
        if self.measuring() {
            self.comments
                .get(&token.offset)
                .cloned()
                .unwrap_or_default()
        } else {
            self.comments.remove(&token.offset).unwrap_or_default()
        }
    }

    fn word(&mut self, text: &str, kind: TokenKind, outdent: usize) {
        if self.must_break {
            let indent = self.indent;
            self.newline(indent);
        }
        if self.at_line_start {
            let width = self.indent.saturating_sub(outdent);
            self.emit(&" ".repeat(width));
            self.at_line_start = false;
        } else if !self.glue_next && !no_space_before(kind) {
            self.emit(" ");
        }
        self.emit(text);
        self.glue_next = no_space_after(kind);
    }

    fn leading_comment(&mut self, comment: &Comment<'a>) {
        // A comment that was on the same line as the previous comment in the original text, or a
        // block comment that was on the same line as the next token, continues mid-line as is
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
            // A comment mid-line gets exactly one space before it regardless of the previous
            // token (the same shape as a trailing comment)
            self.emit(" ");
            self.emit(comment.token.text);
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
        self.emit(" ");
        self.emit(comment.token.text);
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
        // An unterminated string or comment runs to the end of the input, so it is always at the
        // end of the output
        if self.ends_unterminated {
            return self.out;
        }
        let trimmed = self.out.trim_end().len().max(self.keep_len);
        self.out.truncate(trimmed);
        if !self.out.is_empty() {
            self.out.push_str(self.newline);
        }
        self.out
    }
}

/// Lists the tokens of the tree in order. `RawStatement` / `Error` contribute only their first
/// and last tokens.
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

/// Display width. Full-width characters (CJK etc.) count as 2 columns.
pub(super) fn display_width(text: &str) -> usize {
    text.chars()
        .map(|c| match c as u32 {
            0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x20000..=0x3FFFD => 2,
            _ => 1,
        })
        .sum()
}

/// `\r\n` if the first newline in the input is `\r\n`; otherwise `\n` (including when there is no
/// newline at all)
fn newline_style(src: &str) -> &'static str {
    match src.find('\n') {
        Some(i) if src.as_bytes()[..i].ends_with(b"\r") => "\r\n",
        _ => "\n",
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
            | TokenKind::DotDot
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
            | TokenKind::DotDot
    )
}
