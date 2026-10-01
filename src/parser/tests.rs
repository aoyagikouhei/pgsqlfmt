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
    // 改行を挟んだ文字列は 1 つにつながる。同じ行や接頭辞付きはつながらない
    assert_eq!(expr("'a'\n  'b'\n'c'"), "(Literal 'a' 'b' 'c')");
    assert_eq!(
        stmts("SELECT 'a' 'b'"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 'a')) (Error 'b'))))"
    );
    assert_eq!(
        stmts("SELECT 'a'\nE'b'"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 'a')) (Error E'b'))))"
    );
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
        stmts("WITH m AS (NOTIFY c) SELECT 1"),
        "(SelectStmt (WithClause WITH (Cte m AS (SubqueryExpr ( (RawStatement NOTIFY c) )))) (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1)))))"
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
        stmts("VACUUM t (a); SELECT 1;;"),
        "(RawStatement VACUUM t ( a )) ; (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) ; ;"
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
        stmts("WITH x AS (SELECT 1) NOTIFY c"),
        "(SelectStmt (WithClause WITH (Cte x AS (SubqueryExpr ( (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) ))) (Error NOTIFY c)))"
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

// ---- 関数・PL/pgSQL ----

/// `DO $$<body>$$` の本体（区切りを除く）の S 式
fn pl(body: &str) -> String {
    let src = format!("DO $${body}$$");
    let root = parse_checked(&src);
    let body = find(&root, NodeKind::FunctionBody).expect("本体が解析されていない");
    let inner = sexpr(body);
    inner
        .strip_prefix("(FunctionBody $$ ")
        .and_then(|s| s.strip_suffix(" $$)"))
        .unwrap_or_else(|| panic!("{inner}"))
        .to_string()
}

#[test]
fn plpgsql_blocks_and_declarations() {
    assert_eq!(
        pl(
            "<<l>> DECLARE a int := 1; b t.c%TYPE; c CONSTANT text NOT NULL DEFAULT 'x'; d ALIAS FOR $1; e CURSOR (p int) FOR SELECT p; BEGIN END l;"
        ),
        "(PlBlock (PlLabel << l >>) (PlDeclareSection DECLARE (PlDecl a (TypeName int) := (Literal 1) ;) (PlDecl b (TypeName t . c % TYPE) ;) (PlDecl c CONSTANT (TypeName text) NOT NULL DEFAULT (Literal 'x') ;) (PlDecl d ALIAS FOR $1 ;) (PlDecl e CURSOR ( p int ) FOR (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef p))))) ;)) BEGIN END l ;)"
    );
    assert_eq!(
        pl("DECLARE a int; DECLARE b text COLLATE \"C\"; c CURSOR IS SELECT 1; BEGIN END"),
        "(PlBlock (PlDeclareSection DECLARE (PlDecl a (TypeName int) ;) DECLARE (PlDecl b (TypeName text) COLLATE \"C\" ;) (PlDecl c CURSOR IS (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) ;)) BEGIN END)"
    );
    // 最後の `;` は省略できる
    assert_eq!(pl("BEGIN NULL; END"), "(PlBlock BEGIN (PlNull NULL ;) END)");
}

#[test]
fn plpgsql_assignments_and_sql() {
    assert_eq!(
        pl(
            "BEGIN x := 1; y = 2; r.f[1] := 3; SELECT a INTO STRICT x, y FROM t; UPDATE t SET a = 1 RETURNING a INTO x; CREATE TEMP TABLE z (i int); END"
        ),
        "(PlBlock BEGIN (PlAssign (ColumnRef x) := (Literal 1) ;) (PlAssign (ColumnRef y) = (Literal 2) ;) (PlAssign (SubscriptExpr (ColumnRef r . f) [ (Literal 1) ]) := (Literal 3) ;) (PlSqlStmt (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef a))) (IntoClause INTO STRICT x , y) (FromClause FROM (TableRef t)))) ;) (PlSqlStmt (UpdateStmt UPDATE (TableRef t) (SetClause SET (SetItem (ColumnRef a) = (Literal 1))) (ReturningClause RETURNING (TargetItem (ColumnRef a))) (IntoClause INTO x)) ;) (PlSqlStmt (CreateTableStmt CREATE TEMP TABLE z (TableElementList ( (ColumnDef i (TypeName int)) ))) ;) END)"
    );
    // INTO は問い合わせの最後にも書ける
    assert_eq!(
        pl("BEGIN SELECT a FROM t INTO x; END"),
        "(PlBlock BEGIN (PlSqlStmt (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef a))) (FromClause FROM (TableRef t))) (IntoClause INTO x)) ;) END)"
    );
}

