use super::{CommaStyle, FormatOptions, KeywordCase, format, format_with_options};

/// 整形結果が期待どおりで、もう一度整形しても変わらないこと
#[track_caller]
fn check(input: &str, expected: &str) {
    let formatted = format(input);
    assert_eq!(formatted, expected, "\n--- 実際 ---\n{formatted}");
    assert_eq!(format(&formatted), formatted, "2 回目の整形で変わった");
}

/// 行幅を指定した `check`
#[track_caller]
fn check_width(max_width: usize, input: &str, expected: &str) {
    let options = FormatOptions {
        max_width,
        ..FormatOptions::default()
    };
    let formatted = format_with_options(input, &options);
    assert_eq!(formatted, expected, "\n--- 実際 ---\n{formatted}");
    assert_eq!(
        format_with_options(&formatted, &options),
        formatted,
        "2 回目の整形で変わった"
    );
}

/// 設定を指定した `check`
#[track_caller]
fn check_with(options: FormatOptions, input: &str, expected: &str) {
    let formatted = format_with_options(input, &options);
    assert_eq!(formatted, expected, "\n--- 実際 ---\n{formatted}");
    assert_eq!(
        format_with_options(&formatted, &options),
        formatted,
        "2 回目の整形で変わった"
    );
}

#[test]
fn empty_input() {
    assert_eq!(format(""), "");
    assert_eq!(format("  \n "), "");
}

#[test]
fn single_item_stays_on_clause_line() {
    check("select a from t", "SELECT a\nFROM t\n");
}

#[test]
fn multiple_items_use_leading_commas() {
    check(
        "select a, b as c, count(*) cnt from t group by a, b order by a desc, b",
        "\
SELECT
    a
  , b AS c
  , count(*) cnt
FROM t
GROUP BY
    a
  , b
ORDER BY
    a DESC
  , b
",
    );
}

#[test]
fn distinct_on_stays_on_select_line() {
    check(
        "select distinct on (a) a, b from t",
        "SELECT DISTINCT ON (a)\n    a\n  , b\nFROM t\n",
    );
}

#[test]
fn keywords_are_uppercased_but_names_are_kept() {
    check(
        r#"Select DISTINCT Foo, "Bar", Count(x), now()::Date from Baz where x is not null and y = true"#,
        "\
SELECT DISTINCT
    Foo
  , \"Bar\"
  , Count(x)
  , now()::Date
FROM Baz
WHERE x IS NOT NULL
    AND y = TRUE
",
    );
    // 型名の中の語は型名の一部として入力のまま
    check(
        "select x::timestamp with time zone, cast(y as double precision)",
        "\
SELECT
    x::timestamp with time zone
  , CAST(y AS double precision)
",
    );
}

#[test]
fn and_or_chains_break_at_top_level() {
    check(
        "select 1 from t where a = 1 and (b = 2 or c = 3) and d",
        "\
SELECT 1
FROM t
WHERE a = 1
    AND (b = 2 OR c = 3)
    AND d
",
    );
    // AND は OR より強いので、最上位は OR
    check(
        "select 1 where a and b or c",
        "\
SELECT 1
WHERE a AND b
    OR c
",
    );
    check(
        "select 1 from t group by a having count(*) > 1 and max(b) < 2",
        "\
SELECT 1
FROM t
GROUP BY a
HAVING count(*) > 1
    AND max(b) < 2
",
    );
}

#[test]
fn joins() {
    check(
        "select * from a join b on a.id = b.id and b.x = 1 left outer join c using (id) cross join d",
        "\
SELECT *
FROM a
    JOIN b
        ON a.id = b.id
            AND b.x = 1
    LEFT OUTER JOIN c
        USING (id)
    CROSS JOIN d
",
    );
    // FROM の関数も、関数名と括弧の間に空白を入れない
    check(
        "select * from generate_series(1, 3) g",
        "SELECT *\nFROM generate_series(1, 3) g\n",
    );
    // 括弧で囲んだ結合は、中身を 1 段深くする
    check(
        "select * from (a join b on true) j",
        "\
SELECT *
FROM (
    a
        JOIN b
            ON TRUE
) j
",
    );
    // FROM の項目が複数なら、JOIN はその項目より 1 段深い
    check(
        "select * from a join b on true, c",
        "\
SELECT *
FROM
    a
        JOIN b
            ON TRUE
  , c
",
    );
}

