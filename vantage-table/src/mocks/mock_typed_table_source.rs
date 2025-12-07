//! Simple typed table source for testing with TypeColumn
//!
//! Provides a minimal table source that uses TypeColumn instead of MockColumn.

#![allow(refining_impl_trait_reachable)]

use async_trait::async_trait;
use vantage_dataset::traits::Result;
use vantage_expressions::traits::datasource::DataSource;
use vantage_types::{Entity, Record};

use crate::column::column::ColumnType;
use crate::mocks::type_column::TypeColumn;
use crate::{
    column::flags::ColumnFlag,
    table::Table,
    traits::{column_like::ColumnLike, table_like::TableLike, table_source::TableSource},
};
use indexmap::IndexMap;
use std::collections::HashSet;
use vantage_expressions::{Expression, traits::expressive::ExpressiveEnum};

/// Simplified type-erased column for TypedTableSource supporting only String, i64, bool
#[derive(Clone, Debug)]
pub struct TypedAnyColumn {
    name: String,
    alias: Option<String>,
    flags: HashSet<ColumnFlag>,
    column_type: TypedColumnType,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypedColumnType {
    String,
    Integer,
    Boolean,
}

impl TypedAnyColumn {
    pub fn new_string(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alias: None,
            flags: HashSet::new(),
            column_type: TypedColumnType::String,
        }
    }

    pub fn new_integer(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alias: None,
            flags: HashSet::new(),
            column_type: TypedColumnType::Integer,
        }
    }

    pub fn new_boolean(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alias: None,
            flags: HashSet::new(),
            column_type: TypedColumnType::Boolean,
        }
    }

    pub fn from_typed<T: ColumnType>(column: TypeColumn<T>) -> Self {
        // Runtime check to ensure T implements TypeColumnType
        Self::check_supported_type::<T>();

        let name = column.name().to_string();
        let alias = column.alias().map(|s| s.to_string());
        let flags = column.flags();

        let column_type = if std::any::TypeId::of::<T>() == std::any::TypeId::of::<String>() {
            TypedColumnType::String
        } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<i64>() {
            TypedColumnType::Integer
        } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<bool>() {
            TypedColumnType::Boolean
        } else {
            panic!("Unsupported type for TypedAnyColumn")
        };

        Self {
            name,
            alias,
            flags,
            column_type,
        }
    }

    pub fn to_typed<T: ColumnType>(&self) -> Option<TypeColumn<T>> {
        let expected_type = if std::any::TypeId::of::<T>() == std::any::TypeId::of::<String>() {
            TypedColumnType::String
        } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<i64>() {
            TypedColumnType::Integer
        } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<bool>() {
            TypedColumnType::Boolean
        } else {
            return None;
        };

        if self.column_type == expected_type {
            Some(TypeColumn::new(&self.name))
        } else {
            None
        }
    }
}

impl ColumnLike for TypedAnyColumn {
    fn name(&self) -> &str {
        &self.name
    }

    fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    fn flags(&self) -> HashSet<ColumnFlag> {
        self.flags.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        Box::new(self)
    }

    fn get_type(&self) -> &'static str {
        match self.column_type {
            TypedColumnType::String => "string",
            TypedColumnType::Integer => "integer",
            TypedColumnType::Boolean => "boolean",
        }
    }
}

/// Simple typed table source for testing
#[derive(Clone, Default)]
pub struct MockTypedTableSource;

impl MockTypedTableSource {
    pub fn new() -> Self {
        Self
    }
}

impl DataSource for MockTypedTableSource {}

#[async_trait]
impl TableSource for MockTypedTableSource {
    type Column<Type>
        = TypeColumn<Type>
    where
        Type: ColumnType;
    type AnyColumn = TypedAnyColumn;
    type Value = serde_json::Value;
    type Id = String;

    fn create_column<Type>(&self, name: &str) -> Self::Column<Type>
    where
        Type: ColumnType,
    {
        TypeColumn::new(name)
    }

    fn to_any_column<Type: ColumnType>(&self, column: Self::Column<Type>) -> Self::AnyColumn {
        TypedAnyColumn::from_typed(column)
    }

