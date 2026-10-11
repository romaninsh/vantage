//! `GraphqlApiVistaFactory` — typed-table entry point, `VistaFactory`
//! trait impl, and the YAML pipeline (`build_from_spec`, `register_yaml`,
//! `with_resolver`) for the GraphQL adapter.
//!
//! Mutations differ from one schema to the next, so the factory
//! advertises reads only: count always, and ordering, search and
//! paging per what the table's root field accepts. Metadata is harvested from the
//! typed table before erasure so column types, id field, title fields,
//! and relations all reach the Vista layer with full fidelity.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

use vantage_core::{Result, error};
use vantage_table::table::{Table, VistaMetadataOptions};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    Reference as VistaReference, Vista, VistaCapabilities, VistaFactory, VistaMetadata, flags,
};

use crate::graphql::api::GraphqlApi;
use crate::graphql::types::AnyGraphqlType;
use crate::graphql::vista::lowering::{
    GraphqlSpecResolver, TargetResolver, build_table, target_resolver,
};
use crate::graphql::vista::source::GraphqlApiTableShell;
use crate::graphql::vista::spec::{GraphqlApiVistaSpec, GraphqlColumnExtras, GraphqlTableExtras};

type SpecRegistry = Arc<RwLock<HashMap<String, GraphqlApiVistaSpec>>>;

/// Builds `Vista`s over one [`GraphqlApi`]: from typed tables
/// ([`from_table`](Self::from_table)), from YAML specs
/// ([`build_from_spec`](VistaFactory::build_from_spec)), or from specs
/// registered by name ([`register_yaml`](Self::register_yaml) +
/// [`build`](Self::build)).
pub struct GraphqlApiVistaFactory {
    api: GraphqlApi,
    specs: SpecRegistry,
    resolver: Option<GraphqlSpecResolver>,
}

impl GraphqlApiVistaFactory {
    /// A factory over `api` with an empty spec registry and no resolver.
    pub fn new(api: GraphqlApi) -> Self {
        Self {
            api,
            specs: Arc::new(RwLock::new(HashMap::new())),
            resolver: None,
        }
    }

    /// The data source every Vista from this factory reads through.
    pub fn api(&self) -> &GraphqlApi {
        &self.api
    }

    /// Resolve YAML reference targets through `resolver` instead of the
    /// internal registry, for GraphQL specs kept elsewhere (an app-level
    /// inventory, say). Each spec it returns is lowered onto this factory's
    /// `GraphqlApi`, so every target is queried on the same endpoint; it
    /// cannot point a reference at another driver. A target it does not know
    /// fails the traversal.
    pub fn with_resolver(mut self, resolver: GraphqlSpecResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Add a YAML spec to the factory's internal registry. Without an
    /// explicit resolver, references resolve against this registry, so every
    /// model lives on this factory's `GraphqlApi` and is built on first
    /// traversal.
    pub fn register_yaml(&mut self, yaml: &str) -> Result<()> {
        let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).map_err(|e| {
            error!(
                "Failed to parse GraphqlApiVistaSpec YAML",
                detail = e.to_string()
            )
        })?;
        self.specs
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(spec.name.clone(), spec);
        Ok(())
    }

    /// Build the Vista for a model registered with
    /// [`register_yaml`](Self::register_yaml).
    pub fn build(&self, name: &str) -> Result<Vista> {
        let spec = registry_get(&self.specs, name)
            .ok_or_else(|| error!("No registered spec for model name", name = name))?;
        self.build_from_spec(spec)
    }

    /// The installed resolver, else one over the internal registry; either
    /// way paired with this factory's API (see `lowering`).
    fn target_resolver(&self) -> TargetResolver {
        let specs: GraphqlSpecResolver = match &self.resolver {
            Some(resolver) => resolver.clone(),
            None => {
                let registry = self.specs.clone();
                Arc::new(move |name: &str| registry_get(&registry, name))
            }
        };
        target_resolver(self.api.clone(), specs)
    }

    /// Wrap a typed `Table<GraphqlApi, E>` as a `Vista`. Column metadata,
    /// id field, title fields, and references are harvested up front;
    /// the table is erased to `Table<GraphqlApi, EmptyEntity>` so the
    /// shell carries a uniform entity type while still routing
    /// reference traversal through `Reference::resolve_from_row`.
    pub fn from_table<E>(&self, table: Table<GraphqlApi, E>) -> Result<Vista>
    where
        E: Entity<AnyGraphqlType> + 'static,
    {
        let name = table.table_name().to_string();
        let metadata = metadata_from_table(&table);
        let table = table.into_entity::<EmptyEntity>();
        Ok(wrap(table, name, metadata, None))
    }

