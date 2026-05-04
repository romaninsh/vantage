use serde::{Serialize, de::DeserializeOwned};
use vantage_core::{Result, error};

use crate::spec::VistaSpec;
use crate::vista::Vista;

/// Factory that produces `Vista` instances from configuration sources.
///
/// Driver crates implement this trait on a concrete factory struct
/// (e.g. `CsvVistaFactory`, `SqliteVistaFactory`). Each driver also exposes
/// its own inherent `from_table<E>(table: Table<DriverSource, E>) -> Result<Vista>`
/// method on the same factory struct — kept off the trait so vantage-vista
/// does not depend on vantage-table.
///
/// Drivers carry per-driver YAML extension types via the three associated
/// types (table block, column block, reference block). Each defaults to
/// [`crate::spec::NoExtras`] in `VistaSpec` for drivers with no extras.
pub trait VistaFactory: Send + Sync + 'static {
    /// Driver-specific table-level YAML block.
    type TableExtras: Serialize + DeserializeOwned + Default + Send + Sync + 'static;
    /// Driver-specific per-column YAML block.
    type ColumnExtras: Serialize + DeserializeOwned + Default + Send + Sync + 'static;
    /// Driver-specific per-reference YAML block.
    type ReferenceExtras: Serialize + DeserializeOwned + Default + Send + Sync + 'static;

    /// Lower a parsed spec into a `Vista`. Drivers do whatever they need
    /// here — open a file, allocate state, materialize a source.
    fn build_from_spec(
        &self,
        spec: VistaSpec<Self::TableExtras, Self::ColumnExtras, Self::ReferenceExtras>,
    ) -> Result<Vista>;

    /// Default impl: parse YAML into a typed `VistaSpec`, then build.
    /// Drivers may override to add post-parse normalization.
    #[allow(clippy::wrong_self_convention)]
    fn from_yaml(&self, yaml: &str) -> Result<Vista> {
        let spec: VistaSpec<Self::TableExtras, Self::ColumnExtras, Self::ReferenceExtras> =
            serde_yaml_ng::from_str(yaml)
                .map_err(|e| error!("Failed to parse VistaSpec YAML", detail = e.to_string()))?;
        self.build_from_spec(spec)
    }
}
