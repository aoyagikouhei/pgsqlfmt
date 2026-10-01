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
fn leading_bom_is_kept() {
    check(
        "\u{FEFF}select a, b from t",
        "\u{FEFF}SELECT\n    a\n  , b\nFROM t\n",
    );
    assert_eq!(format("\u{FEFF}"), "\u{FEFF}");
}

#[test]
fn newline_style_follows_the_input() {
    // 最初の改行が CRLF なら、出力の改行もすべて CRLF（文字列やコメントの中の改行はもとから入力のまま）
    check(
        "select a, -- c\r\n b from t where x = 1\r\n\r\nand y = 2;\r\n\r\n\r\nselect 'a\r\nb';",
        "SELECT\r\n    a -- c\r\n  , b\r\nFROM t\r\nWHERE x = 1\r\n    AND y = 2;\r\n\r\nSELECT 'a\r\nb';\r\n",
    );
    // 改行がなければ LF
    check("select 1", "SELECT 1\n");
    // 混在していれば最初の改行に合わせる
    check("select 1;\nselect 2;\r\n", "SELECT 1;\nSELECT 2;\n");
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
        "vacuum t\n  (a) -- 対象\n;\nselect 1",
        "vacuum t\n  (a) -- 対象\n;\nSELECT 1\n",
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
    // INTO を項目より前に書く `SELECT INTO target ...` は、SELECT の行に続ける
    check(
        "do $$ begin select into strict r * from t; select into a, b x, y from t; end $$",
        "\
DO $$
BEGIN
    SELECT INTO STRICT r *
    FROM t;
    SELECT INTO a, b
        x
      , y
    FROM t;
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

#[test]
fn copy_from_stdin_data_is_kept_as_is() {
    // データ行の引用符やセミコロンで、後ろの文の整形が止まらない
    check(
        "copy t (a, b) from stdin;\nit's;\tx\n\\.\nselect   2 from t;\n",
        "COPY t (a, b) FROM STDIN;\nit's;\tx\n\\.\nSELECT 2\nFROM t;\n",
    );
    // 空行・行末の空白もデータのまま。終わりの印の後の空行は残る
    check(
        "COPY t FROM STDIN WITH (FORMAT csv);\n\n1, 'a' \n\\.\n\nselect 1;\n",
        "COPY t FROM STDIN WITH (FORMAT csv);\n\n1, 'a' \n\\.\n\nSELECT 1;\n",
    );
    // 終わりの印がなければ入力の終わりまでがデータ
    check(
        "copy t from stdin;\n1\t2\n'\n",
        "COPY t FROM STDIN;\n1\t2\n'\n",
    );
    // 2 つ目以降の COPY のデータも入力のまま
    check(
        "copy a from stdin;\n1\n\\.\ncopy b from stdin;\nselect   1  ;\n2\tdon't\n\\.\nselect   1;\n",
        "COPY a FROM STDIN;\n1\n\\.\nCOPY b FROM STDIN;\nselect   1  ;\n2\tdon't\n\\.\nSELECT 1;\n",
    );
    // 終わりの印がなければ、行末のタブ（空の列）や空行もデータに残す
    check("copy t from stdin;\n1\t\n", "COPY t FROM STDIN;\n1\t\n");
    check("copy t from stdin;\n\t\n", "COPY t FROM STDIN;\n\t\n");
    check("copy t from stdin;\n1\n\n", "COPY t FROM STDIN;\n1\n\n");
    // 括弧の中の FROM stdin は COPY のデータの印ではない
    check(
        "copy (select * from stdin) to stdout;\nselect   1;\n",
        "COPY (\n    SELECT *\n    FROM stdin\n) TO STDOUT;\nSELECT 1;\n",
    );
    // FROM STDIN でない COPY の後ろは普通の文
    check(
        "copy t to stdout;\nselect 1;\n",
        "COPY t TO STDOUT;\nSELECT 1;\n",
    );
}

#[test]
fn tablesample_keywords_are_uppercased() {
    check(
        "select * from t tablesample system (10) repeatable (1)",
        "SELECT *\nFROM t TABLESAMPLE system (10) REPEATABLE (1)\n",
    );
    check(
        "select * from t as s tablesample bernoulli (5) where s.a = 1",
        "SELECT *\nFROM t AS s TABLESAMPLE bernoulli (5)\nWHERE s.a = 1\n",
    );
}

#[test]
fn create_trigger_clauses_are_one_per_line() {
    check(
        "create or replace trigger trg before insert or update of a, b on public.t for each row when (new.a is distinct from old.a) execute function f('x', 1)",
        "\
CREATE OR REPLACE TRIGGER trg
BEFORE INSERT OR UPDATE OF a, b ON public.t
FOR EACH ROW
WHEN (new.a IS DISTINCT FROM old.a)
EXECUTE FUNCTION f('x', 1)
",
    );
    check(
        "create constraint trigger trg after delete on t from u deferrable initially deferred for each row execute procedure f()",
        "\
CREATE CONSTRAINT TRIGGER trg
AFTER DELETE ON t
FROM u
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE PROCEDURE f()
",
    );
    check(
        "create trigger trg after update on t referencing old table as o new table n for each statement execute function f();\ncreate trigger v instead of insert on v for each row execute function g();",
        "\
CREATE TRIGGER trg
AFTER UPDATE ON t
REFERENCING OLD TABLE AS o NEW TABLE n
FOR EACH STATEMENT
EXECUTE FUNCTION f();
CREATE TRIGGER v
INSTEAD OF INSERT ON v
FOR EACH ROW
EXECUTE FUNCTION g();
",
    ); // UPDATE OF の列の後ろのイベントもキーワード
    check(
        "create trigger t before update of a, b or delete on t for each row execute function f()",
        "CREATE TRIGGER t\nBEFORE UPDATE OF a, b OR DELETE ON t\nFOR EACH ROW\nEXECUTE FUNCTION f()\n",
    );
    // 解釈できない部分（psql の変数など）は分けずに、前の句と同じ行にそのまま書く
    check(
        "create trigger t before insert on :tbl for each row execute function :fn();",
        "CREATE TRIGGER t\nBEFORE INSERT ON :tbl\nFOR EACH ROW\nEXECUTE FUNCTION :fn();\n",
    );
    check(
        "create trigger t after insert on x referencing new row as r for each row execute function f()",
        "CREATE TRIGGER t\nAFTER INSERT ON x\nREFERENCING NEW row as r\nFOR EACH ROW\nEXECUTE FUNCTION f()\n",
    );
}

#[test]
fn comment_on_statements() {
    check(
        "comment on table public.t is 'x';\ncomment on column t.c is null;\ncomment on materialized view mv is E'a\\'b';",
        "COMMENT ON TABLE public.t IS 'x';\nCOMMENT ON COLUMN t.c IS NULL;\nCOMMENT ON MATERIALIZED VIEW mv IS E'a\\'b';\n",
    );
    // 引数の括弧は名前に続ける。ON 表は同じ行
    check(
        "comment on function s.f(int, text) is 'f';\ncomment on constraint c on t is 'x';\ncomment on trigger trg on t is $$x$$;",
        "COMMENT ON FUNCTION s.f(int, text) IS 'f';\nCOMMENT ON CONSTRAINT c ON t IS 'x';\nCOMMENT ON TRIGGER trg ON t IS $$x$$;\n",
    );
    check(
        "comment on cast (text as int4) is 'x';\ncomment on large object 123 is 'x';\ncomment on operator + (int, int) is 'x';",
        "COMMENT ON CAST (text AS int4) IS 'x';\nCOMMENT ON LARGE OBJECT 123 IS 'x';\nCOMMENT ON OPERATOR + (int, int) IS 'x';\n",
    );
    // オブジェクトの種類と同じ綴りの名前はそのまま
    check(
        "comment on table data is 'x';\ncomment on column trigger.x is 'x';\ncomment on schema schema is 'x';",
        "COMMENT ON TABLE data IS 'x';\nCOMMENT ON COLUMN trigger.x IS 'x';\nCOMMENT ON SCHEMA schema IS 'x';\n",
    );
    check(
        "comment on transform for text language plperl is 'a';\ncomment on operator family text using btree is 'a';\ncomment on constraint c on domain d is 'a';",
        "COMMENT ON TRANSFORM FOR text LANGUAGE plperl IS 'a';\nCOMMENT ON OPERATOR FAMILY text USING btree IS 'a';\nCOMMENT ON CONSTRAINT c ON DOMAIN d IS 'a';\n",
    );
    // psql の変数や解釈できない部分は、元のまま同じ行に書く
    check(
        "comment on table :tbl is 'x';\ncomment on column :tbl.c is :'v';\ncomment on table t is U&'d\\0061t' uescape '!';\ncomment on column t.c is 'x' 'y';",
        "COMMENT ON TABLE :tbl IS 'x';\nCOMMENT ON COLUMN :tbl.c IS :'v';\nCOMMENT ON TABLE t IS U&'d\\0061t' uescape '!';\nCOMMENT ON COLUMN t.c IS 'x' 'y';\n",
    );
}

#[test]
fn truncate_statements() {
    check(
        "truncate t1, only s.t2 restart identity cascade;\ntruncate table t3 * continue identity restrict;\ntruncate identity;",
        "TRUNCATE t1, ONLY s.t2 RESTART IDENTITY CASCADE;\nTRUNCATE TABLE t3 * CONTINUE IDENTITY RESTRICT;\nTRUNCATE identity;\n",
    );
    // 子の表を含める `*` の後ろにも表が続く
    check("truncate t1*,t2", "TRUNCATE t1 *, t2\n");
    // PL/pgSQL の truncate という名前の変数への代入は TRUNCATE 文ではない
    check(
        "do $$ declare truncate int[]; drop record; begin truncate := '{5}'; truncate[1] := 1; drop.x = 2; truncate = '{}'; end $$",
        "DO $$\nDECLARE\n    truncate int[];\n    drop record;\nBEGIN\n    truncate := '{5}';\n    truncate[1] := 1;\n    drop.x = 2;\n    truncate = '{}';\nEND\n$$\n",
    );
    // psql の変数は分けずに元のまま書く
    check(
        "truncate table :tbl, t cascade;",
        "TRUNCATE TABLE :tbl, t CASCADE;\n",
    );
}

#[test]
fn create_sequence_options_are_one_per_line() {
    check(
        "create sequence if not exists s.seq as bigint increment by 2 minvalue -10 no maxvalue start with 10 cache 5 no cycle owned by t.id;\ncreate temp sequence s2;\ncreate unlogged sequence s3 start 1 owned by none",
        "\
CREATE SEQUENCE IF NOT EXISTS s.seq
AS bigint
INCREMENT BY 2
MINVALUE -10
NO MAXVALUE
START WITH 10
CACHE 5
NO CYCLE
OWNED BY t.id;
CREATE TEMP SEQUENCE s2;
CREATE UNLOGGED SEQUENCE s3
START 1
OWNED BY NONE
",
    );
}

#[test]
fn create_type_statements() {
    // 複合型の列は CREATE TABLE と同じく 1 行ずつ
    check(
        "create type pair as (a int, b text collate \"C\")",
        "CREATE TYPE pair AS (\n    a int\n  , b text COLLATE \"C\"\n)\n",
    );
    check(
        "create type mood as enum ('sad', 'ok');\ncreate type r as range (subtype = int4);\ncreate type shell;",
        "CREATE TYPE mood AS ENUM ('sad', 'ok');\nCREATE TYPE r AS RANGE (subtype = int4);\nCREATE TYPE shell;\n",
    );
}

#[test]
fn create_schema_and_extension_statements() {
    check(
        "create schema if not exists app authorization app_owner;\ncreate schema authorization joe;\ncreate extension if not exists \"uuid-ossp\" with schema public version '1.1' cascade;\ncreate extension pgcrypto",
        "\
CREATE SCHEMA IF NOT EXISTS app AUTHORIZATION app_owner;
CREATE SCHEMA AUTHORIZATION joe;
CREATE EXTENSION IF NOT EXISTS \"uuid-ossp\" WITH SCHEMA public VERSION '1.1' CASCADE;
CREATE EXTENSION pgcrypto
",
    );
}

#[test]
fn grant_and_revoke_statements() {
    check(
        "grant select, insert (a, key), update on table public.t, s.u to app_user, group staff, public with grant option granted by current_user;\ngrant all privileges on all tables in schema app to reader;\ngrant execute on function f(int, text) to app",
        "\
GRANT SELECT, INSERT (a, key), UPDATE ON TABLE public.t, s.u TO app_user, GROUP staff, PUBLIC WITH GRANT OPTION GRANTED BY CURRENT_USER;
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA app TO reader;
GRANT EXECUTE ON FUNCTION f(int, text) TO app
",
    );
    check(
        "revoke grant option for select on t from app cascade;\ngrant admin_role, data to joe with admin option;\nrevoke admin option for admin_role from joe;\ngrant usage on schema app to :role",
        "\
REVOKE GRANT OPTION FOR SELECT ON t FROM app CASCADE;
GRANT admin_role, data TO joe WITH ADMIN OPTION;
REVOKE ADMIN OPTION FOR admin_role FROM joe;
GRANT USAGE ON SCHEMA app TO :role
",
    );
}

#[test]
fn alter_statements() {
    // ALTER SEQUENCE のオプションは CREATE SEQUENCE と同じく 1 行ずつ
    check(
        "alter sequence if exists s.seq increment by 5 restart with 100 no cycle;\nalter sequence s owned by t.id;\nalter sequence s owner to app",
        "\
ALTER SEQUENCE IF EXISTS s.seq
INCREMENT BY 5
RESTART WITH 100
NO CYCLE;
ALTER SEQUENCE s
OWNED BY t.id;
ALTER SEQUENCE s OWNER TO app
",
    );
    check(
        "alter index if exists i rename to j;\nalter index i set tablespace fast;\nalter view v rename column a to data;\nalter materialized view mv set schema app;\nalter trigger trg on t rename to trg2",
        "\
ALTER INDEX IF EXISTS i RENAME TO j;
ALTER INDEX i SET TABLESPACE fast;
ALTER VIEW v RENAME COLUMN a TO data;
ALTER MATERIALIZED VIEW mv SET SCHEMA app;
ALTER TRIGGER trg ON t RENAME TO trg2
",
    );
    check(
        "alter function f(int, text) owner to current_user;\nalter function g() security definer set search_path = public;\nalter type mood add value if not exists 'meh' before 'ok';\nalter type pair add attribute c int, drop attribute if exists b cascade",
        "\
ALTER FUNCTION f(int, text) OWNER TO CURRENT_USER;
ALTER FUNCTION g() SECURITY DEFINER SET search_path = public;
ALTER TYPE mood ADD VALUE IF NOT EXISTS 'meh' BEFORE 'ok';
ALTER TYPE pair ADD ATTRIBUTE c int, DROP ATTRIBUTE IF EXISTS b CASCADE
",
    );
    check(
        "alter domain d set default 0;\nalter domain d add constraint pos check (value > 0) not valid;\nalter schema app owner to joe;\nalter extension pgcrypto update to '1.3';\nalter role joe with login password 'x' valid until 'infinity'",
        "\
ALTER DOMAIN d SET DEFAULT 0;
ALTER DOMAIN d ADD CONSTRAINT pos CHECK (value > 0) NOT VALID;
ALTER SCHEMA app OWNER TO joe;
ALTER EXTENSION pgcrypto UPDATE TO '1.3';
ALTER ROLE joe WITH LOGIN PASSWORD 'x' VALID UNTIL 'infinity'
",
    );
    // ON の後ろの表名はキーワードと同じ綴りでも名前。DEFAULT の後ろは式として整える
    check(
        "alter trigger trg on data rename to x;\nalter domain d set default lower ( 'X' )||'y'",
        "ALTER TRIGGER trg ON data RENAME TO x;\nALTER DOMAIN d SET DEFAULT lower('X') || 'y'\n",
    );
    // ALTER DEFAULT PRIVILEGES の後ろは GRANT / REVOKE。psql の変数は分けない
    check(
        "alter default privileges for role admin in schema app grant select on tables to reader;\nalter index :idx rename to :new_name",
        "\
ALTER DEFAULT PRIVILEGES FOR ROLE admin IN SCHEMA app GRANT SELECT ON TABLES TO reader;
ALTER INDEX :idx RENAME TO :new_name
",
    );
}

#[test]
fn psql_variables_stay_in_one_piece() {
    // 型や名前の位置の psql の変数は、前の語とくっつけない（くっつくと置き換えた値が名前とつながる）
    check(
        "create type t as (h :typ, b :typ[]);\ncreate type :t2 as (a int);\ncreate type e as enum (:'a', :'b');\ncreate schema :s authorization :u;\ndrop table :tbl;\nselect * from :tbl where id = :id and n = :\"col\"",
        "\
CREATE TYPE t AS (
    h :typ
  , b :typ[]
);
CREATE TYPE :t2 AS (
    a int
);
CREATE TYPE e AS ENUM (:'a', :'b');
CREATE SCHEMA :s AUTHORIZATION :u;
DROP TABLE :tbl;
SELECT *
FROM :tbl
WHERE id = :id
    AND n = :\"col\"
",
    );
    // 続けて書いた変数・引用符を重ねた変数・ドットでつないだ変数も 1 つのまま
    check(
        "select :'it''s', x from :a:b, :\"a\"\"b\";\nupdate :s.:t set a = 1",
        "SELECT\n    :'it''s'\n  , x\nFROM\n    :a:b\n  , :\"a\"\"b\";\nUPDATE :s.:t\nSET a = 1\n",
    );
    // 空白を挟んだ配列の範囲指定は詰めない（詰めると psql が `:n` を変数として置き換える）
    check(
        "select a[2: n], a[2 : n], a[ : n] from t",
        "SELECT\n    a[2: n]\n  , a[2: n]\n  , a[: n]\nFROM t\n",
    );
    // 配列の範囲指定と型変換は psql の変数ではない
    check(
        "select a[1:n], a[:2], b[lo:hi], c::int from t",
        "SELECT\n    a[1:n]\n  , a[:2]\n  , b[lo:hi]\n  , c::int\nFROM t\n",
    );
}

#[test]
fn create_sequence_no_takes_one_word_and_schema_elements_stay_verbatim() {
    check(
        "create sequence s no maxvalue cycle",
        "CREATE SEQUENCE s\nNO MAXVALUE\nCYCLE\n",
    );
    // 中に要素を書いた CREATE SCHEMA は、改行の位置を残すために元のまま
    check(
        "create schema s\n  create table t (a int)\n  create view v as select 1;\nselect 1",
        "create schema s\n  create table t (a int)\n  create view v as select 1;\nSELECT 1\n",
    );
}

#[test]
fn copy_statements() {
    check(
        "copy public.t (a, owner) from stdin with (format csv, header true, delimiter ';');\n1;2\n\\.\ncopy t to '/tmp/x.csv' (format csv) where a > 0;\ncopy (select a, b from t where x = 1) to stdout with csv header",
        "\
COPY public.t (a, owner) FROM STDIN WITH (
    FORMAT csv
  , HEADER true
  , DELIMITER ';'
);
1;2
\\.
COPY t TO '/tmp/x.csv' (FORMAT csv) WHERE a > 0;
COPY (
    SELECT
        a
      , b
    FROM t
    WHERE x = 1
) TO STDOUT WITH CSV HEADER
",
    );
}

