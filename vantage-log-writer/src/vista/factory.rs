use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::{Table, VistaMetadataOptions};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{NoExtras, Vista, VistaCapabilities, VistaFactory};

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
        let metadata = table.vista_metadata(VistaMetadataOptions::default());
        let name = table.table_name().to_string();
        let any_table = table.into_entity::<EmptyEntity>();

        let source = LogWriterTableShell::new(
            any_table,
            VistaCapabilities {
                can_insert: true,
                ..VistaCapabilities::default()
            },
            metadata,
        );
        Ok(Vista::new(name, Box::new(source)))
    }

    pub fn table_from_spec(
        &self,
        spec: &LogWriterVistaSpec,
    ) -> Result<Table<LogWriter, EmptyEntity>> {
        let table_name = spec
            .driver
            .log_writer
            .as_ref()
            .and_then(|b| b.filename.clone())
            .unwrap_or_else(|| spec.name.clone());

        let id_column = spec.resolve_id_column();
        let log_writer = self.log_writer.clone().with_id_column(&id_column);
        let mut table = Table::<LogWriter, EmptyEntity>::new(table_name, log_writer);
        table.add_spec_columns(&spec.columns, build_column)?;
        table.set_spec_id_field(&id_column)?;
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

fn build_column(
    name: &str,
    col_spec: &vantage_vista::ColumnSpec<NoExtras>,
) -> Result<TableColumn<AnyJsonType>> {
    TableColumn::from_spec(name, col_spec, None, column_for_type)
}

fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<AnyJsonType>> {
    let col: TableColumn<AnyJsonType> = match ty {
        "int" | "integer" | "i64" | "i32" => {
            TableColumn::from_column(TableColumn::<i64>::new(name))
        }
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
