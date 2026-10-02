//! Checks that the parser neither panics nor hangs on broken input and that the tree's text still
//! matches the input. For every fixture, tries each truncated input and each input with one token removed.

use std::path::Path;

use pgsqlfmt::lexer::tokenize;
use pgsqlfmt::parser::parse;

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
    assert!(!sources.is_empty());
    sources
}

fn assert_lossless(src: &str) {
    let tree = parse(src);
    assert_eq!(tree.text(), src);
}

#[test]
fn every_truncation_parses_losslessly() {
    for src in fixtures() {
        for token in tokenize(&src) {
            assert_lossless(&src[..token.offset]);
        }
    }
}

#[test]
fn every_single_token_deletion_parses_losslessly() {
    for src in fixtures() {
        for token in tokenize(&src) {
            let end = token.offset + token.text.len();
            assert_lossless(&format!("{}{}", &src[..token.offset], &src[end..]));
        }
    }
}
