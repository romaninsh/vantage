//! MySQL's pieces of the set-confined writes in [`crate::set_writes`].

use ciborium::Value as CborValue;
use vantage_expressions::{Expression, Expressive};
use vantage_table::table::Table;
use vantage_types::Entity;

use super::table_source::id_value;
use crate::condition::MysqlCondition;
use crate::mysql::MysqlDB;
use crate::mysql::types::AnyMysqlType;
use crate::primitives::identifier::ident;
use crate::set_writes::SetWrites;

impl SetWrites for MysqlDB {
    fn id_param<E: Entity<AnyMysqlType>>(_table: &Table<Self, E>, id: &str) -> AnyMysqlType {
        id_value(id)
    }

    fn id_cell<E: Entity<AnyMysqlType>>(_table: &Table<Self, E>, id: &str) -> AnyMysqlType {
        id_value(id)
    }

    fn null() -> AnyMysqlType {
        AnyMysqlType::untyped(CborValue::Null)
    }

    fn quoted(name: &str) -> Expression<AnyMysqlType> {
        ident(name).expr()
    }

    fn condition_expr(condition: &MysqlCondition) -> Expression<AnyMysqlType> {
        condition.clone().into_expr()
    }

    fn into_cbor(value: AnyMysqlType) -> CborValue {
        value.into_value()
    }
}