#[test]
fn plpgsql_control_flow() {
    assert_eq!(
        pl("BEGIN IF a THEN NULL; ELSIF b THEN NULL; ELSEIF c THEN NULL; ELSE NULL; END IF; END"),
        "(PlBlock BEGIN (PlIf IF (ColumnRef a) THEN (PlNull NULL ;) (PlElsif ELSIF (ColumnRef b) THEN (PlNull NULL ;)) (PlElsif ELSEIF (ColumnRef c) THEN (PlNull NULL ;)) (PlElse ELSE (PlNull NULL ;)) END IF ;) END)"
    );
    assert_eq!(
        pl("BEGIN CASE x WHEN 1, 2 THEN NULL; ELSE NULL; END CASE; END"),
        "(PlBlock BEGIN (PlCase CASE (ColumnRef x) (PlCaseWhen WHEN (Literal 1) , (Literal 2) THEN (PlNull NULL ;)) (PlElse ELSE (PlNull NULL ;)) END CASE ;) END)"
    );
    // 問い合わせの FOR では LOOP を別名にしない
    assert_eq!(
        pl(
            "BEGIN <<l>> FOR r IN SELECT a FROM t LOOP EXIT l WHEN r.a > 1; CONTINUE; END LOOP l; END"
        ),
        "(PlBlock BEGIN (PlLoop (PlLabel << l >>) FOR r IN (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef a))) (FromClause FROM (TableRef t)))) LOOP (PlExit EXIT l WHEN (BinaryExpr (ColumnRef r . a) > (Literal 1)) ;) (PlExit CONTINUE ;) END LOOP l ;) END)"
    );
    assert_eq!(
        pl(
            "BEGIN FOR i IN REVERSE 10..1 BY 2 LOOP END LOOP; FOR r IN EXECUTE q USING 1 LOOP END LOOP; FOR r IN c(1) LOOP END LOOP; END"
        ),
        "(PlBlock BEGIN (PlLoop FOR i IN REVERSE (Literal 10) .. (Literal 1) BY (Literal 2) LOOP END LOOP ;) (PlLoop FOR r IN EXECUTE (ColumnRef q) (PlUsing USING (Literal 1)) LOOP END LOOP ;) (PlLoop FOR r IN (FuncCall c (ArgList ( (Literal 1) ))) LOOP END LOOP ;) END)"
    );
    assert_eq!(
        pl(
            "BEGIN FOREACH x SLICE 1 IN ARRAY a LOOP END LOOP; WHILE x LOOP END LOOP; LOOP END LOOP; END"
        ),
        "(PlBlock BEGIN (PlLoop FOREACH x SLICE (Literal 1) IN ARRAY (ColumnRef a) LOOP END LOOP ;) (PlLoop WHILE (ColumnRef x) LOOP END LOOP ;) (PlLoop LOOP END LOOP ;) END)"
    );
}

