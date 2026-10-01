//! 整形結果を書き出す。字下げ・トークン間の空白・コメントの配置を受け持つ。
//!
//! コメントは書き出す前に、意味のあるトークンへ結び付けておく。
//! - 直前のトークンと同じ行にあるコメントは、そのトークンの後ろ（trailing）
//! - それ以外は、次のトークンの前（leading）に独立した行として出す
//!
//! `RawStatement` / `Error` ノードは元のテキストのまま出すので、中のコメントは結び付けない。

use std::collections::HashMap;

use super::KeywordCase;
use crate::lexer::{Token, TokenKind};
use crate::syntax::{Element, Node, NodeKind};

#[derive(Clone)]
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

#[derive(Default, Clone)]
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
    /// 測定中なら、出力せずに幅だけを数える
    measure: Option<Measure>,
    keyword_case: KeywordCase,
    /// 出力の最後の空白を削るとき、ここより前は削らない（COPY のデータの行末の空白は値の一部）
    keep_len: usize,
    /// 直前に書いたトークンが `:` なら、元の入力でのその終わりの位置
    after_colon: Option<usize>,
}

/// 1 行で書いたときに行幅に収まるかの測定
struct Measure {
    column: usize,
    max_width: usize,
    fits: bool,
    /// コメントも数えるか
    comments: bool,
}

/// 測定の前の状態（測定のあとに戻す）
pub(super) struct Saved {
    indent: usize,
    at_line_start: bool,
    glue_next: bool,
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

    /// このトークンの前のコメントの、直前の空行を出さないようにする（ブロックの先頭の空行を消すため）
    pub(super) fn drop_blank_line_before(&mut self, token: &Token<'a>) {
        if let Some(first) = self
            .comments
            .get_mut(&token.offset)
            .and_then(|a| a.leading.first_mut())
        {
            first.blank_before = false;
        }
    }

    /// 現在の行の字下げ
    pub(super) fn indent(&self) -> usize {
        self.indent
    }

    /// これから書く位置の桁（行頭なら 0。字下げは次のトークンを書くときに入る）
    fn column(&self) -> usize {
        let line_start = self.out.rfind('\n').map_or(0, |i| i + 1);
        display_width(&self.out[line_start..])
    }

    pub(super) fn measuring(&self) -> bool {
        self.measure.is_some()
    }

    /// 測定中なら、測っている位置の桁
    pub(super) fn measure_column(&self) -> Option<usize> {
        self.measure.as_ref().map(|m| m.column)
    }

    /// いまの行を表す値（行の先頭の位置）。行が変わったかを見分けるのに使う
    pub(super) fn line_id(&self) -> usize {
        self.out.rfind('\n').map_or(0, |i| i + 1)
    }

    /// 測定を始める。以降の出力は捨てて、いまの位置から `max_width` に収まるかだけを調べる。
    pub(super) fn begin_measure(&mut self, max_width: usize) -> Saved {
        let saved = Saved {
            indent: self.indent,
            at_line_start: self.at_line_start,
            glue_next: self.glue_next,
            must_break: self.must_break,
        };
        // 行コメントの後ろなら、実際には次の行の頭から書くので、そこから測る
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

    /// 行の途中に続けて書いたときの幅を測る（前の空白は数え、字下げとコメントは数えない）
    pub(super) fn begin_measure_inline(&mut self) -> Saved {
        let saved = self.begin_measure(usize::MAX);
        if let Some(m) = &mut self.measure {
            m.comments = false;
        }
        // 行頭なら字下げを書いてしまうので、行の途中にいるものとして測る
        self.at_line_start = false;
        self.glue_next = false;
        saved
    }

    /// 測定を終えて状態を戻し、1 行で収まったかを返す
    pub(super) fn end_measure(&mut self, saved: Saved) -> bool {
        let measure = self.measure.take().expect("測定中でない");
        self.indent = saved.indent;
        self.at_line_start = saved.at_line_start;
        self.glue_next = saved.glue_next;
        self.must_break = saved.must_break;
        measure.fits
    }

    /// 文字列を書く。測定中は幅だけを数える。
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

    /// 改行して、次の行を `indent` の字下げで始める。行頭なら字下げだけ変える。
    pub(super) fn newline(&mut self, indent: usize) {
        if !self.at_line_start {
            match &mut self.measure {
                Some(m) => m.fits = false,
                None => self.out.push('\n'),
            }
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
    /// 測定中も動かさない（取り出してしまうと、本番で書くときにコメントがなくなる）。
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
        if let Some(m) = &mut self.measure {
            m.fits = false;
            return;
        }
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

    /// ここまでの出力を、最後の空白を削る対象から外す
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

    /// キーワードを設定どおりの大文字・小文字にする
    pub(super) fn keyword_text(&self, text: &str) -> String {
        match self.keyword_case {
            KeywordCase::Upper => text.to_ascii_uppercase(),
            KeywordCase::Lower => text.to_ascii_lowercase(),
            KeywordCase::Preserve => text.to_string(),
        }
    }

    /// 字下げより `outdent` だけ左から書き始める（行頭カンマ用）
    pub(super) fn token_outdented(&mut self, token: &Token<'a>, outdent: usize) {
        self.token_as(token, token.text, outdent);
    }

    /// トークンを `text` として書く。前後のコメントも書く。
    pub(super) fn token_as(&mut self, token: &Token<'a>, text: &str, outdent: usize) {
        let attached = if self.measure.as_ref().is_some_and(|m| !m.comments) {
            Attached::default()
        } else {
            self.take_comments(token)
        };
        for comment in attached.leading {
            self.leading_comment(&comment);
        }
        // `a[2: n]` のように元の入力で `:` の後ろに空白があれば残す。詰めると psql が `:n` を変数として置き換える
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

    /// `RawStatement` / `Error` を元のテキストのまま書く
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

    /// トークンに結び付けたコメントを取り出す。測定中は取り出さずに写しを返す。
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
        let trimmed = self.out.trim_end().len().max(self.keep_len);
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

/// 表示の幅。全角の文字（CJK など）は 2 桁と数える。
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
