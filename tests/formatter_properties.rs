//! Checks the properties of formatting on every fixture, on each truncation of it, and on each
//! version of it with one token removed.
//! - Every significant token survives, in order (only the case of identifiers may change)
//! - Every comment survives too, in order relative to the other comments
//!   (leading commas turn `a, -- x` into `a -- x` / `, b`, so the position relative to tokens
//!   may change)
//! - Formatting again changes nothing

use std::path::Path;

use pgsqlfmt::lexer::{Token, TokenKind, tokenize};
use pgsqlfmt::{CommaStyle, FormatOptions, KeywordCase, format, format_with_options};

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

/// Runs `check` on every variant of every fixture, in parallel on one thread per CPU.
fn for_each_input(check: impl Fn(&str) + Sync) {
    let inputs: Vec<String> = fixtures().iter().flat_map(|src| variants(src)).collect();
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk = inputs.len().div_ceil(threads);
    let check = &check;
    std::thread::scope(|scope| {
        for inputs in inputs.chunks(chunk) {
            scope.spawn(move || inputs.iter().for_each(|input| check(input)));
        }
    });
}

/// The original input, each truncation of it, and each version of it with one token removed
fn variants(src: &str) -> Vec<String> {
    let mut out = vec![src.to_string()];
    for token in tokenize(src) {
        let end = token.offset + token.text.len();
        out.push(src[..token.offset].to_string());
        out.push(format!("{}{}", &src[..token.offset], &src[end..]));
    }
    out
}

/// The token stream. Terminated dollar-quoted strings (function bodies etc., whose contents change
/// when formatted) are expanded into the delimiters and the tokens of the contents.
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

/// The tokens other than whitespace and comments. Identifiers are compared case-insensitively.
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
    for_each_input(|input| {
        let output = format(input);
        let context = format!("\n--- input ---\n{input}\n--- output ---\n{output}");
        assert_eq!(significant(&output), significant(input), "{context}");
        assert_eq!(comments(&output), comments(input), "{context}");
    });
}

/// Tokens and comments survive with a narrow width or non-default options too, and a second pass
/// changes nothing
#[test]
fn other_options_are_lossless_and_idempotent() {
    let option_sets = [
        FormatOptions {
            max_width: 20,
            ..FormatOptions::default()
        },
        FormatOptions {
            max_width: 30,
            indent_width: 2,
            keyword_case: KeywordCase::Lower,
            comma_style: CommaStyle::Trailing,
        },
        FormatOptions {
            max_width: 20,
            indent_width: 8,
            keyword_case: KeywordCase::Preserve,
            comma_style: CommaStyle::Leading,
        },
    ];
    for_each_input(|input| {
        for options in &option_sets {
            let once = format_with_options(input, options);
            let context = format!(
                "\n--- options ---\n{options:?}\n--- input ---\n{input}\n--- first pass ---\n{once}"
            );
            assert_eq!(significant(&once), significant(input), "{context}");
            assert_eq!(comments(&once), comments(input), "{context}");
            assert_eq!(format_with_options(&once, options), once, "{context}");
        }
    });
}

#[test]
fn formatting_is_idempotent() {
    for_each_input(|input| {
        let once = format(input);
        let twice = format(&once);
        assert_eq!(
            twice, once,
            "\n--- input ---\n{input}\n--- first pass ---\n{once}"
        );
    });
}
