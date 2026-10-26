//! Test 2e: Identifier quoting with mysql_expr! macro.
//! Verifies that Identifier works as an Expressive in expression macros
//! and handles unusual but valid MySQL identifier characters.

use vantage_sql::mysql_expr;
use vantage_sql::primitives::identifier::ident;

#[test]
fn test_id_rendering() {
    let expr = mysql_expr!(
        "SELECT {}, {}, {} WHERE {} = {}",
        (ident("x` ; DROP TABLE users --")),
        (ident("name").dot_of("t")),
        (ident("col").with_alias("a`b")),
        (ident("price")),
        100i64
    );
    assert_eq!(
        expr.preview(),
        "SELECT `x`` ; DROP TABLE users --`, `t`.`name`, `col` AS `a``b` WHERE `price` = 100"
    );
}