    fn from_any_column<Type: ColumnType>(
        &self,
        any_column: &Self::AnyColumn,
    ) -> Option<Self::Column<Type>> {
        any_column.to_typed::<Type>()
    }

    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<vantage_expressions::traits::expressive::ExpressiveEnum<Self::Value>>,
    ) -> vantage_expressions::Expression<Self::Value> {
        vantage_expressions::Expression::new(template, parameters)
    }

    fn search_expression(
        &self,
        table: &impl TableLike,
        search_value: &str,
    ) -> Expression<Self::Value> {
        // Simple mock search
        let columns = table.columns();
        if columns.contains_key("name") {
            Expression::new(
                "name LIKE '%{}%'",
                vec![ExpressiveEnum::Scalar(search_value.into())],
            )
        } else {
            panic!("Mock typed table source can only search column `name`")
        }
    }

    // Minimal implementations - just return empty/default values for testing
    async fn list_table_values<E>(
        &self,
        _table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Ok(IndexMap::new())
    }

    async fn get_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Ok(Record::new())
    }

    async fn get_table_some_value<E>(
        &self,
        _table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Ok(None)
    }

    async fn get_count<E>(&self, _table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Ok(0)
    }

    async fn get_sum<E, Type>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Type>,
    ) -> Result<Type>
    where
        E: Entity<Self::Value>,
        Type: ColumnType,
        Self: Sized,
    {
        // Mock implementation - return default value
        use std::mem;
        let result: Type = unsafe { mem::zeroed() };
        Ok(result)
    }

    async fn insert_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Ok(record.clone())
    }

    async fn replace_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Ok(record.clone())
    }

    async fn patch_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Ok(partial.clone())
    }

    async fn delete_table_value<E>(&self, _table: &Table<Self, E>, _id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Ok(())
    }

    async fn delete_table_all_values<E>(&self, _table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Ok(())
    }

    async fn insert_table_return_id_value<E>(
        &self,
        _table: &Table<Self, E>,
        _record: &Record<Self::Value>,
    ) -> Result<Self::Id>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Ok("mock-id".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Table;
    use vantage_types::EmptyEntity;

    #[test]
    fn test_typed_any_column_conversions() {
        let ds = MockTypedTableSource::new();

        // Test String column conversion
        let string_col = ds.create_column::<String>("name");
        let any_col = ds.to_any_column(string_col);
        assert_eq!(any_col.name(), "name");
        assert_eq!(any_col.get_type(), "string");

        let back_to_string = ds.from_any_column::<String>(&any_col).unwrap();
        assert_eq!(back_to_string.name(), "name");

        // Test i64 column conversion
        let int_col = ds.create_column::<i64>("age");
        let any_int_col = ds.to_any_column(int_col);
        assert_eq!(any_int_col.get_type(), "integer");

        let back_to_int = ds.from_any_column::<i64>(&any_int_col).unwrap();
        assert_eq!(back_to_int.name(), "age");

        // Test bool column conversion
        let bool_col = ds.create_column::<bool>("active");
        let any_bool_col = ds.to_any_column(bool_col);
        assert_eq!(any_bool_col.get_type(), "boolean");

        let back_to_bool = ds.from_any_column::<bool>(&any_bool_col).unwrap();
        assert_eq!(back_to_bool.name(), "active");
    }

    #[test]
    fn test_table_with_typed_columns() {
        let ds = MockTypedTableSource::new();
        let table = Table::<MockTypedTableSource, EmptyEntity>::new("test", ds)
            .with_column_of::<String>("name")
            .with_column_of::<i64>("age")
            .with_column_of::<bool>("active");

        assert!(table.columns().contains_key("name"));
        assert!(table.columns().contains_key("age"));
        assert!(table.columns().contains_key("active"));
        assert_eq!(table.columns().len(), 3);

        // Test retrieving typed columns back
        let name_col = table.get_column::<String>("name").unwrap();
        assert_eq!(name_col.name(), "name");

        let age_col = table.get_column::<i64>("age").unwrap();
        assert_eq!(age_col.name(), "age");

        let active_col = table.get_column::<bool>("active").unwrap();
        assert_eq!(active_col.name(), "active");
    }
}
