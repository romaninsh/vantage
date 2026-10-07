//! PostgreSQL's pieces of the set-confined writes in [`crate::set_writes`].

use ciborium::Value as CborValue;
use vantage_expressions::{Expression, Expressive, expr_any};
use vantage_table::table::Table;
use vantage_types::Entity;

use super::table_source::id_param;
use crate::condition::PostgresCondition;
use crate::postgres::PostgresDB;
use crate::postgres::types::AnyPostgresType;
use crate::primitives::identifier::ident;
use crate::set_writes::SetWrites;

/// The Postgres type for a column's declared Rust type name
/// (`std::any::type_name`, optionally inside `Option`), or `None` where the
/// bound NULL already fits (text) or the type isn't known.
fn postgres_type(rust_type: &str) -> Option<&'static str> {
    let inner = rust_type
        .strip_prefix("core::option::Option<")
        .and_then(|t| t.strip_suffix('>'))
        .unwrap_or(rust_type);
    match inner {
        "bool" => Some("boolean"),
        "i8" | "i16" | "u8" => Some("smallint"),
        "i32" | "u16" => Some("integer"),
        "i64" | "u32" => Some("bigint"),
        "f32" => Some("real"),
        "f64" => Some("double precision"),
        t if t.ends_with("Decimal") => Some("numeric"),
        _ => None,
    }
}

impl SetWrites for PostgresDB {
    fn id_param<E: Entity<AnyPostgresType>>(table: &Table<Self, E>, id: &str) -> AnyPostgresType {
        id_param(table, id)
    }

    fn id_cell<E: Entity<AnyPostgresType>>(table: &Table<Self, E>, id: &str) -> AnyPostgresType {
        id_param(table, id)
    }

    fn null() -> AnyPostgresType {
        AnyPostgresType::untyped(CborValue::Null)
    }

    /// The probe's derived columns take the types of their cells, not of the
    /// table, and a bound NULL is text. Cast an absent column's NULL to the
    /// column's declared type, so an operator condition on it (`price > 10`)
    /// evaluates to NULL — "not in the set" — instead of a type error. An
    /// unknown type keeps the bound NULL.
    fn absent(column_type: Option<&str>) -> Expression<AnyPostgresType> {
        match column_type.and_then(postgres_type) {
            Some(sql_type) => Expression::new(format!("CAST(NULL AS {sql_type})"), vec![]),
            None => {
                let null = Self::null();
                expr_any!("{}", null)
            }
        }
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