#[test]
fn subqueries() {
    check(
        "select (select max(x) from s) as m from (select 1 as x) q where x in (select y from u) and exists (select 1)",
        "\
SELECT (
    SELECT max(x)
    FROM s
) AS m
FROM (
    SELECT 1 AS x
) q
WHERE x IN (
        SELECT y
        FROM u
    )
    AND EXISTS (
        SELECT 1
    )
",
    );
    // 行頭カンマの項目の中では、項目の位置を基準にする
    check(
        "select a, (select 1) b",
        "\
SELECT
    a
  , (
        SELECT 1
    ) b
",
    );
}

#[test]
fn case_expressions() {
    check(
        "select case when a then 1 when b then case x when 1 then 'y' end else 3 end as v",
        "\
SELECT CASE
    WHEN a THEN 1
    WHEN b THEN CASE x
        WHEN 1 THEN 'y'
    END
    ELSE 3
END AS v
",
    );
}

#[test]
fn nested_with_clauses() {
    // 2 つ目以降の CTE のカンマは、深い位置でも CTE の名前と同じ列に置く
    check(
        "select * from (with a as (select 1), b as (select 2) select 1) q",
        "\
SELECT *
FROM (
    WITH a AS (
        SELECT 1
    )
    , b AS (
        SELECT 2
    )
    SELECT 1
) q
",
    );
}

#[test]
fn set_operations_and_paren_selects() {
    check(
        "select 1 union all (select 2 order by 1) intersect select 3 order by 1 limit 2 offset 1",
        "\
SELECT 1
UNION ALL
(
    SELECT 2
    ORDER BY 1
)
INTERSECT
SELECT 3
ORDER BY 1
LIMIT 2
OFFSET 1
",
    );
}

#[test]
fn with_clauses() {
    check(
        "with recursive a as (select 1), b (x) as materialized (select 2) select * from a, b",
        "\
WITH RECURSIVE a AS (
    SELECT 1
)
, b (x) AS MATERIALIZED (
    SELECT 2
)
SELECT *
FROM
    a
  , b
",
    );
}

#[test]
fn insert_statements() {
    check(
        "insert into t (a, b) values (1, 'x'), (2, default) on conflict (a) do nothing returning *",
        "\
INSERT INTO t (a, b)
VALUES
    (1, 'x')
  , (2, DEFAULT)
ON CONFLICT (a) DO NOTHING
RETURNING *
",
    );
    check(
        "with s as (select 1 a) insert into t select a from s on conflict on constraint pk do update set a = excluded.a, b = 2 where t.a <> 0 and t.b is null",
        "\
WITH s AS (
    SELECT 1 a
)
INSERT INTO t
SELECT a
FROM s
ON CONFLICT ON CONSTRAINT pk DO UPDATE
    SET
        a = excluded.a
      , b = 2
    WHERE t.a <> 0
        AND t.b IS NULL
",
    );
    check(
        "insert into t default values",
        "INSERT INTO t DEFAULT VALUES\n",
    );
    // DO より前の WHERE（競合の対象の条件）は同じ行
    check(
        "insert into t values (1) on conflict (a) where b do nothing",
        "INSERT INTO t\nVALUES (1)\nON CONFLICT (a) WHERE b DO NOTHING\n",
    );
}

#[test]
fn update_and_delete_statements() {
    check(
        "update t x set a = 1 from s where s.id = x.id returning x.a, x.b",
        "\
UPDATE t x
SET a = 1
FROM s
WHERE s.id = x.id
RETURNING
    x.a
  , x.b
",
    );
    check(
        "with x as (select 1) update t set a = 1",
        "WITH x AS (\n    SELECT 1\n)\nUPDATE t\nSET a = 1\n",
    );
    check(
        "delete from t using s, u where t.id = s.id",
        "\
DELETE FROM t
USING
    s
  , u
WHERE t.id = s.id
",
    );
}

