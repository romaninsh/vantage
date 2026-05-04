//! `CsvVistaFactory` — typed-table and YAML entry points, plus the
//! `VistaFactory` trait impl. CSV is read-only, so the factory advertises
//! only `can_count`.

use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    Column as VistaColumn, NoExtras, Vista, VistaCapabilities, VistaFactory, VistaMetadata,
    flags as vista_flags,
};

use crate::csv::Csv;
use crate::type_system::AnyCsvType;
use crate::vista::source::CsvVistaSource;
use crate::vista::spec::{CsvColumnExtras, CsvTableExtras, CsvVistaSpec};

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

        let source = CsvVistaSource::new(
            any_table,
            VistaCapabilities {
                can_count: true,
                ..VistaCapabilities::default()
            },
        );
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
            table.add_column(build_column(name, col_spec)?);
            if col_spec.flags.iter().any(|f| f == vista_flags::TITLE) {
                table.add_title_field(name);
            }
        }

        if !table.columns().contains_key(&id_column) {
            return Err(error!(
                "id column not present in spec.columns",
                id = id_column
            ));
        }
        table.set_id_field(&id_column);

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

pub(crate) fn resolve_id_column(spec: &CsvVistaSpec) -> String {
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

pub(crate) fn build_column(
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
pub(crate) fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<AnyCsvType>> {
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

pub(crate) fn metadata_from_table<T, E>(table: &Table<T, E>) -> VistaMetadata
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
