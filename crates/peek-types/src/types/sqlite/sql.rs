//! Small SQL-building helpers shared across the SQLite type.
//!
//! `rusqlite`'s parameter binding (`?1` / `params![…]`) covers
//! **values** — strings, numbers, blobs that go into `WHERE name = ?1`
//! and friends. Identifiers (table names, column names, PRAGMA
//! arguments) cannot be parameterised; SQL bind slots are typed as
//! values, not as schema references, so SQLite would otherwise treat
//! a parameterised identifier as a literal string. The rules below
//! cover the gap with the SQL-standard quoting.

/// Wrap `s` in double quotes for use as a SQL identifier (table
/// name, column name, PRAGMA argument). Embedded `"` characters are
/// doubled per the SQL spec, so a name like `my"tbl` becomes
/// `"my""tbl"` and round-trips losslessly.
///
/// One short helper rather than three inline copies — every caller
/// in the SQLite module that builds a `FROM "<x>"` / `PRAGMA
/// table_info("<x>")` / `COUNT(*) FROM "<x>"` query goes through
/// this, so a future tightening (e.g. rejecting NUL bytes) lands in
/// one place.
pub(crate) fn quote_ident(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_a_plain_identifier() {
        assert_eq!(quote_ident("books"), "\"books\"");
    }

    #[test]
    fn doubles_embedded_quotes() {
        assert_eq!(quote_ident("my\"tbl"), "\"my\"\"tbl\"");
        assert_eq!(quote_ident("a\"b\"c"), "\"a\"\"b\"\"c\"");
    }

    #[test]
    fn handles_empty_and_special_chars() {
        assert_eq!(quote_ident(""), "\"\"");
        // Spaces, dots, hyphens — all legal once quoted.
        assert_eq!(quote_ident("with space"), "\"with space\"");
        assert_eq!(quote_ident("schema.table"), "\"schema.table\"");
        assert_eq!(quote_ident("foo-bar"), "\"foo-bar\"");
    }
}
