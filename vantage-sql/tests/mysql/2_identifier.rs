//! Test 2e: Identifier quoting with mysql_expr! macro.
//! Verifies that Identifier works as an Expressive in expression macros
//! and handles unusual but valid MySQL identifier characters.

use vantage_sql::mysql_expr;
use vantage_sql::primitives::identifier::{Identifier, ident};

// ── ident() in expr macro via (parentheses) ───────────────────────────────────

#[test]
fn test_id_in_select() {
    let expr = mysql_expr!("SELECT {} FROM {}", (ident("name")), (ident("product")));
    assert_eq!(expr.preview(), "SELECT `name` FROM `product`");
}

#[test]
fn test_id_in_condition() {
    let expr = mysql_expr!("{} = {}", (ident("price")), 100i64);
    assert_eq!(expr.preview(), "`price` = 100");
}

#[test]
fn test_id_with_alias() {
    let expr = mysql_expr!("SELECT {}", (ident("name").with_alias("n")));
    assert_eq!(expr.preview(), "SELECT `name` AS `n`");
}

#[test]
fn test_id_qualified() {
    let expr = mysql_expr!("SELECT {}", (Identifier::with_dot("t", "name")));
    assert_eq!(expr.preview(), "SELECT `t`.`name`");
}

// ── Unusual but valid identifier characters ────────────────────────────────
// MySQL and PostgreSQL both allow spaces, hyphens, special chars inside
// quoted identifiers.

#[test]
fn test_id_with_space() {
    let expr = mysql_expr!("SELECT {}", (ident("first name")));
    assert_eq!(expr.preview(), "SELECT `first name`");
}

#[test]
fn test_id_with_hyphen() {
    let expr = mysql_expr!("SELECT {}", (ident("my-column")));
    assert_eq!(expr.preview(), "SELECT `my-column`");
}

#[test]
fn test_id_reserved_word() {
    let expr = mysql_expr!("SELECT {} FROM {}", (ident("select")), (ident("order")));
    assert_eq!(expr.preview(), "SELECT `select` FROM `order`");
}

#[test]
fn test_id_with_dot_in_name() {
    // A single identifier containing a literal dot (not qualified)
    let expr = mysql_expr!("SELECT {}", (ident("weird.name")));
    assert_eq!(expr.preview(), "SELECT `weird.name`");
}

#[test]
fn test_id_unicode() {
    let expr = mysql_expr!("SELECT {}", (ident("名前")));
    assert_eq!(expr.preview(), "SELECT `名前`");
}

#[test]
fn test_id_number_start() {
    let expr = mysql_expr!("SELECT {}", (ident("1bad_name")));
    assert_eq!(expr.preview(), "SELECT `1bad_name`");
}