    /// Build a `Table<GraphqlApi, EmptyEntity>` from a YAML spec. Per-table
    /// dialect / filter-arg / root-field overrides land on the table's
    /// `GraphqlApi` clone; `references:` rebuild their targets through the
    /// resolver (or the registry) at traversal time.
    ///
    /// Wrapping this table with [`from_table`](Self::from_table) gives its
    /// references placeholder metadata (no target or foreign key) and
    /// unchecked targets; [`build_from_spec`](VistaFactory::build_from_spec)
    /// fills both in and checks each target when traversed.
    ///
    /// The returned table's `GraphqlApi` carries this spec's per-table
    /// overrides, column field aliases included; build other tables from
    /// [`api`](Self::api), not from a clone of this one.
    pub fn table_from_spec(
        &self,
        spec: &GraphqlApiVistaSpec,
    ) -> Result<Table<GraphqlApi, EmptyEntity>> {
        build_table(spec, &self.api, Some(self.target_resolver()))
    }
}

impl VistaFactory for GraphqlApiVistaFactory {
    type TableExtras = GraphqlTableExtras;
    type ColumnExtras = GraphqlColumnExtras;
    type ReferenceExtras = vantage_vista::NoExtras;

    fn build_from_spec(&self, spec: GraphqlApiVistaSpec) -> Result<Vista> {
        let table = self.table_from_spec(&spec)?;
        Ok(wrap_spec_table(table, &spec, self.target_resolver()))
    }
}

fn registry_get(specs: &SpecRegistry, name: &str) -> Option<GraphqlApiVistaSpec> {
    specs
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(name)
        .cloned()
}

/// Wrap a table lowered from `spec` as a Vista named after the spec. Its
/// metadata carries the spec's references with their real targets and
/// foreign keys, and its shell traverses them through `resolver`.
pub(crate) fn wrap_spec_table(
    table: Table<GraphqlApi, EmptyEntity>,
    spec: &GraphqlApiVistaSpec,
    resolver: TargetResolver,
) -> Vista {
    let mut metadata = metadata_from_table(&table);
    for (relation, reference) in &spec.references {
        metadata = metadata.with_reference(VistaReference::new(
            relation.clone(),
            reference.table.clone(),
            reference.kind,
            reference.foreign_key_or(relation),
        ));
    }
    wrap(table, spec.name.clone(), metadata, Some(resolver))
}

/// The one place a GraphQL shell is constructed. Capabilities follow what
/// the table's root field accepts, so a consumer either pushes the work to
/// the server or does it locally.
fn wrap(
    table: Table<GraphqlApi, EmptyEntity>,
    name: String,
    metadata: VistaMetadata,
    resolver: Option<TargetResolver>,
) -> Vista {
    let api = table.data_source().clone();
    // Offset paging is all-or-nothing: each of these is `limit:`/`offset:`
    // on a cloned table, so they stand or fall together.
    let paginate = api.can_page();
    let source = GraphqlApiTableShell::new(
        table,
        VistaCapabilities {
            can_count: true,
            can_traverse_to_record: true,
            can_order: api.can_order(),
            can_search: api.can_search(),
            can_filter_operators: api.can_filter_operators(),
            can_set_page_size: paginate,
            can_fetch_page: paginate,
            can_fetch_window: paginate,
            can_fetch_next: paginate,
            ..VistaCapabilities::default()
        },
        metadata,
    )
    .with_resolver(resolver);
    Vista::new(name, Box::new(source))
}

