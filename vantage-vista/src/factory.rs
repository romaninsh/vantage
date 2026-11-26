// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
use serde::{Serialize, de::DeserializeOwned};
use vantage_core::{Result, error};

use crate::spec::VistaSpec;
use crate::vista::Vista;

/// What a load gives every table a factory builds from a spec, reference
/// targets and `base:` tables included.
#[derive(Default)]
#[non_exhaustive]
pub struct BuildContext {
    #[cfg(feature = "rhai")]
    pub(crate) script_host: Option<std::sync::Arc<vantage_rhai::Host>>,
}

impl Clone for BuildContext {
    fn clone(&self) -> Self {
        Self {
            #[cfg(feature = "rhai")]
            script_host: self.script_host.clone(),
        }
    }
}

impl BuildContext {
    /// `lazy:` columns compile on `host` instead of the fixed host with only
    /// the date functions.
    #[cfg(feature = "rhai")]
    pub fn with_script_host(mut self, host: std::sync::Arc<vantage_rhai::Host>) -> Self {
        self.script_host = Some(host);
        self
    }

    pub(crate) fn has_script_host(&self) -> bool {
        #[cfg(feature = "rhai")]
        return self.script_host.is_some();
        #[cfg(not(feature = "rhai"))]
        false
    }
}

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

    /// [`build_from_spec`](Self::build_from_spec) with what the caller's
    /// load provides, such as the script host `lazy:` columns compile on.
    ///
    /// A factory that builds `lazy:` columns overrides this. The default
    /// refuses a spec with a `lazy:` column when `ctx` has a script host,
    /// since it would compile on the fixed host instead.
    fn build_from_spec_in(
        &self,
        spec: VistaSpec<Self::TableExtras, Self::ColumnExtras, Self::ReferenceExtras>,
        ctx: &BuildContext,
    ) -> Result<Vista> {
        if ctx.has_script_host()
            && let Some((column, _)) = spec.columns.iter().find(|(_, c)| c.lazy.is_some())
        {
            return Err(error!(
                "this factory ignores the build context; its `lazy:` columns can't use the script host",
                table = spec.name.as_str(),
                column = column.as_str()
            ));
        }
        self.build_from_spec(spec)
    }

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
