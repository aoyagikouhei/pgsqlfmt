//! Parses `tests/fixtures/{parser,plpgsql}/*.sql` and compares the syntax trees with the snapshots.
//! To add a case, add a `.sql` file and approve it with `make snapshot-review`.

use std::path::Path;

use pgsqlfmt::parser::parse;

fn debug_tree(path: &Path) -> String {
    let src = std::fs::read_to_string(path).unwrap();
    let tree = parse(&src);
    assert_eq!(tree.text(), src, "the tree's text must match the input");
    tree.debug_tree()
}

#[test]
fn parser_snapshots() {
    insta::glob!("fixtures/parser/*.sql", |path| {
        insta::assert_snapshot!(debug_tree(path));
    });
}

#[test]
fn plpgsql_parser_snapshots() {
    insta::glob!("fixtures/plpgsql/*.sql", |path| {
        insta::assert_snapshot!(debug_tree(path));
    });
}