#[test]
fn set_reset_show_statements() {
    check(
        "set search_path = app, public;\nset local work_mem to '64MB';\nset session time zone 'UTC';\nset role none;\nset transaction isolation level repeatable read, read only;\nset constraints all deferred;\nreset all;\nreset search_path;\nshow work_mem",
        "\
SET search_path = app, public;
SET LOCAL work_mem TO '64MB';
SET SESSION TIME ZONE 'UTC';
SET ROLE NONE;
SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY;
SET CONSTRAINTS ALL DEFERRED;
RESET ALL;
RESET search_path;
SHOW work_mem
",
    );
}

#[test]
fn explain_statements() {
    // 対象の文は次の行から整形する
    check(
        "explain analyze verbose select a from t where b = 1;\nexplain (analyze, buffers false, format json) update t set a = 1",
        "\
EXPLAIN ANALYZE VERBOSE
SELECT a
FROM t
WHERE b = 1;
EXPLAIN (ANALYZE, BUFFERS false, FORMAT json)
UPDATE t
SET a = 1
",
    );
}

#[test]
fn transaction_statements() {
    check(
        "savepoint work;\nrelease savepoint work;\nrollback to chain;\nbegin;\nbegin transaction isolation level serializable, read write;\nstart transaction read only, not deferrable;\nsavepoint sp1;\nrelease savepoint sp1;\nrollback to sp1;\nrollback and no chain;\ncommit work;\nend;\nabort;\nprepare transaction 'tx1';\ncommit prepared 'tx1'",
        "\
SAVEPOINT work;
RELEASE SAVEPOINT work;
ROLLBACK TO chain;
BEGIN;
BEGIN TRANSACTION ISOLATION LEVEL SERIALIZABLE, READ WRITE;
START TRANSACTION READ ONLY, NOT DEFERRABLE;
SAVEPOINT sp1;
RELEASE SAVEPOINT sp1;
ROLLBACK TO sp1;
ROLLBACK AND NO CHAIN;
COMMIT WORK;
END;
ABORT;
PREPARE TRANSACTION 'tx1';
COMMIT PREPARED 'tx1'
",
    );
}

