use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_vista::{
    Capability, ColumnFlag, Condition, FilterOp, SortDirection, Vista, VistaCapabilities,
    VistaMetadata, VistaQuery,
};

use super::DioInner;
use super::impls::table_shell::{path_get, record_get};
use vantage_types::Record;

mod watch;

/// `field` names a flat cell: it has no dot, or `metadata` declares it with
/// one in its name.
fn is_flat(field: &str, metadata: &VistaMetadata) -> bool {
    !field.contains('.') || metadata.columns.contains_key(field)
}

/// Narrowing applied to a facade handle.
///
/// Held whole rather than pre-split, because whether a clause can be pushed
/// into the master is decided at read time by *trying* it — capability flags
/// describe a driver in general, not whether it accepts one particular column.
#[derive(Default, Clone)]
pub(crate) struct FacadeQuery {
    pub(crate) conditions: Vec<(String, CborValue)>,
    pub(crate) order: Option<(String, SortDirection)>,
}

impl FacadeQuery {
    pub(crate) fn is_empty(&self) -> bool {
        self.conditions.is_empty() && self.order.is_none()
    }

    /// The facade's equality conditions on flat cells, as set invariants. A
    /// dotted path into a nested map has no flat cell to fill, so writes are
    /// left to the set check; a column `metadata` declares with a dot in its
    /// name is a flat cell.
    pub(crate) fn invariants(&self, metadata: &VistaMetadata) -> IndexMap<String, CborValue> {
        self.conditions
            .iter()
            .filter(|(field, _)| is_flat(field, metadata))
            .cloned()
            .collect()
    }

    /// Whether `row` satisfies every condition, evaluated locally. A flat
    /// cell is read by its key; a dotted path into a map is read by walking
    /// the path only, so a literal `"a.b"` key in `row` can't stand in for it.
    pub(crate) fn matches(&self, row: &Record<CborValue>, metadata: &VistaMetadata) -> bool {
        self.conditions.iter().all(|(field, expected)| {
            let cell = if is_flat(field, metadata) {
                record_get(row, field)
            } else {
                path_get(row, field)
            };
            cell == Some(expected)
        })
    }

    fn to_vista_query(&self) -> VistaQuery {
        let mut query = VistaQuery::new();
        for (field, value) in &self.conditions {
            query = query.with_condition(Condition::new(field, FilterOp::Eq, value.clone()));
        }
        if let Some((column, dir)) = &self.order {
            query = query.with_order(column, *dir);
        }
        query
    }

    /// The equality conditions and order of `query`, the only clauses a
    /// facade handle records.
    fn from_vista_query(query: &VistaQuery) -> Self {
        Self {
            conditions: query
                .conditions
                .iter()
                .map(|c| (c.column.clone(), c.value.clone()))
                .collect(),
            order: query.order.clone(),
        }
    }
}

/// `TableShell` impl that backs the Vista returned by `Dio::vista()`.
///
/// Holds the shared inner Dio state so reads/writes route through the
/// Dio's machinery. Schema (columns, refs, id) is **snapshotted** from the
/// master at construction so the facade doesn't borrow the now-swappable
/// master across reads; a fresh `dio.vista()` after a [`reload`](crate::Dio::reload)
/// captures the new schema. The facade advertises only what it implements
/// itself; see `DioShell::new`.
#[derive(Clone)]
pub struct DioShell {
    pub(crate) dio: Arc<DioInner>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// This handle's narrowing. Per-handle, and cloned rather than shared, so
    /// narrowing one facade never reorders another's view of the same Dio.
    pub(crate) query: FacadeQuery,
}

