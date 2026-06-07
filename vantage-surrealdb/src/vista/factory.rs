//! `SurrealVistaFactory` — typed-table and YAML entry points, plus the
//! `VistaFactory` trait impl. SurrealDB advertises full read/write/count.

use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    Column as VistaColumn, ReferenceKind, Vista, VistaCapabilities, VistaFactory, VistaMetadata,
    flags as vista_flags, reference::Reference as VistaReferenceMeta,
};

use crate::surrealdb::SurrealDB;
use crate::thing::Thing;
use crate::types::AnySurrealType;
use crate::vista::source::SurrealTableShell;
use crate::vista::spec::{
    SurrealColumnExtras, SurrealReferenceExtras, SurrealTableExtras, SurrealVistaSpec,
};

/// Resolves a YAML spec by table name. The factory hands clones of this
/// into each `with_one` / `with_many` closure so child tables can be
/// rebuilt from the live spec at traversal time.
pub type SurrealSpecResolver = Arc<dyn Fn(&str) -> Option<SurrealVistaSpec> + Send + Sync>;

pub struct SurrealVistaFactory {
    db: SurrealDB,
    resolver: Option<SurrealSpecResolver>,
}

impl SurrealVistaFactory {
    pub fn new(db: SurrealDB) -> Self {
        Self { db, resolver: None }
    }

    /// Attach a spec resolver. Required for YAML-declared references to
    /// resolve their target tables — without it, traversals yield a
    /// column-less target Table and the next query fails loudly.
    pub fn with_resolver(mut self, resolver: SurrealSpecResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Wrap a typed table as a Vista. Column metadata is harvested from the
    /// table; CRUD goes through `Table`'s reading path. The original entity
    /// type is preserved so `with_expression` closures remain typecheckable.
    pub fn from_table<E>(&self, table: Table<SurrealDB, E>) -> Result<Vista>
    where
        E: Entity<AnySurrealType> + 'static,
    {
        let name = table.table_name().to_string();
        let metadata = metadata_from_table(&table);
        Ok(self.wrap(table, name, metadata, false))
    }

    /// Single source-construction site shared by `from_table` and
    /// `build_from_spec`. Keeps the capability set and `Vista::new` call in one
    /// place. A query-sourced (e.g. `rhai:` or `base:`) vista is `read_only`,
    /// which clears the write capabilities.
    fn wrap<E>(
        &self,
        table: Table<SurrealDB, E>,
        name: String,
        metadata: VistaMetadata,
        read_only: bool,
    ) -> Vista
    where
        E: Entity<AnySurrealType> + 'static,
    {
        let source = SurrealTableShell::new(
            table,
            VistaCapabilities {
                can_count: true,
                can_insert: !read_only,
                can_update: !read_only,
                can_delete: !read_only,
                can_order: true,
                can_search: true,
                can_set_page_size: true,
                can_fetch_page: true,
                can_fetch_next: true,
                can_traverse_to_record: true,
                can_traverse_to_set: true,
                // Per-reference scripted traversal only works when the script
                // engine is compiled in; without `rhai`, any `build_script` is
                // ignored and the FK eq-condition path still serves.
                can_build_ref_via_script: cfg!(feature = "rhai"),
                ..VistaCapabilities::default()
            },
            metadata,
            self.resolver.clone(),
        );
        Vista::new(name, Box::new(source))
    }

    /// Build a `Table<SurrealDB, EmptyEntity>` from a spec.
    pub fn table_from_spec(
        &self,
        spec: &SurrealVistaSpec,
    ) -> Result<Table<SurrealDB, EmptyEntity>> {
        build_surreal_table(spec, self.db.clone(), self.resolver.clone())
    }
}

impl VistaFactory for SurrealVistaFactory {
    type TableExtras = SurrealTableExtras;
    type ColumnExtras = SurrealColumnExtras;
    type ReferenceExtras = SurrealReferenceExtras;

