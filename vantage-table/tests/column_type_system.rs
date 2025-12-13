//! This example illustrates the usage of `vantage-type` type system with a custom TableSource
//!
//! I borrow example of Type3 system, which support only 3 types: String, Url (as String variant),
//! and custom Email type. Variants are stored in CBOR format using ciborium::Value, which
//! guarantees that we would never mix up "String" and "Email" types. On other hand -
//! Url and String do not have a type boundary - therefore email stored as string can be
//! loaded as string and vice-versa.
//!
//! Table Column is crucial and it would allow definition of type-specific columns. Columns
//! do not operate with Type variants (String/Email) but rather operate with Rust traits.
//! This means you can change between String/Url for your column definition, it would not
//! affect actual storage type. A use-case for this is introducing more user-friendly
//! types, which are stored as Strings, which works without storage type refactoring.
//!
//! Column types introduce type safety when you use conditions, so a good application
//! interface is needed when types are defined. Here is how we define columns:
//!
//! ```rust
//! let mut table = Table::<Type3TableSource, EmptyEntity>::new("test",
//!    Type3TableSource::new());
//! table.add_column_of::<String>("name");
//! table.add_column_of::<Email>("email");
//! table.add_column_of::<Url>("website");
//! ```
//!

use async_trait::async_trait;
use indexmap::IndexMap;
use std::collections::HashSet;
use std::marker::PhantomData;
use url::Url;
use vantage_dataset::traits::Result;
use vantage_expressions::{
    Expression, traits::datasource::DataSource, traits::expressive::ExpressiveEnum,
};
use vantage_table::column::core::ColumnType;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_table::traits::table_like::TableLike;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record, vantage_type_system};

// Generate Type3 system using the macro
vantage_type_system! {
    type_trait: Type3,
    method_name: cbor,
    value_type: ciborium::Value,
    type_variants: [String, Email]
}

// Macro requires us to define variant detection from value_type
impl Type3Variants {
    pub fn from_cbor(value: &ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Text(_) => Some(Type3Variants::String),
            ciborium::Value::Tag(1000, _) => Some(Type3Variants::Email),
            _ => None,
        }
    }
}

// Create compatibility between standard types and the 2 variants of Type3
impl Type3 for String {
    type Target = Type3StringMarker;
    fn to_cbor(&self) -> ciborium::Value {
        ciborium::Value::Text(self.clone())
    }
    fn from_cbor(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Text(s) => Some(s.clone()),
            _ => None,
        }
    }
}

// Url natively maps to String variant
impl Type3 for Url {
    type Target = Type3StringMarker;
    fn to_cbor(&self) -> ciborium::Value {
        ciborium::Value::Text(self.to_string())
    }
    fn from_cbor(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Text(s) => Url::parse(&s).ok(),
            _ => None,
        }
    }
}

// Custom Email struct, using it's own type variant
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Email {
    pub name: String,
    pub domain: String,
}

impl Email {
    pub fn new(name: &str, domain: &str) -> Self {
        Self {
            name: name.to_string(),
            domain: domain.to_string(),
        }
    }
}

impl Type3 for Email {
    type Target = Type3EmailMarker;
    fn to_cbor(&self) -> ciborium::value::Value {
        // Use custom CBOR tag 1000 with array [name, domain]
        let array = vec![
            ciborium::value::Value::Text(self.name.clone()),
            ciborium::value::Value::Text(self.domain.clone()),
        ];
        ciborium::value::Value::Tag(1000, Box::new(ciborium::value::Value::Array(array)))
    }

    fn from_cbor(cbor: ciborium::value::Value) -> Option<Self> {
        let ciborium::value::Value::Tag(1000, boxed_value) = cbor else {
            return None;
        };
        let ciborium::value::Value::Array(arr) = boxed_value.as_ref() else {
            return None;
        };

        let name = match arr.get(0)? {
            ciborium::value::Value::Text(s) => s,
            _ => return None,
        };

        let domain = match arr.get(1)? {
            ciborium::value::Value::Text(s) => s,
            _ => return None,
        };

        Some(Email::new(name, domain))
    }
}

