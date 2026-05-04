//! Vista bridge for the CSV backend.
//!
//! Construct a `Vista` from a typed `Table<Csv, E>` via `Csv::vista_factory()`,
//! or from a YAML spec via `CsvVistaFactory::from_yaml`. The YAML path builds
//! a `Table<Csv, EmptyEntity>` first and then routes through `from_table` —
//! one construction path, one reading path. CSV is read-only.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::column::core::Column as TableColumn;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{EmptyEntity, Entity, Record};
use vantage_vista::{
    Column as VistaColumn, NoExtras, Vista, VistaCapabilities, VistaFactory, VistaMetadata,
    VistaSource, VistaSpec, flags as vista_flags,
};

use crate::csv::Csv;
use crate::type_system::AnyCsvType;

// ---- driver extras --------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvTableExtras {
    pub csv: CsvBlock,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvBlock {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvColumnExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csv: Option<CsvColumnBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvColumnBlock {
    /// CSV header to read this column from when it differs from the spec name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

pub type CsvVistaSpec = VistaSpec<CsvTableExtras, CsvColumnExtras, NoExtras>;

// ---- factory --------------------------------------------------------------

pub struct CsvVistaFactory {
    csv: Csv,
}

impl CsvVistaFactory {
    pub fn new(csv: Csv) -> Self {
        Self { csv }
    }

    /// Wrap a typed table as a Vista. Column metadata is harvested from the
    /// table; CRUD goes through `Table`'s reading path.
    pub fn from_table<E>(&self, table: Table<Csv, E>) -> Result<Vista>
    where
        E: Entity<AnyCsvType> + 'static,
    {
        let metadata = metadata_from_table(&table);
        let name = table.table_name().to_string();
        let any_table = table.into_entity::<EmptyEntity>();

        let source = CsvVistaSource {
            table: any_table,
            capabilities: VistaCapabilities {
                can_count: true,
                ..VistaCapabilities::default()
            },
        };
        Ok(Vista::new(name, Box::new(source), metadata))
    }

    /// Build a `Table<Csv, EmptyEntity>` from a spec. Each column is added
    /// with its YAML-declared type; `csv.source` becomes the column's alias
    /// so `read_csv` knows which CSV header to read from.
    pub fn table_from_spec(&self, spec: &CsvVistaSpec) -> Result<Table<Csv, EmptyEntity>> {
        let csv_path = if spec.driver.csv.path.is_absolute() {
            spec.driver.csv.path.clone()
        } else {
            self.csv.base_dir().join(&spec.driver.csv.path)
        };
        let stem = csv_path
            .file_stem()
            .ok_or_else(|| error!("csv.path must point to a file", path = csv_path.display()))?
            .to_string_lossy()
            .to_string();
        let parent = csv_path
            .parent()
            .ok_or_else(|| error!("csv.path must have a parent", path = csv_path.display()))?
            .to_path_buf();

        let id_column = resolve_id_column(spec);
        let csv = Csv::new(parent).with_id_column(&id_column);
        let mut table = Table::<Csv, EmptyEntity>::new(stem, csv);

        for (name, col_spec) in &spec.columns {
            let column = build_column(name, col_spec)?;
            table.add_column(column);
        }

        if !table.columns().contains_key(&id_column) {
            return Err(error!(
                "id column not present in spec.columns",
                id = id_column
            ));
        }
        table.set_id_field(&id_column);
        for (name, col_spec) in &spec.columns {
            if col_spec.flags.iter().any(|f| f == vista_flags::TITLE) {
                table.add_title_field(name);
            }
        }

        Ok(table)
    }
}

impl VistaFactory for CsvVistaFactory {
    type TableExtras = CsvTableExtras;
    type ColumnExtras = CsvColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: CsvVistaSpec) -> Result<Vista> {
        let vista_name = spec.name.clone();
        let table = self.table_from_spec(&spec)?;
        let mut vista = self.from_table(table)?;
        vista.set_name(vista_name);
        Ok(vista)
    }
}

fn resolve_id_column(spec: &CsvVistaSpec) -> String {
    if let Some(id) = &spec.id_column {
        return id.clone();
    }
    for (name, col_spec) in &spec.columns {
        if col_spec.flags.iter().any(|f| f == vista_flags::ID) {
            return name.clone();
        }
    }
    "id".to_string()
}

fn build_column(
    name: &str,
    col_spec: &vantage_vista::ColumnSpec<CsvColumnExtras>,
) -> Result<TableColumn<AnyCsvType>> {
    let ty = col_spec.col_type.as_deref().unwrap_or("string");
    let alias = col_spec
        .driver
        .csv
        .as_ref()
        .and_then(|b| b.source.clone())
        .filter(|s| s != name);
    let hidden = col_spec.flags.iter().any(|f| f == vista_flags::HIDDEN);

    let mut col = column_for_type(name, ty)?;
    if let Some(alias) = alias {
        col = col.with_alias(alias);
    }
    if hidden {
        col = col.with_flag(ColumnFlag::Hidden);
    }
    Ok(col)
}

/// Runtime type dispatch — YAML type alias to `Column<T>` (then erased to
/// `Column<AnyCsvType>` for storage). New aliases should land in both
/// `column_for_type` and the typed-table convenience macros.
fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<AnyCsvType>> {
    let col: TableColumn<AnyCsvType> = match ty {
        "int" | "integer" | "i64" | "i32" => {
            TableColumn::from_column(TableColumn::<i64>::new(name))
        }
        "float" | "double" | "f64" | "f32" => {
            TableColumn::from_column(TableColumn::<f64>::new(name))
        }
        "bool" | "boolean" => TableColumn::from_column(TableColumn::<bool>::new(name)),
        "string" | "text" | "str" => TableColumn::from_column(TableColumn::<String>::new(name)),
        "json" => TableColumn::from_column(TableColumn::<serde_json::Value>::new(name)),
        other => {
            return Err(error!(
                "Unknown YAML column type",
                column = name,
                ty = other.to_string()
            ));
        }
    };
    Ok(col)
}

fn metadata_from_table<T, E>(table: &Table<T, E>) -> VistaMetadata
where
    T: vantage_table::traits::table_source::TableSource,
    E: Entity<T::Value>,
    T::Column<T::AnyType>: ColumnLike<T::AnyType>,
{
    let mut metadata = VistaMetadata::new();
    for (name, col) in table.columns() {
        let mut vc = VistaColumn::new(name.clone(), col.get_type().to_string());
        if col.flags().contains(&ColumnFlag::Hidden) {
            vc = vc.with_flag(vista_flags::HIDDEN);
        }
        metadata = metadata.with_column(vc);
    }
    if let Some(id_field) = table.id_field() {
        metadata = metadata.with_id_column(id_field.name().to_string());
    }
    for title in table.title_fields() {
        if let Some(col) = metadata.columns.get_mut(title) {
            col.flags.push(vista_flags::TITLE.to_string());
        }
    }
    metadata
}

// ---- source ---------------------------------------------------------------

/// Per-Vista executor — owns the typed `Table` and delegates reads to it.
/// `Vista::eq_conditions` filter on top of whatever the table itself sees
/// (in YAML form, that's just the spec; later stages add spec conditions).
pub struct CsvVistaSource {
    table: Table<Csv, EmptyEntity>,
    capabilities: VistaCapabilities,
}

impl CsvVistaSource {
    async fn read_filtered(&self, vista: &Vista) -> Result<IndexMap<String, Record<CborValue>>> {
        let raw = self.table.list_values().await?;
        let out = raw
            .into_iter()
            .filter(|(_, record)| matches_eq_conditions(record, vista))
            .map(|(id, record)| (id, csv_record_to_cbor(record)))
            .collect();
        Ok(out)
    }
}

fn csv_record_to_cbor(record: Record<AnyCsvType>) -> Record<CborValue> {
    record.into_iter().map(|(k, v)| (k, v.into())).collect()
}

fn matches_eq_conditions(record: &Record<AnyCsvType>, vista: &Vista) -> bool {
    vista
        .eq_conditions()
        .iter()
        .all(|(field, expected)| match record.get(field) {
            Some(v) => {
                let actual: CborValue = v.clone().into();
                &actual == expected
            }
            None => false,
        })
}

#[async_trait]
impl VistaSource for CsvVistaSource {
    async fn list_vista_values(
        &self,
        vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.read_filtered(vista).await
    }

    async fn get_vista_value(
        &self,
        vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        let mut data = self.read_filtered(vista).await?;
        Ok(data.shift_remove(id))
    }

    async fn get_vista_some_value(
        &self,
        vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let data = self.read_filtered(vista).await?;
        Ok(data.into_iter().next())
    }

    async fn get_vista_count(&self, vista: &Vista) -> Result<i64> {
        Ok(self.read_filtered(vista).await?.len() as i64)
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
}

impl Csv {
    /// Return a Vista factory bound to this CSV data source.
    pub fn vista_factory(&self) -> CsvVistaFactory {
        CsvVistaFactory::new(self.clone())
    }
}
