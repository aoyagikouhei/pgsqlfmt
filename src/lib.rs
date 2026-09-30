pub mod lexer;
pub mod parser;
pub mod syntax;

/// PostgreSQL の SQL / ストアドプロシージャを整形する。
///
/// まだ実装していないため、入力をそのまま返す。
pub fn format(sql: &str) -> String {
    sql.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_input_unchanged() {
        assert_eq!(format("SELECT 1"), "SELECT 1");
    }
}