#[test]
fn grant_and_alter_keep_names_and_spacing() {
    check(
        "alter function f(int) set schema public;\nalter role bob set search_path to public, s1;\nalter schema s rename to public;\nalter function g(int) set work_mem = default security definer",
        "\
ALTER FUNCTION f(int) SET SCHEMA public;
ALTER ROLE bob SET search_path TO public, s1;
ALTER SCHEMA s RENAME TO public;
ALTER FUNCTION g(int) SET work_mem = DEFAULT SECURITY DEFINER
",
    );
    check(
        "grant select on t to admin, Option;\ngrant Admins, devs to bob with inherit true, set false granted by Carol;\nalter policy p on t to bob using (a = current_user) with check (b > 0);\nalter extension e add cast (int as text)",
        "\
GRANT SELECT ON t TO admin, Option;
GRANT Admins, devs TO bob WITH INHERIT TRUE, SET FALSE GRANTED BY Carol;
ALTER POLICY p ON t TO bob USING (a = current_user) WITH CHECK (b > 0);
ALTER EXTENSION e ADD CAST (int AS text)
",
    );
}

#[test]
fn utility_statement_edge_cases() {
    // 括弧付きのオプションの値は、折り返しても 1 つのまま
    check(
        "COPY t TO STDOUT WITH (FORMAT csv, HEADER true, FORCE_QUOTE (a, b), FORCE_NOT_NULL (c, d));",
        "\
COPY t TO STDOUT WITH (
    FORMAT csv
  , HEADER true
  , FORCE_QUOTE (a, b)
  , FORCE_NOT_NULL (c, d)
);
",
    );
    // EXPLAIN の直後の括弧の問い合わせはオプションではない
    check(
        "explain (select 1) order by 1",
        "EXPLAIN\n(\n    SELECT 1\n)\nORDER BY 1\n",
    );
    check(
        "set session authorization default;\nset local session authorization 'bob';\nset local.x = 1;\nset session.x to 2;\nshow session.x;\nreset role.x;\nset role = none;\nset role to admin;\nset role admin",
        "\
SET SESSION AUTHORIZATION DEFAULT;
SET LOCAL SESSION AUTHORIZATION 'bob';
SET local.x = 1;
SET session.x TO 2;
SHOW session.x;
RESET role.x;
SET role = NONE;
SET role TO admin;
SET ROLE admin
",
    );
}

