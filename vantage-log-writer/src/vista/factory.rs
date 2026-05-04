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

use crate::log_writer::LogWriter;
use crate::type_system::AnyJsonType;
use crate::vista::source::LogWriterTableShell;
use crate::vista::spec::{LogWriterTableExtras, LogWriterVistaSpec};

pub struct LogWriterVistaFactory {
    log_writer: LogWriter,
}

impl LogWriterVistaFactory {
    pub fn new(log_writer: LogWriter) -> Self {
        Self { log_writer }
    }

    pub fn from_table<E>(&self, table: Table<LogWriter, E>) -> Result<Vista>
    where
        E: Entity<serde_json::Value> + 'static,
    {
        let metadata = metadata_from_table(&table);
        let name = table.table_name().to_string();
        let any_table = table.into_entity::<EmptyEntity>();

        let source = LogWriterTableShell::new(
            any_table,
            VistaCapabilities {
                can_insert: true,
                ..VistaCapabilities::default()
            },
        );
        Ok(Vista::new(name, Box::new(source), metadata))
    }

    pub fn table_from_spec(&self, spec: &LogWriterVistaSpec) -> Result<Table<LogWriter, EmptyEntity>> {
        let table_name = spec
            .driver
            .log_writer
            .as_ref()
            .and_then(|b| b.filename.clone())
            .unwrap_or_else(|| spec.name.clone());

        let id_column = resolve_id_column(spec);
        let log_writer = self.log_writer.clone().with_id_column(&id_column);
        let mut table = Table::<LogWriter, EmptyEntity>::new(table_name, log_writer);

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

impl VistaFactory for LogWriterVistaFactory {
    type TableExtras = LogWriterTableExtras;
    type ColumnExtras = NoExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: LogWriterVistaSpec) -> Result<Vista> {
        let vista_name = spec.name.clone();
        let table = self.table_from_spec(&spec)?;
        let mut vista = self.from_table(table)?;
        vista.set_name(vista_name);
        Ok(vista)
    }
}

fn resolve_id_column(spec: &LogWriterVistaSpec) -> String {
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
    col_spec: &vantage_vista::ColumnSpec<NoExtras>,
) -> Result<TableColumn<AnyJsonType>> {
    let ty = col_spec.col_type.as_deref().unwrap_or("string");
    let hidden = col_spec.flags.iter().any(|f| f == vista_flags::HIDDEN);

    let mut col = column_for_type(name, ty)?;
    if hidden {
        col = col.with_flag(ColumnFlag::Hidden);
    }
    Ok(col)
}

fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<AnyJsonType>> {
    let col: TableColumn<AnyJsonType> = match ty {
        "int" | "integer" | "i64" | "i32" => TableColumn::from_column(TableColumn::<i64>::new(name)),
        "float" | "double" | "f64" | "f32" => {
            TableColumn::from_column(TableColumn::<f64>::new(name))
        }
        "bool" | "boolean" => TableColumn::from_column(TableColumn::<bool>::new(name)),
        "string" | "text" | "str" => TableColumn::from_column(TableColumn::<String>::new(name)),
        "json" | "object" | "array" => {
            TableColumn::from_column(TableColumn::<serde_json::Value>::new(name))
        }
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
