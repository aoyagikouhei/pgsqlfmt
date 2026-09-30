# sql-formatter-rs

PostgreSQL の SQL / ストアドプロシージャ（PL/pgSQL）用フォーマッター（Rust 製）。

## 使い方

標準入力の SQL を整形して標準出力に書き出します。

```sh
docker compose run --rm -T dev cargo run -q < query.sql
# 行幅を変える（既定は 80）
docker compose run --rm -T dev cargo run -q -- --max-width 100 < query.sql
```

整形のスタイル:

- 句ごとに改行し、項目が 2 つ以上なら 1 行ずつ字下げして、カンマは行頭に置く
- WHERE / HAVING / ON の AND・OR で改行する。JOIN は FROM より 1 段、ON はさらに 1 段深くする
- 副問い合わせと CASE は複数行にする
- 行幅（既定 80、全角文字は 2 桁）に収まらない式は折り返す。関数の引数・`IN (...)` などの括弧の中の並びは
  1 行ずつ行頭カンマで、二項演算は演算子の前で、`OVER (...)` は句ごとに、RAISE / EXECUTE は `USING` / `INTO` の前で改行する
- キーワードは大文字にする。識別子・関数名・型名は入力のまま
- コメントと、文やコメントの前後の空行は残す
- `CREATE FUNCTION` / `CREATE PROCEDURE` は RETURNS・LANGUAGE・AS などのオプションを 1 行ずつにする
- 関数本体と `DO` の本体は、`LANGUAGE plpgsql` なら PL/pgSQL、`LANGUAGE sql` なら SQL として中身も整形する
  （`BEGIN ATOMIC ... END` も整形する。ほかの言語の本体はそのまま）
- PL/pgSQL は DECLARE / BEGIN / EXCEPTION / END をブロックの深さに置き、文を 1 段深くする。
  IF / CASE / LOOP / WHILE / FOR / FOREACH の中はさらに 1 段深くする
- 対応していない文（CREATE TABLE など）や解釈できない部分は、元のテキストのまま出す


ローカルに Rust を入れず、Docker だけで開発します。必要なのは Docker と Docker Compose です。

| サービス | 内容 |
| --- | --- |
| `dev` | Rust（stable）+ rustfmt / clippy + psql |
| `db`  | PostgreSQL 18（DB `sql_formatter`、ユーザー/パスワード `postgres`） |

```sh
make up        # コンテナをビルドして起動
make test      # cargo test
make check     # fmt チェック + clippy + test
make shell     # dev コンテナに入る（cargo run などはここで）
make psql      # PostgreSQL に接続
make db-reset  # DB を作り直す（docker/postgres/init を再実行）
make snapshot-review  # スナップショットの差分を確認・承認
make down      # 停止
```

- `dev` コンテナには `DATABASE_URL` と `PG*` 環境変数が入っているので、テストやスクリプトからそのまま `db` に接続できます。
- ビルド成果物（`target`）と cargo のキャッシュは named volume に置いており、作業ディレクトリには出ません。
- ホストの UID/GID が 1000 以外なら、`cp .env.example .env` して `LOCAL_UID` / `LOCAL_GID` を合わせてください。
- ホストの 5432 番が使用中なら `.env` の `POSTGRES_PORT` を変えてください。

## テスト

- 単体テストは各モジュール内（`src/lexer.rs` など）に置いています。
- スナップショットテストは `tests/fixtures/<対象>/*.sql` を入力にし、結果を `tests/snapshots/` に保存します。
  ケースを増やすときは `.sql` を追加し、`make snapshot-review` で内容を確認してから承認します。
- `tests/formatter_properties.rs` は、すべてのフィクスチャとその変形について、整形でトークンやコメントが
  欠けないことと、2 回整形しても結果が変わらないことを確かめます。
- `tests/postgres_equivalence.rs` は、すべてのフィクスチャを整形の前と後で PostgreSQL（`db` コンテナ）に流し、
  結果が同じになることを確かめます（行幅 80 と 20）。SELECT / DML は `EXPLAIN (VERBOSE, COSTS OFF, GENERIC_PLAN)` の
  実行計画と実行結果を、CREATE FUNCTION は本体以外のカタログ上の定義を、DO / 関数の呼び出しは NOTICE を含む出力を比べます。
  スキーマは `tests/postgres/schema.sql`、エラーなく実行できるべき検証用の SQL は `tests/fixtures/postgres/` にあります。
  全体を 1 つのトランザクションで流して最後に ROLLBACK するので、DB には何も残りません。
  環境変数 `PGHOST` がない環境（dev コンテナの外）では何もしません。
- `tests/parser_robustness.rs` は、すべてのフィクスチャを途中で切ったものとトークンを 1 つ抜いたものを構文解析し、
  止まらずに元のテキストを保つことを確かめます。
