//! A formatter for PostgreSQL SQL and PL/pgSQL.
//!
//! This crate is mainly distributed as the `pgsqlfmt` command-line tool. Install it with
//! `cargo install pgsqlfmt`, and see the
//! [usage guide](https://github.com/aoyagikouhei/pgsqlfmt/blob/main/docs/usage.md) for the
//! command-line options.
//!
//! The library exposes the same formatter as a function. Input that cannot be parsed is output
//! verbatim, so formatting never fails.
//!
//! ```
//! let out = pgsqlfmt::format("select a, b from t where x = 1");
//! assert_eq!(out, "SELECT\n    a\n  , b\nFROM t\nWHERE x = 1\n");
//! ```
//!
//! Use [`format_with_options`] to change the line width, indent width, keyword case, or comma
//! position.
//!
//! ```
//! use pgsqlfmt::{CommaStyle, FormatOptions, KeywordCase, format_with_options};
//!
//! let options = FormatOptions {
//!     keyword_case: KeywordCase::Lower,
//!     comma_style: CommaStyle::Trailing,
//!     ..FormatOptions::default()
//! };
//! let out = format_with_options("SELECT a, b FROM t", &options);
//! assert_eq!(out, "select\n    a,\n    b\nfrom t\n");
//! ```
//!
//! Only the items re-exported at the crate root are the public API. The modules are public only
//! so that the tests can use them, and they may change in any release.

#[doc(hidden)]
pub mod formatter;
#[doc(hidden)]
pub mod lexer;
#[doc(hidden)]
pub mod parser;
#[doc(hidden)]
pub mod syntax;

pub use formatter::{CommaStyle, FormatOptions, KeywordCase, format, format_with_options};
