//! PostgreSQL の字句解析器。
//!
//! 空白やコメントも含め、入力のすべてのバイトをいずれかのトークンに割り当てる。
//! そのためトークンの `text` を順につなげると入力と一致する（ロスレス）。
//! 字句規則は PostgreSQL の `src/backend/parser/scan.l` に合わせている。
//! 不正な入力でもエラーにせず、閉じていない文字列などは `terminated: false` として最後まで読む。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// 空白・改行
    Whitespace,
    /// `-- ...`（末尾の改行は含まない）
    LineComment,
    /// `/* ... */`（入れ子にできる）
    BlockComment {
        terminated: bool,
    },
    /// 引用符なしの識別子（キーワードを含む）
    Ident,
    /// キーワードとして読んだ識別子。字句解析器は出さず、パーサーが `Ident` から付け替える
    Keyword,
    /// `"..."` / `U&"..."`
    QuotedIdent {
        terminated: bool,
    },
    /// `'...'` と接頭辞付きの文字列（`E'...'` など）
    String {
        prefix: StringPrefix,
        terminated: bool,
    },
    /// `$$...$$` / `$tag$...$tag$`
    DollarString {
        terminated: bool,
    },
    /// 中身を解析したドル引用符の `$tag$`。字句解析器は出さず、パーサーが `DollarString` を分けて作る
    DollarDelimiter,
    Number,
    /// `$1` などの位置パラメーター
    Param,
    /// `+` `<=` `@>` `->>` などの演算子
    Operator,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Dot,
    /// `..`（PL/pgSQL の `FOR i IN 1..10`）
    DotDot,
    Colon,
    /// `::`（型キャスト）
    DoubleColon,
    /// `:=`（PL/pgSQL の代入）
    ColonEquals,
    /// どの規則にも当てはまらない 1 文字
    Unknown,
    /// psql の変数 `:name` / `:'name'` / `:"name"`。psql が置き換えるので 1 つの名前として扱う。
    /// 関数本体の中（psql は置き換えない）と、`a[1:n]` のように名前や数字の直後の `:` では読まない
    PsqlVariable,
    /// `COPY ... FROM STDIN;` の直後から `\.` だけの行までのデータ（`;` と同じ行の残りを含む）
    CopyData,
}

impl TokenKind {
    /// 空白・コメント。構文上の意味を持たない
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
    /// `E'...'`（バックスラッシュでエスケープする）
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
    /// 入力先頭からのバイト位置
    pub offset: usize,
}

/// `src` の一部を字句解析する。トークンの位置は `base` を足した、元の入力での位置になる。
/// 関数本体などの一部なので、COPY のデータと psql の変数は読まない。
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

