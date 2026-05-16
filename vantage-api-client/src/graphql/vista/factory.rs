//! `GraphqlApiVistaFactory` — typed-table entry point and `VistaFactory`
//! trait impl for the GraphQL adapter.
//!
//! GraphQL is read-only at this stage (writes depend on schema), so the
//! factory advertises `can_count` only. Metadata is harvested from the
//! typed table before erasure so column types, id field, title fields,
//! and relations all reach the Vista layer with full fidelity.

use vantage_core::{Result, error};
use vantage_table::any::AnyTable;
use vantage_table::column::core::Column as TableColumn;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    Column as VistaColumn, ColumnSpec, Reference as VistaReference, ReferenceKind, Vista,
    VistaCapabilities, VistaFactory, VistaMetadata, flags as vista_flags,
};

use crate::graphql::api::GraphqlApi;
use crate::graphql::condition::FilterDialect;
use crate::graphql::types::AnyGraphqlType;
use crate::graphql::vista::source::GraphqlApiTableShell;
use crate::graphql::vista::spec::{
    GraphqlApiVistaSpec, GraphqlColumnExtras, GraphqlTableExtras,
};

pub struct GraphqlApiVistaFactory {
    api: GraphqlApi,
}

impl GraphqlApiVistaFactory {
    pub fn new(api: GraphqlApi) -> Self {
        Self { api }
    }

    pub fn api(&self) -> &GraphqlApi {
        &self.api
    }

    /// Wrap a typed `Table<GraphqlApi, E>` as a `Vista`. Column metadata,
    /// id field, title fields, and references are harvested up front;
    /// the table is then erased through `AnyTable::from_table` so the
    /// CBOR boundary is handled by the shared `CborAdapter` blanket.
    pub fn from_table<E>(&self, table: Table<GraphqlApi, E>) -> Result<Vista>
    where
        E: Entity<AnyGraphqlType> + 'static,
    {
        let name = table.table_name().to_string();
        let metadata = metadata_from_table(&table);
        let any_table = AnyTable::from_table(table);

        let source = GraphqlApiTableShell::new(
            any_table,
            VistaCapabilities {
                can_count: true,
                ..VistaCapabilities::default()
            },
        );
        Ok(Vista::new(name, Box::new(source), metadata))
    }

