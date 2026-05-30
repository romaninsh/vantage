//! Contained relations — records embedded inside a column of the parent row.
//!
//! A `contains_one` relation surfaces a single embedded object (e.g. a
//! product's `inventory`); `contains_many` surfaces an embedded array (e.g. an
//! order's `lines`). Both are presented as a full sub-[`Vista`] — listable,
//! gettable, insertable — backed by an in-memory [`ImTable`]. Writes are
//! **eager**: every mutation re-serializes the whole collection and patches the
//! parent row's host column through a `writeback` closure supplied by whoever
//! built the sub-Vista.
//!
//! See [`build_contained_vista`] for construction and [`ContainedShell`] for the
//! `TableShell` behaviour.

use std::{future::Future, pin::Pin, sync::Arc};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::im::{ImDataSource, ImTable};
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_types::{EmptyEntity, Record};

use crate::{
    capabilities::VistaCapabilities,
    column::Column,
    metadata::VistaMetadata,
    reference::{ContainedKind, ContainedSpec, Reference},
    source::TableShell,
    vista::Vista,
};

/// Persists a contained relation's collection back into the parent row.
///
/// Called after every write with the full re-serialized column value (a
/// `CborValue::Map` for `contains_one`, a `CborValue::Array` for
/// `contains_many`). The closure patches the parent record's host column —
/// the persistence-specific part the host supplies at traversal time.
pub type ContainedWriteback =
    Arc<dyn Fn(CborValue) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;

/// `TableShell` over a contained relation's in-memory records.
///
/// Reads delegate to the seeded [`ImTable`]; writes apply to it and then flush
/// the whole collection through [`ContainedWriteback`]. Holds no parent handle
/// itself — the writeback closure captures whatever the host needs to patch the
/// parent row.
pub struct ContainedShell {
    im: ImTable<EmptyEntity, CborValue>,
    metadata: VistaMetadata,
    kind: ContainedKind,
    id_column: Option<String>,
    capabilities: VistaCapabilities,
    writeback: ContainedWriteback,
}

/// Build a sub-[`Vista`] over the records embedded in `host_value`.
///
/// `host_value` is the parent row's host column: a `CborValue::Map` for
/// `contains_one`, a `CborValue::Array` of maps for `contains_many`, or `None`
/// when the column is absent (an empty contained set). `writeback` is invoked
/// after every mutation with the re-serialized collection.
pub fn build_contained_vista(
    spec: &ContainedSpec,
    host_value: Option<&CborValue>,
    writeback: ContainedWriteback,
) -> Result<Vista> {
    let ds = ImDataSource::<CborValue>::new();
    let im = ImTable::<EmptyEntity, CborValue>::new(&ds, &spec.name);
    im.seed(seed_records(spec, host_value));

    let mut metadata = VistaMetadata::new();
    for column in &spec.columns {
        metadata = metadata.with_column(column.clone());
    }
    if let Some(id) = &spec.id_column {
        metadata = metadata.with_id_column(id.clone());
    }

    let shell = ContainedShell {
        im,
        metadata,
        kind: spec.kind,
        id_column: spec.id_column.clone(),
        capabilities: VistaCapabilities {
            can_count: true,
            can_insert: true,
            can_update: true,
            can_delete: true,
            ..VistaCapabilities::default()
        },
        writeback,
    };
    Ok(Vista::new(spec.name.clone(), Box::new(shell)))
}

/// Materialize the parent column into ordered `(id, record)` rows.
fn seed_records(
    spec: &ContainedSpec,
    host_value: Option<&CborValue>,
) -> IndexMap<String, Record<CborValue>> {
    let mut rows = IndexMap::new();
    match spec.kind {
        ContainedKind::ContainsOne => {
            if let Some(record) = host_value.and_then(map_to_record) {
                rows.insert(ONE_ID.to_string(), record);
            }
        }
        ContainedKind::ContainsMany => {
            if let Some(CborValue::Array(items)) = host_value {
                for (idx, item) in items.iter().enumerate() {
                    if let Some(record) = map_to_record(item) {
                        rows.insert(row_id(spec, &record, idx), record);
                    }
                }
            }
        }
    }
    rows
}

impl ContainedShell {
    /// Re-serialize the in-memory collection and persist it to the parent
    /// column. Runs after every mutation.
    async fn flush(&self) -> Result<()> {
        let rows = self.im.list_values().await?;
        let value = match self.kind {
            ContainedKind::ContainsOne => rows
                .into_values()
                .next()
                .map(CborValue::from)
                .unwrap_or_else(|| CborValue::Map(Vec::new())),
            ContainedKind::ContainsMany => {
                CborValue::Array(rows.into_values().map(CborValue::from).collect())
            }
        };
        (self.writeback)(value).await
    }

    /// The id a record should take on insert: its declared id-column value if
    /// present, the fixed relation id for `contains_one`, otherwise the next
    /// positional index.
    async fn next_id(&self, record: &Record<CborValue>) -> Result<String> {
        if let Some(id) = self.id_column.as_deref().and_then(|c| record.get(c)) {
            return Ok(cbor_scalar_string(id));
        }
        match self.kind {
            ContainedKind::ContainsOne => Ok(ONE_ID.to_string()),
            ContainedKind::ContainsMany => Ok(self.im.list_values().await?.len().to_string()),
        }
    }
}

#[async_trait]
impl TableShell for ContainedShell {
    fn columns(&self) -> &IndexMap<String, Column> {
        &self.metadata.columns
    }

    fn references(&self) -> &IndexMap<String, Reference> {
        &self.metadata.references
    }

    fn id_column(&self) -> Option<&str> {
        self.metadata.id_column.as_deref()
    }

    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.im.list_values().await
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        self.im.get_value(id).await
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        self.im.get_some_value().await
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        Ok(self.im.list_values().await?.len() as i64)
    }

    async fn insert_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let stored = self.im.insert_value(id, record).await?;
        self.flush().await?;
        Ok(stored)
    }

    async fn insert_vista_return_id_value(
        &self,
        _vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String> {
        let id = self.next_id(record).await?;
        self.im.insert_value(&id, record).await?;
        self.flush().await?;
        Ok(id)
    }

    async fn replace_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let stored = self.im.replace_value(id, record).await?;
        self.flush().await?;
        Ok(stored)
    }

    async fn patch_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let stored = self.im.patch_value(id, partial).await?;
        self.flush().await?;
        Ok(stored)
    }

    async fn delete_vista_value(&self, _vista: &Vista, id: &String) -> Result<()> {
        self.im.delete(id).await?;
        self.flush().await
    }

    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        self.im.delete_all().await?;
        self.flush().await
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "contained"
    }
}

/// Fixed id for the single record of a `contains_one` relation.
const ONE_ID: &str = "0";

fn row_id(spec: &ContainedSpec, record: &Record<CborValue>, idx: usize) -> String {
    spec.id_column
        .as_deref()
        .and_then(|c| record.get(c))
        .map(cbor_scalar_string)
        .unwrap_or_else(|| idx.to_string())
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
