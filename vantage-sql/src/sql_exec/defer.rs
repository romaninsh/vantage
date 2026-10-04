//! `defer` for the dialects whose deferred query answers with one value.

use ciborium::Value as CborValue;
use vantage_expressions::traits::expressive::DeferredFn;
use vantage_expressions::{ExprDataSource, Expression};

use super::SqlDialect;

/// `expr` run on `db` when the deferred value is needed, answering with the
/// first column of the first row (untyped), or with every row when there is
/// no such cell.
pub(crate) fn defer_first_cell<D>(db: &D, expr: Expression<D::Value>) -> DeferredFn<D::Value>
where
    D: SqlDialect + ExprDataSource<D::Value> + Clone + Send + Sync + 'static,
{
    let db = db.clone();
    DeferredFn::from_fn(move || {
        let db = db.clone();
        let expr = expr.clone();
        Box::pin(async move { Ok(first_cell::<D>(db.execute(&expr).await?)) })
    })
}

/// The first column of the first row of a row array (untyped), or the rows
/// unchanged when there is no such cell.
pub(crate) fn first_cell<D: SqlDialect>(rows: D::Value) -> D::Value {
    match D::into_cbor(rows) {
        CborValue::Array(arr) => {
            let cell = match arr.first() {
                Some(CborValue::Map(map)) => map.first().map(|(_, v)| v.clone()),
                _ => None,
            };
            match cell {
                Some(v) => D::untyped(v),
                None => D::from_cbor_rows(CborValue::Array(arr)),
            }
        }
        other => D::from_cbor_rows(other),
    }
}

#[cfg(all(test, feature = "postgres"))]
mod tests {
    use super::*;
    use crate::postgres::PostgresDB;

    fn rows(rows: Vec<Vec<(&str, i64)>>) -> CborValue {
        CborValue::Array(
            rows.into_iter()
                .map(|row| {
                    CborValue::Map(
                        row.into_iter()
                            .map(|(k, v)| (CborValue::Text(k.into()), CborValue::Integer(v.into())))
                            .collect(),
                    )
                })
                .collect(),
        )
    }

    #[test]
    fn first_cell_of_first_row_or_every_row() {
        let cell = |r| first_cell::<PostgresDB>(PostgresDB::from_cbor_rows(r)).into_value();
        let two = rows(vec![vec![("a", 1), ("b", 2)], vec![("a", 3)]]);
        assert_eq!(cell(two), CborValue::Integer(1.into()));
        assert_eq!(cell(rows(vec![])), rows(vec![]));
        assert_eq!(cell(rows(vec![vec![]])), rows(vec![vec![]]));
    }
}
