use vantage_core::Result;

use crate::vista::Vista;

/// Factory that produces `Vista` instances from configuration sources.
///
/// Driver crates implement this trait on a concrete factory struct
/// (e.g. `SqliteVistaFactory`). Each driver also exposes its own
/// inherent `from_table<E>(table: Table<DriverSource, E>) -> Result<Vista>`
/// method on the same factory struct — kept off the trait so vantage-vista
/// does not need to depend on vantage-table (which would create a dep cycle
/// via vantage-expressions).
pub trait VistaFactory: Send + Sync + 'static {
    fn from_yaml(&self, yaml: &str) -> Result<Vista>;
}
