//! `CmdVistaFactory` — builds a [`Vista`] from either a typed
//! `Table<Cmd, E>` or a YAML [`CmdVistaSpec`].
//!
//! Unlike `vantage-aws` (whose YAML path is stubbed), the command driver
//! supports full YAML construction: the spec's `cmd.rhai` script is
//! registered on a cloned `Cmd` under the vista name and columns/flags/id
//! are lowered onto a typed table. References declared in YAML are lowered
//! onto the table as real `with_many` / `with_one` registrations by
//! [`crate::models::CmdModelFactory`], so traversal flows through the
//! built-in `Table::get_ref_from_row` path — no bespoke resolver.

use ciborium::Value as CborValue;
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

use crate::cmd::{Cmd, CmdSpec};
use crate::vista::source::CmdTableShell;
use crate::vista::spec::{CmdColumnExtras, CmdTableExtras, CmdVistaSpec};

pub struct CmdVistaFactory {
    cmd: Cmd,
}

impl CmdVistaFactory {
    pub fn new(cmd: Cmd) -> Self {
        Self { cmd }
    }

    /// Wrap a typed `Table<Cmd, E>` as a `Vista` (the script must already
    /// be registered on the table's `Cmd`). References registered on the
    /// table via `with_many` / `with_one` are surfaced for traversal.
    pub fn from_table<E>(&self, table: Table<Cmd, E>) -> Result<Vista>
    where
        E: Entity<CborValue> + 'static,
    {
        let name = table.table_name().to_string();
        let metadata = metadata_from_table(&table);
        let any_table = table.into_entity::<EmptyEntity>();
        Ok(wrap(any_table, name, metadata))
    }

    /// Build a typed `Table<Cmd, EmptyEntity>` from a spec's columns / id /
    /// title flags and the spec's `cmd.rhai` script. References are *not*
    /// added here — the caller (which knows how to resolve target model
    /// names to specs) lowers them via `with_many` / `with_one`.
    pub(crate) fn build_columns_table(
        &self,
        spec: &CmdVistaSpec,
    ) -> Result<Table<Cmd, EmptyEntity>> {
        let cmd_spec = {
            let mut cs = CmdSpec::new(spec.driver.cmd.rhai.clone());
            cs.command = spec.driver.cmd.command.clone();
            cs.env = spec.driver.cmd.env.clone();
            cs
        };
        let cmd = self.cmd.clone().with_table(&spec.name, cmd_spec);
        let mut table = Table::<Cmd, EmptyEntity>::new(&spec.name, cmd);

        for (name, col_spec) in &spec.columns {
            table.add_column(build_column(name, col_spec)?);
            if col_spec.flags.iter().any(|f| f == vista_flags::TITLE) {
                table.add_title_field(name);
            }
        }

        let id_column = resolve_id_column(spec);
        if table.columns().contains_key(&id_column) {
            table.set_id_field(&id_column);
        }

        Ok(table)
    }
}

/// Wrap a column-built table as a `Vista`.
fn wrap(table: Table<Cmd, EmptyEntity>, name: String, metadata: VistaMetadata) -> Vista {
    let source = CmdTableShell::new(
        table,
        VistaCapabilities {
            can_count: true,
            ..VistaCapabilities::default()
        },
        metadata,
    );
    Vista::new(name, Box::new(source))
}

impl VistaFactory for CmdVistaFactory {
    type TableExtras = CmdTableExtras;
    type ColumnExtras = CmdColumnExtras;
    type ReferenceExtras = NoExtras;

    /// YAML → `Vista` with columns only. The reachable-by-name reference
    /// graph lives in [`crate::models::CmdModelFactory`]; a vista built
    /// straight from a lone spec has no traversable relations.
    fn build_from_spec(&self, spec: CmdVistaSpec) -> Result<Vista> {
        let name = spec.name.clone();
        let table = self.build_columns_table(&spec)?;
        let metadata = metadata_from_table(&table);
        Ok(wrap(table, name, metadata))
    }
}

fn resolve_id_column(spec: &CmdVistaSpec) -> String {
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
    col_spec: &vantage_vista::ColumnSpec<CmdColumnExtras>,
) -> Result<TableColumn<CborValue>> {
    let ty = col_spec.col_type.as_deref().unwrap_or("string");
    let hidden = col_spec.flags.iter().any(|f| f == vista_flags::HIDDEN);
    let mut col = column_for_type(name, ty)?;
    if hidden {
        col = col.with_flag(ColumnFlag::Hidden);
    }
    Ok(col)
}

/// Map a YAML type alias to a typed `Column`, erased to `Column<CborValue>`
/// for storage. `get_type()` keeps the original Rust type name so the
/// renderer's `AnyCmdType::from_cbor_typed` can coerce primitives.
fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<CborValue>> {
    let col: TableColumn<CborValue> = match ty {
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
                column = name.to_string(),
                ty = other.to_string()
            ));
        }
    };
    Ok(col)
}

pub(crate) fn metadata_from_table<E>(table: &Table<Cmd, E>) -> VistaMetadata
where
    E: Entity<CborValue>,
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
        let id_name = id_field.name().to_string();
        metadata = metadata.with_id_column(id_name.clone());
        if let Some(col) = metadata.columns.get_mut(&id_name) {
            col.flags.push(vista_flags::ID.to_string());
        }
    }
    for title in table.title_fields() {
        if let Some(col) = metadata.columns.get_mut(title) {
            col.flags.push(vista_flags::TITLE.to_string());
        }
    }
    metadata
}
