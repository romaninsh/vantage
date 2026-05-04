//! `SqliteVistaFactory` — typed-table and YAML entry points, plus the
//! `VistaFactory` trait impl. SQLite advertises full read/write/count.

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

use crate::sqlite::SqliteDB;
use crate::sqlite::types::AnySqliteType;
use crate::sqlite::vista::source::SqliteTableShell;
use crate::sqlite::vista::spec::{SqliteColumnExtras, SqliteTableExtras, SqliteVistaSpec};

pub struct SqliteVistaFactory {
    db: SqliteDB,
}

impl SqliteVistaFactory {
    pub fn new(db: SqliteDB) -> Self {
        Self { db }
    }

    /// Wrap a typed table as a Vista. Column metadata is harvested from the
    /// table; CRUD goes through `Table`'s reading path.
    pub fn from_table<E>(&self, table: Table<SqliteDB, E>) -> Result<Vista>
    where
        E: Entity<AnySqliteType> + 'static,
    {
        let name = table.table_name().to_string();
        let any_table = table.into_entity::<EmptyEntity>();
        Ok(self.wrap(any_table, name))
    }

    /// Single source-construction site shared by `from_table` and
    /// `build_from_spec`. Keeps the capability set and `Vista::new` call in
    /// one place so a future capability flip is a one-line edit.
    fn wrap(&self, table: Table<SqliteDB, EmptyEntity>, name: String) -> Vista {
        let metadata = metadata_from_table(&table);
        let source = SqliteTableShell::new(
            table,
            VistaCapabilities {
                can_count: true,
                can_insert: true,
                can_update: true,
                can_delete: true,
                ..VistaCapabilities::default()
            },
        );
        Vista::new(name, Box::new(source), metadata)
    }

    /// Build a `Table<SqliteDB, EmptyEntity>` from a spec.
    pub fn table_from_spec(&self, spec: &SqliteVistaSpec) -> Result<Table<SqliteDB, EmptyEntity>> {
        let table_name = spec
            .driver
            .sqlite
            .as_ref()
            .and_then(|m| m.table.clone())
            .unwrap_or_else(|| spec.name.clone());

        let mut table = Table::<SqliteDB, EmptyEntity>::new(table_name, self.db.clone());

        for (name, col_spec) in &spec.columns {
            table.add_column(build_column(name, col_spec)?);
            if col_spec.flags.iter().any(|f| f == vista_flags::TITLE) {
                table.add_title_field(name);
            }
        }

        let id_column = resolve_id_column(spec);
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

impl VistaFactory for SqliteVistaFactory {
    type TableExtras = SqliteTableExtras;
    type ColumnExtras = SqliteColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: SqliteVistaSpec) -> Result<Vista> {
        let vista_name = spec.name.clone();
        let table = self.table_from_spec(&spec)?;
        let mut vista = self.wrap(table, vista_name.clone());
        vista.set_name(vista_name);
        Ok(vista)
    }
}

pub(crate) fn resolve_id_column(spec: &SqliteVistaSpec) -> String {
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
    col_spec: &vantage_vista::ColumnSpec<SqliteColumnExtras>,
) -> Result<TableColumn<AnySqliteType>> {
    let ty = col_spec.col_type.as_deref().unwrap_or("string");
    let alias = col_spec
        .driver
        .sqlite
        .as_ref()
        .and_then(|b| b.column.clone())
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

/// YAML type alias → typed `Column` (then erased to `Column<AnySqliteType>`).
pub(crate) fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<AnySqliteType>> {
    let col: TableColumn<AnySqliteType> = match ty {
        "int" | "integer" | "i64" | "i32" => {
            TableColumn::from_column(TableColumn::<i64>::new(name))
        }
        "float" | "double" | "f64" | "f32" => {
            TableColumn::from_column(TableColumn::<f64>::new(name))
        }
        "bool" | "boolean" => TableColumn::from_column(TableColumn::<bool>::new(name)),
        "string" | "text" | "str" => TableColumn::from_column(TableColumn::<String>::new(name)),
        "decimal" | "numeric" => {
            TableColumn::from_column(TableColumn::<rust_decimal::Decimal>::new(name))
        }
        "date" => TableColumn::from_column(TableColumn::<chrono::NaiveDate>::new(name)),
        "time" => TableColumn::from_column(TableColumn::<chrono::NaiveTime>::new(name)),
        "datetime" => TableColumn::from_column(TableColumn::<chrono::NaiveDateTime>::new(name)),
        "timestamp" => {
            TableColumn::from_column(TableColumn::<chrono::DateTime<chrono::Utc>>::new(name))
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