#[test]
fn plpgsql_other_statements() {
    assert_eq!(
        pl(
            "BEGIN RETURN; RETURN x + 1; RETURN NEXT r; RETURN QUERY SELECT 1; RETURN QUERY EXECUTE q USING a, b; END"
        ),
        "(PlBlock BEGIN (PlReturn RETURN ;) (PlReturn RETURN (BinaryExpr (ColumnRef x) + (Literal 1)) ;) (PlReturn RETURN NEXT (ColumnRef r) ;) (PlReturn RETURN QUERY (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) ;) (PlReturn RETURN QUERY EXECUTE (ColumnRef q) (PlUsing USING (ColumnRef a) , (ColumnRef b)) ;) END)"
    );
    assert_eq!(
        pl(
            "BEGIN RAISE; RAISE 'x'; RAISE NOTICE 'a %', b; RAISE EXCEPTION USING MESSAGE = 'm', ERRCODE = 'P0001'; RAISE SQLSTATE '22012'; RAISE division_by_zero; END"
        ),
        "(PlBlock BEGIN (PlRaise RAISE ;) (PlRaise RAISE (Literal 'x') ;) (PlRaise RAISE NOTICE (Literal 'a %') , (ColumnRef b) ;) (PlRaise RAISE EXCEPTION (PlUsing USING MESSAGE = (Literal 'm') , ERRCODE = (Literal 'P0001')) ;) (PlRaise RAISE SQLSTATE (Literal '22012') ;) (PlRaise RAISE (ColumnRef division_by_zero) ;) END)"
    );
    assert_eq!(
        pl(
            "BEGIN PERFORM f(1) FROM t; EXECUTE q INTO x USING 1; GET STACKED DIAGNOSTICS a = ROW_COUNT, b := PG_CONTEXT; ASSERT a > 0, 'm'; CALL p(1); END"
        ),
        "(PlBlock BEGIN (PlPerform (SimpleSelect (SelectClause PERFORM (TargetItem (FuncCall f (ArgList ( (Literal 1) ))))) (FromClause FROM (TableRef t))) ;) (PlExecute EXECUTE (ColumnRef q) (IntoClause INTO x) (PlUsing USING (Literal 1)) ;) (PlGetDiagnostics GET STACKED DIAGNOSTICS a = ROW_COUNT , b := PG_CONTEXT ;) (PlAssert ASSERT (BinaryExpr (ColumnRef a) > (Literal 0)) , (Literal 'm') ;) (PlSqlStmt (CallStmt CALL (FuncCall p (ArgList ( (Literal 1) )))) ;) END)"
    );
    assert_eq!(
        pl(
            "BEGIN OPEN c FOR SELECT 1; OPEN c(1); OPEN c NO SCROLL FOR EXECUTE q; FETCH NEXT FROM c INTO r; CLOSE c; COMMIT AND CHAIN; END"
        ),
        "(PlBlock BEGIN (PlOpen OPEN c FOR (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) ;) (PlOpen OPEN c (ArgList ( (Literal 1) )) ;) (PlOpen OPEN c NO SCROLL FOR EXECUTE (ColumnRef q) ;) (PlSimpleStmt FETCH NEXT FROM c INTO r ;) (PlSimpleStmt CLOSE c ;) (PlSimpleStmt COMMIT AND CHAIN ;) END)"
    );
}

#[test]
fn plpgsql_exceptions() {
    assert_eq!(
        pl(
            "BEGIN NULL; EXCEPTION WHEN a OR b THEN NULL; WHEN SQLSTATE '22012' THEN BEGIN NULL; END; END"
        ),
        "(PlBlock BEGIN (PlNull NULL ;) (PlExceptionSection EXCEPTION (PlExceptionHandler WHEN a OR b THEN (PlNull NULL ;)) (PlExceptionHandler WHEN SQLSTATE (Literal '22012') THEN (PlBlock BEGIN (PlNull NULL ;) END ;))) END)"
    );
}

#[test]
fn plpgsql_recovers_from_errors() {
    // `;` のない文は、次のブロックの区切りまでを Error にする
    assert_eq!(
        pl("BEGIN x := 1 y; IF a THEN NULL END IF; END"),
        "(PlBlock BEGIN (PlAssign (ColumnRef x) := (Literal 1) (Error y) ;) (PlIf IF (ColumnRef a) THEN (PlNull NULL) END IF ;) END)"
    );
    // 対応のない ELSE は Error にして、END を探し続ける
    assert_eq!(
        pl("BEGIN ELSE NULL; END"),
        "(PlBlock BEGIN (Error ELSE) (PlNull NULL ;) END)"
    );
    // 解釈できない文の先頭
    assert_eq!(pl("BEGIN ); END"), "(PlBlock BEGIN (Error )) ; END)");
    // ブロックで始まらない本体
    assert_eq!(pl("select 1"), "(Error select 1)");
}

#[test]
fn stop_keywords_do_not_leak() {
    // PL/pgSQL の外では LOOP も別名にできる
    assert_eq!(
        stmts("SELECT a loop FROM t"),
        "(SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef a) (Alias loop))) (FromClause FROM (TableRef t))))"
    );
}

