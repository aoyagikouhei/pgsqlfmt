use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sql_formatter::{FormatOptions, format_with_options};

const USAGE: &str = "\
使い方: sql-formatter [オプション] [ファイルまたはディレクトリ ...]

SQL を整形する。ファイルを指定しなければ標準入力を整形して標準出力に書き出す。
ディレクトリを指定すると、その下の *.sql を探す（. で始まるものは除く）。
`-` は標準入力を表す。

オプション:
      --write        ファイルを整形結果で上書きする
      --check        整形されていないファイルがあれば一覧を出して終了コード 1 で終わる（書き換えない）
  -w, --max-width N  行幅（既定: 80）
  -h, --help         この説明を表示する

終了コード: 0 = 成功、1 = --check で整形されていないファイルがあった、2 = 引数や読み書きのエラー
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// 整形結果を標準出力に書く
    Print,
    Write,
    Check,
}

#[derive(Debug)]
struct Args {
    options: FormatOptions,
    mode: Mode,
    paths: Vec<String>,
}

enum Input {
    Stdin,
    File(PathBuf),
}

impl Input {
    fn name(&self) -> String {
        match self {
            Input::Stdin => "<標準入力>".to_string(),
            Input::File(path) => path.display().to_string(),
        }
    }

    fn read(&self) -> io::Result<String> {
        match self {
            Input::Stdin => {
                let mut text = String::new();
                io::stdin().read_to_string(&mut text)?;
                Ok(text)
            }
            Input::File(path) => std::fs::read_to_string(path),
        }
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        options: FormatOptions::default(),
        mode: Mode::Print,
        paths: Vec::new(),
    };
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-w" | "--max-width" => {
                let value = args.next().ok_or(format!("{arg} に値がありません"))?;
                parsed.options.max_width = value
                    .parse()
                    .map_err(|_| format!("{arg} の値が数値ではありません: {value}"))?;
            }
            "--write" | "--check" => {
                let mode = if arg == "--write" {
                    Mode::Write
                } else {
                    Mode::Check
                };
                if parsed.mode != Mode::Print && parsed.mode != mode {
                    return Err("--write と --check は同時に指定できません".to_string());
                }
                parsed.mode = mode;
            }
            "-" => parsed.paths.push(arg),
            _ if arg.starts_with('-') => return Err(format!("不明なオプションです: {arg}")),
            _ => parsed.paths.push(arg),
        }
    }
    Ok(parsed)
}

/// 引数のパスを入力の並びにする。ディレクトリはその下の *.sql を名前順に探す。
fn collect_inputs(paths: &[String]) -> Result<Vec<Input>, String> {
    if paths.is_empty() {
        return Ok(vec![Input::Stdin]);
    }
    let mut inputs = Vec::new();
    for path in paths {
        if path == "-" {
            inputs.push(Input::Stdin);
            continue;
        }
        let path = Path::new(path);
        if path.is_dir() {
            let mut files = Vec::new();
            find_sql_files(path, &mut files)
                .map_err(|e| format!("{} を読めませんでした: {e}", path.display()))?;
            files.sort();
            inputs.extend(files.into_iter().map(Input::File));
        } else {
            inputs.push(Input::File(path.to_path_buf()));
        }
    }
    Ok(inputs)
}

fn find_sql_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        // シンボリックリンクのディレクトリはたどらない（循環を避けるため）
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            find_sql_files(&path, files)?;
        } else if path.extension().is_some_and(|e| e == "sql") && path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let usage_error = |message: String| {
        eprint!("{message}\n\n{USAGE}");
        ExitCode::from(2)
    };
    let args = match parse_args(args.into_iter()) {
        Ok(args) => args,
        Err(message) => return usage_error(message),
    };
    let inputs = match collect_inputs(&args.paths) {
        Ok(inputs) => inputs,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    if args.mode == Mode::Print && inputs.len() > 1 {
        return usage_error(
            "複数のファイルを整形するときは --write か --check を指定してください".to_string(),
        );
    }

    let mut failed = false;
    let mut unformatted = 0;
    let mut stdout = io::stdout().lock();
    for input in &inputs {
        let src = match input.read() {
            Ok(src) => src,
            Err(e) => {
                eprintln!("{} を読めませんでした: {e}", input.name());
                failed = true;
                continue;
            }
        };
        let formatted = format_with_options(&src, &args.options);
        let result = match (args.mode, input) {
            (Mode::Check, _) => {
                if formatted != src {
                    eprintln!("整形されていません: {}", input.name());
                    unformatted += 1;
                }
                Ok(())
            }
            (Mode::Write, Input::File(path)) => {
                if formatted == src {
                    Ok(())
                } else {
                    std::fs::write(path, &formatted)
                        .map(|()| eprintln!("整形しました: {}", input.name()))
                }
            }
            // 標準入力は --write でも標準出力に書く
            (Mode::Print | Mode::Write, _) => stdout.write_all(formatted.as_bytes()),
        };
        if let Err(e) = result {
            eprintln!("{} を書けませんでした: {e}", input.name());
            failed = true;
        }
    }

    if unformatted > 0 {
        eprintln!("{unformatted} 個のファイルが整形されていません");
    }
    if failed {
        ExitCode::from(2)
    } else if unformatted > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
