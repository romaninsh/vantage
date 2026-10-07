//! PostgreSQL's pieces of the set-confined writes in [`crate::set_writes`].

use ciborium::Value as CborValue;
use vantage_expressions::{Expression, Expressive};
use vantage_table::table::Table;
use vantage_types::Entity;

use super::table_source::id_param;
use crate::condition::PostgresCondition;
use crate::postgres::PostgresDB;
use crate::postgres::types::AnyPostgresType;
use crate::primitives::identifier::ident;
use crate::set_writes::SetWrites;

impl SetWrites for PostgresDB {
    fn id_param<E: Entity<AnyPostgresType>>(table: &Table<Self, E>, id: &str) -> AnyPostgresType {
        id_param(table, id)
    }

    fn id_cell<E: Entity<AnyPostgresType>>(table: &Table<Self, E>, id: &str) -> AnyPostgresType {
        id_param(table, id)
    }

    /// The probe's derived columns take the types of the bound values, not
    /// of the table: an absent column is a text NULL, so an operator
    /// condition on a non-text column the record leaves out fails the probe
    /// with a type error rather than reading as "not in the set".
    fn null() -> AnyPostgresType {
        AnyPostgresType::untyped(CborValue::Null)
    }

    fn quoted(name: &str) -> Expression<AnyPostgresType> {
        ident(name).expr()
    }

    fn condition_expr(condition: &PostgresCondition) -> Expression<AnyPostgresType> {
        condition.clone().into_expr()
    }

    fn into_cbor(value: AnyPostgresType) -> CborValue {
        value.into_value()
    }
}
