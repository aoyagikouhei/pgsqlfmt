//! CLI（`pgsqlfmt` バイナリ）を実際に起動して、ファイルの整形・確認の動きを確かめる。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const UNFORMATTED: &str = "select a, b from t where x = 1";
const FORMATTED: &str = "SELECT\n    a\n  , b\nFROM t\nWHERE x = 1\n";

/// テストごとの一時ディレクトリ（終わったら消す）
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("pgsqlfmt-cli-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn file(&self, name: &str, content: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str], stdin: &str, cwd: &Path) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pgsqlfmt"))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn formats_stdin_to_stdout() {
    let dir = TempDir::new();
    let output = run(&[], UNFORMATTED, &dir.0);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), FORMATTED);

    // `-` も標準入力。行幅も指定できる
    let output = run(&["-w", "10", "-"], "select f(1, 2)", &dir.0);
    assert_eq!(stdout(&output), "SELECT f(\n    1\n  , 2\n)\n");
}

#[test]
fn formats_a_file_to_stdout_without_changing_it() {
    let dir = TempDir::new();
    let file = dir.file("q.sql", UNFORMATTED);
    let output = run(&["q.sql"], "", &dir.0);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), FORMATTED);
    assert_eq!(read(&file), UNFORMATTED);
}

#[test]
fn write_rewrites_only_unformatted_files() {
    let dir = TempDir::new();
    let unformatted = dir.file("a.sql", UNFORMATTED);
    let formatted = dir.file("b.sql", FORMATTED);
    let output = run(&["--write", "a.sql", "b.sql"], "", &dir.0);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(read(&unformatted), FORMATTED);
    assert_eq!(read(&formatted), FORMATTED);
    assert_eq!(stderr(&output), "整形しました: a.sql\n");
    assert_eq!(stdout(&output), "");
}

#[test]
fn check_reports_unformatted_files_without_changing_them() {
    let dir = TempDir::new();
    let unformatted = dir.file("a.sql", UNFORMATTED);
    dir.file("b.sql", FORMATTED);
    let output = run(&["--check", "a.sql", "b.sql"], "", &dir.0);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        "整形されていません: a.sql\n1 個のファイルが整形されていません\n"
    );
    assert_eq!(read(&unformatted), UNFORMATTED);

    let output = run(&["--check", "b.sql"], "", &dir.0);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stderr(&output), "");

    // 標準入力も確かめられる
    let output = run(&["--check"], UNFORMATTED, &dir.0);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        run(&["--check", "-"], FORMATTED, &dir.0).status.code(),
        Some(0)
    );
}

#[test]
fn crlf_and_bom_files_keep_their_form() {
    let dir = TempDir::new();
    let crlf = FORMATTED.replace('\n', "\r\n");
    let bom = format!("\u{FEFF}{FORMATTED}");
    let crlf_file = dir.file("crlf.sql", &crlf);
    let bom_file = dir.file("bom.sql", &bom);
    // 整形済みなら --check は通り、--write は書き換えない
    let output = run(&["--check", "crlf.sql", "bom.sql"], "", &dir.0);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let output = run(&["--write", "crlf.sql", "bom.sql"], "", &dir.0);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(read(&crlf_file), crlf);
    assert_eq!(read(&bom_file), bom);

    // 未整形なら、改行コードと BOM を保ったまま整形する
    let file = dir.file(
        "u.sql",
        &format!("\u{FEFF}{}", UNFORMATTED.replace(' ', "\r\n")),
    );
    let output = run(&["--write", "u.sql"], "", &dir.0);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(read(&file), format!("\u{FEFF}{crlf}"));
}

#[cfg(unix)]
#[test]
fn write_keeps_file_permissions_and_leaves_no_temp_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new();
    let file = dir.file("a.sql", UNFORMATTED);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let output = run(&["--write", "a.sql"], "", &dir.0);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(read(&file), FORMATTED);
    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let names: Vec<_> = std::fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["a.sql"]);
}

#[test]
fn directories_are_searched_for_sql_files() {
    let dir = TempDir::new();
    dir.file("src/a.sql", UNFORMATTED);
    dir.file("src/sub/b.sql", UNFORMATTED);
    dir.file("src/sub/c.sql", FORMATTED);
    dir.file("src/.hidden/d.sql", UNFORMATTED);
    dir.file("src/note.txt", UNFORMATTED);
    // 拡張子は大文字小文字を区別しない
    dir.file("src/e.SQL", UNFORMATTED);
    let output = run(&["--check", "src"], "", &dir.0);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        "整形されていません: src/a.sql\n整形されていません: src/e.SQL\n整形されていません: src/sub/b.sql\n3 個のファイルが整形されていません\n"
    );

    let output = run(&["--write", "src"], "", &dir.0);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(read(&dir.0.join("src/sub/b.sql")), FORMATTED);
    assert_eq!(read(&dir.0.join("src/e.SQL")), FORMATTED);
    assert_eq!(read(&dir.0.join("src/.hidden/d.sql")), UNFORMATTED);
    assert_eq!(read(&dir.0.join("src/note.txt")), UNFORMATTED);
}

#[test]
fn missing_files_are_reported_and_the_rest_are_processed() {
    let dir = TempDir::new();
    let file = dir.file("a.sql", UNFORMATTED);
    let output = run(&["--write", "missing.sql", "a.sql"], "", &dir.0);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).starts_with("missing.sql を読めませんでした: "));
    assert_eq!(read(&file), FORMATTED);
}

#[test]
fn usage_errors() {
    let dir = TempDir::new();
    dir.file("a.sql", UNFORMATTED);
    dir.file("b.sql", UNFORMATTED);
    for args in [
        &["a.sql", "b.sql"][..],
        &["--write", "--check", "a.sql"],
        &["--unknown"],
        &["--max-width"],
        &["--max-width", "x"],
        &["--indent", "1"],
        &["--indent", "9"],
        &["--keyword-case", "title"],
        &["--comma", "middle"],
        &["--comma"],
    ] {
        let output = run(args, "", &dir.0);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(stderr(&output).contains("使い方:"), "{args:?}");
    }
    let output = run(&["--help"], "", &dir.0);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).starts_with("使い方:"));
}

#[test]
fn files_in_directories_are_processed_in_name_order() {
    let dir = TempDir::new();
    // ディレクトリを読む順番はファイルシステムによって違うので、名前順に並べて処理する
    let names = ["m", "z", "c", "k", "f", "a", "x", "q"];
    for name in names {
        dir.file(&format!("d/{name}.sql"), UNFORMATTED);
    }
    let output = run(&["--check", "d"], "", &dir.0);
    let mut sorted = names;
    sorted.sort();
    let expected: String = sorted
        .iter()
        .map(|name| format!("整形されていません: d/{name}.sql\n"))
        .collect();
    assert_eq!(
        stderr(&output),
        format!("{expected}8 個のファイルが整形されていません\n")
    );
}

#[test]
fn style_options() {
    let dir = TempDir::new();
    let output = run(
        &[
            "--indent",
            "2",
            "--keyword-case",
            "lower",
            "--comma",
            "trailing",
        ],
        UNFORMATTED,
        &dir.0,
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "select\n  a,\n  b\nfrom t\nwhere x = 1\n");
    let output = run(
        &["--keyword-case", "preserve", "--comma", "leading"],
        UNFORMATTED,
        &dir.0,
    );
    assert_eq!(
        stdout(&output),
        "select\n    a\n  , b\nfrom t\nwhere x = 1\n"
    );
}
