//! Lexer for PostgreSQL.
//!
//! Every byte of the input, including whitespace and comments, is assigned to some token,
//! so concatenating the tokens' `text` in order reproduces the input (lossless).
//! The lexical rules follow PostgreSQL's `src/backend/parser/scan.l`.
//! Invalid input is never an error: an unterminated string and the like is read to the end
//! of the input with `terminated: false`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// Whitespace and line breaks
    Whitespace,
    /// `-- ...` (the trailing line break is not included)
    LineComment,
    /// `/* ... */` (may be nested)
    BlockComment {
        terminated: bool,
    },
    /// An unquoted identifier (including keywords)
    Ident,
    /// An identifier read as a keyword. The lexer never produces it; the parser relabels an `Ident`
    Keyword,
    /// `"..."` / `U&"..."`
    QuotedIdent {
        terminated: bool,
    },
    /// `'...'` and prefixed strings (`E'...'` etc.)
    String {
        prefix: StringPrefix,
        terminated: bool,
    },
    /// `$$...$$` / `$tag$...$tag$`
    DollarString {
        terminated: bool,
    },
    /// The `$tag$` of a dollar-quoted string whose contents were parsed. The lexer never produces
    /// it; the parser creates it by splitting a `DollarString`
    DollarDelimiter,
    Number,
    /// A positional parameter such as `$1`
    Param,
    /// An operator such as `+` `<=` `@>` `->>`
    Operator,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Dot,
    /// `..` (PL/pgSQL `FOR i IN 1..10`)
    DotDot,
    Colon,
    /// `::` (type cast)
    DoubleColon,
    /// `:=` (PL/pgSQL assignment)
    ColonEquals,
    /// A single character that matches no rule
    Unknown,
    /// A psql variable `:name` / `:'name'` / `:"name"`. psql substitutes it, so it is treated as a
    /// single name. Not recognized inside function bodies (psql does not substitute there) or when
    /// the `:` directly follows a name or number, as in `a[1:n]`
    PsqlVariable,
    /// The data from right after `COPY ... FROM STDIN;` up to the line containing only `\.`
    /// (including the rest of the line the `;` is on)
    CopyData,
}

impl TokenKind {
    /// Whitespace and comments. Has no syntactic meaning
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            TokenKind::Whitespace | TokenKind::LineComment | TokenKind::BlockComment { .. }
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringPrefix {
    /// `'...'`
    None,
    /// `E'...'` (backslash escapes)
    Escape,
    /// `B'...'`
    Bit,
    /// `X'...'`
    Hex,
    /// `N'...'`
    National,
    /// `U&'...'`
    Unicode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: TokenKind,
    pub text: &'a str,
    /// Byte offset from the start of the input
    pub offset: usize,
}

/// The UTF-8 BOM. Treated as whitespace when it appears at the start of the input
pub const BOM: &str = "\u{FEFF}";

/// Tokenizes a part of the input. Token offsets have `base` added, so they are positions in the
/// original input. Since this is a fragment such as a function body, COPY data and psql variables
/// are not recognized.
pub fn tokenize_with_offset(src: &str, base: usize) -> Vec<Token<'_>> {
    let mut tokens = scan_all(src, false);
    for token in &mut tokens {
        token.offset += base;
    }
    tokens
}

pub fn tokenize(src: &str) -> Vec<Token<'_>> {
    scan_all(src, true)
}

