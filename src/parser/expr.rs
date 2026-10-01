//! 式と型名。
//!
//! 二項演算は Pratt パーサーで読む。優先順位は PostgreSQL のドキュメント
//! 「4.1.6. Operator Precedence」に合わせている。

use super::Parser;
use super::keywords::{is_reserved, is_value_keyword};
use crate::lexer::{StringPrefix, TokenKind};
use crate::syntax::NodeKind;

// 結合力。大きいほど強く結びつく。
const BP_OR: u8 = 1;
const BP_AND: u8 = 2;
const BP_NOT: u8 = 3;
const BP_IS: u8 = 4;
const BP_COMPARISON: u8 = 5;
/// `BETWEEN` `IN` `LIKE` `ILIKE` `SIMILAR`
const BP_PATTERN: u8 = 6;
/// `||` `@>` などその他の演算子
const BP_OTHER_OP: u8 = 7;
const BP_ADD: u8 = 8;
const BP_MUL: u8 = 9;
const BP_EXP: u8 = 10;
const BP_UNARY: u8 = 11;
const BP_COLLATE: u8 = 12;
const BP_AT: u8 = 13;
const BP_SUBSCRIPT: u8 = 14;
const BP_CAST: u8 = 15;
const BP_FIELD: u8 = 16;

/// 比較・論理演算・IN などを含まない式（`BETWEEN` の上下限や `POSITION(a IN b)` の引数）
const BP_B_EXPR: u8 = BP_OTHER_OP;

/// 関数の引数に現れる、式の一部ではないキーワード
/// （`count(DISTINCT x)`、`extract(year FROM d)`、`substring(s FROM 1 FOR 2)`、`trim(BOTH 'x' FROM s)` など）
const ARG_KEYWORDS: &[&str] = &[
    "all", "distinct", "variadic", "from", "for", "in", "as", "both", "leading", "trailing",
    "placing",
];

#[derive(Clone, Copy)]
enum Infix {
    Binary,
    Is,
    Between,
    In,
    Like,
    Cast,
    Collate,
    AtTimeZone,
    Subscript,
    Field,
}

impl Infix {
    fn node_kind(self) -> NodeKind {
        match self {
            Infix::Binary => NodeKind::BinaryExpr,
            Infix::Is => NodeKind::IsExpr,
            Infix::Between => NodeKind::BetweenExpr,
            Infix::In => NodeKind::InExpr,
            Infix::Like => NodeKind::LikeExpr,
            Infix::Cast => NodeKind::CastExpr,
            Infix::Collate => NodeKind::CollateExpr,
            Infix::AtTimeZone => NodeKind::AtTimeZoneExpr,
            Infix::Subscript => NodeKind::SubscriptExpr,
            Infix::Field => NodeKind::FieldAccess,
        }
    }
}

