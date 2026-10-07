//! Set-confined write steps for SurrealDB: existence outside the set, and the
//! fits-the-set probe. The by-id writes in `table_source.rs` use them to meet
//! `TableSource`'s write contract.

use ciborium::Value as CborValue;
use vantage_core::{Result, VantageError, error};
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive};
use vantage_table::table::Table;
use vantage_types::{Entity, Record};

use crate::surrealdb::SurrealDB;
use crate::thing::Thing;
use crate::types::{AnySurrealType, SurrealType};

impl SurrealDB {
    /// Whether a record with `id` exists, ignoring any table conditions.
    pub async fn id_exists_anywhere(&self, id: &Thing) -> Result<bool> {
        let probe = Expression::new("SELECT id FROM {}", vec![ExpressiveEnum::Nested(id.expr())]);
        match self.execute(&probe).await {
            Ok(result) => Ok(has_rows(result)),
            Err(e) if is_missing_table(&e) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Whether `row` — the record as it would be stored, id included —
    /// satisfies every condition of `table`. The database evaluates the
    /// conditions over a one-element array holding the record, so they read
    /// as they do on real rows.
    pub async fn row_in_set<E: Entity<AnySurrealType>>(
        &self,
        table: &Table<Self, E>,
        row: &Record<AnySurrealType>,
    ) -> Result<bool> {
        if table.conditions().next().is_none() {
            return Ok(true);
        }
        let map: Vec<(CborValue, CborValue)> = row
            .iter()
            .map(|(k, v)| (CborValue::Text(k.clone()), v.value().clone()))
            .collect();
        let conditions: Vec<Expression<AnySurrealType>> = table
            .conditions()
            .map(|c| Expression::new("({})", vec![ExpressiveEnum::Nested(c.clone())]))
            .collect();
        let probe = Expression::new(
            "SELECT * FROM [{}] WHERE {}",
            vec![
                ExpressiveEnum::Scalar(AnySurrealType::from(CborValue::Map(map))),
                ExpressiveEnum::Nested(Expression::from_vec(conditions, " AND ")),
            ],
        );
        Ok(has_rows(self.execute(&probe).await?))
    }
}

/// Whether a query failed because its table was never defined. Such a table
/// holds no rows; a first write defines it implicitly.
pub(crate) fn is_missing_table(err: &VantageError) -> bool {
    err.context
        .get("query_error")
        .is_some_and(|m| m.contains("The table") && m.contains("does not exist"))
}

/// Whether a query result holds at least one row.
fn has_rows(result: AnySurrealType) -> bool {
    matches!(result.into_value(), CborValue::Array(rows) if !rows.is_empty())
}

/// The record as the table would store it: `record` with its id field set to `id`.
pub(crate) fn row_with_id<E: Entity<AnySurrealType>>(
    table: &Table<SurrealDB, E>,
    id: &Thing,
    record: &Record<AnySurrealType>,
) -> Record<AnySurrealType> {
    let mut row = record.clone();
    row.insert(table.id_field_name(), AnySurrealType::from(id.to_cbor()));
    row
}

/// Insert or replace of an id that a record outside the table's conditions holds.
pub(crate) fn held_outside(table_name: &str, id: &Thing) -> VantageError {
    error!(
        "id is held by a record outside this set",
        table = table_name,
        id = id.clone()
    )
    .mark_conflict()
}

/// A written record that would not satisfy the table's conditions.
pub(crate) fn not_in_set(table_name: &str, id: Option<&Thing>) -> VantageError {
    match id {
        Some(id) => error!(
            "record does not belong to this set",
            table = table_name,
            id = id.clone()
        ),
        None => error!("record does not belong to this set", table = table_name),
    }
    .mark_conflict()
}

/// A patch whose merged record would no longer satisfy the table's conditions.
pub(crate) fn patch_leaves_set(table_name: &str, id: &Thing) -> VantageError {
    error!(
        "patch would move the record out of this set",
        table = table_name,
        id = id.clone()
    )
    .mark_conflict()
}