#[test]
fn create_function_statements() {
    assert_eq!(
        stmts(
            "CREATE FUNCTION f(a int, int, OUT b double precision, c text DEFAULT 'x', d int = 1) RETURNS SETOF t AS $$ SELECT 1 $$ LANGUAGE sql STABLE"
        ),
        "(CreateFunctionStmt CREATE FUNCTION f (ParamList ( (Param a (TypeName int)) , (Param (TypeName int)) , (Param OUT b (TypeName double precision)) , (Param c (TypeName text) DEFAULT (Literal 'x')) , (Param d (TypeName int) = (Literal 1)) )) (ReturnsClause RETURNS SETOF (TypeName t)) (FunctionOption AS (FunctionBody $$ (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) $$)) (FunctionOption LANGUAGE sql) (FunctionOption STABLE))"
    );
    // LANGUAGE が本体の後ろにあっても PL/pgSQL として解析する
    assert_eq!(
        stmts("CREATE OR REPLACE PROCEDURE p() AS $x$ BEGIN END $x$ LANGUAGE plpgsql"),
        "(CreateFunctionStmt CREATE OR REPLACE PROCEDURE p (ParamList ( )) (FunctionOption AS (FunctionBody $x$ (PlBlock BEGIN END) $x$)) (FunctionOption LANGUAGE plpgsql))"
    );
    assert_eq!(
        stmts(
            "CREATE FUNCTION f() RETURNS TABLE (a int, b text) RETURNS NULL ON NULL INPUT SECURITY DEFINER SET search_path = public, pg_temp NOT LEAKPROOF PARALLEL SAFE COST 10 LANGUAGE sql BEGIN ATOMIC SELECT 1; SELECT 2; END"
        ),
        "(CreateFunctionStmt CREATE FUNCTION f (ParamList ( )) (ReturnsClause RETURNS TABLE (ParamList ( (Param a (TypeName int)) , (Param b (TypeName text)) ))) (FunctionOption RETURNS NULL ON NULL INPUT) (FunctionOption SECURITY DEFINER) (FunctionOption SET search_path = public , pg_temp) (FunctionOption NOT LEAKPROOF) (FunctionOption PARALLEL SAFE) (FunctionOption COST 10) (FunctionOption LANGUAGE sql) (AtomicBody BEGIN ATOMIC (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) ; (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 2))))) ; END))"
    );
    // 型だけの引数（DEFAULT 付き・2 語の型）
    assert_eq!(
        stmts(
            "CREATE FUNCTION f(int DEFAULT 1, double precision) RETURNS int LANGUAGE 'plpgsql' AS $$BEGIN END$$"
        ),
        "(CreateFunctionStmt CREATE FUNCTION f (ParamList ( (Param (TypeName int) DEFAULT (Literal 1)) , (Param (TypeName double precision)) )) (ReturnsClause RETURNS (TypeName int)) (FunctionOption LANGUAGE 'plpgsql') (FunctionOption AS (FunctionBody $$ (PlBlock BEGIN END) $$)))"
    );
    // LANGUAGE がなければ SQL として解析する。LANGUAGE は文の終わりまでしか探さない
    assert_eq!(
        stmts("CREATE FUNCTION f() RETURNS int AS $$ BEGIN $$; DO LANGUAGE plpgsql $$BEGIN END$$"),
        "(CreateFunctionStmt CREATE FUNCTION f (ParamList ( )) (ReturnsClause RETURNS (TypeName int)) (FunctionOption AS (FunctionBody $$ (TransactionStmt BEGIN) $$))) ; (DoStmt DO LANGUAGE plpgsql (FunctionBody $$ (PlBlock BEGIN END) $$))"
    );
    // SQL 標準の本体 `RETURN expr`
    assert_eq!(
        stmts("CREATE FUNCTION f(a int) RETURNS int LANGUAGE sql RETURN a + 1"),
        "(CreateFunctionStmt CREATE FUNCTION f (ParamList ( (Param a (TypeName int)) )) (ReturnsClause RETURNS (TypeName int)) (FunctionOption LANGUAGE sql) (FunctionOption RETURN (BinaryExpr (ColumnRef a) + (Literal 1))))"
    );
    // ほかの言語の本体や、引用符の本体はそのまま
    assert_eq!(
        stmts("CREATE FUNCTION f() RETURNS int AS $$ return 1 $$ LANGUAGE plpython3u"),
        "(CreateFunctionStmt CREATE FUNCTION f (ParamList ( )) (ReturnsClause RETURNS (TypeName int)) (FunctionOption AS $$ return 1 $$) (FunctionOption LANGUAGE plpython3u))"
    );
    assert_eq!(
        stmts("CREATE FUNCTION f() RETURNS int AS 'lib', 'sym' LANGUAGE C"),
        "(CreateFunctionStmt CREATE FUNCTION f (ParamList ( )) (ReturnsClause RETURNS (TypeName int)) (FunctionOption AS 'lib' , 'sym') (FunctionOption LANGUAGE C))"
    );
}

