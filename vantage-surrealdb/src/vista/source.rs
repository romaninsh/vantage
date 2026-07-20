//! `SurrealTableShell` — owns the typed `Table<SurrealDB, E>` and exposes it
//! through the `TableShell` boundary.
//!
//! The shell is generic in `E` so `with_expression` closures (parameterized
//! over `E`) survive the wrap; `Vista` erases `E` once at the
//! `Box<dyn TableShell>` boundary.
//!
//! Vista exposes ids as `String`. SurrealDB's native id is `Thing`
//! (`table:id`). The shell stringifies via `Thing::to_string()` on the way
//! out and parses back via `String::contains(':')` on the way in — bare ids
//! get prefixed with the wrapped table's name.
//!
//! `AnySurrealType` already wraps `ciborium::Value`, so the value boundary
//! is a straight unwrap/rewrap.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use surreal_client::Action;
use vantage_core::{Result, error};
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_table::conditions::ConditionHandle;
use vantage_table::pagination::Pagination;
use vantage_table::sorting::{OrderBy, SortDirection as TableSortDirection};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Entity, Record};
use vantage_vista::{
    Column as VistaColumn, ContainedSpec, Reference as VistaReference, SortDirection, TableShell,
    Vista, VistaCapabilities, VistaChange, VistaChangeStream, VistaMetadata,
};

use crate::identifier::Identifier;
use crate::operation::SurrealOperation;
use crate::surreal_expr;
use crate::surrealdb::SurrealDB;
use crate::thing::Thing;
use crate::types::{AnySurrealType, SurrealType};
use crate::vista::factory::{SurrealSpecResolver, SurrealVistaFactory};

pub struct SurrealTableShell<E = EmptyEntity>
where
    E: Entity<AnySurrealType>,
{
    pub(crate) table: Table<SurrealDB, E>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    pub(crate) current_search_handle: Option<ConditionHandle>,
    pub(crate) page_size: Option<usize>,
    pub(crate) resolver: Option<SurrealSpecResolver>,
}

impl<E> SurrealTableShell<E>
where
    E: Entity<AnySurrealType>,
{
    pub(crate) fn new(
        table: Table<SurrealDB, E>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
        resolver: Option<SurrealSpecResolver>,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
            current_search_handle: None,
            page_size: None,
            resolver,
        }
    }

    /// Resolve a Vista-side string id into a SurrealDB `Thing`. A `table:id`
    /// pair is parsed verbatim; a bare id gets prefixed with the wrapped
    /// table's name so `vista.get_value("biff")` works the same as
    /// `vista.get_value("client:biff")`.
    fn parse_id(&self, id: &str) -> Thing {
        if id.contains(':') {
            id.parse::<Thing>()
                .unwrap_or_else(|_| Thing::new(self.table.table_name(), id))
        } else {
            Thing::new(self.table.table_name(), id)
        }
    }
}

fn to_cbor_record(record: Record<AnySurrealType>) -> Record<CborValue> {
    record
        .into_iter()
        .map(|(k, v)| (k, v.into_value()))
        .collect()
}

fn to_native_record(record: &Record<CborValue>) -> Record<AnySurrealType> {
    record
        .iter()
        .map(|(k, v)| (k.clone(), AnySurrealType::from(v.clone())))
        .collect()
}

