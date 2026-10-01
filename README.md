# pgsqlfmt

PostgreSQL の SQL と PL/pgSQL（ストアドプロシージャ・関数・DO）のフォーマッターです。

- 問い合わせ・DML・DDL・関数定義・PL/pgSQL の本体を、句ごとに改行して整形します
- コメントは残し、解釈できない部分は入力のまま出すので、整形で SQL の意味が変わりません
- 行幅・字下げの幅・キーワードの大文字小文字・カンマの位置を変えられます
- pre-commit のフックとしても使えます

```sql
-- 入力
select c.id, c.name, count(o.id) as orders from customers c left join orders o on o.customer_id = c.id where c.active group by c.id, c.name;
```

```sql
-- 出力
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

## クイックスタート

Rust（cargo）があれば、GitHub から直接インストールできます。

```sh
cargo install --git https://github.com/aoyagikouhei/pgsqlfmt
```

Rust を入れずに Docker で使うこともできます。

```sh
docker build -t pgsqlfmt https://github.com/aoyagikouhei/pgsqlfmt.git
docker run --rm -i pgsqlfmt < query.sql
```

整形のしかたは次のとおりです。

```sh
pgsqlfmt < query.sql      # 標準入力を整形して標準出力へ
pgsqlfmt --write db/      # db/ の下の *.sql を整形して上書き
pgsqlfmt --check .        # 整形されていない *.sql があれば終了コード 1
```

pre-commit では、`.pre-commit-config.yaml` に次のように書きます。

```yaml
repos:
  - repo: https://github.com/aoyagikouhei/pgsqlfmt
    rev: main
    hooks:
      - id: pgsqlfmt
```

## ドキュメント

- [使い方](docs/usage.md): インストール、オプション、pre-commit、整形のスタイル、対応している文
- [開発](docs/development.md): 開発環境、コードの構成、テスト、新しい構文に対応する手順
