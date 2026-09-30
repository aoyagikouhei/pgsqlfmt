use super::parse;
use crate::syntax::{Element, Node, NodeKind};

/// 空白・コメントを省いた S 式。トークンは元のテキストのまま出す。
fn sexpr(node: &Node) -> String {
    let parts: Vec<String> = node
        .children
        .iter()
        .filter_map(|child| match child {
            Element::Node(n) => Some(sexpr(n)),
            Element::Token(t) if t.kind.is_trivia() => None,
            Element::Token(t) => Some(t.text.to_string()),
        })
        .collect();
    format!("({:?} {})", node.kind, parts.join(" "))
}

fn parse_checked(src: &str) -> Node<'_> {
    let root = parse(src);
    assert_eq!(root.text(), src, "木のテキストが入力と一致すること");
    root
}

/// 文全体の S 式（Root は省く）
fn stmts(src: &str) -> String {
    let root = parse_checked(src);
    let inner = sexpr(&root);
    inner
        .strip_prefix("(Root ")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap()
        .to_string()
}

fn find<'n, 'a>(node: &'n Node<'a>, kind: NodeKind) -> Option<&'n Node<'a>> {
    if node.kind == kind {
        return Some(node);
    }
    node.children.iter().find_map(|child| match child {
        Element::Node(n) => find(n, kind),
        Element::Token(_) => None,
    })
}

/// `SELECT <src>` の最初の選択項目の式
fn expr(src: &str) -> String {
    let full = format!("SELECT {src}");
    let root = parse_checked(&full);
    let item = find(&root, NodeKind::TargetItem).expect("選択項目がない");
    let Element::Node(e) = &item.children[0] else {
        panic!("選択項目の先頭がノードでない")
    };
    // 式の後ろに読み残しがないこと
    assert_eq!(
        e.text(),
        src,
        "式が入力全体を覆っていない: {}",
        sexpr(&root)
    );
    sexpr(e)
}

// ---- 式 ----

#[test]
fn arithmetic_precedence_and_associativity() {
    assert_eq!(
        expr("a + b * c"),
        "(BinaryExpr (ColumnRef a) + (BinaryExpr (ColumnRef b) * (ColumnRef c)))"
    );
    assert_eq!(
        expr("a - b - c"),
        "(BinaryExpr (BinaryExpr (ColumnRef a) - (ColumnRef b)) - (ColumnRef c))"
    );
    assert_eq!(
        expr("2 ^ 3 ^ 4"),
        "(BinaryExpr (BinaryExpr (Literal 2) ^ (Literal 3)) ^ (Literal 4))"
    );
    assert_eq!(
        expr("a * b % c / d"),
        "(BinaryExpr (BinaryExpr (BinaryExpr (ColumnRef a) * (ColumnRef b)) % (ColumnRef c)) / (ColumnRef d))"
    );
    // `^` は `*` より強い
    assert_eq!(
        expr("a * b ^ c"),
        "(BinaryExpr (ColumnRef a) * (BinaryExpr (ColumnRef b) ^ (ColumnRef c)))"
    );
    assert_eq!(
        expr("a + b ^ c"),
        "(BinaryExpr (ColumnRef a) + (BinaryExpr (ColumnRef b) ^ (ColumnRef c)))"
    );
}

#[test]
fn logical_precedence() {
    assert_eq!(
        expr("a OR b AND c"),
        "(BinaryExpr (ColumnRef a) OR (BinaryExpr (ColumnRef b) AND (ColumnRef c)))"
    );
    assert_eq!(
        expr("NOT a = b"),
        "(PrefixExpr NOT (BinaryExpr (ColumnRef a) = (ColumnRef b)))"
    );
    assert_eq!(
        expr("NOT a AND b"),
        "(BinaryExpr (PrefixExpr NOT (ColumnRef a)) AND (ColumnRef b))"
    );
}

#[test]
fn comparison_and_other_operators() {
    // `||` などは比較より強い
    assert_eq!(
        expr("a || b = c"),
        "(BinaryExpr (BinaryExpr (ColumnRef a) || (ColumnRef b)) = (ColumnRef c))"
    );
    // 算術は `||` などより強い
    assert_eq!(
        expr("a = b || c"),
        "(BinaryExpr (ColumnRef a) = (BinaryExpr (ColumnRef b) || (ColumnRef c)))"
    );
    assert_eq!(
        expr("a || b + c"),
        "(BinaryExpr (ColumnRef a) || (BinaryExpr (ColumnRef b) + (ColumnRef c)))"
    );
    assert_eq!(
        expr("a = b IS NULL"),
        "(IsExpr (BinaryExpr (ColumnRef a) = (ColumnRef b)) IS NULL)"
    );
    assert_eq!(
        expr("x -> 'k' ->> 'v'"),
        "(BinaryExpr (BinaryExpr (ColumnRef x) -> (Literal 'k')) ->> (Literal 'v'))"
    );
}

