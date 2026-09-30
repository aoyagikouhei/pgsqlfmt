use super::format;

/// 整形結果が期待どおりで、もう一度整形しても変わらないこと
#[track_caller]
fn check(input: &str, expected: &str) {
    let formatted = format(input);
    assert_eq!(formatted, expected, "\n--- 実際 ---\n{formatted}");
    assert_eq!(format(&formatted), formatted, "2 回目の整形で変わった");
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
  , count(*) OVER (PARTITION BY a ORDER BY b ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)
",
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
        "create table t (\n  id int -- key\n);\nselect 1",
        "create table t (\n  id int -- key\n);\nSELECT 1\n",
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
    check("select 1 + -- x\n/* y */ 2", "SELECT 1 + -- x\n/* y */ 2\n");
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
    // 式の途中の行コメントの後ろは改行する
    check("select a + -- c\n b", "SELECT a + -- c\nb\n");
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
