//! Test 2e: Identifier quoting with postgres_expr! macro.

use vantage_sql::postgres_expr;
use vantage_sql::primitives::identifier::{Identifier, ident};

#[test]
fn test_id_in_select() {
    let expr = postgres_expr!("SELECT {} FROM {}", (ident("name")), (ident("product")));
    assert_eq!(expr.preview(), "SELECT \"name\" FROM \"product\"");
}

#[test]
fn test_id_in_condition() {
    let expr = postgres_expr!("{} = {}", (ident("price")), 100i64);
    assert_eq!(expr.preview(), "\"price\" = 100");
}

#[test]
fn test_id_with_alias() {
    let expr = postgres_expr!("SELECT {}", (ident("name").with_alias("n")));
    assert_eq!(expr.preview(), "SELECT \"name\" AS \"n\"");
}

#[test]
fn test_id_qualified() {
    let expr = postgres_expr!("SELECT {}", (ident("name").dot_of("t")));
    assert_eq!(expr.preview(), "SELECT \"t\".\"name\"");
}

#[test]
fn test_id_with_space() {
    let expr = postgres_expr!("SELECT {}", (ident("first name")));
    assert_eq!(expr.preview(), "SELECT \"first name\"");
}

#[test]
fn test_id_with_hyphen() {
    let expr = postgres_expr!("SELECT {}", (ident("my-column")));
    assert_eq!(expr.preview(), "SELECT \"my-column\"");
}

#[test]
fn test_id_reserved_word() {
    let expr = postgres_expr!("SELECT {} FROM {}", (ident("select")), (ident("order")));
    assert_eq!(expr.preview(), "SELECT \"select\" FROM \"order\"");
}

#[test]
fn test_id_with_dot_in_name() {
    let expr = postgres_expr!("SELECT {}", (ident("weird.name")));
    assert_eq!(expr.preview(), "SELECT \"weird.name\"");
}

#[test]
fn test_id_unicode() {
    let expr = postgres_expr!("SELECT {}", (ident("名前")));
    assert_eq!(expr.preview(), "SELECT \"名前\"");
}

#[test]
fn test_id_number_start() {
    let expr = postgres_expr!("SELECT {}", (ident("1bad_name")));
    assert_eq!(expr.preview(), "SELECT \"1bad_name\"");
}
