//! Verifies against a real PostgreSQL that the SQL means the same before and after formatting.
//!
//! Each fixture statement runs, one at a time, inside a transaction that has loaded
//! `tests/postgres/schema.sql`, and the psql output is compared before and after formatting.
//! The transaction ends with ROLLBACK, so nothing is left in the database.
//! - SELECT / INSERT / UPDATE / DELETE: the `EXPLAIN (VERBOSE, COSTS OFF, GENERIC_PLAN)` plan and
//!   the result of running it
//! - CREATE FUNCTION / PROCEDURE: whether it can be created, and its catalog definition other than
//!   the body text (a PL/pgSQL body is syntax-checked at creation; the results of calling it are
//!   compared by later statements)
//! - Anything else (DO / CALL etc.): the result of running it (including NOTICEs and errors)
//! - Finally the catalog of the public schema (columns, types, defaults, constraints, indexes and
//!   view definitions) is compared, along with triggers, sequences, types, schemas, extensions,
//!   privileges (GRANT / REVOKE) and comments (COMMENT ON). A change in the meaning of DDL does not
//!   show in the result of running it (`CREATE TABLE` etc.), so it is checked here
//!
//! psql runs with `ON_ERROR_ROLLBACK=on`, so the statements after a failing one are still compared.
//! Without the `PGHOST` environment variable (outside the dev container) this test does nothing.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use pgsqlfmt::lexer::TokenKind;
use pgsqlfmt::parser::parse;
use pgsqlfmt::syntax::{Element, Node, NodeKind};
use pgsqlfmt::{CommaStyle, FormatOptions, KeywordCase, format_with_options};