    /// Build a `Table<GraphqlApi, EmptyEntity>` from a YAML spec. Per-table
    /// dialect / filter-arg / root-field overrides land on the table's
    /// `GraphqlApi` clone; column types are resolved through
    /// [`column_for_type`].
    pub fn table_from_spec(
        &self,
        spec: &GraphqlApiVistaSpec,
    ) -> Result<Table<GraphqlApi, EmptyEntity>> {
        let mut api = self.api.clone();
        if let Some(block) = spec.driver.graphql.as_ref() {
            if let Some(d) = block.dialect.as_deref() {
                api.dialect = match d.to_ascii_lowercase().as_str() {
                    "hasura" => FilterDialect::Hasura,
                    "generic" => FilterDialect::Generic,
                    other => {
                        return Err(error!(
                            "Unknown filter dialect in spec",
                            dialect = other.to_string()
                        ));
                    }
                };
            }
            if let Some(arg) = block.filter_arg.clone() {
                api.filter_arg_name = Some(arg);
            }
        }

        let root_field = spec
            .driver
            .graphql
            .as_ref()
            .and_then(|b| b.root_field.clone())
            .unwrap_or_else(|| spec.name.clone());

        let id_column = resolve_id_column(spec);
        let mut table = Table::<GraphqlApi, EmptyEntity>::new(root_field, api);

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

impl VistaFactory for GraphqlApiVistaFactory {
    type TableExtras = GraphqlTableExtras;
    type ColumnExtras = GraphqlColumnExtras;
    type ReferenceExtras = vantage_vista::NoExtras;

    fn build_from_spec(&self, spec: GraphqlApiVistaSpec) -> Result<Vista> {
        let vista_name = spec.name.clone();
        let table = self.table_from_spec(&spec)?;
        let mut vista = self.from_table(table)?;
        vista.set_name(vista_name);
        Ok(vista)
    }
}

/// Resolve the id column: explicit `id_column` field, else the first
/// column flagged `id`, else `"id"` as a final fallback.
fn resolve_id_column(spec: &GraphqlApiVistaSpec) -> String {
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
    col_spec: &ColumnSpec<GraphqlColumnExtras>,
) -> Result<TableColumn<AnyGraphqlType>> {
    let ty = col_spec.col_type.as_deref().unwrap_or("string");
    let hidden = col_spec.flags.iter().any(|f| f == vista_flags::HIDDEN);

    let mut col = column_for_type(name, ty)?;
    if hidden {
        col = col.with_flag(ColumnFlag::Hidden);
    }
    Ok(col)
}

/// Map YAML scalar names to typed columns under `AnyGraphqlType`.
/// Mirrors the dialect of `vantage-csv/src/vista/factory.rs:144`.
fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<AnyGraphqlType>> {
    use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
    use uuid::Uuid;

    let col: TableColumn<AnyGraphqlType> = match ty {
        "int" | "integer" | "i32" => TableColumn::from_column(TableColumn::<i32>::new(name)),
        "bigint" | "i64" | "long" => TableColumn::from_column(TableColumn::<i64>::new(name)),
        "float" | "double" | "f64" | "f32" => {
            TableColumn::from_column(TableColumn::<f64>::new(name))
        }
        "bool" | "boolean" => TableColumn::from_column(TableColumn::<bool>::new(name)),
        "string" | "text" | "str" | "id" => {
            TableColumn::from_column(TableColumn::<String>::new(name))
        }
        "datetime" | "timestamp" | "timestamptz" => {
            TableColumn::from_column(TableColumn::<DateTime<Utc>>::new(name))
        }
        "date" => TableColumn::from_column(TableColumn::<NaiveDate>::new(name)),
        "time" => TableColumn::from_column(TableColumn::<NaiveTime>::new(name)),
        "uuid" => TableColumn::from_column(TableColumn::<Uuid>::new(name)),
        "json" => TableColumn::from_column(TableColumn::<serde_json::Value>::new(name)),
        other => {
            return Err(error!(
                "Unknown YAML column type for GraphQL Vista",
                column = name.to_string(),
                ty = other.to_string()
            ));
        }
    };
    Ok(col)
}

fn metadata_from_table<E>(table: &Table<GraphqlApi, E>) -> VistaMetadata
where
    E: Entity<AnyGraphqlType> + 'static,
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
        let id = id_field.name().to_string();
        metadata = metadata.with_id_column(id.clone());
        if let Some(col) = metadata.columns.get_mut(&id) {
            col.flags.push(vista_flags::ID.to_string());
        }
    }
    for title in table.title_fields() {
        if let Some(col) = metadata.columns.get_mut(title) {
            col.flags.push(vista_flags::TITLE.to_string());
        }
    }
    for relation in table.references() {
        metadata = metadata.with_reference(VistaReference::new(
            relation.clone(),
            // Target / FK metadata is internal to the typed-table reference
            // closure — surface a placeholder so the universal shape stays
            // populated, matching REST's posture.
            "",
            ReferenceKind::HasMany,
            "",
        ));
    }
    metadata
}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_types::EmptyEntity;

    #[test]
    fn factory_builds_vista_with_metadata_and_driver_name() {
        let api = GraphqlApi::new("https://api.test/graphql");
        let table: Table<GraphqlApi, EmptyEntity> = Table::new("launches", api.clone())
            .with_id_column("id")
            .with_column_of::<String>("mission_name")
            .with_column_of::<i64>("launch_year");

        let vista = api.vista_factory().from_table(table).unwrap();
        assert_eq!(vista.name(), "launches");

        // Metadata carries every declared column.
        let cols = vista.get_column_names();
        assert!(cols.contains(&"id"));
        assert!(cols.contains(&"mission_name"));
        assert!(cols.contains(&"launch_year"));
        assert_eq!(vista.get_id_column(), Some("id"));

        // Driver name surfaces from the shell so generic UI code can
        // distinguish a GraphQL-backed Vista from a REST one.
        assert_eq!(vista.driver(), "graphql");
    }

    #[test]
    fn factory_capabilities_advertise_count_only() {
        let api = GraphqlApi::new("https://api.test/graphql");
        let table: Table<GraphqlApi, EmptyEntity> = Table::new("launches", api.clone())
            .with_id_column("id");
        let vista = api.vista_factory().from_table(table).unwrap();
        assert!(vista.capabilities().can_count);
        // Writes are stubbed at the TableSource layer so the shell
        // doesn't advertise them.
        assert!(!vista.capabilities().can_insert);
        assert!(!vista.capabilities().can_update);
        assert!(!vista.capabilities().can_delete);
    }

    #[test]
    fn build_from_yaml_spec_with_all_overrides() {
        // YAML covering: id column, multiple types, flags (id, title,
        // hidden), explicit graphql block with root_field, dialect, and
        // filter_arg overrides.
        let yaml = r#"
name: launches
columns:
  id:
    type: string
    flags: [id]
  mission_name:
    type: string
    flags: [title]
  launch_year:
    type: int
  is_tentative:
    type: bool
  internal_score:
    type: float
    flags: [hidden]
graphql:
  root_field: launches
  dialect: generic
  filter_arg: find
"#;
        let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let api = GraphqlApi::new("https://api.spacex.land/graphql/");
        let factory = api.vista_factory();
        let vista = factory.build_from_spec(spec).unwrap();

        assert_eq!(vista.name(), "launches");
        assert_eq!(vista.driver(), "graphql");
        assert_eq!(vista.get_id_column(), Some("id"));

        let cols = vista.get_column_names();
        assert!(cols.contains(&"id"));
        assert!(cols.contains(&"mission_name"));
        assert!(cols.contains(&"launch_year"));
        assert!(cols.contains(&"is_tentative"));
        assert!(cols.contains(&"internal_score"));

        // Title flag survives the YAML → Table → Vista round-trip.
        let titles = vista.get_title_columns();
        assert!(titles.contains(&"mission_name"));
    }

    #[test]
    fn build_from_yaml_spec_minimal_uses_defaults() {
        // No graphql block, no explicit id_column — id flag does the work.
        let yaml = r#"
name: rockets
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
"#;
        let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let api = GraphqlApi::new("https://api.test/graphql");
        let vista = api.vista_factory().build_from_spec(spec).unwrap();
        assert_eq!(vista.name(), "rockets");
        assert_eq!(vista.get_id_column(), Some("id"));
    }

    fn expect_err(result: Result<Vista>, needle: &str) {
        match result {
            Ok(_) => panic!("expected error containing {:?}", needle),
            Err(e) => assert!(
                e.to_string().contains(needle),
                "error {:?} did not contain {:?}",
                e.to_string(),
                needle
            ),
        }
    }

    #[test]
    fn build_from_yaml_spec_rejects_unknown_type() {
        let yaml = r#"
name: things
columns:
  id:
    type: string
    flags: [id]
  weird:
    type: complex
"#;
        let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let api = GraphqlApi::new("https://api.test/graphql");
        expect_err(api.vista_factory().build_from_spec(spec), "Unknown YAML column type");
    }

    #[test]
    fn build_from_yaml_spec_rejects_missing_id_column() {
        // id_column points at a column that isn't in `columns:`.
        let yaml = r#"
name: things
id_column: missing
columns:
  name:
    type: string
"#;
        let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let api = GraphqlApi::new("https://api.test/graphql");
        expect_err(api.vista_factory().build_from_spec(spec), "id column not present");
    }

    #[test]
    fn build_from_yaml_spec_rejects_unknown_dialect() {
        let yaml = r#"
name: things
columns:
  id:
    type: string
    flags: [id]
graphql:
  dialect: relay
"#;
        let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let api = GraphqlApi::new("https://api.test/graphql");
        expect_err(api.vista_factory().build_from_spec(spec), "Unknown filter dialect");
    }
}
