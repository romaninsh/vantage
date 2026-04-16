//! Test 2p: SQL primitives and SqliteOperation — verifies that typed columns,
//! fx! macro, ternary, Case, Concat, and chaining work as documented.

use vantage_expressions::Expressive;
use vantage_sql::condition::SqliteCondition;
use vantage_sql::fx;
use vantage_sql::primitives::*;
use vantage_sql::sqlite::operation::SqliteOperation;
use vantage_sql::sqlite::types::AnySqliteType;
use vantage_sql::sqlite_expr;
use vantage_table::column::core::Column;

// ── SqliteOperation on typed columns ────────────────────────────────

#[test]
fn test_typed_column_eq() {
    let is_deleted = Column::<bool>::new("is_deleted");
    let cond: SqliteCondition = is_deleted.eq(false);
    assert_eq!(cond.into_expr().preview(), "is_deleted = 0");
}

#[test]
fn test_typed_column_gt() {
    let price = Column::<i64>::new("price");
    let cond: SqliteCondition = price.gt(150i64);
    assert_eq!(cond.into_expr().preview(), "price > 150");
}

#[test]
fn test_typed_column_lte() {
    let calories = Column::<i64>::new("calories");
    let cond: SqliteCondition = calories.lte(250i64);
    assert_eq!(cond.into_expr().preview(), "calories <= 250");
}

#[test]
fn test_typed_column_ne() {
    let status = Column::<AnySqliteType>::new("status");
    let cond: SqliteCondition = status.ne(AnySqliteType::from("cancelled".to_string()));
    assert_eq!(cond.into_expr().preview(), "status != 'cancelled'");
}

// ── Chaining across type boundaries ─────────────────────────────────

#[test]
fn test_chain_gt_eq_false() {
    let price = Column::<i64>::new("price");
    let cond = price.gt(10i64).eq(false);
    assert_eq!(cond.into_expr().preview(), "price > 10 = 0");
}

#[test]
fn test_chain_gt_eq_true() {
    let price = Column::<i64>::new("price");
    let cond = price.gt(10i64).eq(true);
    assert_eq!(cond.into_expr().preview(), "price > 10 = 1");
}

#[test]
fn test_chain_gt_eq_string() {
    // After first op, any AnySqliteType-compatible value is accepted
    let price = Column::<i64>::new("price");
    let cond = price.gt(10i64).eq("foobar");
    assert_eq!(cond.into_expr().preview(), "price > 10 = 'foobar'");
}

// ── Same-type column comparison ─────────────────────────────────────

#[test]
fn test_column_eq_column() {
    let price = Column::<i64>::new("price");
    let cond = price.eq(price.clone());
    assert_eq!(cond.into_expr().preview(), "price = price");
}

// ── Cross-type column rejected at compile time ──────────────────────
// price.eq(is_deleted) won't compile: Column<bool> isn't Expressive<i64>
// (verified by negative compile test, not runtime)

// ── fx! macro ───────────────────────────────────────────────────────

#[test]
fn test_fx_count_star() {
    let f: vantage_sql::primitives::Fx<AnySqliteType> = fx!("count", sqlite_expr!("*"));
    assert_eq!(f.expr().preview(), "COUNT(*)");
}

#[test]
fn test_fx_avg_ident() {
    let f: vantage_sql::primitives::Fx<AnySqliteType> = fx!("avg", ident("price"));
    assert_eq!(f.expr().preview(), "AVG(\"price\")");
}

#[test]
fn test_fx_coalesce_multiple_args() {
    let f: vantage_sql::primitives::Fx<AnySqliteType> =
        fx!("coalesce", ident("nickname"), "anonymous");
    assert_eq!(f.expr().preview(), "COALESCE(\"nickname\", 'anonymous')");
}

#[test]
fn test_fx_nested() {
    let f: vantage_sql::primitives::Fx<AnySqliteType> =
        fx!("round", fx!("avg", ident("price")), 2i64);
    assert_eq!(f.expr().preview(), "ROUND(AVG(\"price\"), 2)");
}

// ── ternary ─────────────────────────────────────────────────────────

#[test]
fn test_ternary_bare_strings() {
    let expr = ternary(ident("stock").gt(0i64), "in stock", "sold out");
    assert_eq!(
        expr.expr().preview(),
        "IIF(\"stock\" > 0, 'in stock', 'sold out')"
    );
}

// ── Case ────────────────────────────────────────────────────────────

#[test]
fn test_case_bare_strings() {
    let expr = Case::new()
        .when(ident("status").eq("active"), "yes")
        .when(ident("status").eq("banned"), "no")
        .else_("unknown");
    assert_eq!(
        expr.expr().preview(),
        "CASE WHEN \"status\" = 'active' THEN 'yes' WHEN \"status\" = 'banned' THEN 'no' ELSE 'unknown' END"
    );
}

// ── Concat ──────────────────────────────────────────────────────────

#[test]
fn test_concat_basic() {
    use vantage_sql::concat_;
    let c: Concat<AnySqliteType> = concat_!(ident("first_name"), ident("last_name"));
    assert_eq!(c.expr().preview(), "\"first_name\" || \"last_name\"");
}

#[test]
fn test_concat_ws_string_separator() {
    use vantage_sql::concat_;
    let c: Concat<AnySqliteType> = concat_!(ident("first_name"), ident("last_name")).ws(", ");
    assert_eq!(
        c.expr().preview(),
        "\"first_name\" || ', ' || \"last_name\""
    );
}

#[test]
fn test_concat_with_literal() {
    use vantage_sql::concat_;
    let c: Concat<AnySqliteType> = concat_!(ident("first_name"), " ", ident("last_name"));
    assert_eq!(c.expr().preview(), "\"first_name\" || ' ' || \"last_name\"");
}

// ── ident ───────────────────────────────────────────────────────────

#[test]
fn test_ident_simple() {
    let e: vantage_expressions::Expression<AnySqliteType> = ident("price").expr();
    assert_eq!(e.preview(), "\"price\"");
}

#[test]
fn test_ident_qualified() {
    let e: vantage_expressions::Expression<AnySqliteType> = ident("name").dot_of("u").expr();
    assert_eq!(e.preview(), "\"u\".\"name\"");
}

#[test]
fn test_ident_alias() {
    let e: vantage_expressions::Expression<AnySqliteType> = ident("total").with_alias("t").expr();
    assert_eq!(e.preview(), "\"total\" AS \"t\"");
}

// ── or_ / and_ ─────────────────────────────────────────────────────

#[test]
fn test_or_with_ident() {
    let expr = or_(ident("role").eq("admin"), ident("role").eq("superuser"));
    assert_eq!(
        expr.preview(),
        "(\"role\" = 'admin') OR (\"role\" = 'superuser')"
    );
}

#[test]
fn test_or_with_typed_columns() {
    let price = Column::<i64>::new("price");
    let featured = Column::<bool>::new("featured");
    let expr = or_(price.gt(100i64), featured.eq(true));
    assert_eq!(expr.preview(), "(price > 100) OR (featured = 1)");
}

#[test]
fn test_nested_and_inside_or() {
    let price = Column::<i64>::new("price");
    let in_stock = Column::<bool>::new("in_stock");
    let featured = Column::<bool>::new("featured");
    let expr = or_(and_(price.gt(100i64), in_stock.eq(true)), featured.eq(true));
    assert_eq!(
        expr.preview(),
        "((price > 100) AND (in_stock = 1)) OR (featured = 1)"
    );
}