#[test]
fn unary_operators() {
    assert_eq!(
        expr("-a * b"),
        "(BinaryExpr (PrefixExpr - (ColumnRef a)) * (ColumnRef b))"
    );
    assert_eq!(
        expr("-a::int"),
        "(PrefixExpr - (CastExpr (ColumnRef a) :: (TypeName int)))"
    );
    assert_eq!(
        expr("a * -b"),
        "(BinaryExpr (ColumnRef a) * (PrefixExpr - (ColumnRef b)))"
    );
    assert_eq!(
        expr("@ a + b"),
        "(PrefixExpr @ (BinaryExpr (ColumnRef a) + (ColumnRef b)))"
    );
}

#[test]
fn is_expressions() {
    assert_eq!(expr("a IS NOT NULL"), "(IsExpr (ColumnRef a) IS NOT NULL)");
    assert_eq!(expr("a ISNULL"), "(IsExpr (ColumnRef a) ISNULL)");
    assert_eq!(expr("a NOTNULL"), "(IsExpr (ColumnRef a) NOTNULL)");
    assert_eq!(
        expr("a IS NOT DISTINCT FROM b + 1"),
        "(IsExpr (ColumnRef a) IS NOT DISTINCT FROM (BinaryExpr (ColumnRef b) + (Literal 1)))"
    );
    // IS は比較より弱いので、IS DISTINCT FROM の右辺に IS は入らない
    assert_eq!(
        expr("a IS DISTINCT FROM b IS NULL"),
        "(IsExpr (IsExpr (ColumnRef a) IS DISTINCT FROM (ColumnRef b)) IS NULL)"
    );
    assert_eq!(
        expr("a IS NFC NORMALIZED"),
        "(IsExpr (ColumnRef a) IS NFC NORMALIZED)"
    );
    assert_eq!(
        expr("a IS JSON OBJECT"),
        "(IsExpr (ColumnRef a) IS JSON OBJECT)"
    );
    assert_eq!(expr("a IS TRUE"), "(IsExpr (ColumnRef a) IS TRUE)");
}

#[test]
fn pattern_expressions() {
    assert_eq!(
        expr("a BETWEEN 1 AND 2 + 3 AND c"),
        "(BinaryExpr (BetweenExpr (ColumnRef a) BETWEEN (Literal 1) AND (BinaryExpr (Literal 2) + (Literal 3))) AND (ColumnRef c))"
    );
    assert_eq!(
        expr("a NOT BETWEEN SYMMETRIC 1 AND 2"),
        "(BetweenExpr (ColumnRef a) NOT BETWEEN SYMMETRIC (Literal 1) AND (Literal 2))"
    );
    assert_eq!(
        expr("a NOT IN (1, 2)"),
        "(InExpr (ColumnRef a) NOT IN (ExprList ( (Literal 1) , (Literal 2) )))"
    );
    assert_eq!(
        expr("a IN (SELECT b FROM t)"),
        "(InExpr (ColumnRef a) IN (SubqueryExpr ( (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef b))) (FromClause FROM (TableRef t)))) )))"
    );
    assert_eq!(
        expr("a NOT LIKE 'x!%' ESCAPE '!'"),
        "(LikeExpr (ColumnRef a) NOT LIKE (Literal 'x!%') ESCAPE (Literal '!'))"
    );
    assert_eq!(
        expr("a ILIKE b || '%'"),
        "(LikeExpr (ColumnRef a) ILIKE (BinaryExpr (ColumnRef b) || (Literal '%')))"
    );
    assert_eq!(
        expr("a NOT SIMILAR TO 'x'"),
        "(LikeExpr (ColumnRef a) NOT SIMILAR TO (Literal 'x'))"
    );
    // LIKE は比較より強い
    assert_eq!(
        expr("a LIKE b = c"),
        "(BinaryExpr (LikeExpr (ColumnRef a) LIKE (ColumnRef b)) = (ColumnRef c))"
    );
    assert_eq!(
        expr("a = b AND c LIKE d"),
        "(BinaryExpr (BinaryExpr (ColumnRef a) = (ColumnRef b)) AND (LikeExpr (ColumnRef c) LIKE (ColumnRef d)))"
    );
}

