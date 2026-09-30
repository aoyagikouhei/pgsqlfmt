use std::io::{self, Read, Write};
use std::process::ExitCode;

use sql_formatter::{FormatOptions, format_with_options};

const USAGE: &str = "\
使い方: sql-formatter [--max-width N] < input.sql

標準入力の SQL を整形して標準出力に書き出す。

オプション:
  -w, --max-width N  行幅（既定: 80）
  -h, --help         この説明を表示する
";

fn parse_args(args: impl Iterator<Item = String>) -> Result<FormatOptions, String> {
    let mut options = FormatOptions::default();
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-w" | "--max-width" => {
                let value = args.next().ok_or(format!("{arg} に値がありません"))?;
                options.max_width = value
                    .parse()
                    .map_err(|_| format!("{arg} の値が数値ではありません: {value}"))?;
            }
            _ => return Err(format!("不明な引数です: {arg}")),
        }
    }
    Ok(options)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let options = match parse_args(args.into_iter()) {
        Ok(options) => options,
        Err(message) => {
            eprint!("{message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let mut input = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        eprintln!("標準入力を読めませんでした: {e}");
        return ExitCode::FAILURE;
    }
    let output = format_with_options(&input, &options);
    if let Err(e) = io::stdout().write_all(output.as_bytes()) {
        eprintln!("標準出力に書けませんでした: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