fn metadata_from_table<E>(table: &Table<GraphqlApi, E>) -> VistaMetadata
where
    E: Entity<AnyGraphqlType> + 'static,
{
    let mut metadata = table.vista_metadata(VistaMetadataOptions {
        id_flag: true,
        column_flags: true,
        ..VistaMetadataOptions::default()
    });
    // Any column the server returns can be ordered by, except a dotted path:
    // Hasura spells that as a nested `order_by` object, which isn't rendered.
    if table.data_source().can_order() {
        for (name, column) in metadata.columns.iter_mut() {
            if !name.contains('.') && !column.is_computed() {
                column.flags.push(flags::ORDERABLE.to_string());
            }
        }
    }
    // Cardinality comes from the typed table's reference registry
    // (`ref_kinds`). Target name + foreign-key field stay placeholders
    // because the typed-table `Reference` only exposes the build closure,
    // not the target's name; a spec-built Vista overwrites them from the
    // spec (`wrap_spec_table`).
    for (relation, kind) in table.ref_kinds() {
        metadata = metadata.with_reference(VistaReference::new(relation, "", kind, ""));
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
        let table: Table<GraphqlApi, EmptyEntity> =
            Table::new("launches", api.clone()).with_id_column("id");
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
        expect_err(
            api.vista_factory().build_from_spec(spec),
            "Unknown YAML column type",
        );
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
        expect_err(
            api.vista_factory().build_from_spec(spec),
            "id column not present",
        );
    }

    #[test]
    fn build_from_yaml_spec_rejects_multi_key_references() {
        let yaml = r#"
name: deployments
columns:
  id:
    type: string
    flags: [id]
references:
  release:
    table: releases
    keys:
      - { to: product_id, from: product_id }
      - { to: version_id, from: version_id }
"#;
        let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let api = GraphqlApi::new("https://api.test/graphql");
        expect_err(
            api.vista_factory().build_from_spec(spec),
            "multi-column `keys:` are not supported",
        );
    }

    #[test]
    fn yaml_spec_with_unknown_dialect_fails_to_parse() {
        let yaml = r#"
name: things
columns:
  id:
    type: string
    flags: [id]
graphql:
  dialect: relay
"#;
        let err = serde_yaml_ng::from_str::<GraphqlApiVistaSpec>(yaml)
            .unwrap_err()
            .to_string();
        assert!(err.contains("relay"), "{err}");
    }

    #[test]
    fn column_field_alias_must_be_a_plain_name() {
        let yaml = r#"
name: runs
columns:
  id:
    type: string
    flags: [id]
  run_id:
    type: string
    graphql:
      field: run.id
"#;
        let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let api = GraphqlApi::new("https://api.test/graphql");
        expect_err(
            api.vista_factory().build_from_spec(spec),
            "dotted paths are not supported",
        );

        let yaml = yaml.replace("run.id", "\"run id\"");
        let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(&yaml).unwrap();
        expect_err(
            api.vista_factory().build_from_spec(spec),
            "GraphQL name is not valid",
        );
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;
    use crate::graphql::api::GraphqlApi;

    fn vista_from(yaml: &str) -> vantage_vista::Vista {
        let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
        GraphqlApi::new("https://api.test/graphql")
            .vista_factory()
            .build_from_spec(spec)
            .unwrap()
    }

    /// A generic-dialect table can't spell `order_by` or `_ilike`, so it
    /// must say so and let the consumer sort and search locally.
    #[test]
    fn generic_dialect_advertises_client_side_sort_and_search() {
        let vista = vista_from(
            "name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\ngraphql:\n  dialect: generic\n",
        );
        let caps = vista.capabilities();
        assert!(!caps.can_order);
        assert!(!caps.can_search);
        assert!(!caps.can_filter_operators);
    }

    /// Hasura can, so the same table on that dialect pushes the work down.
    #[test]
    fn hasura_dialect_advertises_server_side_sort_and_search() {
        let vista = vista_from(
            "name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\ngraphql:\n  dialect: hasura\n",
        );
        let caps = vista.capabilities();
        assert!(caps.can_order);
        assert!(caps.can_search);
        assert!(caps.can_filter_operators);
    }

    /// `filter: false` is for a root field that takes no arguments at all;
    /// nothing derived from a condition may be advertised — paging
    /// included, since local filtering would cut server pages short.
    #[test]
    fn filter_false_disables_everything_condition_shaped() {
        let vista = vista_from(
            "name: stacks\ncolumns:\n  id:\n    type: string\n    flags: [id]\ngraphql:\n  dialect: hasura\n  filter: false\n  paginate: true\n",
        );
        let caps = vista.capabilities();
        assert!(!caps.can_search);
        assert!(!caps.can_filter_operators);
        assert!(!caps.can_fetch_window);
        assert!(!caps.can_set_page_size);
    }

    /// The YAML `searchable` flag reaches the Vista column.
    #[test]
    fn searchable_flag_carries_over() {
        let vista = vista_from(
            "name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\n  details:\n    type: string\n    flags: [searchable]\n",
        );
        let col = vista.get_column("details").unwrap();
        assert!(col.has_flag(vantage_vista::flags::SEARCHABLE));
    }
}
