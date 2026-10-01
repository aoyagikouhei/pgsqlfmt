//! `tests/fixtures/{parser,plpgsql}/*.sql` を構文解析し、構文木をスナップショットと比較する。
//! 新しいケースは `.sql` を追加して `make snapshot-review` で承認する。

use std::path::Path;

use pgsqlfmt::parser::parse;

fn debug_tree(path: &Path) -> String {
    let src = std::fs::read_to_string(path).unwrap();
    let tree = parse(&src);
    assert_eq!(tree.text(), src, "木のテキストが入力と一致すること");
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
