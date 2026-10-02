use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pgsqlfmt::{CommaStyle, FormatOptions, KeywordCase, format_with_options};

const USAGE: &str = "\
Usage: pgsqlfmt [options] [file or directory ...]

Formats SQL. Without files, reads standard input and writes the result to standard output.
A directory is searched for *.sql files under it (names starting with . are skipped).
`-` means standard input.

Options:
      --write        Overwrite files with the formatted result
      --check        List files that are not formatted and exit with code 1 (does not rewrite)
  -w, --max-width N  Line width (default: 80)
      --indent N     Indent width, 2 to 8 (default: 4)
      --keyword-case upper|lower|preserve
                     Write keywords in upper case, lower case, or as in the input (default: upper)
      --comma leading|trailing
                     Put commas at the start or end of lines when listing items one per line (default: leading)
  -h, --help         Show this help

Exit codes: 0 = success, 1 = --check found unformatted files, 2 = argument or I/O error
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Write the formatted result to standard output
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
            Input::Stdin => "<stdin>".to_string(),
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
                let value = args.next().ok_or(format!("{arg} requires a value"))?;
                parsed.options.max_width = value
                    .parse()
                    .map_err(|_| format!("{arg} must be a number: {value}"))?;
            }
            "--indent" => {
                let value = args.next().ok_or(format!("{arg} requires a value"))?;
                parsed.options.indent_width = value
                    .parse()
                    .ok()
                    .filter(|n| (2..=8).contains(n))
                    .ok_or(format!("{arg} must be a number from 2 to 8: {value}"))?;
            }
            "--keyword-case" => {
                let value = args.next().ok_or(format!("{arg} requires a value"))?;
                parsed.options.keyword_case = match value.as_str() {
                    "upper" => KeywordCase::Upper,
                    "lower" => KeywordCase::Lower,
                    "preserve" => KeywordCase::Preserve,
                    _ => {
                        return Err(format!(
                            "{arg} must be one of upper / lower / preserve: {value}"
                        ));
                    }
                };
            }
            "--comma" => {
                let value = args.next().ok_or(format!("{arg} requires a value"))?;
                parsed.options.comma_style = match value.as_str() {
                    "leading" => CommaStyle::Leading,
                    "trailing" => CommaStyle::Trailing,
                    _ => {
                        return Err(format!("{arg} must be leading or trailing: {value}"));
                    }
                };
            }
            "--write" | "--check" => {
                let mode = if arg == "--write" {
                    Mode::Write
                } else {
                    Mode::Check
                };
                if parsed.mode != Mode::Print && parsed.mode != mode {
                    return Err("--write and --check cannot be used together".to_string());
                }
                parsed.mode = mode;
            }
            "-" => parsed.paths.push(arg),
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ => parsed.paths.push(arg),
        }
    }
    Ok(parsed)
}

/// Turn the path arguments into a list of inputs. A directory is searched for the *.sql files
/// under it, in name order.
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
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
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
        // `file_type()` does not follow links, so symlinks to directories are not descended
        // into (avoids cycles). Links to files are included, because `is_file()` below does
        // follow them.
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            find_sql_files(&path, files)?;
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("sql"))
            && path.is_file()
        {
            files.push(path);
        }
    }
    Ok(())
}

/// Write to a temporary file in the same directory, then replace the original. If interrupted
/// midway, the original file is left intact. Permissions are copied from the original file.
fn write_atomically(path: &Path, content: &str) -> io::Result<()> {
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file name is missing"))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(file_name);
    tmp_name.push(".pgsqlfmt-tmp");
    let tmp = path.with_file_name(tmp_name);
    let result = (|| {
        std::fs::write(&tmp, content)?;
        std::fs::set_permissions(&tmp, std::fs::metadata(path)?.permissions())?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
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
            "specify --write or --check when formatting multiple files".to_string(),
        );
    }

    let mut failed = false;
    let mut unformatted = 0;
    let mut stdout = io::stdout().lock();
    for input in &inputs {
        let src = match input.read() {
            Ok(src) => src,
            Err(e) => {
                eprintln!("cannot read {}: {e}", input.name());
                failed = true;
                continue;
            }
        };
        let formatted = format_with_options(&src, &args.options);
        let result = match (args.mode, input) {
            (Mode::Check, _) => {
                if formatted != src {
                    eprintln!("not formatted: {}", input.name());
                    unformatted += 1;
                }
                Ok(())
            }
            (Mode::Write, Input::File(path)) => {
                if formatted == src {
                    Ok(())
                } else {
                    write_atomically(path, &formatted)
                        .map(|()| eprintln!("formatted: {}", input.name()))
                }
            }
            // Standard input is written to standard output even with --write
            (Mode::Print | Mode::Write, _) => stdout.write_all(formatted.as_bytes()),
        };
        if let Err(e) = result {
            eprintln!("cannot write {}: {e}", input.name());
            failed = true;
        }
    }

    if unformatted > 0 {
        eprintln!("{unformatted} file(s) not formatted");
    }
    if failed {
        ExitCode::from(2)
    } else if unformatted > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
