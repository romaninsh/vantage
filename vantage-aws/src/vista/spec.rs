//! YAML-facing types for the AWS Vista driver.
//!
//! Placeholders for now — AWS Vista doesn't yet support YAML-driven
//! construction. AWS table names are wire-protocol identifiers of the
//! form `{protocol}/{array_key}:{service}/{target}` (see `lib.rs`); a
//! YAML schema would have to either re-encode that vocabulary or hide
//! it behind another layer of indirection, and neither lowering is
//! useful without a concrete consumer to shape it. The typed
//! constructors under [`crate::models`] are the only sensible source
//! today.
//!
//! The types here are kept to satisfy [`VistaFactory`]'s associated-type
//! contract; `AwsVistaFactory::build_from_spec` returns an error until
//! a real consumer needs it. When that day comes, the natural extras
//! to surface are per-table `region` / `max_pages` overrides and
//! per-column `original_type` hints — sketch them then, not now.
//!
//! [`VistaFactory`]: vantage_vista::VistaFactory

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwsTableExtras {}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwsColumnExtras {}

pub type AwsVistaSpec = VistaSpec<AwsTableExtras, AwsColumnExtras, NoExtras>;
