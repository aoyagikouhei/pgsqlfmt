# pgsqlfmt

A formatter for PostgreSQL SQL and PL/pgSQL (stored procedures, functions, and DO blocks).

- Formats queries, DML, DDL, function definitions, and PL/pgSQL bodies with one clause per line
- Keeps comments and outputs anything it cannot parse verbatim, so formatting never changes the meaning of your SQL
- Lets you change the line width, indent width, keyword case, and comma position
- Can be used as a pre-commit hook

```sql
-- input
select c.id, c.name, count(o.id) as orders from customers c left join orders o on o.customer_id = c.id where c.active group by c.id, c.name;
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
WHERE c.active
GROUP BY
    c.id
  , c.name;
```

## Quick start

On Linux (amd64 and arm64), you can install a prebuilt binary.

```sh
curl -sSL "https://github.com/aoyagikouhei/pgsqlfmt/releases/latest/download/pgsqlfmt-$(uname -m)-unknown-linux-musl.tar.gz" | tar xz pgsqlfmt
sudo mv pgsqlfmt /usr/local/bin/
```

If you have Rust (cargo), you can also install directly from GitHub.

```sh
cargo install --git https://github.com/aoyagikouhei/pgsqlfmt
```

You can also use it through Docker without installing Rust.

```sh
docker build -t pgsqlfmt https://github.com/aoyagikouhei/pgsqlfmt.git
docker run --rm -i pgsqlfmt < query.sql
```

Formatting works like this.

```sh
pgsqlfmt < query.sql      # format stdin and write to stdout
pgsqlfmt --write db/      # format and overwrite *.sql under db/
pgsqlfmt --check .        # exit with code 1 if any *.sql is not formatted
```

For pre-commit, add the following to `.pre-commit-config.yaml`.

```yaml
repos:
  - repo: https://github.com/aoyagikouhei/pgsqlfmt
    rev: v0.2.0
    hooks:
      - id: pgsqlfmt
```

## Documentation

- [Usage](docs/usage.md): installation, options, pre-commit, formatting style, supported statements
- [Development](docs/development.md): development environment, code layout, tests, how to add support for new syntax
- [Changelog](CHANGELOG.md)

## License

[MIT](LICENSE)
