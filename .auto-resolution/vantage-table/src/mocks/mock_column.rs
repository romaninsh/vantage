//! Mock column implementation for testing
//!
//! Provides a simple column implementation that can be used across all mock DataSources.

use crate::column::core::ColumnType;
use crate::column::flags::ColumnFlag;
use crate::traits::column_like::ColumnLike;
use serde_json::Value;
use std::collections::HashSet;
use std::marker::PhantomData;

/// Simple column implementation for testing mocks
#[derive(Debug, Clone)]
pub struct MockColumn<T = Value>
where
    T: ColumnType,
{
    name: String,
    flags: HashSet<ColumnFlag>,
    _phantom: PhantomData<T>,
}

impl<T: ColumnType> MockColumn<T> {
    /// Create a new mock column with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            flags: HashSet::new(),
            _phantom: PhantomData,
        }
    }

    pub fn into_type<T2: ColumnType>(self) -> MockColumn<T2>
    where
        T: ColumnType,
    {
        MockColumn::<T2> {
            name: self.name,
            flags: self.flags,
            _phantom: PhantomData,
        }
    }
}

impl<T: ColumnType> ColumnLike<T> for MockColumn<T> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_column_basic() {
        let col = MockColumn::<Value>::new("test_column");
        assert_eq!(col.name(), "test_column");
        assert_eq!(col.alias(), None);
    }
}