#[test]
fn do_and_call_statements() {
    assert_eq!(
        stmts("DO LANGUAGE plpgsql $$BEGIN END$$; DO $$ x $$ LANGUAGE plperl; CALL s.p(1, a => 2)"),
        "(DoStmt DO LANGUAGE plpgsql (FunctionBody $$ (PlBlock BEGIN END) $$)) ; (DoStmt DO $$ x $$ LANGUAGE plperl) ; (CallStmt CALL (FuncCall s . p (ArgList ( (Literal 1) , (BinaryExpr (ColumnRef a) => (Literal 2)) ))))"
    );
    // 閉じていない本体は解析しない
    assert_eq!(stmts("DO $$ BEGIN"), "(DoStmt DO $$ BEGIN)");
}

// ---- DDL・MERGE ----

#[test]
fn create_table_statements() {
    assert_eq!(
        stmts(
            "CREATE TEMP TABLE IF NOT EXISTS s.t (id bigint GENERATED ALWAYS AS IDENTITY (START WITH 10) PRIMARY KEY, key text NOT NULL DEFAULT 'x' CHECK (key <> ''), p bigint REFERENCES parent (id) ON DELETE SET NULL, g int GENERATED ALWAYS AS (id * 2) STORED, CONSTRAINT u UNIQUE (key, p), LIKE other INCLUDING ALL) PARTITION BY RANGE (id) WITH (fillfactor = 70)"
        ),
        "(CreateTableStmt CREATE TEMP TABLE IF NOT EXISTS s . t (TableElementList ( (ColumnDef id (TypeName bigint) GENERATED ALWAYS AS IDENTITY (ExprList ( START WITH 10 )) PRIMARY KEY) , (ColumnDef key (TypeName text) NOT NULL DEFAULT (Literal 'x') CHECK (ParenExpr ( (BinaryExpr (ColumnRef key) <> (Literal '')) ))) , (ColumnDef p (TypeName bigint) REFERENCES parent (ExprList ( id )) ON DELETE SET NULL) , (ColumnDef g (TypeName int) GENERATED ALWAYS AS (ParenExpr ( (BinaryExpr (ColumnRef id) * (Literal 2)) )) STORED) , (TableConstraint CONSTRAINT u UNIQUE (ExprList ( key , p ))) , (TableConstraint LIKE other INCLUDING ALL) )) PARTITION BY RANGE (ExprList ( id )) WITH (ExprList ( fillfactor = 70 )))"
    );
    // CREATE TABLE ... AS の問い合わせは WITH [NO] DATA の手前で終わる
    assert_eq!(
        stmts("CREATE TABLE t2 AS SELECT a FROM t WITH NO DATA"),
        "(CreateTableStmt CREATE TABLE t2 AS (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (ColumnRef a))) (FromClause FROM (TableRef t)))) WITH NO DATA)"
    );
    // パーティションにも列の制約の並びを書ける
    assert_eq!(
        stmts("CREATE TABLE p2 PARTITION OF p (CONSTRAINT c CHECK (x > 0)) FOR VALUES IN (1)"),
        "(CreateTableStmt CREATE TABLE p2 PARTITION OF p (TableElementList ( (TableConstraint CONSTRAINT c CHECK (ParenExpr ( (BinaryExpr (ColumnRef x) > (Literal 0)) ))) )) FOR VALUES IN (ExprList ( 1 )))"
    );
    assert_eq!(
        stmts("CREATE TABLE p1 PARTITION OF p FOR VALUES FROM (1) TO (10)"),
        "(CreateTableStmt CREATE TABLE p1 PARTITION OF p FOR VALUES FROM (ExprList ( 1 )) TO (ExprList ( 10 )))"
    );
}