#[test]
fn postfix_expressions() {
    assert_eq!(
        expr("a[1]"),
        "(SubscriptExpr (ColumnRef a) [ (Literal 1) ])"
    );
    assert_eq!(
        expr("a[1:2][:3]"),
        "(SubscriptExpr (SubscriptExpr (ColumnRef a) [ (Literal 1) : (Literal 2) ]) [ : (Literal 3) ])"
    );
    assert_eq!(
        expr("(a).b"),
        "(FieldAccess (ParenExpr ( (ColumnRef a) )) . b)"
    );
    assert_eq!(
        expr("(a).*"),
        "(FieldAccess (ParenExpr ( (ColumnRef a) )) . *)"
    );
    assert_eq!(
        expr("x AT TIME ZONE 'UTC'"),
        "(AtTimeZoneExpr (ColumnRef x) AT TIME ZONE (Literal 'UTC'))"
    );
    assert_eq!(
        expr("x AT LOCAL"),
        "(AtTimeZoneExpr (ColumnRef x) AT LOCAL)"
    );
    assert_eq!(
        expr("a COLLATE \"C\" < b"),
        "(BinaryExpr (CollateExpr (ColumnRef a) COLLATE \"C\") < (ColumnRef b))"
    );
}

#[test]
fn primaries() {
    assert_eq!(expr("t.*"), "(ColumnRef t . *)");
    assert_eq!(expr("s.t.\"C\""), "(ColumnRef s . t . \"C\")");
    assert_eq!(expr("$1"), "(ParamRef $1)");
    assert_eq!(expr("NULL"), "(Literal NULL)");
    assert_eq!(expr("current_timestamp"), "(ColumnRef current_timestamp)");
    assert_eq!(
        expr("current_timestamp(3)"),
        "(FuncCall current_timestamp (ArgList ( (Literal 3) )))"
    );
    assert_eq!(expr("interval '1 day'"), "(TypedLiteral interval '1 day')");
    assert_eq!(
        expr("(1, 2)"),
        "(RowExpr (ExprList ( (Literal 1) , (Literal 2) )))"
    );
    assert_eq!(expr("()"), "(RowExpr (ExprList ( )))");
    assert_eq!(expr("ROW(1)"), "(RowExpr ROW (ExprList ( (Literal 1) )))");
    assert_eq!(
        expr("((a))"),
        "(ParenExpr ( (ParenExpr ( (ColumnRef a) )) ))"
    );
    assert_eq!(
        expr("(SELECT 1)"),
        "(SubqueryExpr ( (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) ))"
    );
    assert_eq!(
        expr("EXISTS (SELECT 1)"),
        "(ExistsExpr EXISTS (SubqueryExpr ( (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) )))"
    );
    assert_eq!(
        expr("ARRAY[[1, 2], [3]]"),
        "(ArrayExpr ARRAY [ (ArrayExpr [ (Literal 1) , (Literal 2) ]) , (ArrayExpr [ (Literal 3) ]) ])"
    );
    assert_eq!(
        expr("a = ANY(ARRAY[1])"),
        "(BinaryExpr (ColumnRef a) = (FuncCall ANY (ArgList ( (ArrayExpr ARRAY [ (Literal 1) ]) ))))"
    );
}

#[test]
fn case_and_cast() {
    assert_eq!(
        expr("CASE WHEN a THEN 1 WHEN b THEN 2 ELSE 3 END"),
        "(CaseExpr CASE (WhenClause WHEN (ColumnRef a) THEN (Literal 1)) (WhenClause WHEN (ColumnRef b) THEN (Literal 2)) (ElseClause ELSE (Literal 3)) END)"
    );
    assert_eq!(
        expr("CASE x WHEN 1 THEN 'a' END"),
        "(CaseExpr CASE (ColumnRef x) (WhenClause WHEN (Literal 1) THEN (Literal 'a')) END)"
    );
    assert_eq!(
        expr("CAST(a AS numeric(10, 2))"),
        "(CastCall CAST ( (ColumnRef a) AS (TypeName numeric ( 10 , 2 )) ))"
    );
}

