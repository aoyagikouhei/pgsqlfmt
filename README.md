# sql-formatter-rs

PostgreSQL の SQL / ストアドプロシージャ（PL/pgSQL）用フォーマッター（Rust 製）。

## 開発環境

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
make down      # 停止
```

- `dev` コンテナには `DATABASE_URL` と `PG*` 環境変数が入っているので、テストやスクリプトからそのまま `db` に接続できます。
- ビルド成果物（`target`）と cargo のキャッシュは named volume に置いており、作業ディレクトリには出ません。
- ホストの UID/GID が 1000 以外なら、`cp .env.example .env` して `LOCAL_UID` / `LOCAL_GID` を合わせてください。
- ホストの 5432 番が使用中なら `.env` の `POSTGRES_PORT` を変えてください。
