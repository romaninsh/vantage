//! Mock implementations for testing and examples
//!
//! This module provides mock data sources that can be used for testing
//! and demonstration purposes without requiring external dependencies.

pub mod csv;
pub mod queue;

pub use csv::{CsvFile, MockCsv};
pub use queue::{MockQueue, Topic};
