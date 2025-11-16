//! Basic MockDataSource implementation for unit tests.

use crate::traits::datasource::DataSource;

/// Minimal DataSource implementation for unit testing.
#[derive(Debug, Clone, Default)]
pub struct MockDataSource;

impl MockDataSource {
    /// Create a new MockDataSource
    pub fn new() -> Self {
        Self
    }
}

impl DataSource for MockDataSource {}
