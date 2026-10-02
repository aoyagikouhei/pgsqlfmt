//! Formats `tests/fixtures/*/*.sql` and compares the output with the snapshots.
//! To add a case, add a `.sql` file and approve it with `make snapshot-review`.

use pgsqlfmt::format;

#[test]
fn formatter_snapshots() {
    insta::glob!("fixtures/*/*.sql", |path| {
        let src = std::fs::read_to_string(path).unwrap();
        insta::assert_snapshot!(format(&src));
    });
}