#[test]
fn type_names() {
    let cast = |ty: &str| {
        let e = expr(&format!("x::{ty}"));
        let prefix = "(CastExpr (ColumnRef x) :: ";
        e.strip_prefix(prefix)
            .unwrap()
            .strip_suffix(')')
            .unwrap()
            .to_string()
    };
    assert_eq!(cast("int[]"), "(TypeName int [ ])");
    assert_eq!(cast("int[3][]"), "(TypeName int [ 3 ] [ ])");
    assert_eq!(cast("double precision"), "(TypeName double precision)");
    assert_eq!(
        cast("character varying(10)"),
        "(TypeName character varying ( 10 ))"
    );
    assert_eq!(
        cast("national character varying"),
        "(TypeName national character varying)"
    );
    assert_eq!(
        cast("timestamp(3) with time zone"),
        "(TypeName timestamp ( 3 ) with time zone)"
    );
    assert_eq!(
        cast("time without time zone"),
        "(TypeName time without time zone)"
    );
    assert_eq!(
        cast("interval day to second(3)"),
        "(TypeName interval day to second ( 3 ))"
    );
    assert_eq!(cast("pg_catalog.int4"), "(TypeName pg_catalog . int4)");
    assert_eq!(cast("\"MyType\""), "(TypeName \"MyType\")");
}

#[test]
fn function_calls() {
    assert_eq!(
        expr("count(*)"),
        "(FuncCall count (ArgList ( (ColumnRef *) )))"
    );
    assert_eq!(
        expr("count(DISTINCT a)"),
        "(FuncCall count (ArgList ( DISTINCT (ColumnRef a) )))"
    );
    assert_eq!(
        expr("string_agg(a, ',' ORDER BY b DESC)"),
        "(FuncCall string_agg (ArgList ( (ColumnRef a) , (Literal ',') (OrderByClause ORDER BY (SortItem (ColumnRef b) DESC)) )))"
    );
    assert_eq!(
        expr("position('a' IN b)"),
        "(FuncCall position (ArgList ( (Literal 'a') IN (ColumnRef b) )))"
    );
    assert_eq!(
        expr("extract(year FROM d)"),
        "(FuncCall extract (ArgList ( (ColumnRef year) FROM (ColumnRef d) )))"
    );
    assert_eq!(
        expr("substring(s FROM 1 FOR 2)"),
        "(FuncCall substring (ArgList ( (ColumnRef s) FROM (Literal 1) FOR (Literal 2) )))"
    );
    assert_eq!(
        expr("trim(BOTH 'x' FROM s)"),
        "(FuncCall trim (ArgList ( BOTH (Literal 'x') FROM (ColumnRef s) )))"
    );
    assert_eq!(
        expr("f(a => 1, b := 2)"),
        "(FuncCall f (ArgList ( (BinaryExpr (ColumnRef a) => (Literal 1)) , (ColumnRef b) := (Literal 2) )))"
    );
    assert_eq!(expr("s.f()"), "(FuncCall s . f (ArgList ( )))");
}

#[test]
fn aggregate_and_window_suffixes() {
    assert_eq!(
        expr("count(*) FILTER (WHERE x > 0)"),
        "(FuncCall count (ArgList ( (ColumnRef *) )) (FilterClause FILTER ( (WhereClause WHERE (BinaryExpr (ColumnRef x) > (Literal 0))) )))"
    );
    assert_eq!(
        expr("percentile_cont(0.5) WITHIN GROUP (ORDER BY x)"),
        "(FuncCall percentile_cont (ArgList ( (Literal 0.5) )) (WithinGroupClause WITHIN GROUP ( (OrderByClause ORDER BY (SortItem (ColumnRef x))) )))"
    );
    assert_eq!(
        expr("sum(x) OVER w"),
        "(FuncCall sum (ArgList ( (ColumnRef x) )) (OverClause OVER w))"
    );
    assert_eq!(
        expr(
            "sum(x) OVER (PARTITION BY a, b ORDER BY c ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)"
        ),
        "(FuncCall sum (ArgList ( (ColumnRef x) )) (OverClause OVER (WindowSpec ( (PartitionByClause PARTITION BY (ColumnRef a) , (ColumnRef b)) (OrderByClause ORDER BY (SortItem (ColumnRef c))) (FrameClause ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) ))))"
    );
    assert_eq!(
        expr("rank() OVER (w ORDER BY c)"),
        "(FuncCall rank (ArgList ( )) (OverClause OVER (WindowSpec ( w (OrderByClause ORDER BY (SortItem (ColumnRef c))) ))))"
    );
}

