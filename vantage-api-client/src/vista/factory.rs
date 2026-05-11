//! `RestApiVistaFactory` — typed-table and YAML entry points, plus the
//! `VistaFactory` trait impl. Read-only today (matching `RestApi`'s current
//! `TableSource` impl), so the factory advertises only `can_count` and
//! offset pagination.

use serde_json::Value as JsonValue;
use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    Column as VistaColumn, NoExtras, PaginateKind, Vista, VistaCapabilities, VistaFactory,
    VistaMetadata, flags as vista_flags,
};

use crate::RestApi;
use crate::vista::source::RestApiTableShell;
use crate::vista::spec::{RestApiColumnExtras, RestApiTableExtras, RestApiVistaSpec};

pub struct RestApiVistaFactory {
    api: RestApi,
}

impl RestApiVistaFactory {
    pub fn new(api: RestApi) -> Self {
        Self { api }
    }

    /// Wrap a typed table as a Vista. Column metadata is harvested from the
    /// table; reads go through `Table`'s reading path.
    pub fn from_table<E>(&self, table: Table<RestApi, E>) -> Result<Vista>
    where
        E: Entity<JsonValue> + 'static,
    {
        let metadata = metadata_from_table(&table);
        let name = table.table_name().to_string();
        let any_table = table.into_entity::<EmptyEntity>();

        let source = RestApiTableShell::new(any_table, default_capabilities());
        Ok(Vista::new(name, Box::new(source), metadata))
    }

    /// Build a `Table<RestApi, EmptyEntity>` from a spec. The endpoint path
    /// comes from `api.endpoint` (or the spec name); per-column `api.field`
    /// becomes the column alias so the JSON read path knows which field to
    /// pull from each row.
    pub fn table_from_spec(&self, spec: &RestApiVistaSpec) -> Result<Table<RestApi, EmptyEntity>> {
        let endpoint = spec
            .driver
            .api
            .as_ref()
            .and_then(|m| m.endpoint.clone())
            .unwrap_or_else(|| spec.name.clone());

        let mut table = Table::<RestApi, EmptyEntity>::new(endpoint, self.api.clone());

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

impl VistaFactory for RestApiVistaFactory {
    type TableExtras = RestApiTableExtras;
    type ColumnExtras = RestApiColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: RestApiVistaSpec) -> Result<Vista> {
        let vista_name = spec.name.clone();
        let table = self.table_from_spec(&spec)?;
        let mut vista = self.from_table(table)?;
        vista.set_name(vista_name);
        Ok(vista)
    }
}

fn default_capabilities() -> VistaCapabilities {
    VistaCapabilities {
        can_count: true,
        paginate_kind: PaginateKind::Offset,
        ..VistaCapabilities::default()
    }
}

pub(crate) fn resolve_id_column(spec: &RestApiVistaSpec) -> String {
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
    col_spec: &vantage_vista::ColumnSpec<RestApiColumnExtras>,
) -> Result<TableColumn<JsonValue>> {
    let ty = col_spec.col_type.as_deref().unwrap_or("string");
    let alias = col_spec
        .driver
        .api
        .as_ref()
        .and_then(|b| b.field.clone())
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

/// YAML type alias → typed `Column` (then erased to `Column<serde_json::Value>`).
/// `json` covers arbitrary nested values — common in REST payloads.
pub(crate) fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<JsonValue>> {
    let col: TableColumn<JsonValue> = match ty {
        "int" | "integer" | "i64" | "i32" => {
            TableColumn::from_column(TableColumn::<i64>::new(name))
        }
        "float" | "double" | "f64" | "f32" => {
            TableColumn::from_column(TableColumn::<f64>::new(name))
        }
        "bool" | "boolean" => TableColumn::from_column(TableColumn::<bool>::new(name)),
        "string" | "text" | "str" => TableColumn::from_column(TableColumn::<String>::new(name)),
        "json" => TableColumn::from_column(TableColumn::<JsonValue>::new(name)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_vista::ColumnSpec;

    #[test]
    fn build_column_uses_api_field_as_alias() {
        let mut col_spec = ColumnSpec::<RestApiColumnExtras>::new().with_type("string");
        col_spec.driver.api = Some(crate::vista::spec::RestApiColumnBlock {
            field: Some("name".into()),
        });
        let col = build_column("full_name", &col_spec).unwrap();
        assert_eq!(col.name(), "full_name");
        assert_eq!(col.alias(), Some("name"));
    }

    #[test]
    fn build_column_no_alias_when_field_matches_name() {
        let mut col_spec = ColumnSpec::<RestApiColumnExtras>::new().with_type("string");
        col_spec.driver.api = Some(crate::vista::spec::RestApiColumnBlock {
            field: Some("name".into()),
        });
        let col = build_column("name", &col_spec).unwrap();
        assert_eq!(col.alias(), None);
    }

    #[test]
    fn resolve_id_column_prefers_explicit() {
        let yaml = r#"
name: users
id_column: uuid
columns:
  uuid: { type: string }
  email: { type: string, flags: [id] }
"#;
        let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(resolve_id_column(&spec), "uuid");
    }

    #[test]
    fn resolve_id_column_falls_back_to_flag() {
        let yaml = r#"
name: users
columns:
  email: { type: string, flags: [id] }
  name: { type: string }
"#;
        let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(resolve_id_column(&spec), "email");
    }

    #[test]
    fn resolve_id_column_defaults_to_id() {
        let yaml = r#"
name: users
columns:
  name: { type: string }
"#;
        let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(resolve_id_column(&spec), "id");
    }

    #[test]
    fn column_for_type_rejects_unknown_alias() {
        let err = column_for_type("foo", "blob").unwrap_err();
        assert!(err.to_string().contains("Unknown YAML column type"));
    }
}
