# pgsqlfmt の使い方

PostgreSQL の SQL と PL/pgSQL（ストアドプロシージャ・関数・DO）を整形するコマンドラインツールです。

- [インストール](#インストール)
- [基本の使い方](#基本の使い方)
- [オプション](#オプション)
- [pre-commit で使う](#pre-commit-で使う)
- [整形のスタイル](#整形のスタイル)
- [対応している文](#対応している文)
- [元のまま出すもの](#元のまま出すもの)

## インストール

### cargo でインストールする

Rust（cargo）が入っていれば、GitHub から直接ビルドしてインストールできます。

```sh
cargo install --git https://github.com/aoyagikouhei/pgsqlfmt
```

`~/.cargo/bin/pgsqlfmt` に入ります。

### Docker で使う

Rust を入れずに使うときは、Docker イメージを作ります。

```sh
docker build -t pgsqlfmt https://github.com/aoyagikouhei/pgsqlfmt.git
```

イメージの作業ディレクトリは `/src` です。整形したいディレクトリを `/src` にマウントして使います。

```sh
docker run --rm -i pgsqlfmt < query.sql                  # 標準入力を整形
docker run --rm -v "$PWD:/src" pgsqlfmt --check .        # 整形されていないファイルの一覧
docker run --rm -v "$PWD:/src" pgsqlfmt --write db/      # ファイルを整形して上書き
```

## 基本の使い方

```sh
pgsqlfmt < query.sql             # 標準入力を整形して標準出力へ
pgsqlfmt query.sql               # ファイルを整形して標準出力へ（ファイルは書き換えない）
pgsqlfmt --write db/             # db/ の下の *.sql を整形して上書き
pgsqlfmt --check .               # 整形されていない *.sql の一覧を出す（書き換えない）
```

- ファイルを指定しなければ、標準入力を整形して標準出力に書きます。`-` も標準入力を表します。
- ディレクトリを指定すると、その下の `*.sql` を探します。`.` で始まるファイルとディレクトリは除きます。
- 2 つ以上のファイルやディレクトリを指定するときは、`--write` か `--check` が必要です。
- `--write` は、整形で中身が変わったファイルだけを書き換えます。
- `--check` はファイルを書き換えません。CI での確認に使います。

終了コードは次のとおりです。

| 終了コード | 意味 |
| --- | --- |
| 0 | 成功 |
| 1 | `--check` で、整形されていないファイルがあった |
| 2 | 引数の誤り、またはファイルの読み書きのエラー |

## オプション

| オプション | 値 | 既定 |
| --- | --- | --- |
| `-w`, `--max-width N` | 行幅 | 80 |
| `--indent N` | 字下げの幅（2〜8） | 4 |
| `--keyword-case` | `upper` / `lower` / `preserve`（入力のまま） | `upper` |
| `--comma` | `leading`（行頭）/ `trailing`（行末） | `leading` |
| `--write` | ファイルを整形結果で上書きする | |
| `--check` | 整形されていなければ一覧を出して終了コード 1（書き換えない） | |
| `-h`, `--help` | 説明を表示する | |

既定のスタイルと、`--indent 2 --comma trailing --keyword-case lower` を指定したときの違いです。

```sql
-- 既定
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

## pre-commit で使う

[pre-commit](https://pre-commit.com/) のフックとして使えます。フックは Docker で動くので、Rust は要りません。

```yaml
# .pre-commit-config.yaml
repos:
  - repo: https://github.com/aoyagikouhei/pgsqlfmt
    rev: main  # タグやコミットを指定する
    hooks:
      - id: pgsqlfmt          # 整形して書き換える
      # - id: pgsqlfmt-check  # 確かめるだけ（CI 向け）
```

| フック | 動き |
| --- | --- |
| `pgsqlfmt` | `*.sql` を整形して書き換える |
| `pgsqlfmt-check` | 整形されていない `*.sql` があれば失敗する（書き換えない） |

オプションは `args` で渡します。

```yaml
      - id: pgsqlfmt
        args: [--indent, "2", --comma, trailing]
```

## 整形のスタイル

### 問い合わせ

句ごとに改行します。並びの項目が 2 つ以上なら 1 行ずつ字下げし、カンマは行頭に置きます。
WHERE・HAVING・ON の AND と OR で改行します。JOIN は FROM より 1 段、ON はさらに 1 段深くします。

```sql
-- 入力
select c.id, c.name, count(o.id) as orders from customers c left join orders o on o.customer_id = c.id and o.status <> 'cancelled' where c.active and c.created_at >= '2026-01-01' group by c.id, c.name order by orders desc limit 10;
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
            AND o.status <> 'cancelled'
WHERE c.active
    AND c.created_at >= '2026-01-01'
GROUP BY
    c.id
  , c.name
ORDER BY orders DESC
LIMIT 10;
```

- 副問い合わせと CASE は複数行にします。
- キーワードは大文字にします。表・列・関数・型の名前は入力のまま残します。
- コメントと、文やコメントの前後の空行は残します。

### 行幅による折り返し

行幅（既定 80。全角文字は 2 桁と数える）に収まらない式は折り返します。

- 関数の引数や `IN (...)` などの括弧の中の並びは、1 行ずつ行頭カンマで並べます。
- 二項演算は演算子の前で折り返します。
- `OVER (...)` は句ごとに改行します。
- PL/pgSQL の RAISE と EXECUTE は、`USING` と `INTO` の前で改行します。

```sql
SELECT format_name(
    customer_first_name
  , customer_middle_name
  , customer_last_name
  , customer_title
)
FROM customers;
```

### 関数と PL/pgSQL

`CREATE FUNCTION` と `CREATE PROCEDURE` は、RETURNS・LANGUAGE・AS などのオプションを 1 行ずつにします。
本体は `LANGUAGE plpgsql` なら PL/pgSQL として、`LANGUAGE sql` なら SQL として中身も整形します。
`DO` の本体と `BEGIN ATOMIC ... END` も整形します。ほかの言語の本体はそのまま残します。

PL/pgSQL は DECLARE・BEGIN・EXCEPTION・END をブロックの深さに置き、文を 1 段深くします。
IF・CASE・LOOP・WHILE・FOR・FOREACH の中はさらに 1 段深くします。

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

CREATE TABLE の列と制約は、1 つでも必ず 1 行ずつ行頭カンマで並べます。

```sql
CREATE TABLE orders (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY
  , customer_id bigint NOT NULL REFERENCES customers (id)
  , status text NOT NULL DEFAULT 'new'
  , created_at timestamptz NOT NULL DEFAULT now()
);
```

文ごとのレイアウトは [対応している文](#対応している文) を見てください。

## 対応している文

| 文 | レイアウト |
| --- | --- |
| SELECT・VALUES・WITH・集合演算 | 句ごとに改行する |
| INSERT・UPDATE・DELETE・MERGE | 句ごとに改行する。MERGE は USING と WHEN を行頭に置く |
| CREATE FUNCTION・CREATE PROCEDURE・DO・CALL | オプションを 1 行ずつ並べ、本体の中身も整形する |
| CREATE TABLE | 列と制約を 1 行ずつ並べる |
| CREATE TYPE | 複合型の列は CREATE TABLE と同じく 1 行ずつ。ENUM・RANGE などは 1 行 |
| CREATE SEQUENCE・ALTER SEQUENCE | オプションを 1 行ずつ並べる |
| CREATE TRIGGER | タイミングとイベント・FOR EACH・WHEN・EXECUTE などの句を 1 行ずつ並べる |
| CREATE INDEX・CREATE [MATERIALIZED] VIEW | 1 行。VIEW の問い合わせは次の行から、INDEX の WHERE は次の行に書く |
| ALTER TABLE | 操作が 2 つ以上なら 1 行ずつ並べる |
| ほかの ALTER（INDEX・VIEW・FUNCTION・TYPE・DOMAIN・SCHEMA・ROLE・DEFAULT PRIVILEGES など） | 1 行 |
| CREATE SCHEMA・CREATE EXTENSION・DROP・TRUNCATE・COMMENT ON | 1 行 |
| GRANT・REVOKE | 1 行。権限・オブジェクトの種類・PUBLIC などを大文字にする |
| COPY | 1 行。`COPY (query)` の問い合わせは副問い合わせと同じく複数行にする |
| SET・RESET・SHOW | 1 行 |
| EXPLAIN | オプションの次の行から、対象の文を整形する |
| BEGIN・COMMIT・ROLLBACK・SAVEPOINT などのトランザクション制御 | 1 行 |

キーワードと同じ綴りの名前（`RENAME TO data` の `data` など）は、名前の位置にあれば入力のまま残します。

## 元のまま出すもの

整形で SQL の意味が変わらないように、次のものは入力のまま出します。

- **対応していない文:** VACUUM・LOCK などの文は、文全体を入力のまま出します。
- **解釈できない部分:** 構文として読めない部分は、その部分だけを入力のまま出します。
- **COPY のデータ:** `COPY ... FROM STDIN;` に続くデータは、`\.` だけの行まで入力のまま出します。
- **文字列とコメントの中身:** 文字列・引用符付きの名前・コメントの中身は変えません。

psql の変数（`:name`・`:'name'`・`:"name"`）は 1 つの名前として扱い、前後の語とくっつけません。
くっつけると、psql が置き換えた値が前の語とつながってしまうためです。

psql のメタコマンド（`\set` など）の行の直後の文は、整形せずに入力のまま出します。