#[test]
fn create_index_and_view_statements() {
    assert_eq!(
        stmts(
            "CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS i ON ONLY t USING btree (a, lower(b) text_pattern_ops DESC NULLS LAST) INCLUDE (c) WHERE a > 0"
        ),
        "(CreateIndexStmt CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS i ON ONLY t USING btree (ExprList ( (ColumnRef a) , (FuncCall lower (ArgList ( (ColumnRef b) ))) text_pattern_ops DESC NULLS LAST )) INCLUDE (ExprList ( c )) (WhereClause WHERE (BinaryExpr (ColumnRef a) > (Literal 0))))"
    );
    assert_eq!(
        stmts("CREATE INDEX ON t (a)"),
        "(CreateIndexStmt CREATE INDEX ON t (ExprList ( (ColumnRef a) )))"
    );
    assert_eq!(
        stmts("CREATE OR REPLACE TEMP VIEW v (x) AS SELECT 1 WITH CASCADED CHECK OPTION"),
        "(CreateViewStmt CREATE OR REPLACE TEMP VIEW v (ExprList ( x )) AS (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) WITH CASCADED CHECK OPTION)"
    );
    assert_eq!(
        stmts("CREATE MATERIALIZED VIEW IF NOT EXISTS mv AS SELECT 1 WITH DATA"),
        "(CreateViewStmt CREATE MATERIALIZED VIEW IF NOT EXISTS mv AS (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) WITH DATA)"
    );
}

#[test]
fn alter_and_drop_statements() {
    assert_eq!(
        stmts(
            "ALTER TABLE IF EXISTS ONLY t ADD COLUMN IF NOT EXISTS c int NOT NULL, ADD d text, ADD PRIMARY KEY (c), DROP COLUMN IF EXISTS e CASCADE, DROP f, DROP CONSTRAINT IF EXISTS k, ALTER COLUMN c TYPE bigint USING c::bigint, ALTER c SET DEFAULT 1, ALTER c DROP NOT NULL, RENAME COLUMN c TO d, RENAME TO u, OWNER TO bob"
        ),
        "(AlterTableStmt ALTER TABLE IF EXISTS ONLY t (AlterTableAction ADD COLUMN IF NOT EXISTS c (TypeName int) NOT NULL) , (AlterTableAction ADD d (TypeName text)) , (AlterTableAction ADD PRIMARY KEY (ExprList ( c ))) , (AlterTableAction DROP COLUMN IF EXISTS e CASCADE) , (AlterTableAction DROP f) , (AlterTableAction DROP CONSTRAINT IF EXISTS k) , (AlterTableAction ALTER COLUMN c TYPE (TypeName bigint) USING (CastExpr (ColumnRef c) :: (TypeName bigint))) , (AlterTableAction ALTER c SET DEFAULT (Literal 1)) , (AlterTableAction ALTER c DROP NOT NULL) , (AlterTableAction RENAME COLUMN c TO d) , (AlterTableAction RENAME TO u) , (AlterTableAction OWNER TO bob))"
    );
    assert_eq!(
        stmts(
            "DROP MATERIALIZED VIEW IF EXISTS a, s.b CASCADE; DROP FUNCTION f(int, text); DROP TRIGGER tr ON t"
        ),
        "(DropStmt DROP MATERIALIZED VIEW IF EXISTS a , s . b CASCADE) ; (DropStmt DROP FUNCTION f (ExprList ( int , text ))) ; (DropStmt DROP TRIGGER tr ON t)"
    );
}