#[test]
fn alter_table_keeps_names_spelled_like_keywords() {
    check(
        "alter table t rename to data;\nalter table t rename column key to data;\nalter table t rename constraint c to key;\nalter table t set schema data;\nalter table t set tablespace data;\nalter table t attach partition data for values from (1) to (10);\nalter table t detach partition key;\nalter table t inherit data, no inherit key;\nalter table t enable trigger data, disable trigger all;\nalter table t replica identity using index key;\nalter table t owner to current_user",
        "\
ALTER TABLE t RENAME TO data;
ALTER TABLE t RENAME COLUMN key TO data;
ALTER TABLE t RENAME CONSTRAINT c TO key;
ALTER TABLE t SET SCHEMA data;
ALTER TABLE t SET TABLESPACE data;
ALTER TABLE t ATTACH PARTITION data FOR VALUES FROM (1) TO (10);
ALTER TABLE t DETACH PARTITION key;
ALTER TABLE t
    INHERIT data
  , NO INHERIT key;
ALTER TABLE t
    ENABLE TRIGGER data
  , DISABLE TRIGGER ALL;
ALTER TABLE t REPLICA IDENTITY USING INDEX key;
ALTER TABLE t OWNER TO CURRENT_USER
",
    );
}

#[test]
fn items_with_aliases_wrap_by_their_full_width() {
    // 別名まで含めた長さで、行幅に収まるかを判定する
    check(
        "select format_name(customer_first_name, customer_last_name, customer_title) as display_name from customers",
        "\
SELECT format_name(
    customer_first_name
  , customer_last_name
  , customer_title
) AS display_name
FROM customers
",
    );
    // 別名の幅に行頭の字下げは入れない（78 桁で収まる）。後ろのコメントは数えない（入力によって付く先が変わる）
    check(
        "select rank() over (partition by m.month order by m.total desc nulls last) as rnk, 1 from m",
        "\
SELECT
    rank() OVER (PARTITION BY m.month ORDER BY m.total DESC NULLS LAST) AS rnk
  , 1
FROM m
",
    );
    check(
        "select rank() over (partition by m.month order by m.total desc nulls last) as rnk /* 月内順位 */, 1 from m",
        "\
SELECT
    rank() OVER (PARTITION BY m.month ORDER BY m.total DESC NULLS LAST) AS rnk /* 月内順位 */
  , 1
FROM m
",
    );
    // 別名そのものを書くときは、別名の幅を差し引かない（78 桁で収まる）
    check(
        "select * from generate_series(1, 2) as g(first_column_name, second_column_name, third)",
        "SELECT *\nFROM generate_series(1, 2) AS g (first_column_name, second_column_name, third)\n",
    );
    // 括弧を折り返したあとの行では、別名の幅を差し引かない（別名は閉じ括弧の行に来る）
    check(
        "select format_name(customer_first_name, coalesce(customer_middle_name, customer_nickname, ''), customer_last_name) as display_name_with_a_long_alias from customers",
        "\
SELECT format_name(
    customer_first_name
  , coalesce(customer_middle_name, customer_nickname, '')
  , customer_last_name
) AS display_name_with_a_long_alias
FROM customers
",
    );
    check(
        "select format_name(customer_first_name, customer_last_name, customer_title) as display_name, 1 from customers",
        "\
SELECT
    format_name(
        customer_first_name
      , customer_last_name
      , customer_title
    ) AS display_name
  , 1
FROM customers
",
    );
}