impl Parser<'_> {
    /// 式を 1 つ読む。式が始まらなければ何も取り込まずに false を返す。
    pub(super) fn expr(&mut self) -> bool {
        self.expr_bp(0)
    }

    /// 比較・論理演算・IN などを含まない式（`FOREACH x SLICE 1 IN ARRAY ...` の `1` など）
    pub(super) fn expr_without_in(&mut self) -> bool {
        self.expr_bp(BP_B_EXPR)
    }

    /// UPDATE の `SET` の左辺（`col` / `col[1]` / `col.field`）。`=` の手前で止まる。
    pub(super) fn set_target(&mut self) -> bool {
        self.expr_bp(BP_COMPARISON + 1)
    }

    fn expr_bp(&mut self, min_bp: u8) -> bool {
        let cp = self.checkpoint();
        if !self.prefix_or_primary() {
            return false;
        }
        while let Some((infix, bp)) = self.infix() {
            if bp < min_bp {
                break;
            }
            self.start_node_at(cp, infix.node_kind());
            self.infix_rest(infix, bp);
            self.finish_node();
        }
        true
    }

    /// 次のトークンが中置・後置の演算子なら、その種類と結合力
    fn infix(&self) -> Option<(Infix, u8)> {
        let token = self.current()?;
        let found = match token.kind {
            TokenKind::Operator => {
                let bp = match token.text {
                    "=" | "<" | ">" | "<=" | ">=" | "<>" | "!=" => BP_COMPARISON,
                    "+" | "-" => BP_ADD,
                    "*" | "/" | "%" => BP_MUL,
                    "^" => BP_EXP,
                    _ => BP_OTHER_OP,
                };
                (Infix::Binary, bp)
            }
            TokenKind::DoubleColon => (Infix::Cast, BP_CAST),
            TokenKind::LBracket => (Infix::Subscript, BP_SUBSCRIPT),
            TokenKind::Dot => (Infix::Field, BP_FIELD),
            TokenKind::Ident => {
                // `NOT BETWEEN` などは NOT の次で判定する
                let n = usize::from(self.at_kw("not"));
                if n == 0 && self.at_kw("or") {
                    (Infix::Binary, BP_OR)
                } else if n == 0 && self.at_kw("and") {
                    (Infix::Binary, BP_AND)
                } else if n == 0 && self.at_any_kw(&["is", "isnull", "notnull"]) {
                    (Infix::Is, BP_IS)
                } else if self.nth_kw(n, "between") {
                    (Infix::Between, BP_PATTERN)
                } else if self.nth_kw(n, "in") {
                    (Infix::In, BP_PATTERN)
                } else if self.nth_kw(n, "like")
                    || self.nth_kw(n, "ilike")
                    || (self.nth_kw(n, "similar") && self.nth_kw(n + 1, "to"))
                {
                    (Infix::Like, BP_PATTERN)
                } else if n == 0 && self.at_kw("collate") {
                    (Infix::Collate, BP_COLLATE)
                } else if n == 0
                    && self.at_kw("at")
                    && (self.nth_kw(1, "time") || self.nth_kw(1, "local"))
                {
                    (Infix::AtTimeZone, BP_AT)
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        Some(found)
    }

    /// 演算子と右辺を読む（左辺は読み終えている）
    fn infix_rest(&mut self, infix: Infix, bp: u8) {
        match infix {
            Infix::Binary => {
                self.bump_kw();
                self.expr_bp(bp + 1);
            }
            Infix::Is => self.is_rest(),
            Infix::Between => {
                self.eat_kw("not");
                self.bump_kw();
                if !self.eat_kw("symmetric") {
                    self.eat_kw("asymmetric");
                }
                self.expr_bp(BP_B_EXPR);
                if self.eat_kw("and") {
                    self.expr_bp(BP_B_EXPR);
                }
            }
            Infix::In => {
                self.eat_kw("not");
                self.bump_kw();
                if self.at(TokenKind::LParen) {
                    if self.at_query_start(1) {
                        self.subquery_expr();
                    } else {
                        self.expr_list();
                    }
                }
            }
            Infix::Like => {
                self.eat_kw("not");
                if self.eat_kw("similar") {
                    self.eat_kw("to");
                } else {
                    self.bump_kw();
                }
                self.expr_bp(BP_PATTERN + 1);
                if self.eat_kw("escape") {
                    self.expr_bp(BP_PATTERN + 1);
                }
            }
            Infix::Cast => {
                self.bump();
                self.type_name();
            }
            Infix::Collate => {
                self.bump_kw();
                self.name_path();
            }
            Infix::AtTimeZone => {
                self.bump_kw();
                if !self.eat_kw("local") {
                    self.eat_kw("time");
                    self.eat_kw("zone");
                    self.expr_bp(bp + 1);
                }
            }
            Infix::Subscript => {
                self.bump();
                if !self.at(TokenKind::Colon) {
                    self.expr();
                }
                if self.eat(TokenKind::Colon) && !self.at(TokenKind::RBracket) {
                    self.expr();
                }
                self.expect_closing(TokenKind::RBracket);
            }
            Infix::Field => {
                self.bump();
                if self.at_name() || self.at_op("*") {
                    self.bump();
                }
            }
        }
    }

    /// `IS [NOT] NULL` / `IS [NOT] DISTINCT FROM expr` / `ISNULL` / `NOTNULL` など
    fn is_rest(&mut self) {
        if self.eat_kw("isnull") || self.eat_kw("notnull") {
            return;
        }
        self.bump_kw();
        self.eat_kw("not");
        if self.eat_kw("distinct") {
            self.eat_kw("from");
            self.expr_bp(BP_IS + 1);
        } else if self.at_any_kw(&["nfc", "nfd", "nfkc", "nfkd"]) {
            self.bump_kw();
            self.eat_kw("normalized");
        } else if self.eat_kw("json") {
            if self.at_any_kw(&["value", "array", "object", "scalar"]) {
                self.bump_kw();
            }
        } else if self.at_any_kw(&["null", "true", "false", "unknown", "normalized", "document"]) {
            self.bump_kw();
        }
    }

    fn prefix_or_primary(&mut self) -> bool {
        if self.at_kw("not") {
            self.start_node(NodeKind::PrefixExpr);
            self.bump_kw();
            self.expr_bp(BP_NOT);
            self.finish_node();
            return true;
        }
        if let Some(token) = self.current()
            && token.kind == TokenKind::Operator
            && token.text != "*"
        {
            let bp = if matches!(token.text, "+" | "-") {
                BP_UNARY
            } else {
                BP_OTHER_OP
            };
            self.start_node(NodeKind::PrefixExpr);
            self.bump();
            self.expr_bp(bp);
            self.finish_node();
            return true;
        }
        self.primary()
    }

    fn primary(&mut self) -> bool {
        let Some(token) = self.current() else {
            return false;
        };
        match token.kind {
            TokenKind::String { .. } => {
                self.start_node(NodeKind::Literal);
                self.bump();
                while self.at_string_continuation() {
                    self.bump();
                }
                self.finish_node();
                true
            }
            TokenKind::Number | TokenKind::DollarString { .. } => {
                self.single_token_node(NodeKind::Literal)
            }
            TokenKind::Param => self.single_token_node(NodeKind::ParamRef),
            TokenKind::Operator if token.text == "*" => self.single_token_node(NodeKind::ColumnRef),
            TokenKind::LParen => {
                self.paren_primary();
                true
            }
            TokenKind::QuotedIdent { .. } | TokenKind::PsqlVariable => {
                self.name_or_call();
                true
            }
            TokenKind::Ident => self.ident_primary(token.text),
            _ => false,
        }
    }

    /// 直前の文字列に続く `'...'` か。PostgreSQL では、改行を含む空白だけを挟んだ文字列はつながって
    /// 1 つの文字列になる（`'a'` 改行 `'b'` は `'ab'`）。同じ行に並べると構文エラーになる。
    fn at_string_continuation(&self) -> bool {
        match &self.tokens[self.pos..] {
            [space, next, ..] => {
                space.kind == TokenKind::Whitespace
                    && space.text.contains('\n')
                    && matches!(
                        next.kind,
                        TokenKind::String {
                            prefix: StringPrefix::None,
                            ..
                        }
                    )
            }
            _ => false,
        }
    }

    fn single_token_node(&mut self, kind: NodeKind) -> bool {
        self.start_node(kind);
        self.bump_kw();
        self.finish_node();
        true
    }

    fn ident_primary(&mut self, text: &str) -> bool {
        let next_is = |kind| self.nth_is(1, kind);
        match text.to_ascii_lowercase().as_str() {
            "null" | "true" | "false" | "default" => self.single_token_node(NodeKind::Literal),
            "case" => {
                self.case_expr();
                true
            }
            "cast" if next_is(TokenKind::LParen) => {
                self.cast_call();
                true
            }
            "exists" if next_is(TokenKind::LParen) => {
                self.start_node(NodeKind::ExistsExpr);
                self.bump_kw();
                self.subquery_expr();
                self.finish_node();
                true
            }
            "array" if next_is(TokenKind::LBracket) || next_is(TokenKind::LParen) => {
                self.start_node(NodeKind::ArrayExpr);
                self.bump_kw();
                if self.at(TokenKind::LBracket) {
                    self.array_brackets();
                } else {
                    self.subquery_expr();
                }
                self.finish_node();
                true
            }
            "row" if next_is(TokenKind::LParen) => {
                self.start_node(NodeKind::RowExpr);
                self.bump_kw();
                self.expr_list();
                self.finish_node();
                true
            }
            "any" | "all" | "some" if next_is(TokenKind::LParen) => {
                self.keyword_call();
                true
            }
            word if is_value_keyword(word) => {
                self.keyword_call();
                true
            }
            word if is_reserved(word) => false,
            _ => {
                self.name_or_call();
                true
            }
        }
    }

    /// キーワードの値や関数（`CURRENT_TIMESTAMP` / `CURRENT_TIMESTAMP(3)` / `ANY(...)`）
    fn keyword_call(&mut self) {
        let cp = self.checkpoint();
        self.bump_kw();
        if self.at(TokenKind::LParen) {
            self.arg_list(0);
            self.wrap(cp, NodeKind::FuncCall);
        } else {
            self.wrap(cp, NodeKind::ColumnRef);
        }
    }

    /// 列参照・関数呼び出し・`type 'literal'` のいずれか
    fn name_or_call(&mut self) {
        let cp = self.checkpoint();
        let first = self.current().unwrap();
        let parts = self.name_path();
        if self.at(TokenKind::LParen) {
            let arg_bp = if parts == 1 && first.text.eq_ignore_ascii_case("position") {
                BP_B_EXPR
            } else {
                0
            };
            self.arg_list(arg_bp);
            self.call_suffixes();
            self.wrap(cp, NodeKind::FuncCall);
        } else if parts == 1
            && first.kind == TokenKind::Ident
            && self
                .current()
                .is_some_and(|t| matches!(t.kind, TokenKind::String { .. }))
        {
            self.bump();
            self.wrap(cp, NodeKind::TypedLiteral);
        } else {
            self.wrap(cp, NodeKind::ColumnRef);
        }
    }

    /// `a.b.c` / `t.*` の形の名前を読み、部分の数を返す。名前で始まらなければ 0。
    pub(super) fn name_path(&mut self) -> usize {
        if !self.at_name() {
            return 0;
        }
        self.bump();
        let mut parts = 1;
        while self.at(TokenKind::Dot)
            && self.nth(1).is_some_and(|t| {
                matches!(
                    t.kind,
                    TokenKind::Ident | TokenKind::QuotedIdent { .. } | TokenKind::PsqlVariable
                ) || (t.kind == TokenKind::Operator && t.text == "*")
            })
        {
            self.bump();
            self.bump();
            parts += 1;
        }
        parts
    }

    /// n 番目が名前（識別子・引用符付き識別子・psql の変数）か
    pub(super) fn nth_is_any_name(&self, n: usize) -> bool {
        self.nth(n).is_some_and(|t| {
            matches!(
                t.kind,
                TokenKind::Ident | TokenKind::QuotedIdent { .. } | TokenKind::PsqlVariable
            )
        })
    }

    pub(super) fn at_name(&self) -> bool {
        self.current().is_some_and(|t| {
            matches!(
                t.kind,
                TokenKind::Ident | TokenKind::QuotedIdent { .. } | TokenKind::PsqlVariable
            )
        })
    }

    /// 関数呼び出しの `(...)`。`arg_bp` は引数の式の最小の結合力。
    pub(super) fn arg_list(&mut self, arg_bp: u8) {
        self.start_node(NodeKind::ArgList);
        self.bump();
        loop {
            if self.at(TokenKind::RParen) || self.at_statement_end() {
                break;
            }
            if self.eat(TokenKind::Comma) {
                continue;
            }
            if self.at_kw("order") && self.nth_kw(1, "by") {
                self.order_by_clause(Self::at_clause_keyword);
            } else if self.at_any_kw(ARG_KEYWORDS) || self.at(TokenKind::ColonEquals) {
                self.bump_kw();
            } else if self.at_query_start(0) {
                self.select_stmt();
            } else if !self.expr_bp(arg_bp) {
                self.error_token();
            }
        }
        self.expect_closing(TokenKind::RParen);
        self.finish_node();
    }

    /// `WITHIN GROUP (...)` / `FILTER (WHERE ...)` / `OVER ...`
    fn call_suffixes(&mut self) {
        if self.at_kw("within") && self.nth_kw(1, "group") {
            self.start_node(NodeKind::WithinGroupClause);
            self.bump_kw();
            self.bump_kw();
            if self.eat(TokenKind::LParen) {
                if self.at_kw("order") {
                    self.order_by_clause(Self::at_clause_keyword);
                }
                self.expect_closing(TokenKind::RParen);
            }
            self.finish_node();
        }
        if self.at_kw("filter") && self.nth_is(1, TokenKind::LParen) {
            self.start_node(NodeKind::FilterClause);
            self.bump_kw();
            self.bump();
            if self.at_kw("where") {
                self.where_clause();
            }
            self.expect_closing(TokenKind::RParen);
            self.finish_node();
        }
        if self.at_kw("over") {
            self.start_node(NodeKind::OverClause);
            self.bump_kw();
            if self.at(TokenKind::LParen) {
                self.window_spec();
            } else if self.at_name() {
                self.bump();
            }
            self.finish_node();
        }
    }

    /// `(name PARTITION BY ... ORDER BY ... ROWS ...)`
    pub(super) fn window_spec(&mut self) {
        const FRAME: &[&str] = &["rows", "range", "groups"];
        self.start_node(NodeKind::WindowSpec);
        self.bump();
        if self.at_name() && !self.at_any_kw(&["partition", "order"]) && !self.at_any_kw(FRAME) {
            self.bump();
        }
        if self.at_kw("partition") {
            self.start_node(NodeKind::PartitionByClause);
            self.bump_kw();
            self.eat_kw("by");
            self.comma_list(
                |p| p.at_any_kw(&["order", "rows", "range", "groups"]),
                Self::expr,
            );
            self.finish_node();
        }
        if self.at_kw("order") {
            self.order_by_clause(|p| p.at_clause_keyword() || p.at_any_kw(FRAME));
        }
        if self.at_any_kw(FRAME) {
            self.start_node(NodeKind::FrameClause);
            while !self.at(TokenKind::RParen) && !self.at_statement_end() {
                self.bump_balanced();
            }
            self.finish_node();
        }
        self.expect_closing(TokenKind::RParen);
        self.finish_node();
    }

    /// `(` から始まる式: 副問い合わせ、括弧で囲んだ式、行
    fn paren_primary(&mut self) {
        if self.at_query_start(1) {
            self.subquery_expr();
            return;
        }
        let cp = self.checkpoint();
        self.bump();
        let mut is_row = self.at(TokenKind::RParen);
        if !is_row && !self.expr() {
            self.error_until(|p| p.at(TokenKind::Comma));
        }
        while self.eat(TokenKind::Comma) {
            is_row = true;
            if !self.expr() {
                self.error_until(|p| p.at(TokenKind::Comma));
            }
        }
        self.expect_closing(TokenKind::RParen);
        if is_row {
            self.wrap(cp, NodeKind::ExprList);
            self.wrap(cp, NodeKind::RowExpr);
        } else {
            self.wrap(cp, NodeKind::ParenExpr);
        }
    }

    /// `(SELECT ...)`。CTE の本体の `(INSERT ... RETURNING ...)` なども読む。
    /// それ以外の文はそのまま保持する。
    pub(super) fn subquery_expr(&mut self) {
        self.start_node(NodeKind::SubqueryExpr);
        self.bump();
        if !self.statement_body() && !self.at(TokenKind::RParen) && !self.at_statement_end() {
            self.start_node(NodeKind::RawStatement);
            while !self.at(TokenKind::RParen) && !self.at_statement_end() {
                self.bump_balanced();
            }
            self.finish_node();
        }
        self.expect_closing(TokenKind::RParen);
        self.finish_node();
    }

    /// `(expr, ...)`
    pub(super) fn expr_list(&mut self) {
        self.start_node(NodeKind::ExprList);
        self.bump();
        self.comma_list(|_| false, Self::expr);
        self.expect_closing(TokenKind::RParen);
        self.finish_node();
    }

    /// `[...]`。入れ子の `[...]` は `ARRAY` を省いた配列になる。
    fn array_brackets(&mut self) {
        self.bump();
        loop {
            if self.at(TokenKind::RBracket) || self.at(TokenKind::RParen) || self.at_statement_end()
            {
                break;
            }
            if self.eat(TokenKind::Comma) {
                continue;
            }
            if self.at(TokenKind::LBracket) {
                self.start_node(NodeKind::ArrayExpr);
                self.array_brackets();
                self.finish_node();
            } else if !self.expr() {
                self.error_token();
            }
        }
        self.expect_closing(TokenKind::RBracket);
    }

    fn case_expr(&mut self) {
        self.start_node(NodeKind::CaseExpr);
        self.bump_kw();
        if !self.at_kw("when") {
            self.expr();
        }
        while self.at_kw("when") {
            self.start_node(NodeKind::WhenClause);
            self.bump_kw();
            self.expr();
            if self.eat_kw("then") {
                self.expr();
            }
            self.finish_node();
        }
        if self.at_kw("else") {
            self.start_node(NodeKind::ElseClause);
            self.bump_kw();
            self.expr();
            self.finish_node();
        }
        if !self.eat_kw("end") {
            self.error_until(|p| p.at_kw("end"));
            self.eat_kw("end");
        }
        self.finish_node();
    }

    fn cast_call(&mut self) {
        self.start_node(NodeKind::CastCall);
        self.bump_kw();
        self.bump();
        self.expr();
        if self.eat_kw("as") {
            self.type_name();
        }
        self.expect_closing(TokenKind::RParen);
        self.finish_node();
    }

    /// 型名。`double precision` / `character varying(10)` / `timestamp(3) with time zone` /
    /// `interval day to second` / `int[]` などの複数語・修飾子・配列を含む。
    pub(super) fn type_name(&mut self) {
        let Some(first) = self.current().filter(|_| self.at_name()) else {
            return;
        };
        self.start_node(NodeKind::TypeName);
        let first = first.text.to_ascii_lowercase();
        self.name_path();
        match first.as_str() {
            "double" => {
                self.eat_word("precision");
            }
            "national" => {
                if !self.eat_word("character") {
                    self.eat_word("char");
                }
                self.eat_word("varying");
            }
            "character" | "char" | "nchar" | "bit" => {
                self.eat_word("varying");
            }
            _ => {}
        }
        if self.at(TokenKind::LParen) {
            self.bump_balanced();
        }
        if matches!(first.as_str(), "timestamp" | "time")
            && self.at_any_kw(&["with", "without"])
            && self.nth_kw(1, "time")
        {
            self.bump();
            self.bump();
            self.eat_word("zone");
        }
        const FIELDS: &[&str] = &["year", "month", "day", "hour", "minute", "second"];
        if first == "interval" && self.at_any_kw(FIELDS) {
            self.bump();
            if self.eat_word("to") && self.at_any_kw(FIELDS) {
                self.bump();
            }
            if self.at(TokenKind::LParen) {
                self.bump_balanced();
            }
        }
        while self.at(TokenKind::LBracket) {
            self.bump();
            self.eat(TokenKind::Number);
            self.expect_closing(TokenKind::RBracket);
        }
        // PL/pgSQL の `tbl.col%TYPE` / `tbl%ROWTYPE`
        if self.at_op("%") && (self.nth_kw(1, "type") || self.nth_kw(1, "rowtype")) {
            self.bump();
            self.bump();
        }
        self.finish_node();
    }
}