// ---- SELECT ----

#[test]
fn simple_select_clauses() {
    assert_eq!(
        stmts(
            "SELECT DISTINCT ON (a) a, b AS c, d e FROM t WHERE x GROUP BY a HAVING y ORDER BY a NULLS LAST LIMIT 10 OFFSET 5"
        ),
        "(SelectStmt (SimpleSelect (SelectClause SELECT DISTINCT ON (ExprList ( (ColumnRef a) )) (TargetItem (ColumnRef a)) , (TargetItem (ColumnRef b) (Alias AS c)) , (TargetItem (ColumnRef d) (Alias e))) (FromClause FROM (TableRef t)) (WhereClause WHERE (ColumnRef x)) (GroupByClause GROUP BY (ColumnRef a)) (HavingClause HAVING (ColumnRef y))) (OrderByClause ORDER BY (SortItem (ColumnRef a) NULLS LAST)) (LimitClause LIMIT (Literal 10)) (OffsetClause OFFSET (Literal 5)))"
    );
}

#[test]
fn select_without_targets() {
    assert_eq!(
        stmts("SELECT FROM t"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT) (FromClause FROM (TableRef t))))"
    );
}

#[test]
fn keyword_after_as_is_alias() {
    assert_eq!(
        stmts("SELECT 1 AS from"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1) (Alias AS from)))))"
    );
}

#[test]
fn joins() {
    assert_eq!(
        stmts(
            "SELECT * FROM a x JOIN b AS y ON x.id = y.id LEFT OUTER JOIN c USING (id) CROSS JOIN d"
        ),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef *))) (FromClause FROM (JoinExpr (JoinExpr (JoinExpr (TableRef a (Alias x)) JOIN (TableRef b (Alias AS y)) (JoinCondition ON (BinaryExpr (ColumnRef x . id) = (ColumnRef y . id)))) LEFT OUTER JOIN (TableRef c) (JoinCondition USING (ExprList ( (ColumnRef id) )))) CROSS JOIN (TableRef d)))))"
    );
    // 結合のキーワードは別名にしない
    assert_eq!(
        stmts("SELECT 1 FROM a NATURAL FULL JOIN b"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))) (FromClause FROM (JoinExpr (TableRef a) NATURAL FULL JOIN (TableRef b)))))"
    );
}

#[test]
fn from_items() {
    assert_eq!(
        stmts(
            "SELECT 1 FROM ONLY s.t, (SELECT 1) AS q (c), LATERAL f(x) WITH ORDINALITY AS g, (a JOIN b ON true) j"
        ),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))) (FromClause FROM (TableRef ONLY s . t) , (DerivedTable (SubqueryExpr ( (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) )) (Alias AS q (ExprList ( (ColumnRef c) )))) , (FunctionTable LATERAL f (ArgList ( (ColumnRef x) )) WITH ORDINALITY (Alias AS g)) , (ParenJoin ( (JoinExpr (TableRef a) JOIN (TableRef b) (JoinCondition ON (Literal true))) ) (Alias j)))))"
    );
}

#[test]
fn set_operations() {
    // INTERSECT は UNION より強い
    assert_eq!(
        stmts("SELECT 1 UNION SELECT 2 INTERSECT SELECT 3"),
        "(SelectStmt (SetOperation (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1)))) UNION (SetOperation (SimpleSelect (SelectClause SELECT (TargetItem (Literal 2)))) INTERSECT (SimpleSelect (SelectClause SELECT (TargetItem (Literal 3)))))))"
    );
    // ORDER BY / LIMIT は集合演算全体にかかる
    assert_eq!(
        stmts("SELECT 1 UNION ALL SELECT 2 EXCEPT (SELECT 3 LIMIT 1) ORDER BY 1 LIMIT 2"),
        "(SelectStmt (SetOperation (SetOperation (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1)))) UNION ALL (SimpleSelect (SelectClause SELECT (TargetItem (Literal 2))))) EXCEPT (ParenSelect ( (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 3)))) (LimitClause LIMIT (Literal 1))) ))) (OrderByClause ORDER BY (SortItem (Literal 1))) (LimitClause LIMIT (Literal 2)))"
    );
}