#[test]
fn merge_statements() {
    assert_eq!(
        stmts(
            "WITH x AS (SELECT 1) MERGE INTO t AS d USING s ON s.id = d.id WHEN MATCHED AND s.v = 0 THEN DELETE WHEN MATCHED THEN UPDATE SET v = s.v, w = 1 WHEN NOT MATCHED BY TARGET THEN INSERT (id, v) VALUES (s.id, s.v) WHEN NOT MATCHED BY SOURCE THEN DO NOTHING RETURNING *"
        ),
        "(MergeStmt (WithClause WITH (Cte x AS (SubqueryExpr ( (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1))))) )))) MERGE INTO (TableRef t (Alias AS d)) USING (TableRef s) (JoinCondition ON (BinaryExpr (ColumnRef s . id) = (ColumnRef d . id))) (MergeWhenClause WHEN MATCHED AND (BinaryExpr (ColumnRef s . v) = (Literal 0)) THEN DELETE) (MergeWhenClause WHEN MATCHED THEN UPDATE (SetClause SET (SetItem (ColumnRef v) = (ColumnRef s . v)) , (SetItem (ColumnRef w) = (Literal 1)))) (MergeWhenClause WHEN NOT MATCHED BY TARGET THEN INSERT (ExprList ( (ColumnRef id) , (ColumnRef v) )) (ValuesClause VALUES (ExprList ( (ColumnRef s . id) , (ColumnRef s . v) )))) (MergeWhenClause WHEN NOT MATCHED BY SOURCE THEN DO NOTHING) (ReturningClause RETURNING (TargetItem (ColumnRef *))))"
    );
    // 別名の USING は別名にしない。INSERT DEFAULT VALUES
    assert_eq!(
        stmts(
            "MERGE INTO t USING (SELECT 1 AS id) s ON true WHEN NOT MATCHED THEN INSERT DEFAULT VALUES"
        ),
        "(MergeStmt MERGE INTO (TableRef t) USING (DerivedTable (SubqueryExpr ( (SelectStmt (SimpleSelect (SelectClause SELECT (TargetItem (Literal 1) (Alias AS id))))) )) (Alias s)) (JoinCondition ON (Literal true)) (MergeWhenClause WHEN NOT MATCHED THEN INSERT DEFAULT VALUES))"
    );
}

// ---- 大きさ ----

/// 無限ループの検出は「進まないまま先読みした回数」で数えるので、入力の大きさには上限がない
#[test]
fn large_inputs_do_not_trip_the_progress_guard() {
    // 先読みは 1 トークンあたり数十回あるので、この大きさで累計は 1,000 万回を超える
    let stmt = "select a, b, c from t1 join t2 on t1.id = t2.id where x = 1 and y in (1, 2, 3) order by a;\n";
    let src = stmt.repeat(40_000);
    let root = parse(&src);
    assert_eq!(root.text(), src);
    let statements = root
        .children
        .iter()
        .filter(|c| matches!(c, Element::Node(n) if n.kind == NodeKind::SelectStmt))
        .count();
    assert_eq!(statements, 40_000);
}

#[test]
fn create_trigger_statements() {
    assert_eq!(
        stmts(
            "CREATE TRIGGER t BEFORE UPDATE OF a, b OR DELETE ON s.x FOR EACH ROW WHEN (a > 0) EXECUTE FUNCTION f(1)"
        ),
        "(CreateTriggerStmt CREATE TRIGGER t (TriggerClause BEFORE UPDATE OF a , b OR DELETE ON s . x) (TriggerClause FOR EACH ROW) (TriggerClause WHEN (ParenExpr ( (BinaryExpr (ColumnRef a) > (Literal 0)) ))) (TriggerClause EXECUTE FUNCTION (FuncCall f (ArgList ( (Literal 1) )))))"
    );
    // 知らない語は次の句まで 1 つの Error にして先へ進む
    assert_eq!(
        stmts("CREATE TRIGGER t AFTER INSERT ON x bogus words FOR EACH ROW"),
        "(CreateTriggerStmt CREATE TRIGGER t (TriggerClause AFTER INSERT ON x) (Error bogus words) (TriggerClause FOR EACH ROW))"
    );
}

#[test]
fn comment_statements() {
    assert_eq!(
        stmts("COMMENT ON FUNCTION s.f(int) IS 'x'"),
        "(CommentStmt COMMENT ON FUNCTION s . f (ExprList ( int )) IS (Literal 'x'))"
    );
    assert_eq!(
        stmts("COMMENT ON CONSTRAINT c ON t IS NULL"),
        "(CommentStmt COMMENT ON CONSTRAINT c ON t IS (Literal NULL))"
    );
}