impl DioShell {
    pub(crate) fn new(dio: Arc<DioInner>) -> Self {
        let master = dio.master.read().unwrap();
        // Every column becomes orderable, whatever the master says.
        // `Vista::set_order` refuses a column without the `Orderable` flag, and
        // most drivers only set it for columns their *engine* can sort — CSV
        // sets it for none. But the facade can always fall back to ordering the
        // cache, so refusing here would deny a sort the Dio is perfectly able
        // to perform. `Searchable` gets no such treatment: the facade has no
        // search of its own to fall back on.
        let mut metadata = VistaMetadata::new();
        metadata.columns = master
            .columns()
            .iter()
            .map(|(name, column)| {
                (
                    name.clone(),
                    column.clone().with_flag(ColumnFlag::Orderable),
                )
            })
            .collect();
        metadata.references = master.references().clone();
        metadata.id_column = master.id_column().map(str::to_string);
        drop(master);
        let write_caps = dio.write_capabilities();

        // The facade advertises what `DioShell` itself answers, and nothing
        // it would refuse:
        // - writes follow `Dio::write_capabilities` (master OR `on_flash`
        //   route), and every write keeps to the handle's narrowing;
        // - `Subscribe`: the event bus backs `watch`;
        // - `Order`: pushed into the master when it can answer it, applied
        //   over the cache when it can't;
        // - `Window`: windowed over the cache (hydrating augment gaps).
        //
        // Not advertised: `Search` (the facade has no search of its own),
        // `FilterOps` (it records equality only), `Import` (bulk import goes
        // through `Dio::import_values`), `Traverse` (traverse through
        // `Dio::get_ref`), `Cursor` and `NativeConditions`.
        let capabilities = [
            (write_caps.can_insert, Capability::Insert),
            (write_caps.can_update, Capability::Update),
            (write_caps.can_delete, Capability::Delete),
            (true, Capability::Subscribe),
            (true, Capability::Order),
            (true, Capability::Window),
            (true, Capability::ConfineWrites),
        ]
        .into_iter()
        .filter_map(|(on, capability)| on.then_some(capability))
        .collect();
        Self {
            dio,
            capabilities,
            metadata,
            query: FacadeQuery::default(),
        }
    }

    /// A handle over `dio` narrowed by `query`, with `metadata` and no
    /// capabilities — enough to read the narrowed set and nothing else.
    fn reader(dio: Arc<DioInner>, query: FacadeQuery, metadata: VistaMetadata) -> Self {
        Self {
            dio,
            capabilities: VistaCapabilities::default(),
            metadata,
            query,
        }
    }

    /// Split this handle's narrowing into "the master will do it" and "we do it
    /// over the cache", and build the narrowed master if anything pushed down.
    ///
    /// The rule, in order:
    ///
    /// 1. **An augmented Dio always reads its cache.** Augment columns exist
    ///    only there — the master has never heard of them — so both a filter on
    ///    one and the *values* of one would be lost by reading the master. Every
    ///    clause stays local.
    /// 2. Otherwise the query goes to a clone of the master through
    ///    `Vista::with_query`. Whatever it accepts is answered at the
    ///    source, authoritatively over the whole set rather than over whatever
    ///    the cache happens to hold.
    /// 3. Whatever it refuses (`Unsupported`/`Unimplemented`) is applied here.
    ///    Any other error from the master is a fault and is returned.
    ///
    /// Clauses are *tried* rather than predicted from capability flags: a flag
    /// describes a driver, not whether it accepts one particular column.
    pub(crate) fn plan(&self) -> Result<(Option<Vista>, FacadeQuery)> {
        if self.query.is_empty() {
            return Ok((None, FacadeQuery::default()));
        }
        if self.dio.is_two_pass() {
            return Ok((None, self.query.clone()));
        }

        let (offered, mut residual) = self.dio.split_local(self.query.to_vista_query(), true);
        let master = Vista::clone(&self.dio.master.read().unwrap());
        let (narrowed, refused) = master.with_query(&offered)?;
        residual.conditions.extend(refused.conditions);
        if refused.order.is_some() {
            residual.order = refused.order;
        }

        let local = FacadeQuery::from_vista_query(&residual);
        let pushed = local.conditions.len() < self.query.conditions.len()
            || (self.query.order.is_some() && local.order.is_none());
        Ok((pushed.then_some(narrowed), local))
    }
}