#[async_trait]
impl<E> TableShell for SurrealTableShell<E>
where
    E: Entity<AnySurrealType> + 'static,
{
    fn columns(&self) -> &IndexMap<String, VistaColumn> {
        &self.metadata.columns
    }

    fn references(&self) -> &IndexMap<String, VistaReference> {
        &self.metadata.references
    }

    fn id_column(&self) -> Option<&str> {
        self.metadata.id_column.as_deref()
    }

    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        let raw = self.table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(thing, record)| (thing.to_string(), to_cbor_record(record)))
            .collect())
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        let thing = self.parse_id(id);
        let Some(record) = self.table.get_value(thing.clone()).await? else {
            return Ok(None);
        };
        Ok(Some(to_cbor_record(record)))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let Some((thing, record)) = self.table.get_some_value().await? else {
            return Ok(None);
        };
        Ok(Some((thing.to_string(), to_cbor_record(record))))
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        self.table.get_count().await
    }

    async fn insert_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let thing = self.parse_id(id);
        let inserted = self
            .table
            .insert_value(thing.clone(), &to_native_record(record))
            .await?;
        Ok(to_cbor_record(inserted))
    }

    async fn replace_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let thing = self.parse_id(id);
        let replaced = self
            .table
            .replace_value(thing.clone(), &to_native_record(record))
            .await?;
        Ok(to_cbor_record(replaced))
    }

    async fn patch_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let thing = self.parse_id(id);
        let patched = self
            .table
            .patch_value(thing.clone(), &to_native_record(partial))
            .await?;
        Ok(to_cbor_record(patched))
    }

    async fn delete_vista_value(&self, _vista: &Vista, id: &String) -> Result<()> {
        let thing = self.parse_id(id);
        self.table.delete(thing.clone()).await
    }

    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        self.table.delete_all().await
    }

    async fn insert_vista_return_id_value(
        &self,
        _vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String> {
        let thing = self
            .table
            .insert_return_id_value(&to_native_record(record))
            .await?;
        Ok(thing.to_string())
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        let column = self
            .table
            .columns()
            .get(field)
            .ok_or_else(|| error!("Unknown column for eq condition", field = field))?
            .clone();
        // Id-column comparisons coerce a string value into a Thing —
        // SurrealDB compares record ids by type, so a quoted string
        // (`id = "MG78-N25S"` or `id = "tag:MG78-N25S"`) never matches.
        // `parse_id` accepts both the bare-key and `table:key` forms.
        // This is what lets a cross-persistence relation narrow a surreal
        // table from a plain string held by another backend's row.
        let id_field = self.metadata.id_column.as_deref().unwrap_or("id");
        let surreal_value = match value {
            CborValue::Text(s) if field == id_field => {
                AnySurrealType::from(self.parse_id(s).to_cbor())
            }
            _ => AnySurrealType::from(value.clone()),
        };
        self.table.add_condition(column.eq(surreal_value));
        Ok(())
    }

    /// Route a boxed driver-native condition onto the wrapped table. The boxed
    /// value must be a SurrealDB [`crate::Expr`] (`Expression<AnySurrealType>`,
    /// the table's `Condition` type) — the type a Rhai `with_condition(...)`
    /// expression lowers to. Used by scripted reference traversal.
    fn add_raw_condition(&mut self, condition: Box<dyn std::any::Any + Send + Sync>) -> Result<()> {
        let condition = condition.downcast::<crate::Expr>().map_err(|_| {
            error!(
                "add_raw_condition expected a SurrealDB Expression<AnySurrealType>",
                source_type = std::any::type_name::<Self>()
            )
        })?;
        self.table.add_condition(*condition);
        Ok(())
    }

    fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        // A reference carrying a Rhai `build_script` resolves through the script
        // engine instead of the fixed FK eq-condition path below.
        #[cfg(feature = "rhai")]
        if let Some(script) = self
            .metadata
            .references
            .get(relation)
            .and_then(|r| r.build_script.clone())
        {
            return self.get_ref_via_script(&script, row);
        }

        let native_row = to_native_record(row);
        let target = self
            .table
            .get_ref_from_row::<EmptyEntity>(relation, &native_row)?;
        let mut factory = SurrealVistaFactory::new(self.table.data_source().clone());
        if let Some(resolver) = &self.resolver {
            factory = factory.with_resolver(resolver.clone());
        }
        factory.from_table(target)
    }

    fn contained(&self) -> &IndexMap<String, ContainedSpec> {
        &self.metadata.contained
    }

    /// Resolve a contained relation. SurrealDB stores the embedded object /
    /// array natively, so the host value passes through unchanged and writes
    /// `UPDATE … MERGE` it back. The shared `Table::get_contained_ref` does the
    /// rest; this shim supplies the `Thing` id and the factory wrap (which
    /// carries the cross-persistence resolver for traverse-out).
    fn get_contained_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        let id_field = self.metadata.id_column.as_deref().unwrap_or("id");
        let id_cbor = row
            .get(id_field)
            .ok_or_else(|| error!("contained traversal requires the parent row's id"))?;
        let thing = Thing::from_cbor(id_cbor.clone())
            .or_else(|| match id_cbor {
                CborValue::Text(s) => Some(self.parse_id(s)),
                _ => None,
            })
            .ok_or_else(|| error!("could not resolve parent id into a Thing"))?;

        let db = self.table.data_source().clone();
        let spec_resolver = self.resolver.clone();
        self.table.get_contained_ref(
            relation,
            row,
            thing,
            move |t| {
                let mut factory = crate::vista::factory::SurrealVistaFactory::new(db.clone());
                if let Some(r) = &spec_resolver {
                    factory = factory.with_resolver(r.clone());
                }
                factory.from_table(t)
            },
            |v| Some(v.clone()),
            |c| c,
        )
    }

    fn get_ref_kinds(&self) -> Vec<(String, vantage_vista::ReferenceKind)> {
        self.table.ref_kinds()
    }

    fn add_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for add_order", field = field));
        }
        self.table.clear_orders();
        let expr = surreal_expr!("{}", (Identifier::new(field)));
        let direction = match dir {
            SortDirection::Ascending => TableSortDirection::Ascending,
            SortDirection::Descending => TableSortDirection::Descending,
        };
        self.table.add_order(OrderBy {
            expression: expr,
            direction,
        });
        Ok(())
    }

    fn clear_orders(&mut self) -> Result<()> {
        self.table.clear_orders();
        Ok(())
    }

    fn add_search(&mut self, text: &str) -> Result<()> {
        if let Some(handle) = self.current_search_handle.take() {
            let _ = self.table.temp_remove_condition(handle);
        }
        let condition = self
            .table
            .data_source()
            .search_table_condition(&self.table, text);
        self.current_search_handle = Some(self.table.temp_add_condition(condition));
        Ok(())
    }

    fn clear_search(&mut self) -> Result<()> {
        if let Some(handle) = self.current_search_handle.take() {
            let _ = self.table.temp_remove_condition(handle);
        }
        Ok(())
    }

    fn set_page_size(&mut self, size: usize) -> Result<()> {
        if size == 0 {
            return Err(error!("page size must be > 0"));
        }
        self.page_size = Some(size);
        Ok(())
    }

    async fn fetch_page(
        &self,
        _vista: &Vista,
        page: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        if page == 0 {
            return Err(error!("page is 1-based; got 0"));
        }
        let size = self
            .page_size
            .ok_or_else(|| error!("set_page_size must be called before fetch_page"))?;

        let mut page_table = self.table.clone();
        page_table.set_pagination(Some(Pagination::new(page as i64, size as i64)));
        let raw = page_table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(thing, record)| (thing.to_string(), to_cbor_record(record)))
            .collect())
    }

    async fn fetch_next(
        &self,
        _vista: &Vista,
        token: Option<CborValue>,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<CborValue>)> {
        let size = self
            .page_size
            .ok_or_else(|| error!("set_page_size must be called before fetch_next"))?;

        let page: i64 = match token {
            None => 1,
            Some(CborValue::Integer(n)) => {
                i64::try_from(n).map_err(|_| error!("fetch_next token out of i64 range"))?
            }
            Some(_) => return Err(error!("invalid fetch_next token type for surrealdb driver")),
        };
        if page < 1 {
            return Err(error!("fetch_next token must be a 1-based page number"));
        }

        let mut page_table = self.table.clone();
        page_table.set_pagination(Some(Pagination::new(page, size as i64)));
        let raw = page_table.list_values().await?;
        let records: Vec<(String, Record<CborValue>)> = raw
            .into_iter()
            .map(|(thing, record)| (thing.to_string(), to_cbor_record(record)))
            .collect();

        let next_token = if records.len() == size {
            Some(CborValue::Integer((page + 1).into()))
        } else {
            None
        };
        Ok((records, next_token))
    }

    /// Watch the wrapped table with a SurrealDB `LIVE SELECT` and stream each
    /// change as a [`VistaChange`].
    ///
    /// SurrealDB delivers `CREATE`/`UPDATE`/`DELETE` frames carrying the
    /// affected record's `Thing`. To guarantee a live-updated row is byte-for-
    /// byte the same shape as the initial `list_vista_values` snapshot (so the
    /// consumer's index doesn't see a phantom second key), each `CREATE`/`UPDATE`
    /// re-reads the row through [`get_vista_value`](Self::get_vista_value)'s exact
    /// path rather than reshaping the pushed payload. `DELETE` needs no read.
    ///
    /// The subscription follows the whole table; a narrowed vista's conditions
    /// are honoured by the consumer's cache/scenery, not pushed into the LIVE
    /// query (a v1 limitation — fine for an unconditioned master).
    async fn watch_vista(&self, _vista: &Vista) -> Result<VistaChangeStream> {
        let table_name = self.table.table_name().to_string();
        let live = self.table.data_source().live(&table_name).await?;
        let table = self.table.clone();

        let stream = async_stream::try_stream! {
            let mut live = live;
            while let Some(note) = live.recv().await {
                let Some(thing) = Thing::from_cbor(note.record_id.clone()) else {
                    continue;
                };
                let id = thing.to_string();
                match note.action {
                    Action::Delete => yield VistaChange::Deleted { id },
                    Action::Create | Action::Update => {
                        // Re-read the authoritative, projected row.
                        match table.get_value(thing.clone()).await? {
                            Some(record) => {
                                let value = to_cbor_record(record);
                                if note.action == Action::Create {
                                    yield VistaChange::Inserted { id, value };
                                } else {
                                    yield VistaChange::Updated { id, value };
                                }
                            }
                            // Gone between notification and read — treat as removed.
                            None => yield VistaChange::Deleted { id },
                        }
                    }
                }
            }
        };
        Ok(Box::pin(stream))
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "surrealdb"
    }

    /// Layer SurrealDB's expression vocabulary on top of vantage-vista's
    /// conventional `Vista` verbs, plus a `with_condition(<expr>)` builder that
    /// routes a native `Expression` through [`add_raw_condition`](Self::add_raw_condition).
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(&self, engine: &mut rhai::Engine) {
        use vantage_vista::RhaiVista;

        crate::rhai_engine::register_surreal_onto(engine);

        engine.register_fn(
            "with_condition",
            |v: &mut RhaiVista,
             cond: crate::rhai_engine::RhaiExpr|
             -> std::result::Result<RhaiVista, Box<rhai::EvalAltResult>> {
                v.apply(|vista| vista.add_raw_condition(cond.0))
            },
        );
    }
}

