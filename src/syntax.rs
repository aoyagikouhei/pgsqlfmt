//! The syntax tree.
//!
//! A lossless tree that keeps every token, including whitespace and comments, so concatenating
//! the tree's tokens in order reproduces the input.
//! Nodes have a generic shape: just a kind and a sequence of children (nodes or tokens).

use std::fmt::Write;

use crate::lexer::{Token, TokenKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Root,
    /// An unsupported statement. Holds its tokens as they are
    RawStatement,
    /// A run of tokens that could not be interpreted inside a supported construct
    Error,

    // ---- SELECT ----
    /// A whole query, including WITH, set operations, ORDER BY, LIMIT and so on
    SelectStmt,
    WithClause,
    Cte,
    /// `UNION` / `INTERSECT` / `EXCEPT`
    SetOperation,
    /// `(SELECT ...)` (an operand of a set operation)
    ParenSelect,
    /// `SELECT ... FROM ... WHERE ... GROUP BY ... HAVING ... WINDOW ...`
    SimpleSelect,
    ValuesClause,
    /// `TABLE name`
    TableClause,
    SelectClause,
    TargetItem,
    /// `[AS] name [(col, ...)]`
    Alias,
    IntoClause,
    FromClause,
    TableRef,
    /// `(SELECT ...) alias` in a FROM clause
    DerivedTable,
    /// `func(...) alias` in a FROM clause
    FunctionTable,
    /// `(a JOIN b ...) alias` in a FROM clause
    ParenJoin,
    JoinExpr,
    /// `ON expr` / `USING (...)`
    JoinCondition,
    WhereClause,
    GroupByClause,
    /// `GROUPING SETS (...)` / `ROLLUP (...)` / `CUBE (...)`
    GroupingSets,
    HavingClause,
    WindowClause,
    /// `name AS (...)` in a WINDOW clause
    WindowDef,
    OrderByClause,
    SortItem,
    LimitClause,
    OffsetClause,
    FetchClause,
    LockingClause,

    // ---- INSERT / UPDATE / DELETE ----
    InsertStmt,
    UpdateStmt,
    DeleteStmt,
    OnConflictClause,
    SetClause,
    /// `col = expr` / `(a, b) = (...)`
    SetItem,
    /// `USING ...` in DELETE
    UsingClause,
    ReturningClause,

    // ---- MERGE ----
    MergeStmt,
    /// `WHEN [NOT] MATCHED [BY ...] [AND cond] THEN action`
    MergeWhenClause,

    // ---- DDL ----
    CreateTableStmt,
    /// The `(column, constraint, ...)` of CREATE TABLE
    TableElementList,
    ColumnDef,
    /// A table constraint (including `LIKE source`)
    TableConstraint,
    CreateIndexStmt,
    /// `CREATE [MATERIALIZED] VIEW`
    CreateViewStmt,
    AlterTableStmt,
    AlterTableAction,
    DropStmt,
    /// `CREATE [TEMP | UNLOGGED] SEQUENCE`
    CreateSequenceStmt,
    /// A sequence option (`INCREMENT BY 2` / `NO CYCLE` / `OWNED BY t.c` etc.)
    SequenceOption,
    /// `CREATE TYPE name [AS ENUM (...) | AS (...) | AS RANGE (...) | (...)]`
    CreateTypeStmt,
    /// `CREATE SCHEMA` / `CREATE EXTENSION`
    CreateSchemaStmt,
    CreateExtensionStmt,
    /// `COPY ... {FROM | TO} ...`
    CopyStmt,
    /// `SET` / `RESET` / `SHOW`
    SetStmt,
    /// `EXPLAIN [options] statement`
    ExplainStmt,
    /// Transaction control such as `BEGIN` / `COMMIT` / `ROLLBACK` / `SAVEPOINT`
    TransactionStmt,
    /// `ALTER object name action ...` other than `ALTER TABLE`
    AlterStmt,
    /// `GRANT ... ON ... TO ...` / `REVOKE ... ON ... FROM ...` (including role grants)
    GrantStmt,
    /// `TRUNCATE [TABLE] name, ... [RESTART IDENTITY] [CASCADE]`
    TruncateStmt,
    /// `COMMENT ON object IS 'text'`
    CommentStmt,
    /// `CREATE [OR REPLACE] [CONSTRAINT] TRIGGER`
    CreateTriggerStmt,
    /// A trigger clause (`BEFORE ... ON table` / `FOR EACH ROW` / `WHEN (...)` /
    /// `EXECUTE FUNCTION f()` etc.)
    TriggerClause,

    // ---- Functions and procedures ----
    /// `CREATE [OR REPLACE] FUNCTION / PROCEDURE`
    CreateFunctionStmt,
    /// The list of parameters, or of `RETURNS TABLE` columns
    ParamList,
    Param,
    /// `RETURNS type` / `RETURNS TABLE (...)`
    ReturnsClause,
    /// `LANGUAGE plpgsql` / `AS $$...$$` / `IMMUTABLE` etc.
    FunctionOption,
    /// A dollar-quoted body whose contents were parsed (`$tag$`, the contents and `$tag$`)
    FunctionBody,
    /// `BEGIN ATOMIC ... END`
    AtomicBody,
    DoStmt,
    CallStmt,

    // ---- PL/pgSQL ----
    /// `[<<label>>] [DECLARE ...] BEGIN ... [EXCEPTION ...] END [label];`
    PlBlock,
    /// `<<label>>`
    PlLabel,
    PlDeclareSection,
    /// A variable, cursor or alias declaration
    PlDecl,
    PlExceptionSection,
    /// `WHEN cond THEN ...`
    PlExceptionHandler,
    /// `target := expr;`
    PlAssign,
    PlIf,
    /// `ELSIF cond THEN ...`
    PlElsif,
    /// `ELSE ...` of IF / CASE
    PlElse,
    PlCase,
    /// `WHEN ... THEN ...` of a CASE statement
    PlCaseWhen,
    /// `LOOP` / `WHILE` / `FOR` / `FOREACH`
    PlLoop,
    /// `EXIT` / `CONTINUE`
    PlExit,
    PlReturn,
    PlRaise,
    PlAssert,
    /// `PERFORM ...` (a query written with PERFORM in place of SELECT)
    PlPerform,
    PlExecute,
    PlGetDiagnostics,
    PlOpen,
    /// `NULL;`
    PlNull,
    /// A statement whose contents are not interpreted in detail (`FETCH` / `MOVE` / `CLOSE` /
    /// `COMMIT` / `ROLLBACK`)
    PlSimpleStmt,
    /// A SQL statement inside a body, with the `;` after it
    PlSqlStmt,
    /// `USING expr, ...`
    PlUsing,

    // ---- Expressions ----
    BinaryExpr,
    PrefixExpr,
    /// `IS [NOT] NULL` / `ISNULL` / `IS DISTINCT FROM` etc.
    IsExpr,
    BetweenExpr,
    InExpr,
    /// `LIKE` / `ILIKE` / `SIMILAR TO` (including `ESCAPE`)
    LikeExpr,
    /// `expr::type`
    CastExpr,
    CollateExpr,
    AtTimeZoneExpr,
    /// `expr[i]` / `expr[i:j]`
    SubscriptExpr,
    /// `(expr).field`
    FieldAccess,
    /// `a` / `t.a` / `t.*` / `*`
    ColumnRef,
    /// A number, string, `NULL`, `TRUE`, `FALSE` or `DEFAULT`
    Literal,
    /// `$1`
    ParamRef,
    /// `interval '1 day'` etc.
    TypedLiteral,
    FuncCall,
    /// The `(...)` of a function call
    ArgList,
    WithinGroupClause,
    FilterClause,
    OverClause,
    /// `(PARTITION BY ... ORDER BY ... ROWS ...)`
    WindowSpec,
    PartitionByClause,
    /// `ROWS BETWEEN ... AND ...` etc.
    FrameClause,
    CaseExpr,
    WhenClause,
    ElseClause,
    /// `CAST(expr AS type)`
    CastCall,
    ParenExpr,
    /// `(a, b)` / `ROW(a, b)`
    RowExpr,
    /// `(SELECT ...)` inside an expression
    SubqueryExpr,
    ExistsExpr,
    /// `ARRAY[...]` / `ARRAY(SELECT ...)`
    ArrayExpr,
    /// A parenthesized list of expressions, as in `IN (...)` or `VALUES (...)`
    ExprList,
    TypeName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node<'a> {
    pub kind: NodeKind,
    pub children: Vec<Element<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Element<'a> {
    Node(Node<'a>),
    Token(Token<'a>),
}

impl<'a> Node<'a> {
    /// Calls `f` on every token under this node, in order from the start
    pub fn for_each_token(&self, f: &mut impl FnMut(&Token<'a>)) {
        for child in &self.children {
            match child {
                Element::Node(node) => node.for_each_token(f),
                Element::Token(token) => f(token),
            }
        }
    }

    /// The original text (including whitespace and comments)
    pub fn text(&self) -> String {
        let mut text = String::new();
        self.for_each_token(&mut |t| text.push_str(t.text));
        text
    }

    /// Renders the tree with indentation (whitespace tokens are omitted). For tests and debugging.
    pub fn debug_tree(&self) -> String {
        let mut out = String::new();
        self.write_tree(&mut out, 0);
        out
    }

    fn write_tree(&self, out: &mut String, depth: usize) {
        writeln!(out, "{:indent$}{:?}", "", self.kind, indent = depth * 2).unwrap();
        for child in &self.children {
            match child {
                Element::Node(node) => node.write_tree(out, depth + 1),
                Element::Token(t) if t.kind == TokenKind::Whitespace => {}
                Element::Token(t) => writeln!(
                    out,
                    "{:indent$}{} {:?}",
                    "",
                    token_label(t.kind),
                    t.text,
                    indent = (depth + 1) * 2
                )
                .unwrap(),
            }
        }
    }
}

fn token_label(kind: TokenKind) -> String {
    let (name, terminated) = match kind {
        TokenKind::BlockComment { terminated } => ("BlockComment", terminated),
        TokenKind::QuotedIdent { terminated } => ("QuotedIdent", terminated),
        TokenKind::String { terminated, .. } => ("String", terminated),
        TokenKind::DollarString { terminated } => ("DollarString", terminated),
        other => return format!("{other:?}"),
    };
    if terminated {
        name.to_string()
    } else {
        format!("{name}(unterminated)")
    }
}
