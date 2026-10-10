//! The multi-row `INSERT` statement.

use vantage_expressions::{Expression, Expressive, expr_any};

use super::Prepared;
use crate::mysql::types::AnyMysqlType;
use crate::primitives::identifier::ident;

/// Every column any of `rows` sets, in first-seen order.
pub(super) fn columns(rows: &[Prepared]) -> Vec<&str> {
    let mut columns: Vec<&str> = Vec::new();
    for item in rows {
        for key in item.row.keys() {
            if !columns.contains(&key.as_str()) {
                columns.push(key);
            }
        }
    }
    columns
}

/// One `INSERT` for `rows` over [`columns`]; a row that leaves a column out
/// writes `DEFAULT` there, as a single insert that omits the column would.
pub(super) fn insert(table: &str, rows: &[Prepared]) -> Expression<AnyMysqlType> {
    let columns = columns(rows);
    let names: Vec<Expression<AnyMysqlType>> = columns.iter().map(|c| ident(*c).expr()).collect();
    let tuples: Vec<Expression<AnyMysqlType>> = rows
        .iter()
        .map(|item| {
            let cells: Vec<Expression<AnyMysqlType>> = columns
                .iter()
                .map(|c| match item.row.get(*c) {
                    Some(value) => {
                        let value = value.clone();
                        expr_any!("{}", value)
                    }
                    None => Expression::new("DEFAULT", vec![]),
                })
                .collect();
            expr_any!("({})", (Expression::from_vec(cells, ", ")))
        })
        .collect();
    expr_any!(
        "INSERT INTO {} ({}) VALUES {}",
        (ident(table)),
        (Expression::from_vec(names, ", ")),
        (Expression::from_vec(tuples, ", "))
    )
}
