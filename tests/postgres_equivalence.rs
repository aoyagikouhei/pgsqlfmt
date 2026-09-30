//! PostgreSQL で、整形の前と後の SQL が同じ意味になることを確かめる。
//!
//! フィクスチャの文を 1 つずつ、`tests/postgres/schema.sql` を流したトランザクションの中で実行し、
//! psql の出力を整形の前後で比べる。最後は ROLLBACK するので DB には何も残らない。
//! - SELECT / INSERT / UPDATE / DELETE: `EXPLAIN (VERBOSE, COSTS OFF, GENERIC_PLAN)` の実行計画と、実行した結果
//! - CREATE FUNCTION / PROCEDURE: 作成できるかと、本体のテキスト以外のカタログ上の定義
//!   （PL/pgSQL の本体は作成時に構文が検査され、呼び出しの結果は後続の文で比べる）
//! - それ以外（DO / CALL など）: 実行した結果（NOTICE やエラーを含む）
//!
//! psql は `ON_ERROR_ROLLBACK=on` で動かすので、エラーになった文があっても続きの文を比べられる。
//! 環境変数 `PGHOST` がなければ（dev コンテナの外では）このテストは何もしない。

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use sql_formatter::lexer::TokenKind;
use sql_formatter::parser::parse;
use sql_formatter::syntax::{Element, Node, NodeKind};
use sql_formatter::{FormatOptions, format_with_options};

fn postgres_available() -> bool {
    if std::env::var_os("PGHOST").is_none() {
        eprintln!("PGHOST がないので PostgreSQL での検証を飛ばします");
        return false;
    }
    true
}

fn fixtures(sub: Option<&str>) -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if sub.is_some_and(|s| !path.ends_with(s)) {
            continue;
        }
        for file in std::fs::read_dir(&path).unwrap() {
            let file = file.unwrap().path();
            if file.extension().is_some_and(|e| e == "sql") {
                let name = file.strip_prefix(&dir).unwrap().display().to_string();
                out.push((name, std::fs::read_to_string(&file).unwrap()));
            }
        }
    }
    out.sort();
    out
}

/// CREATE FUNCTION の関数名（`schema.name` なら両方）。引用符なしの名前は小文字にする。
fn function_name(stmt: &Node) -> (String, String) {
    let mut parts = Vec::new();
    let mut after_keyword = false;
    for child in &stmt.children {
        match child {
            Element::Token(t) if t.kind == TokenKind::Keyword => {
                let word = t.text.to_ascii_lowercase();
                after_keyword = word == "function" || word == "procedure";
            }
            Element::Token(t) if after_keyword && t.kind == TokenKind::Ident => {
                parts.push(t.text.to_ascii_lowercase());
            }
            Element::Token(t)
                if after_keyword && matches!(t.kind, TokenKind::QuotedIdent { .. }) =>
            {
                parts.push(t.text.trim_matches('"').replace("\"\"", "\""));
            }
            Element::Node(_) if after_keyword => break,
            _ => {}
        }
    }
    let name = parts.pop().unwrap_or_default();
    (parts.pop().unwrap_or_else(|| "public".to_string()), name)
}