///////// NOW DEFINE COLUMN //////////////

/// Column that stores Type3 values internally
#[derive(Debug, Clone)]
pub struct Type3Column<T = String>
where
    T: ColumnType,
{
    name: String,
    flags: HashSet<ColumnFlag>,
    _phantom: PhantomData<T>,
}

impl<T: ColumnType> Type3Column<T> {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            flags: HashSet::new(),
            _phantom: PhantomData,
        }
    }

    pub fn with_flags(mut self, flags: &[ColumnFlag]) -> Self {
        self.flags.extend(flags.iter().cloned());
        self
    }
}

impl<T: ColumnType> ColumnLike<T> for Type3Column<T> {
    fn name(&self) -> &str {
        &self.name
    }

    fn flags(&self) -> HashSet<ColumnFlag> {
        self.flags.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

///////// TABLE SOURCE /////////////

/// Simple table source operating with CBOR
#[derive(Clone)]
pub struct Type3TableSource {
    data: Vec<IndexMap<String, ciborium::Value>>,
}

impl Type3TableSource {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    pub fn with_data(data: Vec<IndexMap<String, ciborium::Value>>) -> Self {
        Self { data }
    }
}

impl Default for Type3TableSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSource for Type3TableSource {}

#[async_trait]
impl TableSource for Type3TableSource {
    type Column<Type>
        = Type3Column<Type>
    where
        Type: ColumnType;
    type AnyType = AnyType3;
    type Value = ciborium::Value;
    type Id = usize;

    fn create_column<Type: ColumnType>(&self, name: &str) -> Self::Column<Type> {
        // Runtime check for Type3 compatibility
        use std::any::TypeId;
        let type_id = TypeId::of::<Type>();

        if type_id != TypeId::of::<String>()
            && type_id != TypeId::of::<Email>()
            && type_id != TypeId::of::<Url>()
        {
            panic!(
                "Type {:?} is not compatible with Type3 system. Only String, Email, and Url are supported.",
                std::any::type_name::<Type>()
            );
        }

        Type3Column::new(name)
    }

    fn to_any_column<Type: ColumnType>(
        &self,
        column: Self::Column<Type>,
    ) -> Self::Column<Self::AnyType> {
        Type3Column {
            name: column.name,
            flags: column.flags,
            _phantom: PhantomData,
        }
    }

    fn convert_any_column<Type: ColumnType>(
        &self,
        any_column: Self::Column<Self::AnyType>,
    ) -> Option<Self::Column<Type>> {
        Some(Type3Column {
            name: any_column.name.clone(),
            flags: any_column.flags.clone(),
            _phantom: PhantomData,
        })
    }

    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<ExpressiveEnum<Self::Value>>,
    ) -> Expression<Self::Value> {
        Expression::new(template, parameters)
    }

    fn search_expression(
        &self,
        _table: &impl TableLike,
        search_value: &str,
    ) -> Expression<Self::Value> {
        // Simple mock - search in name field if exists
        Expression::new(
            "name CONTAINS {}",
            vec![ExpressiveEnum::Scalar(ciborium::Value::Text(
                search_value.to_string(),
            ))],
        )
    }

    // Implementation using stored data
    async fn list_table_values<E>(
        &self,
        _table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let mut result = IndexMap::new();
        for (i, row) in self.data.iter().enumerate() {
            let record = Record::from_indexmap(row.clone());
            result.insert(i, record);
        }
        Ok(result)
    }

    async fn get_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        if let Some(row) = self.data.get(*id) {
            Ok(Record::from_indexmap(row.clone()))
        } else {
            Err(vantage_core::error!("Record not found", id = id).into())
        }
    }

    async fn get_table_some_value<E>(
        &self,
        _table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        if let Some(row) = self.data.first() {
            let id = 0;
            let record = Record::from_indexmap(row.clone());
            Ok(Some((id, record)))
        } else {
            Ok(None)
        }
    }

    async fn get_count<E>(&self, _table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Ok(self.data.len() as i64)
    }

