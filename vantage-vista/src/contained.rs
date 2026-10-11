//! Contained relations — records embedded inside a column of the parent row.
//!
//! A `contains_one` relation surfaces a single embedded object (e.g. a
//! product's `inventory`); `contains_many` surfaces an embedded array (e.g. an
//! order's `lines`). Both are presented as a full sub-[`Vista`] — listable,
//! narrowable, writable — over the embedded records held in memory. Writes
//! are **eager**: every mutation re-serializes the whole collection and
//! patches the parent row's host column through a writeback closure supplied
//! by the shell that serves the relation from its `get_ref`.

use std::{future::Future, pin::Pin, sync::Arc};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::im::{ImDataSource, ImTable};
use vantage_types::{EmptyEntity, Record};

use crate::{
    capabilities::{Capability, VistaCapabilities},
    metadata::VistaMetadata,
    reference::{Reference, ReferenceKind},
    vista::Vista,
};

mod shell;

/// Persists a contained relation's collection back into the parent row.
///
/// Called after every write with the full re-serialized column value (a
/// `CborValue::Map` for `contains_one`, a `CborValue::Array` for
/// `contains_many`). The closure patches the parent record's host column —
/// the persistence-specific part the host supplies at traversal time.
pub type ContainedWriteback =
    Arc<dyn Fn(CborValue) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;

/// Resolves a relation *of* a contained record (e.g. a line's `product`) back
/// into the real table as a `Vista`. Supplied by the host at traversal time,
/// capturing the contained table's own relations.
pub type ContainedRefResolver =
    Arc<dyn Fn(&str, &Record<CborValue>) -> Result<Vista> + Send + Sync>;

/// [`TableShell`](crate::TableShell) over a contained relation's records.
///
/// Reads and narrowing run over the seeded rows; writes apply to them and
/// then flush the whole collection through [`ContainedWriteback`]. A clone
/// shares the rows and the writeback and owns its conditions.
#[derive(Clone)]
pub(crate) struct ContainedShell {
    im: ImTable<EmptyEntity, CborValue>,
    schema: VistaMetadata,
    kind: ReferenceKind,
    capabilities: VistaCapabilities,
    writeback: ContainedWriteback,
    ref_resolver: Option<ContainedRefResolver>,
    conditions: Vec<crate::query::Condition>,
}

/// Build a sub-[`Vista`] over the records `reference` embeds in `host_value`.
///
/// `reference` is a [`ContainsOne`](ReferenceKind::ContainsOne) or
/// [`ContainsMany`](ReferenceKind::ContainsMany) relation; `schema` gives the
/// contained records' columns and id column (no id column: `contains_many`
/// records are addressed by position, a `contains_one` record by `"0"`).
/// `host_value` is the parent row's host column: a map for `contains_one`, an
/// array of maps for `contains_many`, or `None` for an empty set.
/// `writeback` runs after every mutation with the re-serialized collection;
/// `ref_resolver`, when given, serves the contained records' own relations.
pub fn build_contained_vista(
    reference: &Reference,
    schema: VistaMetadata,
    host_value: Option<&CborValue>,
    writeback: ContainedWriteback,
    ref_resolver: Option<ContainedRefResolver>,
) -> Result<Vista> {
    if !reference.kind.is_contained() {
        return Err(error!(
            "relation is not a contained relation",
            relation = reference.name.as_str()
        )
        .mark_incorrect_usage());
    }
    let ds = ImDataSource::<CborValue>::new();
    let im = ImTable::<EmptyEntity, CborValue>::new(&ds, &reference.name);
    im.seed(seed_records(reference.kind, &schema, host_value));

    let mut capabilities: VistaCapabilities = [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::FilterOps,
        Capability::Window,
    ]
    .into_iter()
    .collect();
    if ref_resolver.is_some() {
        capabilities = capabilities.with(Capability::Traverse);
    }
    let shell = ContainedShell {
        im,
        schema,
        kind: reference.kind,
        capabilities,
        writeback,
        ref_resolver,
        conditions: Vec::new(),
    };
    Ok(Vista::new(reference.name.clone(), shell))
}

/// Fixed id for the single record of a `contains_one` relation.
const ONE_ID: &str = "0";

/// Materialize the parent column into ordered `(id, record)` rows.
fn seed_records(
    kind: ReferenceKind,
    schema: &VistaMetadata,
    host_value: Option<&CborValue>,
) -> IndexMap<String, Record<CborValue>> {
    let mut rows = IndexMap::new();
    match (kind, host_value) {
        (ReferenceKind::ContainsOne, Some(value)) => {
            if let Some(record) = map_to_record(value) {
                rows.insert(ONE_ID.to_string(), record);
            }
        }
        (ReferenceKind::ContainsMany, Some(CborValue::Array(items))) => {
            for (idx, item) in items.iter().enumerate() {
                if let Some(record) = map_to_record(item) {
                    let id = schema
                        .id_column
                        .as_deref()
                        .and_then(|c| record.get(c))
                        .map(cbor_scalar_string)
                        .unwrap_or_else(|| idx.to_string());
                    rows.insert(id, record);
                }
            }
        }
        _ => {}
    }
    rows
}

/// Convert a CBOR map value into a `Record`, or `None` if it isn't a map.
fn map_to_record(value: &CborValue) -> Option<Record<CborValue>> {
    matches!(value, CborValue::Map(_)).then(|| Record::<CborValue>::from(value.clone()))
}

/// Stringify a scalar CBOR value for use as an id.
fn cbor_scalar_string(value: &CborValue) -> String {
    match value {
        CborValue::Text(s) => s.clone(),
        CborValue::Integer(i) => i128::from(*i).to_string(),
        other => format!("{other:?}"),
    }
}
