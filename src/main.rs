use std::io::{self, Read, Write};

fn main() -> io::Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    io::stdout().write_all(sql_formatter::format(&input).as_bytes())
}
