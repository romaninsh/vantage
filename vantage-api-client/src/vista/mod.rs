//! Vista bridge for the REST API backend.
//!
//! Construct a `Vista` from a typed `Table<RestApi, E>` via
//! `RestApi::vista_factory()`, or from a YAML spec via
//! `RestApiVistaFactory::from_yaml`. The YAML path builds a
//! `Table<RestApi, EmptyEntity>` first and then routes through `from_table`
//! — one construction path, one reading path.
//!
//! REST responses are `serde_json::Value`; vista's wire type is
//! `ciborium::Value`. The shell translates each row at the boundary via a
//! serde round-trip (`ciborium::Value::serialized` / `serde_json::to_value`).
//! Conditions translate the other direction: `(field, CborValue)` → JSON
//! value → `eq_condition`, which `RestApi::fetch_records` folds into the
//! request URL query string.

pub mod factory;
pub mod source;
pub mod spec;

pub use factory::RestApiVistaFactory;
pub use source::RestApiTableShell;
pub use spec::{
    RestApiColumnBlock, RestApiColumnExtras, RestApiTableBlock, RestApiTableExtras,
    RestApiVistaSpec,
};

use crate::RestApi;

impl RestApi {
    /// Return a Vista factory bound to this REST API.
    pub fn vista_factory(&self) -> RestApiVistaFactory {
        RestApiVistaFactory::new(self.clone())
    }
}
