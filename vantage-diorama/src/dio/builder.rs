//! [`DioBuilder`]: everything a Dio is, fixed before it is built.

use std::collections::HashSet;
use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_vista::{Vista, VistaMetadata, VistaQuery};

use super::Dio;
use crate::augment::Augmentation;
use crate::lens::Lens;
use crate::lens::registry::DioKey;

/// Builds a [`Dio`] over one master Vista. Made by [`Lens::dio`].
///
/// Augmentation and live follow are chosen here and can't change after
/// [`build`](Self::build). To narrow a built Dio, derive a new one with
/// [`Dio::derive`].
#[must_use = "a DioBuilder does nothing until `build().await`"]
pub struct DioBuilder {
    lens: Arc<Lens>,
    master: Vista,
    augmentations: Option<Arc<Vec<Augmentation>>>,
    live: bool,
    residual: VistaQuery,
    hub: Option<String>,
    recheck_facts: bool,
}

/// What [`DioBuilder::build`] resolved, handed to the Lens to spawn the Dio.
pub(crate) struct DioParts {
    pub(crate) master: Vista,
    pub(crate) residual: VistaQuery,
    pub(crate) query: VistaQuery,
    pub(crate) metadata: VistaMetadata,
    pub(crate) id_column: String,
    pub(crate) augmentations: Option<Arc<Vec<Augmentation>>>,
    pub(crate) augmented_columns: HashSet<String>,
    pub(crate) live: bool,
    pub(crate) cache_table_name: String,
    /// The hub this Dio joins, `None` for one that joins none.
    pub(crate) hub_key: Option<String>,
    /// The key given to [`DioBuilder::hub`], which a derived Dio inherits.
    pub(crate) explicit_hub: Option<String>,
    pub(crate) recheck_facts: bool,
}

impl DioBuilder {
    pub(crate) fn new(lens: Arc<Lens>, master: Vista) -> Self {
        Self {
            lens,
            master,
            augmentations: None,
            live: false,
            residual: VistaQuery::new(),
            hub: None,
            recheck_facts: false,
        }
    }

    /// Exchange settled changes with the other Dios on this Lens built with
    /// the same `key`, instead of those over the same table.
    ///
    /// By default a Dio joins the Dios whose master has the same driver and
    /// name: a Dio and every Dio derived from it, or two built from the same
    /// vista. A layer that renames vistas (one name per session, say) passes
    /// the backend table here, so its Dios still hear each other. Dios with
    /// different keys never exchange anything, and never share a cache table
    /// either: the key is part of the cache table's name, so rows cached by
    /// one are never listed by the other. A Dio derived from this one keeps
    /// the key.
    pub fn hub(mut self, key: impl Into<String>) -> Self {
        self.hub = Some(key.into());
        self
    }

    /// Re-read every row another Dio reports changed through this Dio's
    /// master, instead of judging the reported row against this Dio's query.
    ///
    /// For a master narrowed by something its [`query`](Vista::query)
    /// doesn't show (a rule a table hook adds, such as "only rows whose
    /// parent this user may see"), judging the row in memory would admit
    /// rows the master would never return. The master's answer decides
    /// instead: a row it returns joins or stays, one it doesn't leaves.
    /// Facts for one id that arrive together cost one read. A Dio derived
    /// from this one re-reads too.
    ///
    /// This doesn't keep the Dio's rows apart from another Dio's: two Dios
    /// whose vistas have the same name and recorded query share a cache
    /// table. Give a hidden rule its own vista name, a vista condition, or
    /// its own [`hub`](Self::hub) key.
    pub fn recheck_facts(mut self) -> Self {
        self.recheck_facts = true;
        self
    }

    /// Enrich each row from a second Vista, one row at a time: a cheap list
    /// pass over the master, then a detail pass per row that fetches and
    /// merges each augmentation's columns. Table sceneries on the Dio become
    /// two-pass. A condition or order on a merged column is evaluated by
    /// diorama, never offered to the master.
    pub fn augment(mut self, augmentations: Vec<Augmentation>) -> Self {
        self.augmentations = Some(Arc::new(augmentations));
        self
    }

    /// Follow the master's change stream when it advertises
    /// [`Subscribe`](vantage_vista::Capability::Subscribe). Over a master that
    /// doesn't, this does nothing, and the Lens's refresher keeps the cache
    /// fresh instead.
    ///
    /// The subscription is made before the eager start copies the set, and
    /// its changes are applied once the copy has landed, so a change made
    /// during the copy is neither lost nor overwritten by it. A follow that
    /// falls behind re-reads the set (see [`Dio::refresh`]).
    ///
    /// Each change the master pushes also reaches every other Dio over the
    /// table on this Lens, live or not, judged against its own query. The
    /// first live Dio over the whole table (unnarrowed, nothing in its
    /// residual) becomes the table's feed: a narrowed live Dio built while
    /// it is up makes no subscription of its own and follows the feed's
    /// changes instead, keeping the feed alive while it does.
    pub fn live(mut self) -> Self {
        self.live = true;
        self
    }

    /// A derived Dio: the parent's augmentations (the same `Arc`), its live
    /// setting, and the residual the derivation computed.
    pub(crate) fn derived(
        mut self,
        augmentations: Option<Arc<Vec<Augmentation>>>,
        live: bool,
        residual: VistaQuery,
    ) -> Self {
        self.augmentations = augmentations;
        self.live = live;
        self.residual = residual;
        self
    }

    /// The parent's hub key and recheck setting, for a Dio narrowed from it:
    /// it is still a Dio over the parent's table.
    pub(crate) fn same_table_as(mut self, hub: Option<String>, recheck_facts: bool) -> Self {
        self.hub = hub;
        self.recheck_facts = recheck_facts;
        self
    }