#[cfg(feature = "rhai")]
impl<E> SurrealTableShell<E>
where
    E: Entity<AnySurrealType> + 'static,
{
    /// Build a reference's traversal target by evaluating its Rhai
    /// `build_script`. The conventional `Vista` vocabulary plus SurrealDB's
    /// vendor extensions are registered onto a fresh engine; `table(name)`
    /// resolves a fresh target through the shell's spec resolver, and the parent
    /// `row` is exposed to the script.
    fn get_ref_via_script(&self, script: &str, row: &Record<CborValue>) -> Result<Vista> {
        use vantage_vista::VistaFactory;

        let db = self.table.data_source().clone();
        let resolver = self.resolver.clone();
        let target_resolver: vantage_vista::TargetResolver = std::sync::Arc::new(move |name| {
            let resolver = resolver.as_ref().ok_or_else(|| {
                error!("scripted reference traversal requires a spec resolver on the factory")
            })?;
            let spec = resolver(name).ok_or_else(|| {
                error!("scripted reference traversal: unknown table", table = name)
            })?;
            SurrealVistaFactory::new(db.clone())
                .with_resolver(resolver.clone())
                .build_from_spec(spec)
        });

        // Vendor vocab first, conventional second: this makes the conventional
        // `table(name) -> Vista` win over SurrealDB's `table` alias for `ident`
        // (which stays reachable as `ident(...)`), so a build-script's
        // `table("order")` resolves a Vista rather than an identifier.
        let mut engine = rhai::Engine::new();
        self.register_rhai_extensions(&mut engine);
        vantage_vista::register_conventional_onto(&mut engine, target_resolver);
        vantage_vista::eval_ref_script(&engine, script, row)
    }
}
