//! Test 2e: Identifier quoting with sqlite_expr! macro.

use vantage_sql::primitives::identifier::ident;
use vantage_sql::sqlite_expr;

#[test]
fn test_id_in_condition() {
    let expr = sqlite_expr!("{} = {}", (ident("price")), 100i64);
    assert_eq!(expr.preview(), "\"price\" = 100");
}

#[test]
fn test_id_with_alias() {
    let expr = sqlite_expr!("SELECT {}", (ident("name").with_alias("n")));
    assert_eq!(expr.preview(), "SELECT \"name\" AS \"n\"");
}

#[test]
fn test_id_embedded_quote_is_doubled() {
    let expr = sqlite_expr!("SELECT {}", (ident("x\" ; DROP TABLE users --")));
    assert_eq!(expr.preview(), "SELECT \"x\"\" ; DROP TABLE users --\"");
}

#[test]
fn test_id_embedded_quote_in_alias_is_doubled() {
    let expr = sqlite_expr!("SELECT {}", (ident("col").with_alias("a\"b")));
    assert_eq!(expr.preview(), "SELECT \"col\" AS \"a\"\"b\"");
}

#[test]
fn test_id_qualified() {
    let expr = sqlite_expr!("SELECT {}", (ident("name").dot_of("t")));
    assert_eq!(expr.preview(), "SELECT \"t\".\"name\"");
}
