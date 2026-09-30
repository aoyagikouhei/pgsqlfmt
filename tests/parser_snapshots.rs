//! `tests/fixtures/parser/*.sql` を構文解析し、構文木をスナップショットと比較する。
//! 新しいケースは `.sql` を追加して `make snapshot-review` で承認する。

use sql_formatter::parser::parse;

#[test]
fn parser_snapshots() {
    insta::glob!("fixtures/parser/*.sql", |path| {
        let src = std::fs::read_to_string(path).unwrap();
        let tree = parse(&src);
        assert_eq!(tree.text(), src, "木のテキストが入力と一致すること");
        insta::assert_snapshot!(tree.debug_tree());
    });
}
