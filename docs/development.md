# Developing pgsqlfmt

- [Development environment](#development-environment)
- [Code layout](#code-layout)
- [Design principles](#design-principles)
- [Tests](#tests)
- [How to add support for new syntax](#how-to-add-support-for-new-syntax)
- [CI](#ci)
- [Releases](#releases)
- [Distribution files](#distribution-files)

## Development environment

Development happens entirely in Docker, without installing Rust locally. All you need is Docker and Docker Compose.

| Service | Contents |
| --- | --- |
| `dev` | Rust (stable), rustfmt, clippy, cargo-insta, psql |
| `db` | PostgreSQL 18 (database `sql_formatter`, user and password `postgres`) |

```sh
make up               # build and start the containers
make test             # cargo test
make check            # fmt check, clippy, test (run before committing)
make shell            # open a shell in the dev container
make psql             # connect to PostgreSQL
make snapshot-review  # review and accept snapshot diffs
make db-reset         # recreate the database (re-runs docker/postgres/init)
make image            # build the pgsqlfmt distribution image
make down             # stop
```

Run a locally built binary inside the dev container.

```sh
docker compose run --rm -T dev cargo run -q -- tests/fixtures/format/comments.sql
```

- **Environment variables:** the dev container has `DATABASE_URL` and the `PG*` environment variables set. Tests and scripts can connect to `db` as is.
- **Build artifacts:** `target` and the cargo caches live in named volumes and do not appear in the working directory.
- **User UID and GID:** if your host UID and GID are not 1000, run `cp .env.example .env` and set `LOCAL_UID` and `LOCAL_GID` to match.
- **Port:** if port 5432 on the host is in use, change `POSTGRES_PORT` in `.env`.
- **Environments without a terminal:** when there is no terminal, as in CI, use `make check RUN_FLAGS=-T`.

## Code layout

The input is lexed into a token sequence, parsed into a syntax tree, and the syntax tree is formatted into a string.

| File | Role |
| --- | --- |
| `src/main.rs` | Command-line arguments, reading and writing files, `--check` and `--write` |
| `src/lib.rs` | Library entry point. Exposes `format` and `format_with_options` |
| `src/lexer.rs` | Lexer. Turns every byte of the input, including whitespace and comments, into tokens |
| `src/syntax.rs` | Syntax tree node kinds and helpers for displaying the tree |
| `src/parser/mod.rs` | Parser foundation and dispatch by statement kind (`statement`) |
| `src/parser/select.rs` | SELECT, VALUES, WITH, set operations |
| `src/parser/dml.rs` | INSERT, UPDATE, DELETE, MERGE |
| `src/parser/expr.rs` | Expressions and type names. Binary operations are read with a Pratt parser |
| `src/parser/ddl.rs` | CREATE, ALTER, DROP, TRUNCATE, COMMENT ON, GRANT, REVOKE |
| `src/parser/function.rs` | CREATE FUNCTION, CREATE PROCEDURE, DO, CALL. Also parses the body |
| `src/parser/plpgsql.rs` | PL/pgSQL bodies |
| `src/parser/utility.rs` | COPY, SET, RESET, SHOW, EXPLAIN, transaction control |
| `src/parser/keywords.rs` | Classification of keywords such as reserved words |
| `src/formatter/mod.rs` | Formatter entry point and dispatch by node kind |
| `src/formatter/stmt.rs` | Clause layout for queries and DML |
| `src/formatter/ddl.rs` | Layout for DDL, MERGE, and single-line statements |
| `src/formatter/plpgsql.rs` | Layout for function definitions, DO, and PL/pgSQL bodies |
| `src/formatter/wrap.rs` | Wrapping by line width |
| `src/formatter/writer.rs` | Indentation, whitespace between tokens, comment placement, keyword case |

## Design principles

- **Hand-written lexer and parser:** pg_query (libpg_query), parser combinators, and parser generators are not used.
  pg_query discards comments, cannot turn PL/pgSQL back into a string, and only tracks line numbers for positions, so it is not a good foundation for a formatter.
  The lexical rules follow PostgreSQL's `scan.l`, and PL/pgSQL follows `pl_gram.y`.
- **Lossless syntax tree:** the syntax tree holds every token, including whitespace and comments. Concatenating the tree's tokens in order reproduces the input exactly.
- **Never fails:** invalid input and unsupported syntax do not cause errors.
  Unparseable parts become `Error` or `RawStatement` nodes, which the formatter writes as in the input.
- **Keywords and names:** whether a word is a keyword is decided by the parser (by re-tagging it as `TokenKind::Keyword`).
  Case is applied according to the settings when writing. A word in a name position is kept as in the input, as a name, even when spelled like a keyword.
- **Meaning is preserved:** there is no mechanism that mechanically compares the syntax tree before and after formatting.
  That the meaning does not change is confirmed by the property tests below and by verification against a real PostgreSQL.

## Tests

| Test | Location | What it checks |
| --- | --- | --- |
| Unit tests | Inside each module and `src/*/tests.rs` | Individual behaviors of the lexer, parser, and formatter |
| Snapshot tests | `tests/*_snapshots.rs` | That the formatted output, token sequence, and syntax tree of the fixtures have not changed |
| Property tests | `tests/formatter_properties.rs` | That no tokens or comments are lost, and that formatting twice gives the same result |
| Robustness tests | `tests/parser_robustness.rs` | That truncated input, or input with one token removed, neither panics nor hangs, and the original text is preserved |
| CLI tests | `tests/cli.rs` | File reading and writing and exit codes when running the binary |
| Verification against a real PostgreSQL | `tests/postgres_equivalence.rs` | That PostgreSQL gives the same results before and after formatting |

### Snapshot tests

Fixtures live in `tests/fixtures/<target>/*.sql`, and the results are stored in `tests/snapshots/`.

| Fixtures | Used by |
| --- | --- |
| `tests/fixtures/*/*.sql` (all) | Formatted output |
| `tests/fixtures/lexer/*.sql` | Token sequence |
| `tests/fixtures/parser/*.sql`, `tests/fixtures/plpgsql/*.sql` | Syntax tree |

To add a case, add a `.sql` file, then review the diff with `make snapshot-review` and accept it.
The property tests and robustness tests also take every fixture as input, so adding a fixture extends the coverage of those tests too.

### Verification against a real PostgreSQL

Every fixture under `tests/fixtures/` is run against PostgreSQL (the `db` container) before and after formatting, and the outputs are compared.
Three settings are used: the default; line width 20; and indent 2 with lower case and trailing commas.

- **SELECT and DML:** the `EXPLAIN (VERBOSE, COSTS OFF, GENERIC_PLAN)` plan and the execution result are compared.
- **CREATE FUNCTION:** the catalog definition, excluding the body, is compared.
- **Other statements:** the execution result, including NOTICEs and errors, is compared.
- **Catalog:** at the end, the definitions of tables and columns, constraints, indexes, views, triggers, sequences, types, schemas, extensions, privileges, default privileges, and comments are compared.
  Differences in the meaning of DDL do not show up in execution results, so they are caught here.

The fixtures under `tests/fixtures/postgres/` are also checked to run without errors. The tables used for verification are created by `tests/postgres/schema.sql`.

Everything runs in a single transaction that is rolled back at the end, so nothing is left in the database. Keep the following in mind when writing fixtures.

- **Statements that close the transaction:** do not write BEGIN, COMMIT, or ROLLBACK (except ROLLBACK TO a savepoint). They would close the outer transaction.
- **COPY ... FROM STDIN:** do not write it. The verification inserts separator lines between statements, which would corrupt the data.
- **Roles:** do not write CREATE ROLE. Tests running in parallel would collide trying to create the same role. To check privileges, use built-in roles (such as `pg_read_all_data`) and PUBLIC.

In environments without the `PGHOST` environment variable (outside the dev container), this test does nothing.

## How to add support for new syntax

Unsupported statements are output verbatim as `RawStatement`. To add support, proceed in this order.

1. **Write a test:** add a test with the input and expected output to `src/formatter/tests.rs`, and confirm that it fails.
2. **Add node kinds:** add the statement, and clause nodes if needed, to `NodeKind` in `src/syntax.rs`.
3. **Write the parser:** write the parsing in the relevant file under `src/parser/`, and dispatch to it from `statement` in `src/parser/mod.rs`.
   Read name positions with `name_path`, and consume keywords with `bump_kw` or `eat_kw`.
   Turn unparseable parts into `Error` with `raw_until` so they are kept as in the input.
4. **Write the formatter:** in `statement` in `src/formatter/mod.rs`, dispatch the new node to a layout function.
   Also add new statement nodes to `is_statement`. For a single-line statement, the default dispatch is enough.
5. **Add to the verification against a real PostgreSQL:** add the statement to the fixtures under `tests/fixtures/postgres/`.
   If differences need to be caught in the catalog, add a query to `CATALOG_SNAPSHOT` in `tests/postgres_equivalence.rs`.
6. **Accept the snapshots:** review the diff with `make snapshot-review` and accept it.
7. **Run everything:** run `make check` for fmt, clippy, and all tests (including the verification against a real PostgreSQL).

## CI

GitHub Actions (`.github/workflows/ci.yml`) runs two jobs on every push to main and every PR.

| Job | Contents |
| --- | --- |
| `check` | Runs `make check` in the same Docker Compose environment as local development (including the verification against a real PostgreSQL) |
| `image` | Builds the distribution image and confirms that it actually formats and that `--check` passes |

## Releases

Pushing a tag such as `v0.1.0` makes `.github/workflows/release.yml` build the binaries on GitHub runners and upload them to GitHub Releases, and then publish the crate to [crates.io](https://crates.io/crates/pgsqlfmt).

| Target | Runner |
| --- | --- |
| `x86_64-unknown-linux-musl` | `ubuntu-latest` |
| `aarch64-unknown-linux-musl` | `ubuntu-24.04-arm` |

Each runner first checks that the tag matches the version in Cargo.toml, then runs the tests, builds, and smoke-tests the binary.
The binary is statically linked with musl, bundled with `README.md` into a tar.gz, and accompanied by a SHA-256 checksum.
The asset names do not include the version, so the latest version can be fetched from `releases/latest/download/<name>`.

The release procedure is as follows.

1. Bump `version` in `Cargo.toml` and update `Cargo.lock` too. In `CHANGELOG.md`, turn `[Unreleased]` into a heading with the new version and date, add the comparison link at the bottom, and push to main.
2. Confirm that CI on main passes.
3. Create the tag and push it.

   ```sh
   git tag v0.1.0
   git push origin v0.1.0
   ```

4. Confirm that the Release workflow passes, the assets appear on GitHub Releases, and the new version appears on crates.io.

The `publish` job authenticates to crates.io with [Trusted Publishing](https://crates.io/docs/trusted-publishing) (GitHub OIDC), so no API token is stored in the repository.
It requires this repository and `release.yml` to be registered as a Trusted Publisher in the crate's settings on crates.io.
If the version is already on crates.io, the job skips publishing.

Before publishing, check the contents of the package. Only `src/`, `README.md`, `LICENSE`, and `CHANGELOG.md` are included (`include` in Cargo.toml).

```sh
cargo package --list
cargo publish --dry-run
```

To try only the build on the runners without creating a tag, run the workflow manually. It stops after the build and smoke test and does not create a release.

```sh
gh workflow run Release --ref main
```

## Distribution files

| File | Contents |
| --- | --- |
| `.github/workflows/release.yml` | Uploads Linux binaries to GitHub Releases and publishes the crate to crates.io when a tag is pushed |
| `Dockerfile` | The distribution image. Contains only the release-built binary |
| `.pre-commit-hooks.yaml` | The pre-commit hooks (`pgsqlfmt` and `pgsqlfmt-check`). They run on the image above |

The development image is built from `docker/dev/Dockerfile` and `compose.yaml`. It is separate from the distribution image.