fn quote_literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// フィクスチャを検証用の psql スクリプトにする
fn script(src: &str) -> String {
    let root = parse(src);
    let mut out = String::from("BEGIN;\n");
    out.push_str(include_str!("postgres/schema.sql"));
    out.push('\n');
    let mut n = 0;
    for child in &root.children {
        let Element::Node(stmt) = child else {
            continue;
        };
        n += 1;
        let text = stmt.text();
        out.push_str(&format!("\\echo '--- 文 {n}'\n"));
        match stmt.kind {
            NodeKind::SelectStmt
            | NodeKind::InsertStmt
            | NodeKind::UpdateStmt
            | NodeKind::DeleteStmt => {
                out.push_str(&format!(
                    "EXPLAIN (VERBOSE, COSTS OFF, GENERIC_PLAN)\n{text}\n;\n"
                ));
                out.push_str(&format!("{text}\n;\n"));
            }
            NodeKind::CreateFunctionStmt => {
                out.push_str(&format!("{text}\n;\n"));
                let (schema, name) = function_name(stmt);
                out.push_str(&format!(
                    "SELECT p.proname, pg_get_function_arguments(p.oid), pg_get_function_result(p.oid), \
                     l.lanname, p.prokind, p.provolatile, p.proisstrict, p.prosecdef, p.proleakproof, \
                     p.proparallel, p.procost, p.prorows, p.proconfig, pg_get_function_sqlbody(p.oid) \
                     FROM pg_proc p JOIN pg_language l ON l.oid = p.prolang \
                     WHERE p.proname = {} AND p.pronamespace = to_regnamespace({}) ORDER BY p.oid;\n",
                    quote_literal(&name),
                    quote_literal(&schema)
                ));
            }
            _ => out.push_str(&format!("{text}\n;\n")),
        }
    }
    out.push_str("ROLLBACK;\n");
    out
}

/// psql でスクリプトを流し、出力（エラーや NOTICE を含む）を返す
fn run_psql(script: &str) -> String {
    let mut child = Command::new("sh")
        .args([
            "-c",
            "psql -X -v ON_ERROR_ROLLBACK=on -v VERBOSITY=terse -P pager=off -f - 2>&1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("psql を起動できない");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        !text.contains("could not connect") && !text.contains("connection to server"),
        "PostgreSQL に接続できない:\n{text}"
    );
    // エラーの位置（`psql:<stdin>:12: ERROR: ... at character 210`）は整形で変わるので消す
    text.lines()
        .map(|line| {
            let line = match line.strip_prefix("psql:<stdin>:") {
                Some(rest) => rest.split_once(": ").map_or(rest, |(_, msg)| msg),
                None => line,
            };
            line.split_once(" at character ")
                .map_or(line, |(msg, _)| msg)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 整形の前後で、psql の出力が同じか
fn assert_equivalent(name: &str, src: &str, max_width: usize) -> String {
    let formatted = format_with_options(src, &FormatOptions { max_width });
    let original_root = parse(src);
    let formatted_root = parse(&formatted);
    let statements = |root: &Node| {
        root.children
            .iter()
            .filter(|c| matches!(c, Element::Node(_)))
            .count()
    };
    assert_eq!(
        statements(&formatted_root),
        statements(&original_root),
        "{name}: 文の数が変わった"
    );

    let original = run_psql(&script(src));
    let after = run_psql(&script(&formatted));
    assert_eq!(
        after, original,
        "{name}（行幅 {max_width}）: 整形の前後で PostgreSQL の結果が違う\n--- 整形後 ---\n{formatted}"
    );
    original
}

#[test]
fn formatted_fixtures_behave_the_same_in_postgres() {
    if !postgres_available() {
        return;
    }
    for (name, src) in fixtures(None) {
        for max_width in [80, 20] {
            assert_equivalent(&name, &src, max_width);
        }
    }
}

/// 検証用のフィクスチャは、整形前の SQL がすべて成功すること（比べる対象が空にならないように）
#[test]
fn postgres_fixtures_run_without_errors() {
    if !postgres_available() {
        return;
    }
    let fixtures = fixtures(Some("postgres"));
    assert!(fixtures.len() >= 2);
    let mut plans = 0;
    for (name, src) in fixtures {
        let output = assert_equivalent(&name, &src, 80);
        let unexpected: Vec<_> = output
            .lines()
            .filter(|l| l.starts_with("ERROR:") && !l.contains("negative: -1"))
            .collect();
        assert!(unexpected.is_empty(), "{name}: 想定外のエラー\n{output}");
        plans += output.matches("QUERY PLAN").count();
    }
    assert!(plans >= 10, "実行計画を比べた文が少ない: {plans}");
}