#[test]
fn token_spacing() {
    check(
        "select - a, + b, not c, f ( x , y ), a [ 1 : 2 ], t . *, x :: int [ ], array [ 1 ], row ( 1 ), $1, e'\\n', interval '1 day', count ( * ) over ( partition by a order by b rows between unbounded preceding and current row )",
        "\
SELECT
    -a
  , +b
  , NOT c
  , f(x, y)
  , a[1:2]
  , t.*
  , x::int[]
  , ARRAY[1]
  , ROW(1)
  , $1
  , e'\\n'
  , interval '1 day'
  , count(*) OVER (
        PARTITION BY a
        ORDER BY b
        ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
    )
",
    );
}

#[test]
fn string_continuations_keep_their_newlines() {
    // 同じ行に並べると構文エラーになるので、続きは次の行に書く
    check(
        "select 'a'\n'b' as s, 1",
        "SELECT\n    'a'\n        'b' AS s\n  , 1\n",
    );
}

#[test]
fn prefix_operator_does_not_merge_with_operand() {
    // `--a` にするとコメントになり、`-@a` は別の演算子になる
    check(
        "select - -a, - @ b, -(1)",
        "\
SELECT
    - -a
  , - @b
  , -(1)
",
    );
}

#[test]
fn loose_clauses_uppercase_known_keywords() {
    check(
        "select 1 from t fetch first 1 rows only for update of t skip locked",
        "\
SELECT 1
FROM t
FETCH FIRST 1 ROWS ONLY
FOR UPDATE OF t SKIP LOCKED
",
    );
}

#[test]
fn statements_are_separated_and_blank_lines_kept() {
    check(
        "select 1;select 2;\n\n\nselect 3",
        "SELECT 1;\nSELECT 2;\n\nSELECT 3\n",
    );
}

#[test]
fn blank_lines_around_comments_are_kept() {
    check(
        "select 1;\n\n-- 区切り\n\nselect 2;\n-- 説明\nselect 3",
        "SELECT 1;\n\n-- 区切り\n\nSELECT 2;\n-- 説明\nSELECT 3\n",
    );
    check("select 1;\n\n\n-- end", "SELECT 1;\n\n-- end\n");
}

#[test]
fn raw_statements_are_kept_verbatim() {
    check(
        "create sequence s\n  start 1 -- 開始\n;\nselect 1",
        "create sequence s\n  start 1 -- 開始\n;\nSELECT 1\n",
    );
}

#[test]
fn comments() {
    check(
        "\
-- head
select a, -- after a
  /* before b */ b
from t -- after t
-- before where
where x -- after x
;
-- tail",
        "\
-- head
SELECT
    a -- after a
  , /* before b */ b
FROM t -- after t
-- before where
WHERE x -- after x
;
-- tail
",
    );
    // 項目の前の独立した行のコメントは、行頭カンマより前に出す
    check(
        "select a,\n  -- before b\n  b,\n  /* c1 */\n  /* c2 */ c\nfrom t",
        "\
SELECT
    a
    -- before b
  , b
    /* c1 */
  , /* c2 */ c
FROM t
",
    );
    check(
        "with a as (select 1),\n-- b の説明\nb as (select 2) select 1",
        "\
WITH a AS (
    SELECT 1
)
-- b の説明
, b AS (
    SELECT 2
)
SELECT 1
",
    );
    // 同じ行に並んでいたコメントは同じ行のまま
    check(
        "select 1\n/* A */ /* B */\nfrom t",
        "SELECT 1\n/* A */ /* B */\nFROM t\n",
    );
    // 行コメントで終わる行には、後ろのコメントを続けない（行コメントに飲み込まれる）
    check(
        "select a -- x\n, -- y\nb",
        "SELECT\n    a -- x\n  , -- y\n    b\n",
    );
    // 行コメントの後ろの短い式は折り返さない
    check(
        "select 1 where -- c\n a = 1",
        "SELECT 1\nWHERE -- c\n    a = 1\n",
    );
    // 行コメントを含む式は 1 行にできないので、演算子の前で折り返す
    check(
        "select 1 + -- x\n/* y */ 2",
        "SELECT 1\n    + -- x\n    /* y */ 2\n",
    );
    // CTE のカンマの行末コメントは、前の CTE の行末に残す
    check(
        "with a as (select 1), -- a の後\nb as (select 2) select 1",
        "\
WITH a AS (
    SELECT 1
) -- a の後
, b AS (
    SELECT 2
)
SELECT 1
",
    );
    // 式の途中の行コメントの後ろは改行する（1 行にできないので折り返す）
    check("select a + -- c\n b", "SELECT a\n    + -- c\n    b\n");
    check("select /* x */ 1 /* y */", "SELECT /* x */ 1 /* y */\n");
}