#[test]
fn with_clause() {
    assert_eq!(
        stmts(
            "WITH RECURSIVE r (n) AS (SELECT 1), m AS MATERIALIZED (VALUES (1), (2)) SELECT n FROM r"
        ),
        "(SelectStmt (WithClause WITH RECURSIVE (Cte r (ExprList ( (ColumnRef n) )) AS (SubqueryExpr ( (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) ))) , (Cte m AS MATERIALIZED (SubqueryExpr ( (SelectStmt (ValuesClause VALUES (ExprList ( (Literal 1) )) , (ExprList ( (Literal 2) )))) )))) (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef n))) (FromClause FROM (TableRef r))))"
    );
    // CTE の本体には DML も書ける
    assert_eq!(
        stmts("WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d"),
        "(SelectStmt (WithClause WITH (Cte d AS (SubqueryExpr ( (DeleteStmt DELETE FROM (TableRef t) (ReturningClause RETURNING (TargetItem (ColumnRef *)))) )))) (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef *))) (FromClause FROM (TableRef d))))"
    );
    // 対応していない文の CTE 本体はそのまま保持する
    assert_eq!(
        stmts("WITH m AS (MERGE INTO t USING s ON true DO NOTHING) SELECT 1"),
        "(SelectStmt (WithClause WITH (Cte m AS (SubqueryExpr ( (RawStatement MERGE INTO t USING s ON true DO NOTHING) )))) (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1)))))"
    );
}

#[test]
fn other_select_clauses() {
    assert_eq!(
        stmts(
            "SELECT a INTO TEMP t FROM s WINDOW w AS (PARTITION BY a) FETCH FIRST 1 ROW ONLY FOR UPDATE OF s NOWAIT"
        ),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef a))) (IntoClause INTO TEMP t) (FromClause FROM (TableRef s)) (WindowClause WINDOW (WindowDef w AS (WindowSpec ( (PartitionByClause PARTITION BY (ColumnRef a)) ))))) (FetchClause FETCH FIRST 1 ROW ONLY) (LockingClause FOR UPDATE OF s NOWAIT))"
    );
    assert_eq!(
        stmts("SELECT 1 GROUP BY GROUPING SETS ((a, b), ()), ROLLUP (c)"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))) (GroupByClause GROUP BY (GroupingSets GROUPING SETS (ExprList ( (RowExpr (ExprList ( (ColumnRef a) , (ColumnRef b) ))) , (RowExpr (ExprList ( ))) ))) , (FuncCall ROLLUP (ArgList ( (ColumnRef c) ))))))"
    );
    assert_eq!(
        stmts("SELECT 1 OFFSET 5 ROWS FETCH NEXT 3 ROWS ONLY"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1)))) (OffsetClause OFFSET (Literal 5) ROWS) (FetchClause FETCH NEXT 3 ROWS ONLY))"
    );
    assert_eq!(stmts("TABLE t"), "(SelectStmt (TableClause TABLE t))");
}

// ---- INSERT / UPDATE / DELETE ----

#[test]
fn insert_statements() {
    assert_eq!(
        stmts("INSERT INTO s.t AS x (a, b) VALUES (1, DEFAULT), (2, 3) RETURNING a, b AS c"),
        "(InsertStmt INSERT INTO (TableRef s . t (Alias AS x)) (ExprList ( (ColumnRef a) , (ColumnRef b) )) (SelectStmt (ValuesClause VALUES (ExprList ( (Literal 1) , (Literal DEFAULT) )) , (ExprList ( (Literal 2) , (Literal 3) )))) (ReturningClause RETURNING (TargetItem (ColumnRef a)) , (TargetItem (ColumnRef b) (Alias AS c))))"
    );
    // VALUES は予約語ではないが、AS なしの別名にはしない
    assert_eq!(
        stmts("INSERT INTO t VALUES (1)"),
        "(InsertStmt INSERT INTO (TableRef t) (SelectStmt (ValuesClause VALUES (ExprList ( (Literal 1) )))))"
    );
    assert_eq!(
        stmts("INSERT INTO t DEFAULT VALUES"),
        "(InsertStmt INSERT INTO (TableRef t) DEFAULT VALUES)"
    );
    // 括弧で囲んだ問い合わせは列名の並びと区別する
    assert_eq!(
        stmts("INSERT INTO t (SELECT 1)"),
        "(InsertStmt INSERT INTO (TableRef t) (SelectStmt (ParenSelect ( (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) ))))"
    );
    assert_eq!(
        stmts("INSERT INTO t (a) OVERRIDING SYSTEM VALUE SELECT a FROM s"),
        "(InsertStmt INSERT INTO (TableRef t) (ExprList ( (ColumnRef a) )) OVERRIDING SYSTEM VALUE (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef a))) (FromClause FROM (TableRef s)))))"
    );
}

