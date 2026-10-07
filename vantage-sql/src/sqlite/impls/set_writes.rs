//! Set-confined write steps for SQLite: existence outside the set, and the
//! fits-the-set probe. The by-id writes in `table_source.rs` use them to meet
//! `TableSource`'s write contract.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_expressions::{Expression, traits::datasource::ExprDataSource};
use vantage_table::table::Table;
use vantage_types::{Entity, Record};

use crate::primitives::identifier::ident;
use crate::sqlite::SqliteDB;
use crate::sqlite::types::AnySqliteType;

impl SqliteDB {
    /// Whether any row of the table has `id`, ignoring its conditions.
    pub(crate) async fn id_exists_anywhere<E: Entity<AnySqliteType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
    ) -> Result<bool> {
        let id_field = table.id_field_name();
        let id_val = id.to_string();
        let probe = sqlite_expr!(
            "SELECT 1 AS {} FROM {} WHERE {} = {} LIMIT 1",
            (ident(&id_field)),
            (ident(table.table_name())),
            (ident(&id_field)),
            id_val
        );
        Ok(has_rows(self.execute(&probe).await?))
    }

    /// Whether `row` — the record as it would be stored, id included —
    /// satisfies every condition of `table`. The database evaluates the
    /// conditions over a one-row derived table aliased as the table itself, so
    /// plain, table-qualified and subquery conditions read as they do on the
    /// real rows. A table column the record leaves out reads as NULL.
    pub(crate) async fn row_in_set<E: Entity<AnySqliteType>>(
        &self,
        table: &Table<Self, E>,
        row: &Record<AnySqliteType>,
    ) -> Result<bool> {
        if table.conditions().next().is_none() {
            return Ok(true);
        }
        let mut names: Vec<String> = table
            .columns()
            .keys()
            .filter(|n| !table.is_calculated_column(n))
            .cloned()
            .collect();
        for key in row.keys() {
            if !names.contains(key) {
                names.push(key.clone());
            }
        }
        let fields: Vec<Expression<AnySqliteType>> = names
            .iter()
            .map(|name| {
                let value = row
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| AnySqliteType::untyped(CborValue::Null));
                sqlite_expr!("{} AS {}", value, (ident(name)))
            })
            .collect();
        let conditions: Vec<Expression<AnySqliteType>> = table
            .conditions()
            .map(|c| sqlite_expr!("({})", (c.clone().into_expr())))
            .collect();
        let id_field = table.id_field_name();
        let probe = sqlite_expr!(
            "SELECT 1 AS {} FROM (SELECT {}) AS {} WHERE {}",
            (ident(&id_field)),
            (Expression::from_vec(fields, ", ")),
            (ident(table.table_name())),
            (Expression::from_vec(conditions, " AND "))
        );
        Ok(has_rows(self.execute(&probe).await?))
    }
}

/// Whether a query result holds at least one row.
fn has_rows(result: AnySqliteType) -> bool {
    matches!(result.into_value(), CborValue::Array(rows) if !rows.is_empty())
}

/// The id as the table stores it: an integer for a numeric id on a table not
/// flagged `with_text_id`, so `id = 5` conditions compare like for like.
pub(crate) fn id_cell<E: Entity<AnySqliteType>>(
    table: &Table<SqliteDB, E>,
    id: &str,
) -> AnySqliteType {
    match id.parse::<i64>() {
        Ok(n) if !table.id_is_text() => AnySqliteType::new(n),
        _ => AnySqliteType::from(id.to_string()),
    }
}