    /// Resolve the id column and the cache identity, open the cache table,
    /// and start the Dio's write worker, refresh timer, eager start and, with
    /// [`live`](Self::live), its live follow.
    ///
    /// While a Dio with the same cache identity, the same augmentations (the
    /// same `Arc`, as a derivation passes on), the same `live` setting, hub
    /// and [`recheck_facts`](Self::recheck_facts) setting is held anywhere,
    /// this returns that Dio instead, after checking its cache
    /// against the refresher's `max_age`. Once every holder has dropped it,
    /// the next build makes a new one.
    ///
    /// Fails when the master declares no id column, since every cached row
    /// is keyed by it; when the cache table can't be opened; when a blocking
    /// eager start fails; and when the live subscription can't be made.
    pub async fn build(self) -> Result<Dio> {
        let Self {
            lens,
            master,
            augmentations,
            live,
            residual,
            hub,
            recheck_facts,
        } = self;
        let id_column = master
            .id_column()
            .ok_or_else(|| {
                error!(
                    "a Dio needs a vista with an id column",
                    table = master.name()
                )
            })?
            .to_string();
        let query = full_query(&master, &residual);
        let cache_table_name = cache_identity(master.name(), &query, hub.as_deref());
        let hub_key = hub_key(&master, hub.as_deref());
        let key = DioKey::new(
            &cache_table_name,
            augmentations.as_ref(),
            live,
            hub_key.as_deref(),
            recheck_facts,
        );
        if let Some(dio) = lens.dios.live(&key) {
            // The shared Dio may be serving a cache older than `max_age`; a
            // build checks that whether or not it spawns.
            dio.inner.refresh_if_stale();
            return Ok(dio);
        }
        let augmented_columns = augmentations
            .iter()
            .flat_map(|augs| augs.iter())
            .flat_map(|aug| aug.merge.columns.iter().cloned())
            .collect();
        let metadata = metadata_of(&master);
        lens.spawn_dio(
            key,
            DioParts {
                master,
                residual,
                query,
                metadata,
                id_column,
                augmentations,
                augmented_columns,
                live,
                cache_table_name,
                hub_key,
                explicit_hub: hub,
                recheck_facts,
            },
        )
        .await
    }
}

/// The hub a Dio over `master` joins: `explicit` when given, else one per
/// driver and table name. `None` over another Dio's facade (driver `"dio"`):
/// such a Dio already follows its parent's bus when live, and its writes
/// land on the parent, which publishes them, so joining would deliver every
/// fact twice. Nor over an aggregate, whose rows are computed, not stored.
fn hub_key(master: &Vista, explicit: Option<&str>) -> Option<String> {
    match (explicit, master.driver_name()) {
        (_, "dio" | "aggregate") => None,
        (Some(key), _) => Some(key.to_string()),
        (None, driver) => Some(format!("{driver}:{}", master.name())),
    }
}

/// The master's applied query with `residual` laid over it. A residual search
/// or order replaces the master's: it came from a later derivation.
fn full_query(master: &Vista, residual: &VistaQuery) -> VistaQuery {
    let mut query = master.query().clone();
    query.conditions.extend(residual.conditions.iter().cloned());
    if residual.search.is_some() {
        query.search = residual.search.clone();
    }
    if residual.order.is_some() {
        query.order = residual.order.clone();
    }
    query
}

/// The cache table a Dio over `name` narrowed by `query` reads and writes:
/// `name` when nothing narrows or orders it, else `name?key`. Two Dios share
/// a table only when their queries are the same.
///
/// [`VistaQuery::key`] carries column names and values verbatim, so a
/// control character in a value would reach the table name. Cache backends
/// may reserve those (redb marks its meta tables with one), so they are
/// percent-encoded in the name and the key, along with `%` itself to keep
/// the encoding reversible. A `?` in the name is encoded too, so the first
/// `?` always separates the name from the key, and no vista's name can pass
/// for another vista's narrowed table. A name without any of these is used
/// as it is.
///
/// A Dio given an explicit [`hub`](DioBuilder::hub) key gets `#key` on the
/// end. A layer that keys its Dios apart (one key per session) does so
/// because their masters differ in ways the vista doesn't record, so their
/// rows must never meet in one table. A `#` in the name or the query key is
/// encoded, so the first `#` always starts the hub key.
pub(crate) fn cache_identity(name: &str, query: &VistaQuery, hub: Option<&str>) -> String {
    let mut identity = String::new();
    encode(&mut identity, name, &['?', '#']);
    if !query.is_empty() {
        identity.push('?');
        encode(&mut identity, &query.key(), &['#']);
    }
    if let Some(hub) = hub {
        identity.push('#');
        encode(&mut identity, hub, &[]);
    }
    identity
}

/// Push `s` onto `out`, percent-encoding `%`, control characters and `also`.
fn encode(out: &mut String, s: &str, also: &[char]) {
    for ch in s.chars() {
        if ch == '%' || ch.is_control() || also.contains(&ch) {
            let mut buf = [0; 4];
            for byte in ch.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        } else {
            out.push(ch);
        }
    }
}

/// The master's schema, as the in-memory evaluator reads it.
fn metadata_of(master: &Vista) -> VistaMetadata {
    let mut metadata = VistaMetadata::new();
    metadata.columns = master.columns().clone();
    metadata.references = master.references().clone();
    metadata.id_column = master.id_column().map(str::to_string);
    metadata.id_generator = master.id_generator().cloned();
    metadata
}
