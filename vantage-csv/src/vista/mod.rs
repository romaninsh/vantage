//! Vista bridge for the CSV backend.
//!
//! Construct a `Vista` from a typed `Table<Csv, E>` via `Csv::vista_factory()`,
//! or from a YAML spec via `CsvVistaFactory::from_yaml`. The YAML path builds
//! a `Table<Csv, EmptyEntity>` first and then routes through `from_table` —
//! one construction path, one reading path. CSV is read-only.

pub mod factory;
pub mod source;
pub mod spec;

pub use factory::CsvVistaFactory;
pub use source::CsvVistaSource;
pub use spec::{CsvBlock, CsvColumnBlock, CsvColumnExtras, CsvTableExtras, CsvVistaSpec};

use crate::csv::Csv;

impl Csv {
    /// Return a Vista factory bound to this CSV data source.
    pub fn vista_factory(&self) -> CsvVistaFactory {
        CsvVistaFactory::new(self.clone())
    }
}