#[test]
fn broken_input_is_kept() {
    check("select (a from t", "SELECT (a from t\n");
    check("select 1) from t", "SELECT 1\n) from t\n");
    check("select exists ()", "SELECT EXISTS ()\n");
    // 右辺のない AND は、それ全体を 1 つの被演算子にする
    check(
        "select 1 where a and and b",
        "SELECT 1\nWHERE a AND\n    AND b\n",
    );
}

// ---- 関数・PL/pgSQL ----

#[test]
fn create_function_options_are_one_per_line() {
    check(
        "create or replace function f(a int, b text default 'x') returns table (x int, y text) language sql stable security definer set search_path = public as $$ select 1 $$",
        "\
CREATE OR REPLACE FUNCTION f(a int, b text DEFAULT 'x')
RETURNS TABLE (x int, y text)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public
AS $$
SELECT 1
$$
",
    );
    // SQL の本体の文の間の空行は残す
    check(
        "create function f() returns int language sql as $$\nselect 1;\n\nselect 2;\n$$",
        "CREATE FUNCTION f()\nRETURNS int\nLANGUAGE sql\nAS $$\nSELECT 1;\n\nSELECT 2;\n$$\n",
    );
    // ほかの言語の本体はそのまま
    check(
        "create function f() returns int as $$\n  return 1\n$$ language plpython3u",
        "CREATE FUNCTION f()\nRETURNS int\nAS $$\n  return 1\n$$\nLANGUAGE plpython3u\n",
    );
}

#[test]
fn plpgsql_block_layout() {
    check(
        "do $$ <<outer>> declare a int := 0; b t.c%type; begin a := 1; if a > 0 then raise notice 'x %', a; elsif a < 0 then null; else return; end if; exception when others then raise; end outer $$",
        "\
DO $$
<<outer>>
DECLARE
    a int := 0;
    b t.c%type;
BEGIN
    a := 1;
    IF a > 0 THEN
        RAISE NOTICE 'x %', a;
    ELSIF a < 0 THEN
        NULL;
    ELSE
        RETURN;
    END IF;
EXCEPTION
    WHEN others THEN
        RAISE;
END outer
$$
",
    );
}

#[test]
fn plpgsql_loops_and_case() {
    check(
        "do $$ begin <<l>> for i in 1..10 loop exit l when i > 5; end loop l; for r in select a, b from t loop continue; end loop; case x when 1 then null; else null; end case; end $$",
        "\
DO $$
BEGIN
    <<l>>
    FOR i IN 1..10 LOOP
        EXIT l WHEN i > 5;
    END LOOP l;
    FOR r IN
        SELECT
            a
          , b
        FROM t
    LOOP
        CONTINUE;
    END LOOP;
    CASE x
        WHEN 1 THEN
            NULL;
        ELSE
            NULL;
    END CASE;
END
$$
",
    );
}

