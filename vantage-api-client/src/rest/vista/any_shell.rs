//! Generic `TableShell` adapter over `AnyTable`.
//!
//! When a typed `Table::get_ref(...)` returns a related table it comes
//! back type-erased as `AnyTable` — the static `E2` is hidden by the
//! reference machinery. To keep the Vista driver path uniform we wrap
//! that `AnyTable` in `AnyTableShell` and harvest the metadata it
//! already exposes (column names + types, id field, title fields).
//!
//! The shell forwards everything to `AnyTable`. `AnyTable` itself uses
//! the CBOR adapter when its inner table doesn't natively use
//! `CborValue`, so `RestApi` (which is JSON-native) round-trips
//! through CBOR transparently here.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::any::AnyTable;
use vantage_table::traits::table_like::TableLike;
use vantage_types::Record;
use vantage_vista::{
    Column as VistaColumn, TableShell, Vista, VistaCapabilities, VistaMetadata,
    flags as vista_flags,
};

pub struct AnyTableShell {
    table: AnyTable,
    capabilities: VistaCapabilities,
}

impl AnyTableShell {
    pub fn new(table: AnyTable, capabilities: VistaCapabilities) -> Self {
        Self {
            table,
            capabilities,
        }
    }

    /// Build a `Vista` from an `AnyTable`, harvesting metadata
    /// (columns, id field, title fields) from the table itself.
    ///
    /// Capabilities default to `can_count` only — REST API is the
    /// initial caller and is read-only. Callers needing different
    /// capabilities should build the shell explicitly via `new`.
    pub fn into_vista(table: AnyTable) -> Result<Vista> {
        let caps = VistaCapabilities {
            can_count: true,
            ..VistaCapabilities::default()
        };
        Self::into_vista_with(table, caps)
    }

    pub fn into_vista_with(table: AnyTable, capabilities: VistaCapabilities) -> Result<Vista> {
        let name = table.table_name().to_string();
        let metadata = metadata_from_any_table(&table);
        let shell = Self::new(table, capabilities);
        Ok(Vista::new(name, Box::new(shell), metadata))
    }
}

fn metadata_from_any_table(table: &AnyTable) -> VistaMetadata {
    let mut metadata = VistaMetadata::new();
    let types = table.column_types();
    let id_field = table.id_field_name();
    let title_fields = table.title_field_names();
    for (name, ty) in types {
        let mut col = VistaColumn::new(name.clone(), ty.to_string());
        if Some(name.as_str()) == id_field.as_deref() {
            col = col.with_flag(vista_flags::ID);
        }
        if title_fields.iter().any(|t| t == &name) {
            col = col.with_flag(vista_flags::TITLE);
        }
        metadata = metadata.with_column(col);
    }
    if let Some(id) = id_field {
        metadata = metadata.with_id_column(id);
    }
    metadata
}

#[async_trait]
impl TableShell for AnyTableShell {
    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.table.list_values().await
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        self.table.get_value(id).await
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        self.table.get_some_value().await
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        self.table.get_count().await
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        // AnyTable only accepts stringy filters at this layer; coerce
        // scalar CBOR values into their natural string form. Compound
        // values (arrays, maps) can't be expressed as a flat eq, so
        // we refuse them rather than silently truncating.
        let s = match value {
            CborValue::Text(s) => s.clone(),
            CborValue::Integer(i) => i128::from(*i).to_string(),
            CborValue::Float(f) => f.to_string(),
            CborValue::Bool(b) => b.to_string(),
            CborValue::Null => String::new(),
            other => {
                return Err(error!(
                    "AnyTableShell: eq value must be a scalar",
                    field = field,
                    value = format!("{:?}", other)
                ));
            }
        };
        self.table.add_condition_eq(field, &s)
    }

    fn get_ref(&self, relation: &str) -> Result<Vista> {
        let any_table = self.table.get_ref(relation)?;
        AnyTableShell::into_vista(any_table)
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "any-table"
    }
}
