# Using pgsqlfmt

A command-line tool that formats PostgreSQL SQL and PL/pgSQL (stored procedures, functions, and DO blocks).

- [Installation](#installation)
- [Basic usage](#basic-usage)
- [Options](#options)
- [Using with pre-commit](#using-with-pre-commit)
- [Formatting style](#formatting-style)
- [Supported statements](#supported-statements)
- [What is output verbatim](#what-is-output-verbatim)

## Installation

### Using a prebuilt binary (Linux)

Binaries for Linux amd64 and arm64 are distributed on [GitHub Releases](https://github.com/aoyagikouhei/pgsqlfmt/releases).
They are statically linked, so they run on any distribution.

```sh
curl -sSL "https://github.com/aoyagikouhei/pgsqlfmt/releases/latest/download/pgsqlfmt-$(uname -m)-unknown-linux-musl.tar.gz" | tar xz pgsqlfmt
sudo mv pgsqlfmt /usr/local/bin/
```

| CPU | File |
| --- | --- |
| amd64 (x86_64) | `pgsqlfmt-x86_64-unknown-linux-musl.tar.gz` |
| arm64 (aarch64) | `pgsqlfmt-aarch64-unknown-linux-musl.tar.gz` |

Each file comes with a SHA-256 checksum (`.sha256`). You can verify a downloaded file like this.

```sh
sha256sum -c pgsqlfmt-x86_64-unknown-linux-musl.tar.gz.sha256
```

### Installing with cargo

If you have Rust (cargo), you can build and install directly from GitHub.

```sh
cargo install --git https://github.com/aoyagikouhei/pgsqlfmt
```

The binary is installed to `~/.cargo/bin/pgsqlfmt`.

### Using Docker

To use it without installing Rust, build the Docker image.

```sh
docker build -t pgsqlfmt https://github.com/aoyagikouhei/pgsqlfmt.git
```

The image's working directory is `/src`. Mount the directory you want to format at `/src`.

```sh
docker run --rm -i pgsqlfmt < query.sql                  # format stdin
docker run --rm -v "$PWD:/src" pgsqlfmt --check .        # list files that are not formatted
docker run --rm -v "$PWD:/src" pgsqlfmt --write db/      # format and overwrite files
```

## Basic usage

```sh
pgsqlfmt < query.sql             # format stdin and write to stdout
pgsqlfmt query.sql               # format a file and write to stdout (the file is not modified)
pgsqlfmt --write db/             # format and overwrite *.sql under db/
pgsqlfmt --check .               # list *.sql files that are not formatted (does not rewrite)
```

- With no file given, stdin is formatted and written to stdout. `-` also means stdin.
- Given a directory, `*.sql` files under it are searched for. Files and directories starting with `.` are skipped.
- When two or more files or directories are given, `--write` or `--check` is required.
- `--write` rewrites only the files whose contents change after formatting.
- `--check` does not modify files. Use it for verification in CI.

The exit codes are as follows.

| Exit code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | `--check` found files that are not formatted |
| 2 | Argument error, or an error reading or writing a file |

## Options

| Option | Value | Default |
| --- | --- | --- |
| `-w`, `--max-width N` | Line width | 80 |
| `--indent N` | Indent width (2 to 8) | 4 |
| `--keyword-case` | `upper` / `lower` / `preserve` (as in the input) | `upper` |
| `--comma` | `leading` (start of line) / `trailing` (end of line) | `leading` |
| `--write` | Overwrite files with the formatted result | |
| `--check` | List files that are not formatted and exit with code 1 (does not rewrite) | |
| `-h`, `--help` | Show this help | |

Here is the difference between the default style and `--indent 2 --comma trailing --keyword-case lower`.

```sql
-- default
SELECT
    c.id
  , c.name
FROM customers c
WHERE c.active;

-- --indent 2 --comma trailing --keyword-case lower
select
  c.id,
  c.name
from customers c
where c.active;
```

## Using with pre-commit

pgsqlfmt can be used as a [pre-commit](https://pre-commit.com/) hook. The hook runs in Docker, so Rust is not required.

```yaml
# .pre-commit-config.yaml
repos:
  - repo: https://github.com/aoyagikouhei/pgsqlfmt
    rev: v0.2.1  # a release tag
    hooks:
      - id: pgsqlfmt          # format and rewrite
      # - id: pgsqlfmt-check  # check only (for CI)
```

| Hook | Behavior |
| --- | --- |
| `pgsqlfmt` | Formats and rewrites `*.sql` |
| `pgsqlfmt-check` | Fails if any `*.sql` is not formatted (does not rewrite) |

Set `rev` to a tag from the [releases](https://github.com/aoyagikouhei/pgsqlfmt/releases).
When a new version is out, `pre-commit autoupdate` bumps it to the latest tag.

Options are passed with `args`.

```yaml
      - id: pgsqlfmt
        args: [--indent, "2", --comma, trailing]
```

## Formatting style

### Queries

Each clause starts on a new line. When a list has two or more items, each item goes on its own indented line with leading commas.
AND and OR in WHERE, HAVING, and ON start new lines. JOIN is indented one level deeper than FROM, and ON one level deeper still.

```sql
-- input
select c.id, c.name, count(o.id) as orders from customers c left join orders o on o.customer_id = c.id and o.status <> 'cancelled' where c.active and c.created_at >= '2026-01-01' group by c.id, c.name order by orders desc limit 10;
```

```sql
-- output
SELECT
    c.id
  , c.name
  , count(o.id) AS orders
FROM customers c
    LEFT JOIN orders o
        ON o.customer_id = c.id
            AND o.status <> 'cancelled'
WHERE c.active
    AND c.created_at >= '2026-01-01'
GROUP BY
    c.id
  , c.name
ORDER BY orders DESC
LIMIT 10;
```

- Subqueries and CASE are written on multiple lines.
- Keywords are written in upper case. Table, column, function, and type names are kept as in the input.
- Comments, and blank lines before and after statements and comments, are kept.

### Wrapping by line width

Expressions that do not fit in the line width (default 80; full-width characters count as two columns) are wrapped.

- Lists inside parentheses, such as function arguments and `IN (...)`, are written one per line with leading commas.
- Binary operations are wrapped before the operator.
- `OVER (...)` is written with one clause per line.
- PL/pgSQL RAISE and EXECUTE are broken before `USING` and `INTO`.

```sql
SELECT format_name(
    customer_first_name
  , customer_middle_name
  , customer_last_name
  , customer_title
)
FROM customers;
```

### Functions and PL/pgSQL

In `CREATE FUNCTION` and `CREATE PROCEDURE`, options such as RETURNS, LANGUAGE, and AS go one per line.
The body is formatted as PL/pgSQL for `LANGUAGE plpgsql` and as SQL for `LANGUAGE sql`.
`DO` bodies and `BEGIN ATOMIC ... END` are formatted too. Bodies in other languages are kept as they are.

In PL/pgSQL, DECLARE, BEGIN, EXCEPTION, and END are placed at the block's depth, and statements one level deeper.
The contents of IF, CASE, LOOP, WHILE, FOR, and FOREACH go one level deeper still.

```sql
CREATE OR REPLACE FUNCTION add_points(p_id bigint, p_points int)
RETURNS int
LANGUAGE plpgsql
AS $$
DECLARE
    v_total int;
BEGIN
    UPDATE customers
    SET score = score + p_points
    WHERE id = p_id
    RETURNING score
    INTO v_total;
    IF v_total > 100 THEN
        RAISE NOTICE 'vip: %', p_id;
    END IF;
    RETURN v_total;
END
$$;
```

### DDL

The columns and constraints of CREATE TABLE are always written one per line with leading commas, even when there is only one.

```sql
CREATE TABLE orders (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY
  , customer_id bigint NOT NULL REFERENCES customers (id)
  , status text NOT NULL DEFAULT 'new'
  , created_at timestamptz NOT NULL DEFAULT now()
);
```

See [Supported statements](#supported-statements) for the layout of each statement.

## Supported statements

| Statement | Layout |
| --- | --- |
| SELECT, VALUES, WITH, set operations | One clause per line |
| INSERT, UPDATE, DELETE, MERGE | One clause per line. MERGE puts USING and WHEN at the start of a line |
| CREATE FUNCTION, CREATE PROCEDURE, DO, CALL | Options one per line; the body is formatted too |
| CREATE TABLE | Columns and constraints one per line |
| CREATE TYPE | Columns of a composite type one per line, like CREATE TABLE. ENUM, RANGE, etc. on one line |
| CREATE SEQUENCE, ALTER SEQUENCE | Options one per line |
| CREATE TRIGGER | Timing and events, FOR EACH, WHEN, EXECUTE, and other clauses one per line |
| CREATE INDEX, CREATE [MATERIALIZED] VIEW | One line. The VIEW's query starts on the next line; the INDEX's WHERE goes on the next line |
| ALTER TABLE | Actions one per line when there are two or more |
| Other ALTER statements (INDEX, VIEW, FUNCTION, TYPE, DOMAIN, SCHEMA, ROLE, DEFAULT PRIVILEGES, etc.) | One line |
| CREATE SCHEMA, CREATE EXTENSION, DROP, TRUNCATE, COMMENT ON | One line |
| GRANT, REVOKE | One line. Privileges, object kinds, PUBLIC, etc. are written in upper case |
| COPY | One line. The query in `COPY (query)` is written on multiple lines, like a subquery |
| SET, RESET, SHOW | One line |
| EXPLAIN | The target statement is formatted starting on the line after the options |
| Transaction control such as BEGIN, COMMIT, ROLLBACK, SAVEPOINT | One line |

Names spelled like keywords (such as `data` in `RENAME TO data`) are kept as in the input when they appear in a name position.

## What is output verbatim

So that formatting never changes the meaning of your SQL, the following are output as in the input.

- **Unsupported statements:** statements such as VACUUM and LOCK are output verbatim as a whole.
- **Unparseable parts:** parts that cannot be read as syntax are output verbatim, limited to just that part.
- **COPY data:** the data following `COPY ... FROM STDIN;` is output verbatim up to the line containing only `\.`.
- **Contents of strings and comments:** the contents of strings, quoted identifiers, and comments are not changed.
- **Leading BOM and line endings:** a leading BOM is kept. Line endings follow the first line ending in the input (CRLF files stay CRLF).

psql variables (`:name`, `:'name'`, `:"name"`) are treated as a single name and are never joined to the surrounding words.
Joining them would make the value substituted by psql run into the preceding word.

A statement immediately following a line with a psql meta-command (such as `\set`) is output verbatim without formatting.
