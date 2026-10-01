//! `tests/fixtures/*/*.sql` を整形し、結果をスナップショットと比較する。
//! 新しいケースは `.sql` を追加して `make snapshot-review` で承認する。

use pgsqlfmt::format;

#[test]
fn formatter_snapshots() {
    insta::glob!("fixtures/*/*.sql", |path| {
        let src = std::fs::read_to_string(path).unwrap();
        insta::assert_snapshot!(format(&src));
    });
}