/// `top_level` says whether `src` is the whole input (a script given to psql). COPY data and psql
/// variables are recognized only there
fn scan_all(src: &str, top_level: bool) -> Vec<Token<'_>> {
    let mut lexer = Lexer {
        src,
        bytes: src.as_bytes(),
        pos: 0,
        psql_variables: top_level,
    };
    let mut tokens = Vec::new();
    // Split off a leading BOM as whitespace (as an identifier character it would stick to `select`)
    if let Some(rest) = src.strip_prefix(BOM) {
        lexer.pos = src.len() - rest.len();
        tokens.push(Token {
            kind: TokenKind::Whitespace,
            text: BOM,
            offset: 0,
        });
    }
    while lexer.pos < src.len() {
        let start = lexer.pos;
        let kind = lexer.scan();
        // Everything from right after `COPY ... FROM STDIN;` to the end of the data becomes one
        // token. If nothing but the final line break remains, there is no data
        let copy_follows = top_level
            && kind == TokenKind::Semicolon
            && ends_copy_from_stdin(&tokens)
            && !strip_last_newline(&src[lexer.pos..]).is_empty();
        tokens.push(Token {
            kind,
            text: &src[start..lexer.pos],
            offset: start,
        });
        if copy_follows {
            let start = lexer.pos;
            lexer.copy_data();
            tokens.push(Token {
                kind: TokenKind::CopyData,
                text: &src[start..lexer.pos],
                offset: start,
            });
        }
    }
    tokens
}

/// Whether the last statement in `tokens` is `COPY ... FROM STDIN`. Anything inside parentheses
/// (`COPY (SELECT ... FROM stdin) TO ...`) is ignored
fn ends_copy_from_stdin(tokens: &[Token]) -> bool {
    let mut depth = 0usize;
    let mut words = Vec::new();
    for t in tokens.iter().rev() {
        match t.kind {
            TokenKind::Semicolon | TokenKind::CopyData => break,
            TokenKind::RParen => depth += 1,
            TokenKind::LParen => depth = depth.saturating_sub(1),
            kind if kind.is_trivia() || depth > 0 => {}
            TokenKind::Ident => words.push(t.text),
            _ => words.push(""),
        }
    }
    words.last().is_some_and(|w| w.eq_ignore_ascii_case("copy"))
        && words
            .windows(2)
            .any(|w| w[0].eq_ignore_ascii_case("stdin") && w[1].eq_ignore_ascii_case("from"))
}

/// Removes one trailing line break (`\n` or `\r\n`), since the formatted output always ends with
/// a line break
fn strip_last_newline(text: &str) -> &str {
    let text = text.strip_suffix('\n').unwrap_or(text);
    text.strip_suffix('\r').unwrap_or(text)
}

// All delimiters are ASCII, so advancing byte by byte never splits a UTF-8 character.
// Non-ASCII bytes are treated as identifier characters (like `\200-\377` in scan.l).
struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    psql_variables: bool,
}