#[test]
fn insert_on_conflict() {
    // 問い合わせの FROM 句は ON CONFLICT の手前で終わる
    assert_eq!(
        stmts(
            "INSERT INTO t SELECT * FROM s ON CONFLICT (id) WHERE a DO UPDATE SET v = EXCLUDED.v WHERE t.v <> EXCLUDED.v"
        ),
        "(InsertStmt INSERT INTO (TableRef t) (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef *))) (FromClause FROM (TableRef s)))) (OnConflictClause ON CONFLICT (ExprList ( (ColumnRef id) )) (WhereClause WHERE (ColumnRef a)) DO UPDATE (SetClause SET (SetItem (ColumnRef v) = (ColumnRef EXCLUDED . v))) (WhereClause WHERE (BinaryExpr (ColumnRef t . v) <> (ColumnRef EXCLUDED . v)))))"
    );
    assert_eq!(
        stmts("INSERT INTO t VALUES (1) ON CONFLICT ON CONSTRAINT t_pkey DO NOTHING RETURNING *"),
        "(InsertStmt INSERT INTO (TableRef t) (SelectStmt (ValuesClause VALUES (ExprList ( (Literal 1) )))) (OnConflictClause ON CONFLICT ON CONSTRAINT t_pkey DO NOTHING) (ReturningClause RETURNING (TargetItem (ColumnRef *))))"
    );
}

#[test]
fn update_statements() {
    // SET は予約語ではないが、AS なしの別名にはしない
    assert_eq!(
        stmts("UPDATE t SET a = 1"),
        "(UpdateStmt UPDATE (TableRef t) (SetClause SET (SetItem (ColumnRef a) = (Literal 1))))"
    );
    assert_eq!(
        stmts(
            "UPDATE ONLY t * AS x SET a[1] = DEFAULT, (b, c) = (SELECT 1, 2), d = a = b FROM s JOIN u ON true WHERE x.id = s.id RETURNING x.*"
        ),
        "(UpdateStmt UPDATE (TableRef ONLY t * (Alias AS x)) (SetClause SET (SetItem (SubscriptExpr (ColumnRef a) [ (Literal 1) ]) = (Literal DEFAULT)) , (SetItem (ExprList ( (ColumnRef b) , (ColumnRef c) )) = (SubqueryExpr ( (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1)) , (TargetItem (Literal 2))))) ))) , (SetItem (ColumnRef d) = (BinaryExpr (ColumnRef a) = (ColumnRef b)))) (FromClause FROM (JoinExpr (TableRef s) JOIN (TableRef u) (JoinCondition ON (Literal true)))) (WhereClause WHERE (BinaryExpr (ColumnRef x . id) = (ColumnRef s . id))) (ReturningClause RETURNING (TargetItem (ColumnRef x . *))))"
    );
    assert_eq!(
        stmts("UPDATE t x SET a = 1 WHERE CURRENT OF c"),
        "(UpdateStmt UPDATE (TableRef t (Alias x)) (SetClause SET (SetItem (ColumnRef a) = (Literal 1))) (WhereClause WHERE CURRENT OF c))"
    );
}

#[test]
fn delete_statements() {
    assert_eq!(
        stmts("DELETE FROM t x USING s, u WHERE x.id = s.id RETURNING WITH (OLD AS o) o.id"),
        "(DeleteStmt DELETE FROM (TableRef t (Alias x)) (UsingClause USING (TableRef s) , (TableRef u)) (WhereClause WHERE (BinaryExpr (ColumnRef x . id) = (ColumnRef s . id))) (ReturningClause RETURNING WITH ( OLD AS o ) (TargetItem (ColumnRef o . id))))"
    );
    assert_eq!(
        stmts("DELETE FROM t"),
        "(DeleteStmt DELETE FROM (TableRef t))"
    );
}

#[test]
fn with_before_dml() {
    assert_eq!(
        stmts("WITH x AS (SELECT 1) INSERT INTO t SELECT * FROM x"),
        "(InsertStmt (WithClause WITH (Cte x AS (SubqueryExpr ( (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) )))) INSERT INTO (TableRef t) (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef *))) (FromClause FROM (TableRef x)))))"
    );
    assert_eq!(
        stmts("WITH x AS (UPDATE t SET a = 1 RETURNING a) DELETE FROM s"),
        "(DeleteStmt (WithClause WITH (Cte x AS (SubqueryExpr ( (UpdateStmt UPDATE (TableRef t) (SetClause SET (SetItem (ColumnRef a) = (Literal 1))) (ReturningClause RETURNING (TargetItem (ColumnRef a)))) )))) DELETE FROM (TableRef s))"
    );
}

