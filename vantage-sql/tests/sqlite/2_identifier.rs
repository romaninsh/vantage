//! Test 2e: Identifier quoting with sqlite_expr! macro.

use vantage_sql::primitives::identifier::ident;
use vantage_sql::sqlite_expr;

#[test]
fn test_id_rendering() {
    let expr = sqlite_expr!(
        "SELECT {}, {}, {} WHERE {} = {}",
        (ident("x\" ; DROP TABLE users --")),
        (ident("name").dot_of("t")),
        (ident("col").with_alias("a\"b")),
        (ident("price")),
        100i64
    );
    assert_eq!(
        expr.preview(),
        r#"SELECT "x"" ; DROP TABLE users --", "t"."name", "col" AS "a""b" WHERE "price" = 100"#
    );
}
