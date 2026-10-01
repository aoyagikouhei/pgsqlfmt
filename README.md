# sql-formatter-rs

PostgreSQL の SQL / ストアドプロシージャ（PL/pgSQL）用フォーマッター（Rust 製）。

## 使い方

配布用のイメージ（ルートの `Dockerfile`、リリースビルドのバイナリだけを含む）を作って使います。

```sh
make image                                            # sql-formatter イメージを作る
docker run --rm -i sql-formatter < query.sql          # 標準入力を整形して標準出力へ
docker run --rm -v "$PWD:/src" sql-formatter --check .    # 整形されていない *.sql の一覧（あれば終了コード 1）
docker run --rm -v "$PWD:/src" sql-formatter --write db/  # ファイルを整形して上書き
docker run --rm -i sql-formatter --max-width 100 < query.sql  # 行幅を変える（既定は 80）
```

- ファイルを指定しなければ標準入力を整形します（`-` も標準入力）。1 つのファイルだけなら整形結果を標準出力に書きます。
- ディレクトリを指定すると、その下の `*.sql` を探します（`.` で始まるファイルやディレクトリは除きます）。
- `--write` は変わったファイルだけを書き換え、`--check` はファイルを書き換えません。
- 終了コード: 0 = 成功、1 = `--check` で整形されていないファイルがあった、2 = 引数や読み書きのエラー。

整形の設定（既定値は下のスタイル）:

| オプション | 値 | 既定 |
| --- | --- | --- |
| `-w`, `--max-width N` | 行幅 | 80 |
| `--indent N` | 字下げの幅（2〜8） | 4 |
| `--keyword-case` | `upper` / `lower` / `preserve`（入力のまま） | `upper` |
| `--comma` | `leading`（行頭）/ `trailing`（行末） | `leading` |

pre-commit では `args: [--indent, "2", --comma, trailing]` のように渡せます。

開発中は `docker compose run --rm -T dev cargo run -q -- [オプション] [ファイル]` でも動かせます。

### pre-commit

Docker があれば、Rust を入れなくても pre-commit のフックとして使えます（このリポジトリの `Dockerfile` でイメージを作ります）。

```yaml
# .pre-commit-config.yaml
repos:
  - repo: https://github.com/aoyagikouhei/sql-formatter-rs
    rev: main  # タグやコミットを指定する
    hooks:
      - id: sql-formatter        # 整形して書き換える
      # - id: sql-formatter-check  # 確かめるだけ（CI 向け）
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
- CREATE TABLE の列と制約は 1 行ずつ行頭カンマで並べる。CREATE INDEX / CREATE [MATERIALIZED] VIEW /
  ALTER TABLE / DROP / MERGE にも対応する
- CREATE TRIGGER は、タイミングとイベント・FOR EACH・WHEN・EXECUTE などの句を 1 行ずつにする
- COMMENT ON は 1 行にする（オブジェクトの種類と IS・NULL をキーワードとして大文字にする）
- TRUNCATE は 1 行にする
- 対応していない文（CREATE SEQUENCE / GRANT など）や解釈できない部分は、元のテキストのまま出す
- `COPY ... FROM STDIN;` に続くデータ（`\.` だけの行まで）は、元のテキストのまま出す


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

- GitHub Actions（`.github/workflows/ci.yml`）が push と PR ごとに、Docker Compose で `make check` を流します
  （PostgreSQL での実機検証を含む）。あわせて配布用イメージをビルドし、実際に整形できるかを確かめます。
  端末のない環境では `make check RUN_FLAGS=-T` とします。
- 単体テストは各モジュール内（`src/lexer.rs` など）に置いています。
- スナップショットテストは `tests/fixtures/<対象>/*.sql` を入力にし、結果を `tests/snapshots/` に保存します。
  ケースを増やすときは `.sql` を追加し、`make snapshot-review` で内容を確認してから承認します。
- `tests/formatter_properties.rs` は、すべてのフィクスチャとその変形について、整形でトークンやコメントが
  欠けないことと、2 回整形しても結果が変わらないことを確かめます。
- `tests/postgres_equivalence.rs` は、すべてのフィクスチャを整形の前と後で PostgreSQL（`db` コンテナ）に流し、
  結果が同じになることを確かめます（既定の設定、行幅 20、字下げ 2・小文字・行末カンマの 3 通り）。SELECT / DML は `EXPLAIN (VERBOSE, COSTS OFF, GENERIC_PLAN)` の
  実行計画と実行結果を、CREATE FUNCTION は本体以外のカタログ上の定義を、DO / 関数の呼び出しは NOTICE を含む出力を比べます。
  最後に public スキーマのカタログ（列・型・既定値・制約・インデックス・ビューの定義）も比べるので、DDL の違いも見つけられます。
  スキーマは `tests/postgres/schema.sql`、エラーなく実行できるべき検証用の SQL は `tests/fixtures/postgres/` にあります。
  全体を 1 つのトランザクションで流して最後に ROLLBACK するので、DB には何も残りません。
  環境変数 `PGHOST` がない環境（dev コンテナの外）では何もしません。
- `tests/parser_robustness.rs` は、すべてのフィクスチャを途中で切ったものとトークンを 1 つ抜いたものを構文解析し、
  止まらずに元のテキストを保つことを確かめます。