    async fn get_sum<E, Type: ColumnType>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Type>,
    ) -> Result<Type>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(vantage_core::error!("Sum not implemented for Type3TableSource").into())
    }

    async fn insert_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(vantage_core::error!("Insert operations not supported").into())
    }

    async fn replace_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(vantage_core::error!("Replace operations not supported").into())
    }

    async fn patch_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(vantage_core::error!("Patch operations not supported").into())
    }

    async fn delete_table_value<E>(&self, _table: &Table<Self, E>, _id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(vantage_core::error!("Delete operations not supported").into())
    }

    async fn delete_table_all_values<E>(&self, _table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(vantage_core::error!("Delete operations not supported").into())
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
        Err(vantage_core::error!("Insert operations not supported").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_types::EmptyEntity;

    #[test]
    fn test_column_creation() {
        let ds = Type3TableSource::new();

        let name_col = ds.create_column::<String>("name");
        let email_col = ds.create_column::<Email>("email");
        let url_col = ds.create_column::<Url>("website");

        assert_eq!(name_col.name(), "name");
        assert_eq!(email_col.name(), "email");
        assert_eq!(url_col.name(), "website");
    }

    #[test]
    fn test_table_with_columns() {
        let ds = Type3TableSource::new();
        let mut table = Table::<Type3TableSource, EmptyEntity>::new("test", ds);

        table.add_column_of::<String>("name");
        table.add_column_of::<Email>("email");
        table.add_column_of::<Url>("website");

        assert!(table.columns().contains_key("name"));
        assert!(table.columns().contains_key("email"));
        assert!(table.columns().contains_key("website"));
        assert_eq!(table.columns().len(), 3);
    }

    #[test]
    fn test_column_conversion() {
        let ds = Type3TableSource::new();

        let typed_col = ds.create_column::<String>("name");
        let any_col = ds.to_any_column(typed_col);
        let back_to_typed = ds.convert_any_column::<String>(any_col.clone()).unwrap();

        assert_eq!(any_col.name(), "name");
        assert_eq!(back_to_typed.name(), "name");
    }

    #[test]
    fn test_type3_values() {
        let name = String::from("test");
        let email = Email::new("user", "example.com");
        let website = Url::parse("https://example.com").unwrap();

        let name_any = AnyType3::new(name);
        let email_any = AnyType3::new(email);
        let website_any = AnyType3::new(website);

        assert_eq!(name_any.type_variant(), Some(Type3Variants::String));
        assert_eq!(email_any.type_variant(), Some(Type3Variants::Email));
        assert_eq!(website_any.type_variant(), Some(Type3Variants::String));
    }

    #[test]
    fn test_incompatible_type_usage() {
        let ds = Type3TableSource::new();
        let mut table = Table::<Type3TableSource, EmptyEntity>::new("test", ds);

        // These should work fine - compatible types
        table.add_column_of::<String>("name");
        table.add_column_of::<Email>("email");
        table.add_column_of::<Url>("website");

        // This should panic at runtime because i32 is not Type3-compatible:
        let result = std::panic::catch_unwind(|| {
            let mut test_table =
                Table::<Type3TableSource, EmptyEntity>::new("test2", Type3TableSource::new());
            test_table.add_column_of::<i32>("age");
        });
        assert!(result.is_err()); // Should panic

        // Only Type3-compatible types work:
        assert!(table.columns().contains_key("name"));
        assert!(table.columns().contains_key("email"));
        assert!(table.columns().contains_key("website"));

        // The real issue would appear when trying to use Type3 conversion:
        // AnyType3::new(42i32) would fail to compile because i32 doesn't implement Type3

        // Demonstrate that non-Type3 types can't be used with AnyType3:
        let string_val = String::from("test");
        let _any_string = AnyType3::new(string_val); // This works

        let email_val = Email::new("test", "example.com");
        let _any_email = AnyType3::new(email_val); // This works

        // let int_val = 42i32;
        // let _any_int = AnyType3::new(int_val); // This would NOT compile!

        assert_eq!(table.columns().len(), 3); // Only the 3 compatible types
    }
}
