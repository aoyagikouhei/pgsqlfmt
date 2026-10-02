//! Keyword classification. All lists are kept lowercase and compared case-insensitively.

/// PostgreSQL reserved words (RESERVED_KEYWORD in `src/include/parser/kwlist.h`).
/// They cannot be used as a column name or alias without AS.
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

/// Reserved words that are values on their own (SQL value functions)
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

/// Keywords that start a clause of SELECT. They mark the end of a list and guide error recovery.
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
    // ON CONFLICT after INSERT ... SELECT / VALUES
    "on",
];

/// Keywords that start a join in the FROM clause. They are not reserved words, but cannot be
/// used as a table alias without AS.
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
