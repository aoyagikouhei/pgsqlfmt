//! Tokenizes `tests/fixtures/lexer/*.sql` and compares the token streams with the snapshots.
//! To add a case, add a `.sql` file and approve it with `cargo insta test --review`.

use pgsqlfmt::lexer::{TokenKind, tokenize};

#[test]
fn lexer_snapshots() {
    insta::glob!("fixtures/lexer/*.sql", |path| {
        let src = std::fs::read_to_string(path).unwrap();
        let tokens = tokenize(&src);
        assert_eq!(
            tokens.iter().map(|t| t.text).collect::<String>(),
            src,
            "concatenating the tokens must reproduce the input"
        );
        let dump = tokens
            .iter()
            .filter(|t| t.kind != TokenKind::Whitespace)
            .map(|t| format!("{:>5} {:?} {:?}", t.offset, t.kind, t.text))
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!(dump);
    });
}