fn postgres_available() -> bool {
    if std::env::var_os("PGHOST").is_none() {
        eprintln!("PGHOST is not set; skipping verification against PostgreSQL");
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

/// The function name of a CREATE FUNCTION (both parts for `schema.name`). Unquoted names are
/// lowercased.
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

/// Turns a fixture into a psql script for verification
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
        out.push_str(&format!("\\echo '--- statement {n}'\n"));
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
    out.push_str(CATALOG_SNAPSHOT);
    out.push_str("ROLLBACK;\n");
    out
}

/// The definitions in the public schema. PostgreSQL prints them all in normalized form, so
/// differences in spelling do not show
const CATALOG_SNAPSHOT: &str = "\
\\echo '--- catalog'
SELECT c.relname, c.relkind, c.relpersistence, a.attnum, a.attname, format_type(a.atttypid, a.atttypmod),
       a.attnotnull, pg_get_expr(d.adbin, d.adrelid), a.attidentity, a.attgenerated, co.collname
FROM pg_class c
JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
LEFT JOIN pg_attrdef d ON d.adrelid = c.oid AND d.adnum = a.attnum
LEFT JOIN pg_collation co ON co.oid = a.attcollation AND a.attcollation <> 100
WHERE c.relnamespace = 'public'::regnamespace AND c.relkind IN ('r', 'p', 'v', 'm', 'f', 'c')
ORDER BY c.relname, a.attnum;
SELECT conrelid::regclass::text, conname, contype, pg_get_constraintdef(oid), convalidated
FROM pg_constraint WHERE connamespace = 'public'::regnamespace ORDER BY 1, 2;
SELECT indexrelid::regclass::text, pg_get_indexdef(indexrelid)
FROM pg_index WHERE indrelid::regclass::text NOT LIKE 'pg\\_%' ORDER BY 1;
SELECT c.relname, c.relkind, pg_get_viewdef(c.oid), c.reloptions, pg_get_partkeydef(c.oid),
       pg_get_expr(c.relpartbound, c.oid)
FROM pg_class c WHERE c.relnamespace = 'public'::regnamespace ORDER BY 1;
SELECT tgrelid::regclass::text, tgname, pg_get_triggerdef(oid), tgenabled
FROM pg_trigger WHERE NOT tgisinternal ORDER BY 1, 2;
SELECT schemaname, sequencename, data_type, start_value, min_value, max_value, increment_by, cycle, cache_size,
       last_value
FROM pg_sequences ORDER BY 1, 2;
SELECT d.refobjid::regclass::text, c.relname
FROM pg_depend d JOIN pg_class c ON c.oid = d.objid
WHERE c.relkind = 'S' AND d.deptype = 'a' ORDER BY 1, 2;
SELECT n.nspname, t.typname, t.typtype, t.typcategory, t.typdefault
FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace
WHERE n.nspname NOT LIKE 'pg\\_%' AND n.nspname <> 'information_schema' ORDER BY 1, 2;
SELECT enumtypid::regtype::text, enumlabel, enumsortorder FROM pg_enum ORDER BY 1, 3;
SELECT rngtypid::regtype::text, rngsubtype::regtype::text, rngsubdiff::text
FROM pg_range WHERE rngtypid::regtype::text NOT IN (SELECT typname FROM pg_type WHERE typnamespace = 'pg_catalog'::regnamespace)
ORDER BY 1;
SELECT nspname, pg_get_userbyid(nspowner) FROM pg_namespace
WHERE nspname NOT LIKE 'pg\\_%' AND nspname <> 'information_schema' ORDER BY 1;
SELECT extname, extversion, extnamespace::regnamespace::text FROM pg_extension ORDER BY 1;
SELECT n.nspname, c.relname, c.relacl::text,
       (SELECT string_agg(a.attname || '=' || a.attacl::text, ', ' ORDER BY a.attnum)
        FROM pg_attribute a WHERE a.attrelid = c.oid AND a.attacl IS NOT NULL)
FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname IN ('public', 'app') ORDER BY 1, 2;
SELECT nspname, nspacl::text FROM pg_namespace WHERE nspname IN ('public', 'app') ORDER BY 1;
SELECT proname, proacl::text, prosecdef, proconfig FROM pg_proc WHERE pronamespace = 'public'::regnamespace ORDER BY 1;
SELECT defaclrole::regrole::text, defaclnamespace::regnamespace::text, defaclobjtype, defaclacl::text
FROM pg_default_acl ORDER BY 1, 2, 3;
SELECT roleid::regrole::text, member::regrole::text, admin_option FROM pg_auth_members
WHERE roleid = 'pg_read_all_data'::regrole ORDER BY 2;
SELECT pg_describe_object(classoid, objoid, objsubid), description
FROM pg_description WHERE objoid >= 16384 ORDER BY 1;
";

/// Runs the script through psql and returns the output (including errors and NOTICEs)
fn run_psql(script: &str) -> String {
    let mut child = Command::new("sh")
        .args([
            "-c",
            "psql -X -v ON_ERROR_ROLLBACK=on -v VERBOSITY=terse -P pager=off -f - 2>&1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("cannot start psql");
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
        "cannot connect to PostgreSQL:\n{text}"
    );
    // Error positions (`psql:<stdin>:12: ERROR: ... at character 210`) change with formatting,
    // so strip them
    text.lines()
        .map(|line| {
            let line = match line.strip_prefix("psql:<stdin>:") {
                Some(rest) => rest.split_once(": ").map_or(rest, |(_, msg)| msg),
                None => line,
            };
            let line = line
                .split_once(" at character ")
                .map_or(line, |(msg, _)| msg);
            // Error messages quote tokens from the input (`syntax error at or near "rename"`), so
            // merely uppercasing a keyword changes them. Compare case-insensitively
            if line.starts_with("ERROR:") {
                line.to_lowercase()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Is the psql output the same before and after formatting?
fn assert_equivalent(name: &str, src: &str, options: &FormatOptions) -> String {
    let formatted = format_with_options(src, options);
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
        "{name}: the number of statements changed"
    );

    let original = run_psql(&script(src));
    let after = run_psql(&script(&formatted));
    if after != original {
        panic!(
            "{name} ({options:?}): PostgreSQL results differ before and after formatting\n{}\n\
             --- formatted ---\n{formatted}",
            first_difference(&original, &after)
        );
    }
    original
}

/// The first differing line and a few lines around it
fn first_difference(expected: &str, actual: &str) -> String {
    let expected: Vec<_> = expected.lines().collect();
    let actual: Vec<_> = actual.lines().collect();
    let at = (0..expected.len().max(actual.len()))
        .find(|&i| expected.get(i) != actual.get(i))
        .unwrap_or(0);
    let window = |lines: &[&str]| {
        lines[at.saturating_sub(5).min(lines.len())..(at + 5).min(lines.len())].join("\n")
    };
    format!(
        "--- output before formatting (around line {at}) ---\n{}\n\
         --- output after formatting ---\n{}",
        window(&expected),
        window(&actual)
    )
}

/// The default options, a narrow width, and a combination of non-default options
fn option_sets() -> [FormatOptions; 3] {
    [
        FormatOptions::default(),
        FormatOptions {
            max_width: 20,
            ..FormatOptions::default()
        },
        FormatOptions {
            max_width: 30,
            indent_width: 2,
            keyword_case: KeywordCase::Lower,
            comma_style: CommaStyle::Trailing,
        },
    ]
}

#[test]
fn formatted_fixtures_behave_the_same_in_postgres() {
    if !postgres_available() {
        return;
    }
    // Each fixture gets its own transaction (its own connection), so run them in parallel
    let fixtures = fixtures(None);
    std::thread::scope(|scope| {
        for (name, src) in &fixtures {
            scope.spawn(move || {
                for options in option_sets() {
                    assert_equivalent(name, src, &options);
                }
            });
        }
    });
}

/// Every statement of the verification fixtures must succeed before formatting (so that there is
/// something to compare)
#[test]
fn postgres_fixtures_run_without_errors() {
    if !postgres_available() {
        return;
    }
    let fixtures = fixtures(Some("postgres"));
    assert!(fixtures.len() >= 2);
    let mut plans = 0;
    for (name, src) in fixtures {
        let output = assert_equivalent(&name, &src, &FormatOptions::default());
        let unexpected: Vec<_> = output
            .lines()
            // run_psql lowercases error lines
            .filter(|l| l.to_lowercase().starts_with("error:") && !l.contains("negative: -1"))
            .collect();
        assert!(unexpected.is_empty(), "{name}: unexpected error\n{output}");
        plans += output.matches("QUERY PLAN").count();
    }
    assert!(
        plans >= 10,
        "too few statements had their plans compared: {plans}"
    );
}
