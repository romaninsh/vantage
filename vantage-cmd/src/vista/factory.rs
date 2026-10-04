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
use vantage_table::table::{Table, VistaMetadataOptions};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{NoExtras, Vista, VistaCapabilities, VistaFactory, VistaMetadata};

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
            cs.detail = spec.driver.cmd.detail.clone().map(Into::into);
            cs
        };
        let cmd = self.cmd.clone().with_table(&spec.name, cmd_spec);
        let mut table = Table::<Cmd, EmptyEntity>::new(&spec.name, cmd);
        table.add_spec_columns(&spec.columns, build_column)?;

        // A command's output may have no id column; the id is then optional.
        let id_column = spec.resolve_id_column();
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

fn build_column(
    name: &str,
    col_spec: &vantage_vista::ColumnSpec<CmdColumnExtras>,
) -> Result<TableColumn<CborValue>> {
    TableColumn::from_spec(name, col_spec, None, column_for_type)
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
    table.vista_metadata(VistaMetadataOptions {
        id_flag: true,
        ..VistaMetadataOptions::default()
    })
}
