//! `tests/fixtures/lexer/*.sql` を字句解析し、トークン列をスナップショットと比較する。
//! 新しいケースは `.sql` を追加して `cargo insta test --review` で承認する。

use pgsqlfmt::lexer::{TokenKind, tokenize};

#[test]
fn lexer_snapshots() {
    insta::glob!("fixtures/lexer/*.sql", |path| {
        let src = std::fs::read_to_string(path).unwrap();
        let tokens = tokenize(&src);
        assert_eq!(
            tokens.iter().map(|t| t.text).collect::<String>(),
            src,
            "トークンをつなげると入力に戻ること"
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