#[test]
fn plpgsql_embedded_sql() {
    check(
        "do $$ begin select a into strict x from t where id = 1; update t set a = 1 where id = 2 returning a into y; insert into t values (1) returning id into z; return query select 1; open c for select 2; open d(1); perform f(); end $$",
        "\
DO $$
BEGIN
    SELECT a
    INTO STRICT x
    FROM t
    WHERE id = 1;
    UPDATE t
    SET a = 1
    WHERE id = 2
    RETURNING a
    INTO y;
    INSERT INTO t
    VALUES (1)
    RETURNING id
    INTO z;
    RETURN QUERY
        SELECT 1;
    OPEN c FOR
        SELECT 2;
    OPEN d(1);
    PERFORM f();
END
$$
",
    );
    check(
        "do $$ begin perform f(x), g(x) from t where a; end $$",
        "\
DO $$
BEGIN
    PERFORM
        f(x)
      , g(x)
    FROM t
    WHERE a;
END
$$
",
    );
}

#[test]
fn plpgsql_statements_keep_blank_lines_and_comments() {
    check(
        "do $$\nbegin\n\n  -- 最初\n  a := 1; -- 行末\n\n  /* 空行のあと */\n  b := 2;\nend\n$$",
        "\
DO $$
BEGIN
    -- 最初
    a := 1; -- 行末

    /* 空行のあと */
    b := 2;
END
$$
",
    );
}

#[test]
fn raise_exception_stays_on_one_line() {
    check(
        "do $$ begin raise exception 'x' using errcode = 'P0001'; fetch next from c into r; end $$",
        "\
DO $$
BEGIN
    RAISE EXCEPTION 'x' USING ERRCODE = 'P0001';
    FETCH NEXT FROM c INTO r;
END
$$
",
    );
}

#[test]
fn atomic_bodies_and_call() {
    check(
        "create procedure p(x int) begin atomic insert into t values (x); select 1; end; call p(1)",
        "\
CREATE PROCEDURE p(x int)
BEGIN ATOMIC
    INSERT INTO t
    VALUES (x);
    SELECT 1;
END;
CALL p(1)
",
    );
}

// ---- 行幅による折り返し ----

#[test]
fn default_width_is_80() {
    // ちょうど 80 桁は折り返さず、81 桁で折り返す
    let fits = format!("SELECT f(1, {})\n", "a".repeat(80 - "SELECT f(1, )".len()));
    assert_eq!(format(&fits), fits);
    let long = format!("SELECT f(1, {})", "a".repeat(81 - "SELECT f(1, )".len()));
    assert!(format(&long).starts_with("SELECT f(\n"));
    // 項目が 1 つなら、行幅を超えても折り返さない
    let single = format!("SELECT f({})", "a".repeat(81 - "SELECT f()".len()));
    assert_eq!(format(&single), format!("{single}\n"));
}

#[test]
fn long_argument_lists_are_broken() {
    check_width(
        30,
        "select coalesce(first_name, last_name, 'unknown') as name",
        "\
SELECT coalesce(
    first_name
  , last_name
  , 'unknown'
) AS name
",
    );
    // 外側を折り返したあと、内側が収まれば 1 行のまま
    check_width(
        30,
        "select f(g(a, b), h(c, d), i(e, f))",
        "\
SELECT f(
    g(a, b)
  , h(c, d)
  , i(e, f)
)
",
    );
    // 内側も収まらなければ、さらに折り返す
    check_width(
        20,
        "select f(g(aaaa, bbbb, cccc), 1)",
        "\
SELECT f(
    g(
        aaaa
      , bbbb
      , cccc
    )
  , 1
)
",
    );
}

#[test]
fn long_lists_in_parentheses_are_broken() {
    check_width(
        30,
        "select 1 where a in (1000, 2000, 3000, 4000)",
        "\
SELECT 1
WHERE a IN (
        1000
      , 2000
      , 3000
      , 4000
    )
",
    );
    check_width(
        30,
        "insert into t (alpha, beta, gamma) values (1, 2, 3)",
        "\
INSERT INTO t (
    alpha
  , beta
  , gamma
)
VALUES (1, 2, 3)
",
    );
    check_width(
        30,
        "create function f(a int, b text, c numeric) returns int as $$ select 1 $$ language sql",
        "\
CREATE FUNCTION f(
    a int
  , b text
  , c numeric
)
RETURNS int
AS $$
SELECT 1
$$
LANGUAGE sql
",
    );
}