    fn build_from_spec(&self, spec: SurrealVistaSpec) -> Result<Vista> {
        let vista_name = spec.name.clone();
        let read_only = spec
            .driver
            .surreal
            .as_ref()
            .is_some_and(|m| m.rhai.is_some() || m.base.is_some());
        let table = self.table_from_spec(&spec)?;
        let mut metadata = metadata_from_table(&table);
        // YAML `references:` carry the target table name explicitly; fold them
        // in here rather than via `table.vista_references()`, whose target is
        // the (erased) entity type name, not the table name.
        for (rel_name, ref_spec) in &spec.references {
            let fk = ref_spec
                .foreign_key
                .clone()
                .unwrap_or_else(|| rel_name.clone());
            let mut reference = VistaReferenceMeta::new(
                rel_name.clone(),
                ref_spec.table.clone(),
                ref_spec.kind,
                fk,
            );
            if let Some(script) = ref_spec
                .driver
                .surreal
                .as_ref()
                .and_then(|b| b.rhai.clone())
            {
                reference = reference.with_build_script(script);
            }
            metadata = metadata.with_reference(reference);
        }
        let mut vista = self.wrap(table, vista_name.clone(), metadata, read_only);
        vista.set_name(vista_name);

        // Final step: a `surreal: { modify }` script tweaks the just-built vista
        // (e.g. an expression condition YAML can't express). Runs last, so it
        // composes with `table`/`rhai`/`base`.
        #[cfg(feature = "rhai")]
        if let Some(code) = spec.driver.surreal.as_ref().and_then(|b| b.modify.clone()) {
            vista = self.apply_modify(vista, &code)?;
        }

        Ok(vista)
    }
}

#[cfg(feature = "rhai")]
impl SurrealVistaFactory {
    /// Run a `surreal: { modify }` script against an already-built vista,
    /// layering SurrealDB's expression vocabulary plus the conventional verbs
    /// onto a fresh engine. `table(name)` inside the script resolves through the
    /// factory's spec resolver (if attached); `self` is the built vista.
    fn apply_modify(&self, vista: Vista, code: &str) -> Result<Vista> {
        let db = self.db.clone();
        let resolver = self.resolver.clone();
        let target_resolver: vantage_vista::TargetResolver = Arc::new(move |name| {
            let resolver = resolver
                .as_ref()
                .ok_or_else(|| error!("modify script `table()` requires a spec resolver"))?;
            let spec = resolver(name)
                .ok_or_else(|| error!("modify script: unknown table", table = name))?;
            SurrealVistaFactory::new(db.clone())
                .with_resolver(resolver.clone())
                .build_from_spec(spec)
        });

        // Vendor vocab first, conventional second (so `table` resolves a Vista,
        // not SurrealDB's `ident` alias — same ordering as scripted traversal).
        let mut engine = rhai::Engine::new();
        vista.source.register_rhai_extensions(&mut engine);
        vantage_vista::register_conventional_onto(&mut engine, target_resolver);
        vantage_vista::eval_modify_script(&engine, code, vista)
    }
}

/// Build a `Table<SurrealDB, EmptyEntity>` from a spec, registering each
/// `references:` entry as a typed `with_one` / `with_many` on the parent.
///
/// Each reference closure captures a clone of the resolver `Arc` and the
/// target table name; at traversal time it asks the resolver for the
/// target's current spec and rebuilds the child table. On a resolver miss,
/// the closure falls back to an empty `Table::new(target_name, db)` — the
/// next query then fails loudly when it discovers no columns are defined.
pub(crate) fn build_surreal_table(
    spec: &SurrealVistaSpec,
    db: SurrealDB,
    resolver: Option<SurrealSpecResolver>,
) -> Result<Table<SurrealDB, EmptyEntity>> {
    let block = spec.driver.surreal.as_ref();

    if let Some(base_name) = block.and_then(|m| m.base.clone()) {
        return build_derived_table(spec, &base_name, db, resolver);
    }

    let mut table = match block.and_then(|m| m.rhai.clone()) {
        Some(code) => table_from_rhai(spec, &code, db.clone())?,
        None => {
            let table_name = block
                .and_then(|m| m.table.clone())
                .unwrap_or_else(|| spec.name.clone());
            Table::<SurrealDB, EmptyEntity>::new(table_name, db.clone())
        }
    };

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

    for (rel_name, ref_spec) in &spec.references {
        let target_name = ref_spec.table.clone();
        let fk = ref_spec
            .foreign_key
            .clone()
            .unwrap_or_else(|| rel_name.clone());
        let resolver_clone = resolver.clone();

        let build_child = move |db: SurrealDB| -> Table<SurrealDB, EmptyEntity> {
            if let Some(r) = &resolver_clone
                && let Some(child_spec) = r(&target_name)
                && let Ok(child) = build_surreal_table(&child_spec, db.clone(), Some(r.clone()))
            {
                return child;
            }
            Table::<SurrealDB, EmptyEntity>::new(target_name.clone(), db)
        };

        table = match ref_spec.kind {
            ReferenceKind::HasOne => table.with_one::<EmptyEntity>(rel_name, &fk, build_child),
            ReferenceKind::HasMany => table.with_many::<EmptyEntity>(rel_name, &fk, build_child),
        };
    }

    let table = table.with_contained_specs(&spec.contained, build_column)?;
    Ok(table)
}

/// Build a query-sourced table from a `rhai:` script.
#[cfg(feature = "rhai")]
fn table_from_rhai(
    spec: &SurrealVistaSpec,
    code: &str,
    db: SurrealDB,
) -> Result<Table<SurrealDB, EmptyEntity>> {
    let select = crate::vista::rhai_source::eval_to_select(code, None)?;
    Ok(Table::from_select(db, spec.name.clone(), select))
}

#[cfg(not(feature = "rhai"))]
fn table_from_rhai(
    _spec: &SurrealVistaSpec,
    _code: &str,
    _db: SurrealDB,
) -> Result<Table<SurrealDB, EmptyEntity>> {
    Err(error!(
        "vista declares a `rhai:` source but vantage-surrealdb was built without the `rhai` feature"
    ))
}

/// Build a derived table: resolve `base_name` eagerly via the resolver, build
/// the base table, optionally transform its `select()` through a `rhai:` script
/// (transform mode — `base` is seeded into the engine scope), and inherit the
/// listed columns/relations via [`Table::derive_from`]. The derived vista's own
/// `columns:` (e.g. aggregate outputs) are added on top.
fn build_derived_table(
    spec: &SurrealVistaSpec,
    base_name: &str,
    db: SurrealDB,
    resolver: Option<SurrealSpecResolver>,
) -> Result<Table<SurrealDB, EmptyEntity>> {
    let resolver = resolver.ok_or_else(|| {
        error!(
            "vista declares `base:` but no spec resolver is attached to the factory",
            base = base_name
        )
    })?;
    let base_spec = resolver(base_name)
        .ok_or_else(|| error!("base vista not found via resolver", base = base_name))?;
    let base_table = build_surreal_table(&base_spec, db.clone(), Some(resolver.clone()))?;

    let block = spec.driver.surreal.as_ref();
    let transformed = match block.and_then(|m| m.rhai.clone()) {
        Some(code) => eval_transform(&code, base_table.select())?,
        None => base_table.select(),
    };

    let inherit = block.and_then(|m| m.inherit.clone()).unwrap_or_default();
    let cols: Vec<&str> = inherit.columns.iter().map(String::as_str).collect();
    let rels: Vec<&str> = inherit.relations.iter().map(String::as_str).collect();

    let mut table = Table::derive_from(
        &base_table,
        spec.name.clone(),
        move |_| transformed,
        &cols,
        &rels,
    );

    // The derived vista's own declared columns (e.g. aggregate outputs).
    for (name, col_spec) in &spec.columns {
        if !table.columns().contains_key(name) {
            table.add_column(build_column(name, col_spec)?);
        }
        if col_spec.flags.iter().any(|f| f == vista_flags::TITLE) {
            table.add_title_field(name);
        }
    }

    // Explicit id override; otherwise the id inherited from the base stands.
    if let Some(id) = &spec.id_column {
        table.set_id_field(id);
    }

    let table = table.with_contained_specs(&spec.contained, build_column)?;
    Ok(table)
}

/// Apply a `rhai:` transform to a base select. Feature-gated like
/// [`table_from_rhai`].
#[cfg(feature = "rhai")]
fn eval_transform(
    code: &str,
    base: crate::statements::SurrealSelect,
) -> Result<crate::statements::SurrealSelect> {
    crate::vista::rhai_source::eval_to_select(code, Some(base))
}

#[cfg(not(feature = "rhai"))]
fn eval_transform(
    _code: &str,
    _base: crate::statements::SurrealSelect,
) -> Result<crate::statements::SurrealSelect> {
    Err(error!(
        "vista declares a `rhai:` transform but vantage-surrealdb was built without the `rhai` feature"
    ))
}

pub(crate) fn resolve_id_column(spec: &SurrealVistaSpec) -> String {
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
    col_spec: &vantage_vista::ColumnSpec<SurrealColumnExtras>,
) -> Result<TableColumn<AnySurrealType>> {
    let ty = col_spec.col_type.as_deref().unwrap_or("string");
    let alias = col_spec
        .driver
        .surreal
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

/// YAML type alias → typed `Column` (then erased to `Column<AnySurrealType>`).
pub(crate) fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<AnySurrealType>> {
    let col: TableColumn<AnySurrealType> = match ty {
        "int" | "integer" | "i64" | "i32" => {
            TableColumn::from_column(TableColumn::<i64>::new(name))
        }
        "float" | "double" | "f64" | "f32" => {
            TableColumn::from_column(TableColumn::<f64>::new(name))
        }
        "bool" | "boolean" => TableColumn::from_column(TableColumn::<bool>::new(name)),
        "string" | "text" | "str" => TableColumn::from_column(TableColumn::<String>::new(name)),
        "thing" | "record" | "record_id" => {
            TableColumn::from_column(TableColumn::<Thing>::new(name))
        }
        "datetime" => {
            TableColumn::from_column(TableColumn::<chrono::DateTime<chrono::Utc>>::new(name))
        }
        #[cfg(feature = "decimal")]
        "decimal" | "numeric" => {
            TableColumn::from_column(TableColumn::<rust_decimal::Decimal>::new(name))
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
        // SurrealDB sorts on any field; flag every column ORDERABLE.
        let mut vc = VistaColumn::new(name.clone(), col.get_type().to_string())
            .with_flag(vista_flags::ORDERABLE);
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
    for spec in table.vista_contained() {
        metadata = metadata.with_contained(spec);
    }
    metadata
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use surreal_client::{MockSurrealEngine, SurrealClient};
    use vantage_vista::VistaFactory;

    fn test_db() -> SurrealDB {
        let client = SurrealClient::new(
            Box::new(MockSurrealEngine::new()),
            Some("test".into()),
            Some("test".into()),
        );
        SurrealDB::new(client)
    }

    fn parse(yaml: &str) -> SurrealVistaSpec {
        serde_yaml_ng::from_str(yaml).expect("yaml parse")
    }

    fn registry_resolver(specs: Vec<(String, SurrealVistaSpec)>) -> SurrealSpecResolver {
        let map: IndexMap<String, SurrealVistaSpec> = specs.into_iter().collect();
        let map = Arc::new(map);
        Arc::new(move |name: &str| map.get(name).cloned())
    }

    #[test]
    fn build_from_spec_surfaces_references_in_metadata() {
        let yaml = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
  primary_product:
    table: product
    kind: has_one
    foreign_key: primary_product
"#;
        let spec = parse(yaml);
        let factory = SurrealVistaFactory::new(test_db());
        let vista = factory.build_from_spec(spec).expect("build");

        let mut listed = vista.get_references();
        listed.sort();
        assert_eq!(listed, vec!["primary_product", "products"]);

        let products = vista.get_reference("products").expect("products ref");
        assert_eq!(products.target, "product");
        assert_eq!(products.kind, ReferenceKind::HasMany);
        assert_eq!(products.foreign_key, "bakery");

        let primary = vista.get_reference("primary_product").expect("primary ref");
        assert_eq!(primary.kind, ReferenceKind::HasOne);
    }

    #[test]
    fn resolver_supplies_child_columns_on_traversal() {
        let bakery_yaml = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
"#;
        let product_yaml = r#"
name: product
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
  price: { type: int }
  bakery: { type: thing }
"#;
        let bakery_spec = parse(bakery_yaml);
        let product_spec = parse(product_yaml);

        let resolver = registry_resolver(vec![
            ("bakery".into(), bakery_spec.clone()),
            ("product".into(), product_spec.clone()),
        ]);

        let factory = SurrealVistaFactory::new(test_db()).with_resolver(resolver);
        let bakery = factory.build_from_spec(bakery_spec).expect("build bakery");

        let mut row: vantage_types::Record<ciborium::Value> = vantage_types::Record::new();
        row.insert(
            "id".into(),
            ciborium::Value::Text("bakery:hill_valley".into()),
        );

        let child = bakery.get_ref("products", &row).expect("traverse products");

        let mut cols = child.get_column_names();
        cols.sort();
        assert_eq!(cols, vec!["bakery", "id", "name", "price"]);
        assert_eq!(child.get_id_column(), Some("id"));
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn scripted_reference_builds_target_via_rhai() {
        let bakery_yaml = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
    surreal:
      rhai: |
        table("product").add_condition_eq("bakery", row.id)
"#;
        let product_yaml = r#"
name: product
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
  price: { type: int }
  bakery: { type: thing }
"#;
        let bakery_spec = parse(bakery_yaml);
        let product_spec = parse(product_yaml);
        let resolver = registry_resolver(vec![
            ("bakery".into(), bakery_spec.clone()),
            ("product".into(), product_spec),
        ]);

        let factory = SurrealVistaFactory::new(test_db()).with_resolver(resolver);
        let bakery = factory.build_from_spec(bakery_spec).expect("build bakery");

        // The build_script rode through the extras slot onto the Reference, and
        // the backend advertises the scripted-traversal capability.
        assert!(bakery.capabilities().can_build_ref_via_script);
        let reference = bakery.get_reference("products").expect("products ref");
        assert!(reference.build_script.is_some());

        // Traversal evaluates the script: `table("product")` resolves a fresh
        // product Vista (proving the script path fired, not the FK path).
        let mut row: vantage_types::Record<ciborium::Value> = vantage_types::Record::new();
        row.insert(
            "id".into(),
            ciborium::Value::Text("bakery:hill_valley".into()),
        );
        let child = bakery
            .get_ref("products", &row)
            .expect("scripted traverse products");

        let mut cols = child.get_column_names();
        cols.sort();
        assert_eq!(cols, vec!["bakery", "id", "name", "price"]);
        assert_eq!(child.name(), "product");
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn scripted_reference_routes_vendor_condition() {
        // `with_condition(<surreal expr>)` boxes an `Expression<AnySurrealType>`
        // and routes it through the type-erased `add_raw_condition`. A clean eval
        // proves the boxed type and the downcast type match (a mismatch would
        // surface as an `Unimplemented` error here).
        let bakery_yaml = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
    surreal:
      rhai: |
        table("product").with_condition(ident("bakery") == row.id)
"#;
        let product_yaml = r#"
name: product
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
  bakery: { type: thing }
"#;
        let bakery_spec = parse(bakery_yaml);
        let resolver = registry_resolver(vec![
            ("bakery".into(), bakery_spec.clone()),
            ("product".into(), parse(product_yaml)),
        ]);
        let factory = SurrealVistaFactory::new(test_db()).with_resolver(resolver);
        let bakery = factory.build_from_spec(bakery_spec).expect("build bakery");

        let mut row: vantage_types::Record<ciborium::Value> = vantage_types::Record::new();
        row.insert(
            "id".into(),
            ciborium::Value::Text("bakery:hill_valley".into()),
        );
        let child = bakery
            .get_ref("products", &row)
            .expect("vendor-condition traverse");
        assert_eq!(child.name(), "product");
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn modify_script_tweaks_built_vista_with_vendor_condition() {
        // The YAML builds a normal, writable `client` table; the `modify` script
        // then narrows it with an expression condition YAML can't express. A
        // clean build proves the post-build hook ran and the vendor condition
        // routed through `add_raw_condition`.
        let yaml = r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
  is_paying_client: { type: bool }
surreal:
  table: clients
  modify: |
    self.with_condition(ident("is_paying_client") == true)
       .add_order("name", "asc")
"#;
        let spec = parse(yaml);
        let factory = SurrealVistaFactory::new(test_db());
        let vista = factory.build_from_spec(spec).expect("build + modify");

        // The base table stays writable (a condition narrows, it doesn't make
        // the vista read-only), and the schema is intact.
        assert!(vista.capabilities().can_insert);
        let mut cols = vista.get_column_names();
        cols.sort();
        assert_eq!(cols, vec!["id", "is_paying_client", "name"]);
    }

    #[test]
    fn resolver_miss_falls_back_to_empty_child_table() {
        let bakery_yaml = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
references:
  ghosts:
    table: missing_table
    kind: has_many
    foreign_key: bakery
"#;
        let bakery_spec = parse(bakery_yaml);
        let resolver = registry_resolver(vec![]);
        let factory = SurrealVistaFactory::new(test_db()).with_resolver(resolver);
        let bakery = factory.build_from_spec(bakery_spec).expect("build");

        let mut row: vantage_types::Record<ciborium::Value> = vantage_types::Record::new();
        row.insert(
            "id".into(),
            ciborium::Value::Text("bakery:hill_valley".into()),
        );

        let child = bakery.get_ref("ghosts", &row).expect("fallback child");
        assert!(child.get_column_names().is_empty());
    }
}
