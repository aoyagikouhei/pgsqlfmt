# pgsqlfmt の開発

- [開発環境](#開発環境)
- [コードの構成](#コードの構成)
- [設計の方針](#設計の方針)
- [テスト](#テスト)
- [新しい構文に対応する手順](#新しい構文に対応する手順)
- [CI](#ci)
- [配布物](#配布物)

## 開発環境

ローカルに Rust を入れず、Docker だけで開発します。必要なのは Docker と Docker Compose です。

| サービス | 内容 |
| --- | --- |
| `dev` | Rust（stable）、rustfmt、clippy、cargo-insta、psql |
| `db` | PostgreSQL 18（DB `sql_formatter`、ユーザーとパスワードは `postgres`） |

```sh
make up               # コンテナをビルドして起動
make test             # cargo test
make check            # fmt のチェック、clippy、test（コミットの前に流す）
make shell            # dev コンテナに入る
make psql             # PostgreSQL に接続
make snapshot-review  # スナップショットの差分を確認して承認
make db-reset         # DB を作り直す（docker/postgres/init を流し直す）
make image            # 配布用の pgsqlfmt イメージを作る
make down             # 停止
```

手元で作ったバイナリは、dev コンテナの中で動かします。

```sh
docker compose run --rm -T dev cargo run -q -- tests/fixtures/format/comments.sql
```

- **環境変数:** dev コンテナには `DATABASE_URL` と `PG*` の環境変数が入っています。テストやスクリプトから、そのまま `db` に接続できます。
- **ビルド成果物:** `target` と cargo のキャッシュは名前付きボリュームに置いていて、作業ディレクトリには出ません。
- **ユーザーの UID と GID:** ホストの UID と GID が 1000 以外なら、`cp .env.example .env` して `LOCAL_UID` と `LOCAL_GID` を合わせます。
- **ポート:** ホストの 5432 番が使用中なら、`.env` の `POSTGRES_PORT` を変えます。
- **端末がない環境:** CI のように端末がないときは、`make check RUN_FLAGS=-T` とします。

## コードの構成

入力を字句解析してトークン列にし、構文解析して構文木を作り、構文木を整形して文字列にします。

| ファイル | 役割 |
| --- | --- |
| `src/main.rs` | コマンドラインの引数、ファイルの読み書き、`--check` と `--write` |
| `src/lib.rs` | ライブラリの入口。`format` と `format_with_options` を公開する |
| `src/lexer.rs` | 字句解析。空白とコメントを含め、入力のすべてのバイトをトークンにする |
| `src/syntax.rs` | 構文木のノードの種類と、木を表示する補助 |
| `src/parser/mod.rs` | 構文解析の土台と、文の種類の振り分け（`statement`） |
| `src/parser/select.rs` | SELECT・VALUES・WITH・集合演算 |
| `src/parser/dml.rs` | INSERT・UPDATE・DELETE・MERGE |
| `src/parser/expr.rs` | 式と型名。二項演算は Pratt パーサーで読む |
| `src/parser/ddl.rs` | CREATE・ALTER・DROP・TRUNCATE・COMMENT ON・GRANT・REVOKE |
| `src/parser/function.rs` | CREATE FUNCTION・CREATE PROCEDURE・DO・CALL。本体の中身も解析する |
| `src/parser/plpgsql.rs` | PL/pgSQL の本体 |
| `src/parser/utility.rs` | COPY・SET・RESET・SHOW・EXPLAIN・トランザクション制御 |
| `src/parser/keywords.rs` | 予約語などのキーワードの分類 |
| `src/formatter/mod.rs` | 整形の入口と、ノードの種類ごとの振り分け |
| `src/formatter/stmt.rs` | 問い合わせと DML の句のレイアウト |
| `src/formatter/ddl.rs` | DDL・MERGE と、1 行に書く文のレイアウト |
| `src/formatter/plpgsql.rs` | 関数定義・DO・PL/pgSQL の本体のレイアウト |
| `src/formatter/wrap.rs` | 行幅による折り返し |
| `src/formatter/writer.rs` | 字下げ、トークンの間の空白、コメントの配置、キーワードの大文字小文字 |

## 設計の方針

- **手書きの字句解析と構文解析:** pg_query（libpg_query）、パーサーコンビネーター、パーサー生成器は使いません。
  pg_query はコメントを捨て、PL/pgSQL を文字列に戻せず、位置も行番号しか持たないので、整形の土台に向きません。
  字句規則は PostgreSQL の `scan.l` に、PL/pgSQL は `pl_gram.y` に合わせています。
- **ロスレスな構文木:** 構文木は空白とコメントを含むすべてのトークンを持ちます。木のトークンを順につなげると入力と一致します。
- **失敗しない:** 不正な入力や対応していない構文でもエラーにしません。
  解釈できない部分は `Error` か `RawStatement` のノードにして、整形では入力のまま書きます。
- **キーワードと名前:** キーワードかどうかは構文解析で決めます（`TokenKind::Keyword` に付け替える）。
  大文字小文字は書き出すときに設定どおりにします。名前の位置の語は、キーワードと同じ綴りでも名前として入力のまま残します。
- **意味を変えない:** 整形の前後で構文木が同じかを機械的に比べる仕組みは入れていません。
  意味が変わらないことは、下の性質テストと PostgreSQL での実機検証で確かめます。

## テスト

| テスト | 場所 | 確かめること |
| --- | --- | --- |
| 単体テスト | 各モジュールの中と `src/*/tests.rs` | 字句解析・構文解析・整形の個々の動き |
| スナップショットテスト | `tests/*_snapshots.rs` | フィクスチャの整形結果・トークン列・構文木が変わっていないこと |
| 性質テスト | `tests/formatter_properties.rs` | トークンとコメントが欠けないこと、2 回整形しても変わらないこと |
| 頑健性テスト | `tests/parser_robustness.rs` | 途中で切った入力やトークンを 1 つ抜いた入力でも止まらず、元のテキストを保つこと |
| CLI のテスト | `tests/cli.rs` | バイナリを起動したときのファイルの読み書きと終了コード |
| 実機検証 | `tests/postgres_equivalence.rs` | 整形の前と後で PostgreSQL での結果が同じこと |

### スナップショットテスト

フィクスチャは `tests/fixtures/<対象>/*.sql` に置き、結果を `tests/snapshots/` に保存します。

| フィクスチャ | 使うテスト |
| --- | --- |
| `tests/fixtures/*/*.sql`（すべて） | 整形の結果 |
| `tests/fixtures/lexer/*.sql` | トークン列 |
| `tests/fixtures/parser/*.sql`、`tests/fixtures/plpgsql/*.sql` | 構文木 |

ケースを増やすときは `.sql` を足し、`make snapshot-review` で差分を確認してから承認します。
性質テストと頑健性テストも、すべてのフィクスチャを入力にします。フィクスチャを足すと、これらのテストの対象も増えます。

### PostgreSQL での実機検証

`tests/fixtures/` のすべてのフィクスチャを、整形の前と後で PostgreSQL（`db` コンテナ）に流し、出力を比べます。
設定は、既定・行幅 20・字下げ 2 と小文字と行末カンマ、の 3 通りです。

- **SELECT と DML:** `EXPLAIN (VERBOSE, COSTS OFF, GENERIC_PLAN)` の実行計画と、実行した結果を比べます。
- **CREATE FUNCTION:** 本体以外のカタログ上の定義を比べます。
- **そのほかの文:** NOTICE やエラーを含む実行の結果を比べます。
- **カタログ:** 最後に、表と列、制約、インデックス、ビュー、トリガー、シーケンス、型、スキーマ、拡張、権限、既定の権限、コメントの定義を比べます。
  DDL の意味の違いは実行結果に出ないので、ここで見つけます。

`tests/fixtures/postgres/` のフィクスチャは、エラーなく実行できることも確かめます。検証用の表は `tests/postgres/schema.sql` で作ります。

全体を 1 つのトランザクションで流し、最後に ROLLBACK するので、DB には何も残りません。フィクスチャを書くときは次の点に気を付けます。

- **トランザクションを閉じる文:** BEGIN・COMMIT・ROLLBACK（セーブポイントへの ROLLBACK TO を除く）は書きません。外側のトランザクションが閉じてしまいます。
- **COPY ... FROM STDIN:** 書きません。検証では文の間に区切りの行を挟むので、データが壊れます。
- **ロール:** CREATE ROLE は書きません。並行して走るテスト同士で同じロールを作ろうとしてぶつかります。権限の確認には、組み込みのロール（`pg_read_all_data` など）と PUBLIC を使います。

環境変数 `PGHOST` がない環境（dev コンテナの外）では、このテストは何もしません。

## 新しい構文に対応する手順

対応していない文は `RawStatement` として入力のまま出ています。対応するときは、次の順に進めます。

1. **テストを書く:** `src/formatter/tests.rs` に、入力と期待する出力のテストを足し、落ちることを確かめます。
2. **ノードの種類を足す:** `src/syntax.rs` の `NodeKind` に、文と、必要なら句のノードを足します。
3. **構文解析を書く:** `src/parser/` の該当するファイルに解析を書き、`src/parser/mod.rs` の `statement` から振り分けます。
   名前の位置では `name_path` で読み、キーワードは `bump_kw` か `eat_kw` で取り込みます。
   解釈できない部分は `raw_until` で `Error` にして、入力のまま残します。
4. **整形を書く:** `src/formatter/mod.rs` の `statement` で、新しいノードをレイアウトの関数に振り分けます。
   新しい文のノードは `is_statement` にも足します。1 行に書く文なら、既定の振り分けのままで済みます。
5. **実機検証に足す:** `tests/fixtures/postgres/` のフィクスチャに文を足します。
   カタログで違いを見つける必要があれば、`tests/postgres_equivalence.rs` の `CATALOG_SNAPSHOT` に問い合わせを足します。
6. **スナップショットを承認する:** `make snapshot-review` で差分を確認して承認します。
7. **全体を流す:** `make check` で、fmt・clippy・すべてのテスト（実機検証を含む）を流します。

## CI

GitHub Actions（`.github/workflows/ci.yml`）が、main への push と PR ごとに 2 つのジョブを流します。

| ジョブ | 内容 |
| --- | --- |
| `check` | 手元と同じ Docker Compose の環境で `make check` を流す（実機検証を含む） |
| `image` | 配布用イメージをビルドし、実際に整形できることと `--check` が通ることを確かめる |

## 配布物

| ファイル | 内容 |
| --- | --- |
| `Dockerfile` | 配布用のイメージ。リリースビルドのバイナリだけを含む |
| `.pre-commit-hooks.yaml` | pre-commit のフック（`pgsqlfmt` と `pgsqlfmt-check`）。上のイメージで動く |

開発用のイメージは `docker/dev/Dockerfile` と `compose.yaml` で作ります。配布用のイメージとは別です。
