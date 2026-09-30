//! 整形の性質を、すべてのフィクスチャと、それを途中で切ったもの・トークンを 1 つ抜いたもので確かめる。
//! - 意味のあるトークンが、順番どおりに 1 つも欠けずに残る（識別子の大文字小文字だけは変わってよい）
//! - コメントも、コメント同士の順番どおりに 1 つも欠けずに残る
//!   （行頭カンマにするとき `a, -- x` を `a -- x` / `, b` にするので、トークンとの前後は変わってよい）
//! - もう一度整形しても変わらない

use std::path::Path;

use sql_formatter::format;
use sql_formatter::lexer::{Token, TokenKind, tokenize};

fn fixtures() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut sources = Vec::new();
    for sub in std::fs::read_dir(dir).unwrap() {
        for entry in std::fs::read_dir(sub.unwrap().path()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "sql") {
                sources.push(std::fs::read_to_string(path).unwrap());
            }
        }
    }
    assert!(sources.len() >= 10);
    sources
}

/// 元の入力と、それを途中で切ったもの・トークンを 1 つ抜いたもの
fn variants(src: &str) -> Vec<String> {
    let mut out = vec![src.to_string()];
    for token in tokenize(src) {
        let end = token.offset + token.text.len();
        out.push(src[..token.offset].to_string());
        out.push(format!("{}{}", &src[..token.offset], &src[end..]));
    }
    out
}

/// トークン列。閉じたドル引用符（関数本体など、整形で中身が変わるもの）は、区切りと中身のトークンに展開する。
fn expanded_tokens(src: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    for token in tokenize(src) {
        if token.kind != (TokenKind::DollarString { terminated: true }) {
            out.push(token);
            continue;
        }
        let text = token.text;
        let delimiter_len = text[1..].find('$').unwrap() + 2;
        let close_start = text.len() - delimiter_len;
        let delimiter = |text| Token {
            kind: TokenKind::DollarDelimiter,
            text,
            offset: 0,
        };
        out.push(delimiter(&text[..delimiter_len]));
        out.extend(expanded_tokens(&text[delimiter_len..close_start]));
        out.push(delimiter(&text[close_start..]));
    }
    out
}

/// 空白・コメント以外のトークン。識別子は大文字小文字を区別しない。
fn significant(src: &str) -> Vec<(TokenKind, String)> {
    expanded_tokens(src)
        .into_iter()
        .filter(|t| !t.kind.is_trivia())
        .map(|t| {
            let text = if t.kind == TokenKind::Ident {
                t.text.to_ascii_lowercase()
            } else {
                t.text.to_string()
            };
            (t.kind, text)
        })
        .collect()
}

fn comments(src: &str) -> Vec<String> {
    expanded_tokens(src)
        .into_iter()
        .filter(|t| t.kind.is_trivia() && t.kind != TokenKind::Whitespace)
        .map(|t| t.text.to_string())
        .collect()
}

#[test]
fn tokens_and_comments_are_preserved() {
    for src in fixtures() {
        for input in variants(&src) {
            let output = format(&input);
            let context = format!("\n--- 入力 ---\n{input}\n--- 出力 ---\n{output}");
            assert_eq!(significant(&output), significant(&input), "{context}");
            assert_eq!(comments(&output), comments(&input), "{context}");
        }
    }
}

#[test]
fn formatting_is_idempotent() {
    for src in fixtures() {
        for input in variants(&src) {
            let once = format(&input);
            let twice = format(&once);
            assert_eq!(
                twice, once,
                "\n--- 入力 ---\n{input}\n--- 1 回目 ---\n{once}"
            );
        }
    }
}
