//! 構文木。
//!
//! 空白・コメントを含むすべてのトークンを保持するロスレスな木で、
//! 木のトークンを順につなげると入力と一致する。
//! ノードは種類と子（ノードかトークン）の並びだけを持つ汎用の形にしている。

use std::fmt::Write;

use crate::lexer::{Token, TokenKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Root,
    /// 対応していない文。トークンをそのまま保持する
    RawStatement,
    /// 対応している構文の中で解釈できなかったトークン列
    Error,

    // ---- SELECT ----
    /// WITH・集合演算・ORDER BY・LIMIT などを含む問い合わせ全体
    SelectStmt,
    WithClause,
    Cte,
    /// `UNION` / `INTERSECT` / `EXCEPT`
    SetOperation,
    /// `(SELECT ...)`（集合演算の項）
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
    /// FROM 句の `(SELECT ...) alias`
    DerivedTable,
    /// FROM 句の `func(...) alias`
    FunctionTable,
    /// FROM 句の `(a JOIN b ...) alias`
    ParenJoin,
    JoinExpr,
    /// `ON expr` / `USING (...)`
    JoinCondition,
    WhereClause,
    GroupByClause,
    GroupingSets,
    HavingClause,
    WindowClause,
    /// WINDOW 句の `name AS (...)`
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
    /// DELETE の `USING ...`
    UsingClause,
    ReturningClause,

    // ---- MERGE ----
    MergeStmt,
    /// `WHEN [NOT] MATCHED [BY ...] [AND cond] THEN action`
    MergeWhenClause,

    // ---- DDL ----
    CreateTableStmt,
    /// CREATE TABLE の `(列, 制約, ...)`
    TableElementList,
    ColumnDef,
    /// 表制約（`LIKE source` を含む）
    TableConstraint,
    CreateIndexStmt,
    /// `CREATE [MATERIALIZED] VIEW`
    CreateViewStmt,
    AlterTableStmt,
    AlterTableAction,
    DropStmt,
    /// `CREATE [TEMP | UNLOGGED] SEQUENCE`
    CreateSequenceStmt,
    /// シーケンスのオプション（`INCREMENT BY 2` / `NO CYCLE` / `OWNED BY t.c` など）
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
    /// `BEGIN` / `COMMIT` / `ROLLBACK` / `SAVEPOINT` などのトランザクション制御
    TransactionStmt,
    /// `ALTER TABLE` 以外の `ALTER object name action ...`
    AlterStmt,
    /// `GRANT ... ON ... TO ...` / `REVOKE ... ON ... FROM ...`（ロールの付与も）
    GrantStmt,
    /// `TRUNCATE [TABLE] name, ... [RESTART IDENTITY] [CASCADE]`
    TruncateStmt,
    /// `COMMENT ON object IS 'text'`
    CommentStmt,
    /// `CREATE [OR REPLACE] [CONSTRAINT] TRIGGER`
    CreateTriggerStmt,
    /// トリガーの句（`BEFORE ... ON table` / `FOR EACH ROW` / `WHEN (...)` / `EXECUTE FUNCTION f()` など）
    TriggerClause,

    // ---- 関数・プロシージャ ----
    /// `CREATE [OR REPLACE] FUNCTION / PROCEDURE`
    CreateFunctionStmt,
    /// 引数や `RETURNS TABLE` の列の並び
    ParamList,
    Param,
    /// `RETURNS type` / `RETURNS TABLE (...)`
    ReturnsClause,
    /// `LANGUAGE plpgsql` / `AS $$...$$` / `IMMUTABLE` など
    FunctionOption,
    /// 中身を解析したドル引用符の本体（`$tag$` と中身と `$tag$`）
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
    /// 変数・カーソル・別名の宣言
    PlDecl,
    PlExceptionSection,
    /// `WHEN cond THEN ...`
    PlExceptionHandler,
    /// `target := expr;`
    PlAssign,
    PlIf,
    /// `ELSIF cond THEN ...`
    PlElsif,
    /// IF / CASE の `ELSE ...`
    PlElse,
    PlCase,
    /// CASE 文の `WHEN ... THEN ...`
    PlCaseWhen,
    /// `LOOP` / `WHILE` / `FOR` / `FOREACH`
    PlLoop,
    /// `EXIT` / `CONTINUE`
    PlExit,
    PlReturn,
    PlRaise,
    PlAssert,
    /// `PERFORM ...`（SELECT の代わりに PERFORM を書く問い合わせ）
    PlPerform,
    PlExecute,
    PlGetDiagnostics,
    PlOpen,
    /// `NULL;`
    PlNull,
    /// 中身を細かく解釈しない文（`FETCH` / `MOVE` / `CLOSE` / `COMMIT` / `ROLLBACK`）
    PlSimpleStmt,
    /// 本体の中の SQL 文と、その後ろの `;`
    PlSqlStmt,
    /// `USING expr, ...`
    PlUsing,

    // ---- 式 ----
    BinaryExpr,
    PrefixExpr,
    /// `IS [NOT] NULL` / `ISNULL` / `IS DISTINCT FROM` など
    IsExpr,
    BetweenExpr,
    InExpr,
    /// `LIKE` / `ILIKE` / `SIMILAR TO`（`ESCAPE` を含む）
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
    /// 数値・文字列・`NULL`・`TRUE`・`FALSE`・`DEFAULT`
    Literal,
    /// `$1`
    ParamRef,
    /// `interval '1 day'` など
    TypedLiteral,
    FuncCall,
    /// 関数呼び出しの `(...)`
    ArgList,
    WithinGroupClause,
    FilterClause,
    OverClause,
    /// `(PARTITION BY ... ORDER BY ... ROWS ...)`
    WindowSpec,
    PartitionByClause,
    /// `ROWS BETWEEN ... AND ...` など
    FrameClause,
    CaseExpr,
    WhenClause,
    ElseClause,
    /// `CAST(expr AS type)`
    CastCall,
    ParenExpr,
    /// `(a, b)` / `ROW(a, b)`
    RowExpr,
    /// 式の中の `(SELECT ...)`
    SubqueryExpr,
    ExistsExpr,
    /// `ARRAY[...]` / `ARRAY(SELECT ...)`
    ArrayExpr,
    /// `IN (...)` や `VALUES (...)` などの括弧付きの式の並び
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
    /// 配下のトークンを先頭から順に呼び出す
    pub fn for_each_token(&self, f: &mut impl FnMut(&Token<'a>)) {
        for child in &self.children {
            match child {
                Element::Node(node) => node.for_each_token(f),
                Element::Token(token) => f(token),
            }
        }
    }

    /// 元のテキスト（空白・コメントを含む）
    pub fn text(&self) -> String {
        let mut text = String::new();
        self.for_each_token(&mut |t| text.push_str(t.text));
        text
    }

    /// 木を字下げ付きで表示する（空白トークンは省く）。テストとデバッグ用。
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
