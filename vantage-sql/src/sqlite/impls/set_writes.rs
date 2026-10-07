//! SQLite's pieces of the set-confined writes in [`crate::set_writes`].

use ciborium::Value as CborValue;
use vantage_expressions::{Expression, Expressive};
use vantage_table::table::Table;
use vantage_types::Entity;

use crate::condition::SqliteCondition;
use crate::primitives::identifier::ident;
use crate::set_writes::SetWrites;
use crate::sqlite::SqliteDB;
use crate::sqlite::types::AnySqliteType;

impl SetWrites for SqliteDB {
    fn id_param<E: Entity<AnySqliteType>>(_table: &Table<Self, E>, id: &str) -> AnySqliteType {
        AnySqliteType::from(id.to_string())
    }

    /// An integer for a numeric id on a table not flagged `with_text_id`.
    fn id_cell<E: Entity<AnySqliteType>>(table: &Table<Self, E>, id: &str) -> AnySqliteType {
        match id.parse::<i64>() {
            Ok(n) if !table.id_is_text() => AnySqliteType::new(n),
            _ => AnySqliteType::from(id.to_string()),
        }
    }

    fn null() -> AnySqliteType {
        AnySqliteType::untyped(CborValue::Null)
    }

    fn quoted(name: &str) -> Expression<AnySqliteType> {
        ident(name).expr()
    }

    fn condition_expr(condition: &SqliteCondition) -> Expression<AnySqliteType> {
        condition.clone().into_expr()
    }

    fn into_cbor(value: AnySqliteType) -> CborValue {
        value.into_value()
    }
}
