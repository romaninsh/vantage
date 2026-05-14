//! YAML-facing types for the REST-API Vista driver.
//!
//! REST APIs carry no driver-specific blocks for now, so all three
//! extension types use `NoExtras`. Kept as separate typedefs so we can
//! grow a per-table/per-column block later (e.g. response-shape
//! overrides) without breaking callers.

use vantage_vista::{NoExtras, VistaSpec};

pub type NoApiExtras = NoExtras;
pub type RestApiVistaSpec = VistaSpec<NoApiExtras, NoApiExtras, NoApiExtras>;
