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
}

impl DioBuilder {
    pub(crate) fn new(lens: Arc<Lens>, master: Vista) -> Self {
        Self {
            lens,
            master,
            augmentations: None,
            live: false,
            residual: VistaQuery::new(),
        }
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

    /// Resolve the id column and the cache identity, open the cache table,
    /// and start the Dio's write worker, refresh timer, eager start and, with
    /// [`live`](Self::live), its live follow.
    ///
    /// While a Dio with the same cache identity, the same augmentations (the
    /// same `Arc`, as a derivation passes on) and the same `live` setting is
    /// held anywhere, this returns that Dio instead, after checking its cache
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
        let cache_table_name = cache_identity(master.name(), &query);
        let key = DioKey::new(&cache_table_name, augmentations.as_ref(), live);
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
            },
        )
        .await
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
pub(crate) fn cache_identity(name: &str, query: &VistaQuery) -> String {
    let mut identity = String::new();
    encode(&mut identity, name, &['?']);
    if !query.is_empty() {
        identity.push('?');
        encode(&mut identity, &query.key(), &[]);
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
    metadata
}
