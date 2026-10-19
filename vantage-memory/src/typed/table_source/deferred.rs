//! Conditions and expressions that read another table's rows when they are
//! evaluated, not when they are built.

use ciborium::Value as CborValue;
use vantage_expressions::{DeferredFn, ExprDataSource, Expression, ExpressiveEnum, expr_any};
use vantage_table::table::Table;
use vantage_types::Entity;

use crate::eval::MemoryCondition;
use crate::typed::MemoryDB;
use crate::typed::convert::{column_values, with_id_forms};
use crate::types::AnyMemoryType;

impl MemoryDB {
    /// `target_field` in the `source_column` cells of `source_table`'s rows,
    /// each id-like cell widened to every id form.
    pub(super) fn related_values<SourceE: Entity<AnyMemoryType> + 'static>(
        &self,
        target_field: &str,
        source_table: &Table<Self, SourceE>,
        source_column: &str,
    ) -> MemoryCondition {
        let db = self.clone();
        let source = source_table.clone();
        let col = source_column.to_string();
        let target = target_field.to_string();
        let deferred = DeferredFn::from_fn(move || {
            let (db, source, col, target) =
                (db.clone(), source.clone(), col.clone(), target.clone());
            async move {
                let rows = db.rows(&source, true).await?;
                let id_column = db.id_column(&source);
                let values = with_id_forms(column_values(&rows, &col, &id_column));
                let payload =
                    CborValue::Array(vec![CborValue::Text(target), CborValue::Array(values)]);
                Ok(AnyMemoryType::untyped(payload))
            }
        });
        MemoryCondition::DeferredIn {
            path: target_field.to_string(),
            deferred,
        }
    }

    /// The values of column `col` across `table`'s rows, as one array.
    pub(super) fn column_values_expr<E: Entity<AnyMemoryType> + 'static>(
        &self,
        table: &Table<Self, E>,
        col: String,
    ) -> Expression<AnyMemoryType> {
        let db = self.clone();
        let table = table.clone();
        let inner = expr_any!("{}", {
            DeferredFn::new(move || {
                let (db, table, col) = (db.clone(), table.clone(), col.clone());
                Box::pin(async move {
                    let rows = db.rows(&table, true).await?;
                    let id_column = db.id_column(&table);
                    let values = column_values(&rows, &col, &id_column);
                    Ok(ExpressiveEnum::Scalar(AnyMemoryType::untyped(
                        CborValue::Array(values),
                    )))
                })
            })
        });
        expr_any!("{}", { self.defer(inner) })
    }
}