#[test]
fn long_binary_expressions_break_before_operators() {
    check_width(
        30,
        "select first_name || ' ' || middle_name || ' ' || last_name as full_name",
        "\
SELECT first_name
    || ' '
    || middle_name
    || ' '
    || last_name AS full_name
",
    );
    // WHERE の条件の中の式は、AND の行よりさらに 1 段深く折り返す
    check_width(
        30,
        "select 1 where a = 1 and total_amount + tax_amount > limit_amount",
        "\
SELECT 1
WHERE a = 1
    AND total_amount
            + tax_amount
        > limit_amount
",
    );
}

#[test]
fn long_window_specs_are_broken() {
    check_width(
        40,
        "select rank() over (partition by dept order by salary desc) from emp",
        "\
SELECT rank() OVER (
    PARTITION BY dept
    ORDER BY salary DESC
)
FROM emp
",
    );
}

#[test]
fn long_raise_and_execute_break_before_options() {
    check_width(
        40,
        "do $$ begin raise exception 'failed: %', reason using errcode = 'P0001'; execute q into r using a, b; end $$",
        "\
DO $$
BEGIN
    RAISE EXCEPTION 'failed: %', reason
        USING ERRCODE = 'P0001';
    EXECUTE q INTO r USING a, b;
END
$$
",
    );
}

#[test]
fn comments_in_broken_lists_stay_with_items() {
    check_width(
        30,
        "select f(aaaa, -- a の説明\n bbbb, cccc)",
        "\
SELECT f(
    aaaa -- a の説明
  , bbbb
  , cccc
)
",
    );
}

#[test]
fn lists_containing_multiline_parts_are_broken() {
    // 副問い合わせは複数行になるので、それを含む引数の並びは折り返す。
    // 測っているあいだに中のコメントを動かさない
    check(
        "select coalesce((select a, -- a の説明\n b from t), 0)",
        "\
SELECT coalesce(
    (
        SELECT
            a -- a の説明
          , b
        FROM t
    )
  , 0
)
",
    );
    // 複数行の文字列や、前に空行のあるコメントを含む並びも 1 行にはしない
    check("select f('a\nb', c)", "SELECT f(\n    'a\nb'\n  , c\n)\n");
    check(
        "select f(a,\n\n/* c */ b)",
        "SELECT f(\n    a\n  ,\n\n    /* c */ b\n)\n",
    );
    // 独立した行のコメントは、行頭カンマより前に出す
    check_width(
        20,
        "select f(aaaa,\n-- b の前\nbbbb, cccc)",
        "\
SELECT f(
    aaaa
    -- b の前
  , bbbb
  , cccc
)
",
    );
}

#[test]
fn wide_characters_count_as_two_columns() {
    // 全角 10 文字は 20 桁なので 35 桁（文字数で数えると 25 桁）
    check_width(
        35,
        "select f(1, 'あいうえおかきくけこ')",
        "SELECT f(1, 'あいうえおかきくけこ')\n",
    );
    check_width(
        34,
        "select f(1, 'あいうえおかきくけこ')",
        "SELECT f(\n    1\n  , 'あいうえおかきくけこ'\n)\n",
    );
}

// ---- DDL・MERGE ----

#[test]
fn create_table_layout() {
    // 列が 1 つでも 1 行ずつ
    check(
        "create table t (id int)",
        "CREATE TABLE t (\n    id int\n)\n",
    );
    check(
        "create table if not exists t (id bigint primary key, name text not null default '', constraint c check (id > 0)) with (fillfactor = 70)",
        "\
CREATE TABLE IF NOT EXISTS t (
    id bigint PRIMARY KEY
  , name text NOT NULL DEFAULT ''
  , CONSTRAINT c CHECK (id > 0)
) WITH (fillfactor = 70)
",
    );
    check(
        "create table t2 as select a, b from t with no data",
        "CREATE TABLE t2 AS\nSELECT\n    a\n  , b\nFROM t\nWITH NO DATA\n",
    );
}

