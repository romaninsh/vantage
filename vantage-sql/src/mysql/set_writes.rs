//! Set-confined write steps for MySQL: existence outside the set, and the
//! fits-the-set probe. The by-id writes in `table_source.rs` use them to meet
//! `TableSource`'s write contract.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_expressions::{Expression, traits::datasource::ExprDataSource};
use vantage_table::table::Table;
use vantage_types::{Entity, Record};

use super::table_source::id_value;
use crate::mysql::MysqlDB;
use crate::mysql::types::AnyMysqlType;
use crate::primitives::identifier::ident;

impl MysqlDB {
    /// Whether any row of the table has `id`, ignoring its conditions.
    pub(crate) async fn id_exists_anywhere<E: Entity<AnyMysqlType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
    ) -> Result<bool> {
        let id_field = table.id_field_name();
        let id_val = id_value(id);
        let probe = mysql_expr!(
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
    pub(crate) async fn row_in_set<E: Entity<AnyMysqlType>>(
        &self,
        table: &Table<Self, E>,
        row: &Record<AnyMysqlType>,
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
        let fields: Vec<Expression<AnyMysqlType>> = names
            .iter()
            .map(|name| {
                let value = row
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| AnyMysqlType::untyped(CborValue::Null));
                mysql_expr!("{} AS {}", value, (ident(name)))
            })
            .collect();
        let conditions: Vec<Expression<AnyMysqlType>> = table
            .conditions()
            .map(|c| mysql_expr!("({})", (c.clone().into_expr())))
            .collect();
        let id_field = table.id_field_name();
        let probe = mysql_expr!(
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
fn has_rows(result: AnyMysqlType) -> bool {
    matches!(result.into_value(), CborValue::Array(rows) if !rows.is_empty())
}