impl Lexer<'_> {
    fn peek(&self, n: usize) -> Option<u8> {
        self.bytes.get(self.pos + n).copied()
    }

    fn peek_is(&self, n: usize, pred: impl Fn(u8) -> bool) -> bool {
        self.peek(n).is_some_and(pred)
    }

    fn eat_while(&mut self, pred: impl Fn(u8) -> bool) {
        while self.peek_is(0, &pred) {
            self.pos += 1;
        }
    }

    /// Reads COPY data up to the line containing only `\.` (a trailing `\r` is allowed). Without
    /// one, reads up to the final line break of the input (whitespace is kept too: a trailing tab
    /// is an empty column and an empty line is an empty row).
    /// As in psql, the data starts on the line after the `;`. The rest of the line the `;` is on
    /// is included in the data so that formatting does not move where the data starts
    fn copy_data(&mut self) {
        let start = self.pos;
        let mut line_start = start;
        for (i, line) in self.src[start..].split_inclusive('\n').enumerate() {
            if i > 0 && line.trim_end_matches(['\n', '\r']) == "\\." {
                self.pos = line_start + 2;
                return;
            }
            line_start += line.len();
        }
        self.pos = start + strip_last_newline(&self.src[start..]).len();
    }

    /// Whether a psql variable starts at the current `:`
    fn at_psql_variable(&self) -> bool {
        let prev = self.pos.checked_sub(1).map(|i| self.bytes[i]);
        let after_value = prev
            .is_some_and(|p| is_ident_cont(p) || matches!(p, b')' | b']' | b'[' | b'\'' | b'"'));
        self.psql_variables && !after_value && self.psql_variable_follows()
    }

    /// Whether the current `:` is directly followed by a variable name or by a quote that is closed
    /// later
    fn psql_variable_follows(&self) -> bool {
        match self.peek(1) {
            Some(b'\'' | b'"') => {
                let quote = self.bytes[self.pos + 1];
                self.bytes[self.pos + 2..].contains(&quote)
            }
            Some(c) => is_ident_start(c),
            None => false,
        }
    }

    /// `:name` / `:'name'` / `:"name"`
    /// Adjacent variables such as `:a:b` become one token, since psql substitutes each and joins
    /// the results
    fn psql_variable(&mut self) -> TokenKind {
        loop {
            self.pos += 1;
            match self.bytes[self.pos] {
                quote @ (b'\'' | b'"') => {
                    // A doubled quote, as in `:'it''s'`, is one name
                    self.pos += 1;
                    loop {
                        self.eat_while(|b| b != quote);
                        self.pos += 1;
                        // Even if the next byte is the same quote, it is not a doubled quote
                        // unless a closing quote follows later (input ending in `:'a''`)
                        if self.peek(0) != Some(quote)
                            || !self.bytes[self.pos + 1..].contains(&quote)
                        {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                _ => self.eat_while(|b| is_ident_start(b) || is_dec_digit(b)),
            }
            if !(self.peek(0) == Some(b':') && self.psql_variable_follows()) {
                return TokenKind::PsqlVariable;
            }
        }
    }

    /// Reads one token and returns its kind. `pos` always advances.
    fn scan(&mut self) -> TokenKind {
        let c = self.bytes[self.pos];
        match c {
            _ if is_whitespace(c) => {
                self.eat_while(is_whitespace);
                TokenKind::Whitespace
            }
            b'-' if self.peek(1) == Some(b'-') => {
                self.eat_while(|b| b != b'\n' && b != b'\r');
                TokenKind::LineComment
            }
            b'/' if self.peek(1) == Some(b'*') => self.block_comment(),
            b'\'' => self.string(StringPrefix::None),
            b'"' => self.quoted_ident(),
            b'$' => self.dollar(),
            b'0'..=b'9' => self.number(),
            b'.' if self.peek(1) == Some(b'.') => self.punct(2, TokenKind::DotDot),
            b'.' if self.peek_is(1, is_dec_digit) => self.number(),
            b'.' => self.punct(1, TokenKind::Dot),
            b':' => match self.peek(1) {
                Some(b':') => self.punct(2, TokenKind::DoubleColon),
                Some(b'=') => self.punct(2, TokenKind::ColonEquals),
                _ if self.at_psql_variable() => self.psql_variable(),
                _ => self.punct(1, TokenKind::Colon),
            },
            b'(' => self.punct(1, TokenKind::LParen),
            b')' => self.punct(1, TokenKind::RParen),
            b'[' => self.punct(1, TokenKind::LBracket),
            b']' => self.punct(1, TokenKind::RBracket),
            b',' => self.punct(1, TokenKind::Comma),
            b';' => self.punct(1, TokenKind::Semicolon),
            _ if is_op_char(c) => self.operator(),
            _ if is_ident_start(c) => self.ident_or_prefixed_literal(),
            _ => {
                self.pos += 1;
                TokenKind::Unknown
            }
        }
    }

    fn punct(&mut self, len: usize, kind: TokenKind) -> TokenKind {
        self.pos += len;
        kind
    }

    fn block_comment(&mut self) -> TokenKind {
        self.pos += 2;
        let mut depth = 1;
        while self.pos < self.bytes.len() {
            if self.bytes[self.pos..].starts_with(b"/*") {
                depth += 1;
                self.pos += 2;
            } else if self.bytes[self.pos..].starts_with(b"*/") {
                depth -= 1;
                self.pos += 2;
                if depth == 0 {
                    return TokenKind::BlockComment { terminated: true };
                }
            } else {
                self.pos += 1;
            }
        }
        TokenKind::BlockComment { terminated: false }
    }

    /// `pos` must point at the opening `'`.
    fn string(&mut self, prefix: StringPrefix) -> TokenKind {
        self.pos += 1;
        let terminated = loop {
            match self.peek(0) {
                None => break false,
                Some(b'\'') if self.peek(1) == Some(b'\'') => self.pos += 2,
                Some(b'\'') => {
                    self.pos += 1;
                    break true;
                }
                Some(b'\\') if prefix == StringPrefix::Escape => {
                    self.pos = (self.pos + 2).min(self.bytes.len());
                }
                Some(_) => self.pos += 1,
            }
        };
        TokenKind::String { prefix, terminated }
    }

    /// `pos` must point at the opening `"`.
    fn quoted_ident(&mut self) -> TokenKind {
        self.pos += 1;
        let terminated = loop {
            match self.peek(0) {
                None => break false,
                Some(b'"') if self.peek(1) == Some(b'"') => self.pos += 2,
                Some(b'"') => {
                    self.pos += 1;
                    break true;
                }
                Some(_) => self.pos += 1,
            }
        };
        TokenKind::QuotedIdent { terminated }
    }

    /// One of `$1`, `$$...$$` or `$tag$...$tag$`. Otherwise the single `$` becomes `Unknown`.
    fn dollar(&mut self) -> TokenKind {
        if self.peek_is(1, is_dec_digit) {
            self.pos += 1;
            self.eat_while(is_dec_digit);
            return TokenKind::Param;
        }
        let Some(delim_len) = self.dollar_delimiter_len() else {
            self.pos += 1;
            return TokenKind::Unknown;
        };
        let delim = &self.src[self.pos..self.pos + delim_len];
        self.pos += delim_len;
        match self.src[self.pos..].find(delim) {
            Some(i) => {
                self.pos += i + delim_len;
                TokenKind::DollarString { terminated: true }
            }
            None => {
                self.pos = self.bytes.len();
                TokenKind::DollarString { terminated: false }
            }
        }
    }

    /// Length of the `$tag$` starting at `pos`. The tag may be empty and cannot start with a digit.
    fn dollar_delimiter_len(&self) -> Option<usize> {
        let mut i = 1;
        if self.peek_is(i, is_ident_start) {
            i += 1;
            while self.peek_is(i, |b| is_ident_start(b) || is_dec_digit(b)) {
                i += 1;
            }
        }
        (self.peek(i) == Some(b'$')).then_some(i + 1)
    }

    fn number(&mut self) -> TokenKind {
        if self.peek(0) == Some(b'0') {
            let radix_digit: Option<fn(u8) -> bool> =
                match self.peek(1).map(|b| b.to_ascii_lowercase()) {
                    Some(b'x') => Some(|b| b.is_ascii_hexdigit()),
                    Some(b'o') => Some(|b| matches!(b, b'0'..=b'7')),
                    Some(b'b') => Some(|b| matches!(b, b'0' | b'1')),
                    _ => None,
                };
            if let Some(is_digit) = radix_digit {
                let save = self.pos;
                self.pos += 2;
                if self.eat_digits(is_digit, true) {
                    return TokenKind::Number;
                }
                self.pos = save;
            }
        }

        if self.peek(0) == Some(b'.') {
            self.pos += 1;
            self.eat_digits(is_dec_digit, false);
        } else {
            self.eat_digits(is_dec_digit, false);
            // The `..` in `1..10` is a range, not a decimal point
            if self.peek(0) == Some(b'.') && self.peek(1) != Some(b'.') {
                self.pos += 1;
                self.eat_digits(is_dec_digit, false);
            }
        }

        if matches!(self.peek(0), Some(b'e' | b'E')) {
            let save = self.pos;
            self.pos += 1;
            if matches!(self.peek(0), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !self.eat_digits(is_dec_digit, false) {
                self.pos = save;
            }
        }
        TokenKind::Number
    }

    /// Reads a digit sequence in which a single `_` may separate digits, as in `1_000`.
    /// A leading `_`, as in `0x_FF`, is allowed only with a radix prefix. Returns true if at
    /// least one digit was read.
    fn eat_digits(&mut self, is_digit: fn(u8) -> bool, allow_leading_underscore: bool) -> bool {
        let start = self.pos;
        loop {
            let underscore =
                self.peek(0) == Some(b'_') && (self.pos > start || allow_leading_underscore);
            let skip = usize::from(underscore);
            if !self.peek_is(skip, is_digit) {
                break;
            }
            self.pos += skip + 1;
        }
        self.pos > start
    }

    /// Reads an operator. As in scan.l, it stops before `--` / `/*`, and a trailing `+` / `-` is
    /// split off an operator that contains none of `~!@#^&|`?%` (so `*-1` becomes `*` and `-1`).
    fn operator(&mut self) -> TokenKind {
        let start = self.pos;
        let mut end = start;
        while end < self.bytes.len() && is_op_char(self.bytes[end]) {
            let rest = &self.bytes[end..];
            if end > start && (rest.starts_with(b"--") || rest.starts_with(b"/*")) {
                break;
            }
            end += 1;
        }

        let op = &self.bytes[start..end];
        let mut len = op.len();
        if len > 1
            && matches!(op[len - 1], b'+' | b'-')
            && !op[..len - 1].iter().any(|c| b"~!@#^&|`?%".contains(c))
        {
            while len > 1 && matches!(op[len - 1], b'+' | b'-') {
                len -= 1;
            }
        }
        self.pos = start + len;
        TokenKind::Operator
    }

    fn ident_or_prefixed_literal(&mut self) -> TokenKind {
        let prefix = match (self.bytes[self.pos].to_ascii_lowercase(), self.peek(1)) {
            (b'e', Some(b'\'')) => Some(StringPrefix::Escape),
            (b'b', Some(b'\'')) => Some(StringPrefix::Bit),
            (b'x', Some(b'\'')) => Some(StringPrefix::Hex),
            (b'n', Some(b'\'')) => Some(StringPrefix::National),
            (b'u', Some(b'&')) => match self.peek(2) {
                Some(b'\'') => {
                    self.pos += 2;
                    return self.string(StringPrefix::Unicode);
                }
                Some(b'"') => {
                    self.pos += 2;
                    return self.quoted_ident();
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(prefix) = prefix {
            self.pos += 1;
            return self.string(prefix);
        }
        self.eat_while(is_ident_cont);
        TokenKind::Ident
    }
}

fn is_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn is_dec_digit(b: u8) -> bool {
    b.is_ascii_digit()
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

fn is_ident_cont(b: u8) -> bool {
    is_ident_start(b) || is_dec_digit(b) || b == b'$'
}

fn is_op_char(b: u8) -> bool {
    b"~!@#^&|`?+-*/%<>=".contains(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The (kind, text) sequence with whitespace removed
    fn lex(src: &str) -> Vec<(TokenKind, &str)> {
        let tokens = tokenize(src);
        assert_eq!(tokens.iter().map(|t| t.text).collect::<String>(), src);
        tokens
            .into_iter()
            .filter(|t| t.kind != TokenKind::Whitespace)
            .map(|t| (t.kind, t.text))
            .collect()
    }

    fn texts(src: &str) -> Vec<&str> {
        lex(src).into_iter().map(|(_, text)| text).collect()
    }

    fn single(src: &str) -> TokenKind {
        let tokens = lex(src);
        assert_eq!(tokens.len(), 1, "{src:?} -> {tokens:?}");
        tokens[0].0
    }

    #[test]
    fn psql_variables() {
        assert_eq!(
            lex("select :a, :'b', :\"c\", x[1:n], y[:n], z::int, w :=1"),
            [
                (TokenKind::Ident, "select"),
                (TokenKind::PsqlVariable, ":a"),
                (TokenKind::Comma, ","),
                (TokenKind::PsqlVariable, ":'b'"),
                (TokenKind::Comma, ","),
                (TokenKind::PsqlVariable, ":\"c\""),
                (TokenKind::Comma, ","),
                (TokenKind::Ident, "x"),
                (TokenKind::LBracket, "["),
                (TokenKind::Number, "1"),
                (TokenKind::Colon, ":"),
                (TokenKind::Ident, "n"),
                (TokenKind::RBracket, "]"),
                (TokenKind::Comma, ","),
                (TokenKind::Ident, "y"),
                (TokenKind::LBracket, "["),
                (TokenKind::Colon, ":"),
                (TokenKind::Ident, "n"),
                (TokenKind::RBracket, "]"),
                (TokenKind::Comma, ","),
                (TokenKind::Ident, "z"),
                (TokenKind::DoubleColon, "::"),
                (TokenKind::Ident, "int"),
                (TokenKind::Comma, ","),
                (TokenKind::Ident, "w"),
                (TokenKind::ColonEquals, ":="),
                (TokenKind::Number, "1"),
            ]
        );
        // An unterminated quote is not a variable
        assert_eq!(lex(":'a")[0], (TokenKind::Colon, ":"));
        // If the input ends in the middle of a doubled quote, the variable ends at the closing
        // quote
        assert_eq!(
            lex(":'a''"),
            [
                (TokenKind::PsqlVariable, ":'a'"),
                (
                    TokenKind::String {
                        prefix: StringPrefix::None,
                        terminated: false
                    },
                    "'"
                ),
            ]
        );
        assert_eq!(
            lex(":\"a\"\""),
            [
                (TokenKind::PsqlVariable, ":\"a\""),
                (TokenKind::QuotedIdent { terminated: false }, "\""),
            ]
        );
        assert_eq!(lex(":'it''s'"), [(TokenKind::PsqlVariable, ":'it''s'")]);
        // Not recognized inside a function body
        let body = tokenize_with_offset("x := :a", 0);
        assert!(body.iter().all(|t| t.kind != TokenKind::PsqlVariable));
    }

    #[test]
    fn copy_data() {
        // The rest of the line the `;` is on (a comment here) is data too. The end marker is looked
        // for from the next line
        assert_eq!(
            lex("COPY t FROM stdin; -- c \\.\n1\t'\n\\.\r\nSELECT"),
            [
                (TokenKind::Ident, "COPY"),
                (TokenKind::Ident, "t"),
                (TokenKind::Ident, "FROM"),
                (TokenKind::Ident, "stdin"),
                (TokenKind::Semicolon, ";"),
                (TokenKind::CopyData, " -- c \\.\n1\t'\n\\."),
                (TokenKind::Ident, "SELECT"),
            ]
        );
        // psql does not treat a `\.` on the same line as the `;` as the end of the data
        assert_eq!(
            texts("copy t from stdin;\\.\n1\n\\.\nselect"),
            ["copy", "t", "from", "stdin", ";", "\\.\n1\n\\.", "select"]
        );
        // Without an end marker, data runs to the final line break. Only a line break means no data
        assert_eq!(
            texts("copy t from stdin;\n\n1\t\n\n"),
            ["copy", "t", "from", "stdin", ";", "\n\n1\t\n"]
        );
        assert_eq!(
            texts("copy t from stdin;\r\n"),
            ["copy", "t", "from", "stdin", ";"]
        );
        // No data unless it is FROM STDIN
        assert_eq!(
            texts("copy t to stdout;\n'a'"),
            ["copy", "t", "to", "stdout", ";", "'a'"]
        );
        // Data is not recognized inside a function body
        let body = tokenize_with_offset("copy t from stdin;\n'a'", 0);
        assert!(body.iter().all(|t| t.kind != TokenKind::CopyData));
    }

    #[test]
    fn empty_input() {
        assert!(tokenize("").is_empty());
    }

    #[test]
    fn leading_bom_is_whitespace() {
        assert_eq!(
            tokenize("\u{FEFF}select")
                .iter()
                .map(|t| (t.kind, t.text, t.offset))
                .collect::<Vec<_>>(),
            [
                (TokenKind::Whitespace, "\u{FEFF}", 0),
                (TokenKind::Ident, "select", 3),
            ]
        );
        assert_eq!(tokenize("\u{FEFF}").len(), 1);
        // Anywhere but the start it stays an identifier character
        assert_eq!(texts("a \u{FEFF}b"), ["a", "\u{FEFF}b"]);
    }

    #[test]
    fn offsets_are_byte_positions() {
        let tokens = tokenize("あ 1");
        let offsets: Vec<_> = tokens.iter().map(|t| t.offset).collect();
        assert_eq!(offsets, [0, 3, 4]);
    }

    #[test]
    fn identifiers() {
        assert_eq!(single("foo_Bar1$"), TokenKind::Ident);
        assert_eq!(single("_x"), TokenKind::Ident);
        assert_eq!(single("テーブル"), TokenKind::Ident);
        assert_eq!(texts("a.b"), ["a", ".", "b"]);
    }

    #[test]
    fn quoted_identifiers() {
        let ok = TokenKind::QuotedIdent { terminated: true };
        assert_eq!(single(r#""a ""b"" c""#), ok);
        assert_eq!(single(r#"U&"d\0061t""#), ok);
        assert_eq!(single(r#"u&"x""#), ok);
        assert_eq!(
            single(r#""abc"#),
            TokenKind::QuotedIdent { terminated: false }
        );
    }

    #[test]
    fn strings() {
        let s = |prefix| TokenKind::String {
            prefix,
            terminated: true,
        };
        assert_eq!(single("'it''s'"), s(StringPrefix::None));
        assert_eq!(single(r"'a\'"), s(StringPrefix::None));
        assert_eq!(single(r"E'it\'s'"), s(StringPrefix::Escape));
        assert_eq!(single(r"e'\\'"), s(StringPrefix::Escape));
        assert_eq!(single("B'1010'"), s(StringPrefix::Bit));
        assert_eq!(single("x'ff'"), s(StringPrefix::Hex));
        assert_eq!(single("N'abc'"), s(StringPrefix::National));
        assert_eq!(single(r"U&'d\0061t'"), s(StringPrefix::Unicode));
        assert_eq!(texts("E 'x'"), ["E", "'x'"]);
        assert_eq!(texts("U& 'x'"), ["U", "&", "'x'"]);
    }

    #[test]
    fn unterminated_strings_run_to_end() {
        assert_eq!(
            single("'abc\nselect 1"),
            TokenKind::String {
                prefix: StringPrefix::None,
                terminated: false
            }
        );
        assert_eq!(
            single("E'abc\\"),
            TokenKind::String {
                prefix: StringPrefix::Escape,
                terminated: false
            }
        );
    }

    #[test]
    fn dollar_quoted_strings() {
        let ok = TokenKind::DollarString { terminated: true };
        assert_eq!(single("$$ it's $x$ $$"), ok);
        assert_eq!(single("$fn$ select $$a$$ $fn$"), ok);
        assert_eq!(single("$_1$x$_1$"), ok);
        assert_eq!(texts("$a$x$a$;"), ["$a$x$a$", ";"]);
        assert_eq!(
            single("$fn$ never closed $$"),
            TokenKind::DollarString { terminated: false }
        );
    }

    #[test]
    fn params_and_lone_dollar() {
        assert_eq!(lex("$12"), [(TokenKind::Param, "$12")]);
        assert_eq!(
            lex("$ 1"),
            [(TokenKind::Unknown, "$"), (TokenKind::Number, "1")]
        );
        // A tag cannot start with a digit
        assert_eq!(lex("$1a$")[0], (TokenKind::Param, "$1"));
    }

    #[test]
    fn numbers() {
        for src in [
            "0",
            "42",
            "1_000_000",
            "3.14",
            "5.",
            ".5",
            "1e10",
            "1.5E-3",
            "2e+4",
            ".5e1",
            "0x1F",
            "0X_ff",
            "0o17",
            "0b1010",
            "0b_1",
        ] {
            assert_eq!(single(src), TokenKind::Number, "{src}");
        }
    }

    #[test]
    fn number_boundaries() {
        assert_eq!(texts("1..10"), ["1", "..", "10"]);
        assert_eq!(texts("1e"), ["1", "e"]);
        assert_eq!(texts("1e+"), ["1", "e", "+"]);
        assert_eq!(texts("1__0"), ["1", "__0"]);
        assert_eq!(texts("1_"), ["1", "_"]);
        assert_eq!(texts("0x"), ["0", "x"]);
        assert_eq!(texts("0b2"), ["0", "b2"]);
        assert_eq!(texts("0o8"), ["0", "o8"]);
        assert_eq!(texts("0xg"), ["0", "xg"]);
        assert_eq!(texts("1.x"), ["1.", "x"]);
        // In decimal, `_` cannot precede the digits
        assert_eq!(texts("1._5"), ["1.", "_5"]);
        assert_eq!(texts("1e_5"), ["1", "e_5"]);
    }

    #[test]
    fn punctuation() {
        let kinds: Vec<_> = lex("( ) [ ] , ; . .. : :: :=")
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        use TokenKind::*;
        assert_eq!(
            kinds,
            [
                LParen,
                RParen,
                LBracket,
                RBracket,
                Comma,
                Semicolon,
                Dot,
                DotDot,
                Colon,
                DoubleColon,
                ColonEquals
            ]
        );
        assert_eq!(texts("a::int"), ["a", "::", "int"]);
        assert_eq!(texts(":::"), ["::", ":"]);
    }

    #[test]
    fn operators() {
        for op in [
            "+", "<=", ">=", "<>", "!=", "=>", "||", "@>", "<@", "->", "->>", "#>>", "~~*", "!~",
            "&&", "-|-", "@-", "~-",
        ] {
            assert_eq!(single(op), TokenKind::Operator, "{op}");
        }
    }

    #[test]
    fn operator_trailing_sign_is_split() {
        assert_eq!(texts("a*-1"), ["a", "*", "-", "1"]);
        assert_eq!(texts("a=-1"), ["a", "=", "-", "1"]);
        assert_eq!(texts("a<>-1"), ["a", "<>", "-", "1"]);
        assert_eq!(texts("+-"), ["+", "-"]);
        assert_eq!(texts("<=+-"), ["<=", "+", "-"]);
        // An operator containing `~!@#^&|`?%` keeps its trailing +/-
        assert_eq!(texts("a@-1"), ["a", "@-", "1"]);
        assert_eq!(texts("a%-1"), ["a", "%-", "1"]);
    }

    #[test]
    fn operator_stops_before_comment() {
        assert_eq!(
            lex("a+--c\n"),
            [
                (TokenKind::Ident, "a"),
                (TokenKind::Operator, "+"),
                (TokenKind::LineComment, "--c"),
            ]
        );
        assert_eq!(texts("*/* c */"), ["*", "/* c */"]);
        // Even an operator that keeps its trailing +/- stops before `--`
        assert_eq!(texts("a@--c"), ["a", "@", "--c"]);
    }

    #[test]
    fn comments() {
        assert_eq!(
            lex("-- a\r\nb"),
            [(TokenKind::LineComment, "-- a"), (TokenKind::Ident, "b")]
        );
        assert_eq!(single("--"), TokenKind::LineComment);
        let ok = TokenKind::BlockComment { terminated: true };
        assert_eq!(single("/* a /* nested */ b */"), ok);
        assert_eq!(single("/**/"), ok);
        assert_eq!(texts("/* a */ */"), ["/* a */", "*/"]);
        assert_eq!(
            single("/* a /* b */"),
            TokenKind::BlockComment { terminated: false }
        );
    }

    #[test]
    fn whitespace_is_one_token() {
        let tokens = tokenize(" \t\r\n\x0b\x0c");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Whitespace);
    }

    #[test]
    fn unknown_characters() {
        assert_eq!(
            lex(r"\d {"),
            [
                (TokenKind::Unknown, "\\"),
                (TokenKind::Ident, "d"),
                (TokenKind::Unknown, "{"),
            ]
        );
    }
}
