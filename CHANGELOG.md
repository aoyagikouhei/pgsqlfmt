# 変更履歴

このファイルには、利用者から見える変更を版ごとに書きます。
形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、版の付け方は [Semantic Versioning](https://semver.org/lang/ja/) に従います。

## [Unreleased]

## [0.2.0] - 2026-10-02

### 追加

- PL/pgSQL の `SELECT INTO target expr, ... FROM ...`（INTO を項目より前に書く形）を整形する。`SELECT INTO STRICT r *` のように SELECT の行に続ける
- `timestamp with time zone '...'` / `double precision '1'` / `character varying 'x'` のような複数語の型付きリテラルを式として読む
- `GROUP BY` の `ROLLUP (...)` / `CUBE (...)` を `GROUPING SETS` と同じくキーワードとして大文字にする
- `IN :list` の psql の変数を IN の式として同じ行に書く

### 変更

- 改行コードを入力の最初の改行に合わせる。CRLF のファイルは CRLF のまま整形し、`--check` も通る（これまでは常に LF に変えていた）
- 入力の先頭の BOM を残す（これまでは BOM が `select` にくっついて、文全体が整形されなかった）
- 別名の列の並びを名前に続けて書く（`AS g (n, i)` → `AS g(n, i)`）
- `--write` は同じディレクトリの一時ファイルに書いてから置き換え、元のファイルの権限を保つ（途中で止まっても元のファイルが欠けない）
- 単項の `+` / `-` を `COLLATE` / `AT TIME ZONE` より強く結び付ける（PostgreSQL の文法に合わせた。整形結果は変わらない）

### 修正

- `:'a''` のように重ねた引用符の途中で終わる psql の変数で、字句解析器が panic していた
- 閉じていない文字列・コメントで入力が終わるとき、その中身の末尾の空白を削り、改行を足していた

## [0.1.0] - 2026-10-01

最初のリリース。

### 追加

- 手書きの字句解析器・構文解析器による、PostgreSQL の SQL と PL/pgSQL のフォーマッター。不正な入力や対応していない構文でも失敗せず、解釈できない部分は入力のまま残す
- SELECT（WITH・集合演算・ウィンドウ関数・LATERAL・TABLESAMPLE など）、INSERT / UPDATE / DELETE / MERGE、VALUES、TABLE の整形
- DDL の整形: CREATE TABLE / INDEX / VIEW / MATERIALIZED VIEW / TRIGGER / SEQUENCE / TYPE / SCHEMA / EXTENSION、ALTER TABLE とそのほかの ALTER、DROP、TRUNCATE、COMMENT ON、GRANT / REVOKE
- ユーティリティ文の整形: COPY（`FROM STDIN` のデータは入力のまま残す）、SET / RESET / SHOW、EXPLAIN、トランザクション制御
- CREATE FUNCTION / PROCEDURE、DO、`BEGIN ATOMIC` の整形。`LANGUAGE plpgsql` の本体は PL/pgSQL として、`LANGUAGE sql` の本体は SQL として中身も整形する
- 句ごとの改行、行頭カンマ、キーワードの大文字化、行幅による折り返し（全角文字は 2 桁）
- オプション: `--max-width`、`--indent`、`--keyword-case upper|lower|preserve`、`--comma leading|trailing`
- CLI: 標準入力の整形、ファイル・ディレクトリの `--write` / `--check`
- psql の変数（`:name` / `:'name'` / `:"name"`）を 1 つの名前として扱う
- 配布: GitHub Releases の Linux（x86_64 / aarch64、musl 静的リンク）バイナリ、Docker イメージ、pre-commit フック
- MIT ライセンス

[Unreleased]: https://github.com/aoyagikouhei/pgsqlfmt/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/aoyagikouhei/pgsqlfmt/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/aoyagikouhei/pgsqlfmt/releases/tag/v0.1.0
