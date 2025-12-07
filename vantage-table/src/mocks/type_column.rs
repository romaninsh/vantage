//! Type-specific column implementation for testing
//!
//! Provides a column implementation that supports String, i64, and bool types.

use crate::column::column::ColumnType;
use crate::column::flags::ColumnFlag;
use crate::traits::column_like::ColumnLike;
use std::collections::HashSet;
use std::marker::PhantomData;

/// Type-specific column implementation supporting String, i64, and bool
#[derive(Debug, Clone)]
pub struct TypeColumn<T = String>
where
    T: ColumnType,
{
    name: String,
    flags: HashSet<ColumnFlag>,
    _phantom: PhantomData<T>,
}

/// Trait to constrain TypeColumn to supported types
pub trait TypeColumnType: ColumnType {}

// Implement TypeColumnType for the allowed types
impl TypeColumnType for String {}
impl TypeColumnType for i64 {}
impl TypeColumnType for bool {}

impl<T: ColumnType> TypeColumn<T> {
    /// Create a new typed column with the given name
    pub fn new(name: impl Into<String>) -> Self {
        // Runtime check to ensure T implements TypeColumnType
        Self::check_supported_type();

        Self {
            name: name.into(),
            flags: HashSet::new(),
            _phantom: PhantomData,
        }
    }

    /// Runtime check that T implements TypeColumnType
    fn check_supported_type() {
        // We can't directly check trait implementation at runtime,
        // but we can check if the type is one of the known implementors
        // by using a whitelist approach through TypeColumnType
        fn check_implements_type_column_type<U: 'static>() -> bool {
            use std::any::TypeId;

            // Check against the types we know implement TypeColumnType
            let target_type = TypeId::of::<U>();

            // We still need to enumerate the types, but at least it's cleaner
            // and we're checking against the actual trait implementations
            matches!(target_type,
                t if t == TypeId::of::<String>() ||
                     t == TypeId::of::<i64>() ||
                     t == TypeId::of::<bool>()
            )
        }

        if !check_implements_type_column_type::<T>() {
            panic!(
                "TypeColumn only supports types that implement TypeColumnType (String, i64, bool). Found: {}",
                std::any::type_name::<T>()
            );
        }
    }

    /// Add flags to this column (builder pattern)
    pub fn with_flags(mut self, flags: &[ColumnFlag]) -> Self {
        self.flags.extend(flags.iter().cloned());
        self
    }

    /// Add a single flag to this column (builder pattern)
    pub fn with_flag(mut self, flag: ColumnFlag) -> Self {
        self.flags.insert(flag);
        self
    }

    /// Get the type name for this column
    pub fn type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }
}

impl<T: ColumnType> ColumnLike<T> for TypeColumn<T> {
    fn name(&self) -> &str {
        &self.name
    }

    fn alias(&self) -> Option<&str> {
        None
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

    fn get_type(&self) -> &'static str {
        match std::any::type_name::<T>() {
            "alloc::string::String" | "&str" => "string",
            "i64" => "integer",
            "bool" => "boolean",
            _ => "unknown",
        }
    }
}

// Convenience constructors for specific types
impl TypeColumn<String> {
    /// Create a string column
    pub fn string(name: impl Into<String>) -> Self {
        Self::new(name)
    }
}

impl TypeColumn<i64> {
    /// Create an integer column
    pub fn integer(name: impl Into<String>) -> Self {
        Self::new(name)
    }
}

impl TypeColumn<bool> {
    /// Create a boolean column
    pub fn boolean(name: impl Into<String>) -> Self {
        Self::new(name)
    }
}

// From implementations for convenience
impl From<&str> for TypeColumn<String> {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl From<String> for TypeColumn<String> {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_column_string_default() {
        let col = TypeColumn::string("name");
        assert_eq!(col.name(), "name");
        assert_eq!(col.alias(), None);
        assert_eq!(col.get_type(), "string");
    }

    #[test]
    fn test_type_column_explicit_types() {
        let string_col = TypeColumn::<String>::new("name");
        let int_col = TypeColumn::<i64>::new("age");
        let bool_col = TypeColumn::<bool>::new("active");

        assert_eq!(string_col.get_type(), "string");
        assert_eq!(int_col.get_type(), "integer");
        assert_eq!(bool_col.get_type(), "boolean");
    }

    #[test]
    fn test_convenience_constructors() {
        let string_col = TypeColumn::string("name");
        let int_col = TypeColumn::integer("age");
        let bool_col = TypeColumn::boolean("active");

        assert_eq!(string_col.name(), "name");
        assert_eq!(int_col.name(), "age");
        assert_eq!(bool_col.name(), "active");
    }

    #[test]
    fn test_from_implementations() {
        let col1: TypeColumn<String> = "email".into();
        let col2: TypeColumn<String> = "phone".to_string().into();

        assert_eq!(col1.name(), "email");
        assert_eq!(col2.name(), "phone");
    }

    #[test]
    fn test_with_flags() {
        let col = TypeColumn::<String>::new("test");

        // Test that flags collection is initialized as empty
        assert!(col.flags().is_empty());
    }

    #[test]
    fn test_type_column_with_table() {
        use crate::mocks::mock_typed_table_source::MockTypedTableSource;
        use crate::prelude::TableLike;
        use crate::table::Table;
        use vantage_types::EmptyEntity;

        // Create table with TypeColumns using builder pattern
        let ds = MockTypedTableSource::new();
        let table = Table::<MockTypedTableSource, EmptyEntity>::new("products", ds)
            .with_column(TypeColumn::string("title"))
            .with_column(TypeColumn::integer("price"))
            .with_column(TypeColumn::boolean("in_stock"));

        // Verify all columns exist
        assert!(table.columns().contains_key("title"));
        assert!(table.columns().contains_key("price"));
        assert!(table.columns().contains_key("in_stock"));
        assert_eq!(table.columns().len(), 3);
    }

    #[test]
    fn test_type_column_with_column_of() {
        use crate::mocks::mock_typed_table_source::MockTypedTableSource;
        use crate::prelude::TableLike;
        use crate::table::Table;
        use vantage_types::EmptyEntity;

        // Create table using with_column_of for typed columns
        let ds = MockTypedTableSource::new();
        let table = Table::<MockTypedTableSource, EmptyEntity>::new("users", ds)
            .with_column_of::<String>("name")
            .with_column_of::<i64>("age")
            .with_column_of::<bool>("active");

        // Verify columns were added
        assert!(table.columns().contains_key("name"));
        assert!(table.columns().contains_key("age"));
        assert!(table.columns().contains_key("active"));
        assert_eq!(table.columns().len(), 3);
    }
}
