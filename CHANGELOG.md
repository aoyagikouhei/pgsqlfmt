# Changelog

This file records user-visible changes for each version.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versioning follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.2] - 2026-10-02

### Added

- Published on crates.io. Install with `cargo install pgsqlfmt`, or with `cargo binstall pgsqlfmt` to download the prebuilt binary from GitHub Releases
- Library documentation on docs.rs. The public API is `format`, `format_with_options`, `FormatOptions`, `KeywordCase`, and `CommaStyle`. The `formatter` / `lexer` / `parser` / `syntax` modules are hidden from the documentation and are not covered by semantic versioning

## [0.2.1] - 2026-10-02

### Changed

- The `--help` text and all CLI messages are now in English (for example `formatted: a.sql`, `not formatted: a.sql`, `N file(s) not formatted`; stdin is shown as `<stdin>`). The documentation and the comments in the source are in English as well

## [0.2.0] - 2026-10-02

### Added

- Format PL/pgSQL `SELECT INTO target expr, ... FROM ...` (the form with INTO before the items). Like `SELECT INTO STRICT r *`, it continues on the SELECT line
- Read multi-word typed literals such as `timestamp with time zone '...'` / `double precision '1'` / `character varying 'x'` as expressions
- Write `ROLLUP (...)` / `CUBE (...)` in `GROUP BY` in upper case as keywords, like `GROUPING SETS`
- Write a psql variable in `IN :list` on the same line as the IN expression

### Changed

- Line endings follow the first line ending in the input. CRLF files are formatted as CRLF and pass `--check` (previously they were always converted to LF)
- Keep a leading BOM in the input (previously the BOM stuck to `select` and the whole statement was left unformatted)
- Write an alias's column list directly after the name (`AS g (n, i)` → `AS g(n, i)`)
- `--write` writes to a temporary file in the same directory and then replaces the original, preserving the original file's permissions (the original file is not lost if interrupted)
- Bind unary `+` / `-` more tightly than `COLLATE` / `AT TIME ZONE` (matches PostgreSQL's grammar; the formatted output does not change)

### Fixed

- The lexer panicked on a psql variable that ended in the middle of a doubled quote, such as `:'a''`
- When the input ended inside an unterminated string or comment, trailing whitespace inside it was removed and a newline was appended

## [0.1.0] - 2026-10-01

Initial release.

### Added

- A formatter for PostgreSQL SQL and PL/pgSQL with a hand-written lexer and parser. It never fails on invalid input or unsupported syntax, and keeps unparseable parts as in the input
- Formatting of SELECT (WITH, set operations, window functions, LATERAL, TABLESAMPLE, etc.), INSERT / UPDATE / DELETE / MERGE, VALUES, and TABLE
- Formatting of DDL: CREATE TABLE / INDEX / VIEW / MATERIALIZED VIEW / TRIGGER / SEQUENCE / TYPE / SCHEMA / EXTENSION, ALTER TABLE and other ALTER statements, DROP, TRUNCATE, COMMENT ON, GRANT / REVOKE
- Formatting of utility statements: COPY (the `FROM STDIN` data is kept as in the input), SET / RESET / SHOW, EXPLAIN, transaction control
- Formatting of CREATE FUNCTION / PROCEDURE, DO, and `BEGIN ATOMIC`. Bodies are formatted too: as PL/pgSQL for `LANGUAGE plpgsql` and as SQL for `LANGUAGE sql`
- One clause per line, leading commas, upper-case keywords, wrapping by line width (full-width characters count as two columns)
- Options: `--max-width`, `--indent`, `--keyword-case upper|lower|preserve`, `--comma leading|trailing`
- CLI: formatting of stdin, `--write` / `--check` for files and directories
- psql variables (`:name` / `:'name'` / `:"name"`) are treated as a single name
- Distribution: Linux binaries (x86_64 / aarch64, statically linked with musl) on GitHub Releases, a Docker image, and pre-commit hooks
- MIT license

[Unreleased]: https://github.com/aoyagikouhei/pgsqlfmt/compare/v0.2.2...HEAD
[0.2.2]: https://github.com/aoyagikouhei/pgsqlfmt/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/aoyagikouhei/pgsqlfmt/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/aoyagikouhei/pgsqlfmt/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/aoyagikouhei/pgsqlfmt/releases/tag/v0.1.0