#[test]
fn incomplete_dml_is_kept() {
    assert_eq!(stmts("INSERT"), "(InsertStmt INSERT)");
    assert_eq!(
        stmts("UPDATE t SET"),
        "(UpdateStmt UPDATE (TableRef t) (SetClause SET))"
    );
    assert_eq!(
        stmts("UPDATE t SET a, b = 1"),
        "(UpdateStmt UPDATE (TableRef t) (SetClause SET (SetItem (ColumnRef a)) , (SetItem (ColumnRef b) = (Literal 1))))"
    );
    assert_eq!(
        stmts("DELETE t WHERE x"),
        "(DeleteStmt DELETE (TableRef t) (WhereClause WHERE (ColumnRef x)))"
    );
}

// ---- 文の区切りと未対応の文 ----

#[test]
fn statements_and_raw_statements() {
    assert_eq!(
        stmts("CREATE TABLE t (id int); SELECT 1;;"),
        "(RawStatement CREATE TABLE t ( id int )) ; (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) ; ;"
    );
    assert_eq!(stmts(""), "");
}

#[test]
fn trivia_goes_before_nodes() {
    let root = parse_checked("-- head\nSELECT a, -- after comma\n b /* tail */");
    assert_eq!(
        root.debug_tree(),
        "\
Root
  LineComment \"-- head\"
  SelectStmt
    SimpleSelect
      SelectClause
        Keyword \"SELECT\"
        TargetItem
          ColumnRef
            Ident \"a\"
        Comma \",\"
        LineComment \"-- after comma\"
        TargetItem
          ColumnRef
            Ident \"b\"
  BlockComment \"/* tail */\"
"
    );
}

// ---- エラーからの回復 ----

#[test]
fn recovers_inside_lists() {
    assert_eq!(
        stmts("SELECT a, , b FROM t"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef a)) , , (TargetItem (ColumnRef b))) (FromClause FROM (TableRef t))))"
    );
    assert_eq!(
        stmts("SELECT a b c, d FROM t"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef a) (Alias b)) (Error c) , (TargetItem (ColumnRef d))) (FromClause FROM (TableRef t))))"
    );
    // 括弧の中のエラーは閉じ括弧の手前で止まる
    assert_eq!(
        stmts("SELECT (SELECT a b c) + 1"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (BinaryExpr (SubqueryExpr ( (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef a) (Alias b)) (Error c)))) )) + (Literal 1))))))"
    );
}

#[test]
fn recovers_from_missing_parts() {
    assert_eq!(
        stmts("SELECT a FROM t WHERE"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef a))) (FromClause FROM (TableRef t)) (WhereClause WHERE)))"
    );
    assert_eq!(
        stmts("SELECT (a FROM t"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ParenExpr ( (ColumnRef a) (Error FROM t))))))"
    );
    // 予約語は式にしない
    assert_eq!(
        stmts("SELECT a + FROM t"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (BinaryExpr (ColumnRef a) +))) (FromClause FROM (TableRef t))))"
    );
    assert_eq!(
        stmts("SELECT f(a, ) + 1"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (BinaryExpr (FuncCall f (ArgList ( (ColumnRef a) , ))) + (Literal 1))))))"
    );
}

#[test]
fn stray_tokens_become_errors() {
    assert_eq!(
        stmts("SELECT 1) FROM t; SELECT 2"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) (Error ) FROM t) ; (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 2)))))"
    );
    assert_eq!(
        stmts("WITH x AS (SELECT 1) MERGE INTO t USING x ON true DO NOTHING"),
        "(SelectStmt (WithClause WITH (Cte x AS (SubqueryExpr ( (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) )))) (Error MERGE INTO t USING x ON true DO NOTHING))"
    );
}

#[test]
fn unterminated_input_is_kept() {
    assert_eq!(
        stmts("SELECT 'abc"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 'abc)))))"
    );
    assert_eq!(
        stmts("SELECT (((1"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ParenExpr ( (ParenExpr ( (ParenExpr ( (Literal 1))))))))"
    );
}