#[test]
fn index_view_alter_drop_layout() {
    check(
        "create index i on t (a) where b > 0",
        "CREATE INDEX i ON t (a)\nWHERE b > 0\n",
    );
    check(
        "create view v as select a from t with local check option",
        "CREATE VIEW v AS\nSELECT a\nFROM t\nWITH LOCAL CHECK OPTION\n",
    );
    // 操作が 1 つなら 1 行、2 つ以上なら行頭カンマ
    check(
        "alter table t add column c int",
        "ALTER TABLE t ADD COLUMN c int\n",
    );
    check(
        "alter table t add column c int, drop column d",
        "ALTER TABLE t\n    ADD COLUMN c int\n  , DROP COLUMN d\n",
    );
    check(
        "drop function if exists f(int, text) cascade",
        "DROP FUNCTION IF EXISTS f(int, text) CASCADE\n",
    );
}

#[test]
fn merge_layout() {
    check(
        "merge into t using s on s.id = t.id and s.x = 1 when matched then update set v = s.v when not matched then insert (id, v) values (s.id, s.v) when not matched by source then delete",
        "\
MERGE INTO t
USING s
    ON s.id = t.id
        AND s.x = 1
WHEN MATCHED THEN
    UPDATE SET v = s.v
WHEN NOT MATCHED THEN
    INSERT (id, v)
    VALUES (s.id, s.v)
WHEN NOT MATCHED BY SOURCE THEN
    DELETE
",
    );
}

// ---- 設定 ----

const SAMPLE: &str = "select a, b from t join u on t.id = u.id where x = 1 and y = 2";

#[test]
fn indent_width() {
    let options = FormatOptions {
        indent_width: 2,
        ..FormatOptions::default()
    };
    check_with(
        options.clone(),
        SAMPLE,
        "\
SELECT
  a
, b
FROM t
  JOIN u
    ON t.id = u.id
WHERE x = 1
  AND y = 2
",
    );
    check_with(
        options,
        "do $$ begin if a then return; end if; end $$",
        "DO $$\nBEGIN\n  IF a THEN\n    RETURN;\n  END IF;\nEND\n$$\n",
    );
    check_with(
        FormatOptions {
            indent_width: 8,
            ..FormatOptions::default()
        },
        "select a, b",
        "SELECT\n        a\n      , b\n",
    );
}

#[test]
fn keyword_case() {
    let lower = FormatOptions {
        keyword_case: KeywordCase::Lower,
        ..FormatOptions::default()
    };
    check_with(
        lower,
        "SELECT Count(*) FROM T WHERE X IS NOT NULL FOR UPDATE SKIP LOCKED",
        "select Count(*)\nfrom T\nwhere X is not null\nfor update skip locked\n",
    );
    let preserve = FormatOptions {
        keyword_case: KeywordCase::Preserve,
        ..FormatOptions::default()
    };
    check_with(
        preserve,
        "Select a From t wHeRe x Is Null",
        "Select a\nFrom t\nwHeRe x Is Null\n",
    );
}

#[test]
fn trailing_commas() {
    let options = FormatOptions {
        comma_style: CommaStyle::Trailing,
        max_width: 30,
        ..FormatOptions::default()
    };
    check_with(
        options.clone(),
        "with a as (select 1), b as (select 2) select x, coalesce(first_name, last_name, 'unknown') as name from a",
        "\
WITH a AS (
    SELECT 1
),
b AS (
    SELECT 2
)
SELECT
    x,
    coalesce(
        first_name,
        last_name,
        'unknown'
    ) AS name
FROM a
",
    );
    check_with(
        options.clone(),
        "create table t (id int, name text); alter table t add column c int, drop column d",
        "\
CREATE TABLE t (
    id int,
    name text
);
ALTER TABLE t
    ADD COLUMN c int,
    DROP COLUMN d
",
    );
    // コメントは項目に付いたまま
    check_with(
        options,
        "select a, -- a の説明\n-- b の前\nb",
        "SELECT\n    a, -- a の説明\n    -- b の前\n    b\n",
    );
}
