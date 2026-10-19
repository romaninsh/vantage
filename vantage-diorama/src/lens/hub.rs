//! Table hubs: how the Dios over one table on a Lens hear each other.
//!
//! Each Dio keeps its own cache table, keyed by everything it is narrowed
//! by, and its own event bus. Without the hub, a row that one Dio writes or
//! re-reads never reaches the others: a Dio derived for "client 7" goes on
//! listing an invoice that its parent moved to client 8.
//!
//! The hub carries *facts about the table*: a row's settled value, a
//! delete, "re-check this id", "re-read your set". Every Dio over the table
//! joins its hub when it spawns, publishes what settles in its own cache,
//! and judges what the others publish against its own query (see
//! `dio/hub_follow.rs`). Writes stay on the Dio that made them; only their
//! confirmed outcome travels.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use ciborium::Value as CborValue;
use tokio::sync::broadcast;
use vantage_types::Record;

use super::Lens;
use crate::dio::{Dio, WeakDio};

/// Facts a hub holds before a slow follower lags. A lagging follower
/// re-reads its set, so this bounds memory, not correctness.
pub(crate) const HUB_CAPACITY: usize = 256;

/// One thing that became true about a row of the table, or about all of it.
#[derive(Clone, Debug)]
pub(crate) enum TableChange {
    /// Row `id` now holds `row`, confirmed. `complete` is false when the
    /// row lacks augmented values its publisher hadn't fetched yet: a
    /// receiver then re-reads the row instead of judging the gap.
    Upserted {
        id: String,
        row: Record<CborValue>,
        complete: bool,
    },
    /// Row `id` is gone from the table.
    Deleted { id: String },
    /// Row `id` may have changed in a way the publisher can't state: it
    /// left the publisher's narrowed set, which says nothing about anyone
    /// else's. A receiver re-reads it through its own master.
    Recheck { id: String },
    /// Too much changed to list: every receiver re-reads its set.
    Invalidated,
}

/// A [`TableChange`] and who published it.
#[derive(Clone, Debug)]
pub(crate) struct HubFact {
    /// The publishing Dio's serial, so it can skip its own facts.
    pub(crate) origin: u64,
    /// The publisher's cache table. A receiver over the same table already
    /// holds the bytes, and only needs the event.
    pub(crate) store: String,
    pub(crate) change: TableChange,
}

/// The broadcast every Dio over one table joins. Held by `Arc` in each
/// member, and by `Weak` in the Lens, so it drops with its last member.
pub(crate) struct TableHub {
    tx: broadcast::Sender<HubFact>,
    /// The live Dio whose subscription serves the whole table, when one is
    /// up. Other live Dios over the table follow its facts instead of
    /// subscribing themselves.
    feed: Mutex<Option<WeakDio>>,
}

impl TableHub {
    fn new() -> Self {
        Self {
            tx: broadcast::channel(HUB_CAPACITY).0,
            feed: Mutex::new(None),
        }
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<HubFact> {
        self.tx.subscribe()
    }

    /// Send `fact` to every follower. A send with no follower is not an
    /// error: nobody needed it.
    pub(crate) fn send(&self, fact: HubFact) {
        let _ = self.tx.send(fact);
    }

    /// How many followers are listening, the publisher's own included.
    pub(crate) fn members(&self) -> usize {
        self.tx.receiver_count()
    }

    /// The table's live feed, while it is up.
    pub(crate) fn feed(&self) -> Option<Dio> {
        let feed = self.feed.lock().unwrap_or_else(|e| e.into_inner());
        feed.as_ref().and_then(WeakDio::upgrade)
    }

    /// Make `dio` the table's live feed, unless a live one is already up.
    /// `true` when `dio` took the place.
    pub(crate) fn claim_feed(&self, dio: &Dio) -> bool {
        let mut feed = self.feed.lock().unwrap_or_else(|e| e.into_inner());
        if feed.as_ref().and_then(WeakDio::upgrade).is_some() {
            return false;
        }
        *feed = Some(dio.downgrade());
        true
    }
}

/// The Lens's hubs by table key.
#[derive(Default)]
pub(crate) struct HubRegistry {
    hubs: Mutex<HashMap<String, Weak<TableHub>>>,
}

impl Lens {
    /// The hub for the table `key`, made on first use. Every Dio over the
    /// table that holds the returned `Arc` shares it; once none does, the
    /// next call makes a fresh one.
    pub(crate) fn hub(&self, key: &str) -> Arc<TableHub> {
        let mut hubs = self.hubs.hubs.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hub) = hubs.get(key).and_then(Weak::upgrade) {
            return hub;
        }
        hubs.retain(|_, weak| weak.strong_count() > 0);
        let hub = Arc::new(TableHub::new());
        hubs.insert(key.to_string(), Arc::downgrade(&hub));
        hub
    }

    /// A serial no other Dio of this Lens has, for telling a Dio's own hub
    /// facts from its peers'.
    pub(crate) fn next_serial(&self) -> u64 {
        self.serials
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
}