/// `top_level` は入力全体（psql に渡すスクリプト）か。COPY のデータと psql の変数はそこでだけ読む
fn scan_all(src: &str, top_level: bool) -> Vec<Token<'_>> {
    let mut lexer = Lexer {
        src,
        bytes: src.as_bytes(),
        pos: 0,
        psql_variables: top_level,
    };
    let mut tokens = Vec::new();
    while lexer.pos < src.len() {
        let start = lexer.pos;
        let kind = lexer.scan();
        // `COPY ... FROM STDIN;` の直後からデータの終わりまでを 1 つのトークンにする。
        // 最後の改行のほかに何も残っていなければデータはない
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

/// `tokens` の最後の文が `COPY ... FROM STDIN` か。括弧の中（`COPY (SELECT ... FROM stdin) TO ...`）は見ない
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

/// 最後の改行（`\n` か `\r\n`）を 1 つ除く。整形結果の最後にはいつも改行が付くので
fn strip_last_newline(text: &str) -> &str {
    let text = text.strip_suffix('\n').unwrap_or(text);
    text.strip_suffix('\r').unwrap_or(text)
}

// 区切りはすべて ASCII なので、バイト単位で進めても UTF-8 の文字の途中で切れることはない。
// 非 ASCII のバイトは識別子の文字として扱われる（scan.l の `\200-\377` と同じ）。
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

    /// COPY のデータを `\.` だけの行（`\r` が付いてもよい）まで読む。なければ入力の最後の改行の前まで
    /// （行末のタブは空の列、空行は空の行なので、空白も削らない）。
    /// psql と同じく、データは `;` の次の行から始まる。`;` と同じ行の残りもデータに含めて、
    /// 整形でデータの始まる位置が変わらないようにする
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

    /// `:` から psql の変数が始まるか
    fn at_psql_variable(&self) -> bool {
        let prev = self.pos.checked_sub(1).map(|i| self.bytes[i]);
        let after_value = prev
            .is_some_and(|p| is_ident_cont(p) || matches!(p, b')' | b']' | b'[' | b'\'' | b'"'));
        self.psql_variables && !after_value && self.psql_variable_follows()
    }

    /// 現在の `:` の直後が、変数の名前か、後ろで閉じている引用符か
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
    /// `:a:b` のように続けて書いた変数は、psql がそれぞれを置き換えてつなぐので 1 つのトークンにする
    fn psql_variable(&mut self) -> TokenKind {
        loop {
            self.pos += 1;
            match self.bytes[self.pos] {
                quote @ (b'\'' | b'"') => {
                    // 引用符を重ねた `:'it''s'` は 1 つの名前
                    self.pos += 1;
                    loop {
                        self.eat_while(|b| b != quote);
                        self.pos += 1;
                        if self.peek(0) != Some(quote) {
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

    /// 次のトークンを 1 つ読み、その種類を返す。`pos` は必ず進む。
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

    /// `pos` は開きの `'` を指していること。
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

    /// `pos` は開きの `"` を指していること。
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

    /// `$1`、`$$...$$`、`$tag$...$tag$` のいずれか。どれでもなければ `$` 1 文字を `Unknown` にする。
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

    /// `pos` から始まる `$tag$` の長さ。タグは省略でき、数字から始められない。
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
            // `1..10` の `..` は範囲なので、小数点として読まない
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

    /// `1_000` のように、数字の間に `_` を 1 つずつ挟める数字列を読む。
    /// `0x_FF` のように先頭に `_` を置けるのは基数付きのときだけ。1 桁以上読めたら true。
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

    /// 演算子を読む。scan.l と同じく、`--` / `/*` の手前で切り、
    /// `~!@#^&|`?%` を含まない演算子の末尾の `+` / `-` は切り離す（`*-1` を `*` と `-1` にするため）。
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

    /// 空白を除いた (種類, テキスト) の列
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
        // 閉じていない引用符は変数にしない
        assert_eq!(lex(":'a")[0], (TokenKind::Colon, ":"));
        // 関数本体の一部では読まない
        let body = tokenize_with_offset("x := :a", 0);
        assert!(body.iter().all(|t| t.kind != TokenKind::PsqlVariable));
    }

    #[test]
    fn copy_data() {
        // `;` と同じ行の残り（ここではコメント）もデータ。終わりの印は次の行から探す
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
        // psql は `;` と同じ行の `\.` をデータの終わりにしない
        assert_eq!(
            texts("copy t from stdin;\\.\n1\n\\.\nselect"),
            ["copy", "t", "from", "stdin", ";", "\\.\n1\n\\.", "select"]
        );
        // 終わりの印がなければ最後の改行の前まで。改行しかなければデータはない
        assert_eq!(
            texts("copy t from stdin;\n\n1\t\n\n"),
            ["copy", "t", "from", "stdin", ";", "\n\n1\t\n"]
        );
        assert_eq!(
            texts("copy t from stdin;\r\n"),
            ["copy", "t", "from", "stdin", ";"]
        );
        // FROM STDIN でなければデータはない
        assert_eq!(
            texts("copy t to stdout;\n'a'"),
            ["copy", "t", "to", "stdout", ";", "'a'"]
        );
        // 関数本体の一部ではデータを読まない
        let body = tokenize_with_offset("copy t from stdin;\n'a'", 0);
        assert!(body.iter().all(|t| t.kind != TokenKind::CopyData));
    }

    #[test]
    fn empty_input() {
        assert!(tokenize("").is_empty());
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
        // タグは数字から始められない
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
        // 10 進数では数字の前に `_` を置けない
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
        // `~!@#^&|`?%` を含む演算子は末尾の +/- ごと 1 つ
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
        // 末尾の +/- を切り離さない演算子でも、`--` の手前で切る
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
