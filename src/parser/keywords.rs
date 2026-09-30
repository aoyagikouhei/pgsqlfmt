//! キーワードの分類。いずれも小文字で持ち、大文字小文字を区別せずに比べる。

/// PostgreSQL の予約語（`src/include/parser/kwlist.h` の RESERVED_KEYWORD）。
/// 列名や別名に AS なしでは使えない。
const RESERVED: &[&str] = &[
    "all",
    "analyse",
    "analyze",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "asymmetric",
    "both",
    "case",
    "cast",
    "check",
    "collate",
    "column",
    "constraint",
    "create",
    "current_catalog",
    "current_date",
    "current_role",
    "current_time",
    "current_timestamp",
    "current_user",
    "default",
    "deferrable",
    "desc",
    "distinct",
    "do",
    "else",
    "end",
    "except",
    "false",
    "fetch",
    "for",
    "foreign",
    "from",
    "grant",
    "group",
    "having",
    "in",
    "initially",
    "intersect",
    "into",
    "lateral",
    "leading",
    "limit",
    "localtime",
    "localtimestamp",
    "not",
    "null",
    "offset",
    "on",
    "only",
    "or",
    "order",
    "placing",
    "primary",
    "references",
    "returning",
    "select",
    "session_user",
    "some",
    "symmetric",
    "system_user",
    "table",
    "then",
    "to",
    "trailing",
    "true",
    "union",
    "unique",
    "user",
    "using",
    "variadic",
    "when",
    "where",
    "window",
    "with",
];

/// 予約語だが、それだけで値になるもの（SQL の値関数）
const VALUE_KEYWORDS: &[&str] = &[
    "current_catalog",
    "current_date",
    "current_role",
    "current_time",
    "current_timestamp",
    "current_user",
    "localtime",
    "localtimestamp",
    "session_user",
    "system_user",
    "user",
];

/// SELECT の句の始まり。並びの終わりやエラーからの回復の目印にする。
pub(super) const CLAUSE_KEYWORDS: &[&str] = &[
    "from",
    "into",
    "where",
    "group",
    "having",
    "window",
    "order",
    "limit",
    "offset",
    "fetch",
    "for",
    "union",
    "intersect",
    "except",
    "returning",
    // INSERT ... SELECT / VALUES の後ろの ON CONFLICT
    "on",
];

/// FROM 句で結合を始めるキーワード。予約語ではないが、AS なしの表の別名にはできない。
pub(super) const JOIN_KEYWORDS: &[&str] = &[
    "join", "inner", "left", "right", "full", "outer", "cross", "natural",
];

fn contains(list: &[&str], word: &str) -> bool {
    list.iter().any(|kw| kw.eq_ignore_ascii_case(word))
}

pub(super) fn is_reserved(word: &str) -> bool {
    contains(RESERVED, word)
}

pub(super) fn is_value_keyword(word: &str) -> bool {
    contains(VALUE_KEYWORDS, word)
}

pub(super) fn is_join_keyword(word: &str) -> bool {
    contains(JOIN_KEYWORDS, word)
}
