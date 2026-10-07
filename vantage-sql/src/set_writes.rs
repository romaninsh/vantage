//! The two probes behind set-confined writes, shared by the SQL dialects:
//! whether an id exists anywhere in the table, and whether a row would
//! satisfy the table's conditions. A dialect supplies only how it binds ids
//! and NULL, and how it quotes identifiers — see [`SetWrites`].

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::{Expression, expr_any};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

/// The dialect-specific pieces of the set-confined write paths.
pub(crate) trait SetWrites:
    TableSource<Id = String> + ExprDataSource<<Self as TableSource>::Value>
{
    /// The id as bound in a `WHERE <id> = …` lookup.
    fn id_param<E: Entity<Self::Value>>(table: &Table<Self, E>, id: &str) -> Self::Value;

    /// The id as the table stores it — the cell the fits-the-set probe puts
    /// on the row, so `id = 5` conditions compare like for like.
    fn id_cell<E: Entity<Self::Value>>(table: &Table<Self, E>, id: &str) -> Self::Value;

    /// The cell a column the record leaves out reads as in the probe.
    fn null() -> Self::Value;

    /// `name` quoted as an identifier.
    fn quoted(name: &str) -> Expression<Self::Value>;

    fn condition_expr(condition: &Self::Condition) -> Expression<Self::Value>;

    fn into_cbor(value: Self::Value) -> CborValue;
}

/// Whether any row of the table has `id`, ignoring its conditions.
pub(crate) async fn id_exists_anywhere<T, E>(db: &T, table: &Table<T, E>, id: &str) -> Result<bool>
where
    T: SetWrites,
    E: Entity<T::Value>,
{
    let id_field = T::quoted(&table.id_field_name());
    let id_val = T::id_param(table, id);
    let probe: Expression<T::Value> = expr_any!(
        "SELECT 1 AS {} FROM {} WHERE {} = {} LIMIT 1",
        (id_field),
        (T::quoted(table.table_name())),
        (id_field),
        id_val
    );
    Ok(has_rows::<T>(db.execute(&probe).await?))
}

/// Whether `row` — the record as it would be stored, id included —
/// satisfies every condition of `table`. The database evaluates the
/// conditions over a one-row derived table aliased as the table itself, so
/// plain, table-qualified and subquery conditions read as they do on the
/// real rows. A table column the record leaves out reads as
/// [`SetWrites::null`].
pub(crate) async fn row_in_set<T, E>(
    db: &T,
    table: &Table<T, E>,
    row: &Record<T::Value>,
) -> Result<bool>
where
    T: SetWrites,
    E: Entity<T::Value>,
{
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
    let fields: Vec<Expression<T::Value>> = names
        .iter()
        .map(|name| {
            let value = row.get(name).cloned().unwrap_or_else(T::null);
            expr_any!("{} AS {}", value, (T::quoted(name)))
        })
        .collect();
    let conditions: Vec<Expression<T::Value>> = table
        .conditions()
        .map(|c| expr_any!("({})", (T::condition_expr(c))))
        .collect();
    let probe: Expression<T::Value> = expr_any!(
        "SELECT 1 AS {} FROM (SELECT {}) AS {} WHERE {}",
        (T::quoted(&table.id_field_name())),
        (Expression::from_vec(fields, ", ")),
        (T::quoted(table.table_name())),
        (Expression::from_vec(conditions, " AND "))
    );
    Ok(has_rows::<T>(db.execute(&probe).await?))
}

/// Whether a query result holds at least one row.
fn has_rows<T: SetWrites>(result: T::Value) -> bool {
    matches!(T::into_cbor(result), CborValue::Array(rows) if !rows.is_empty())
}
