//! The Lens's live Dios, one per identity, held by `Weak`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use crate::augment::Augmentation;
use crate::dio::{Dio, DioInner};

/// What makes two builds the same Dio: the cache identity, the very same
/// augmentations `Arc`, and the `live` setting.
///
/// The augmentations are compared by address. A live Dio holds its `Arc`, so
/// while an entry can be upgraded no other allocation can take that address.
#[derive(Hash, PartialEq, Eq)]
pub(crate) struct DioKey {
    identity: String,
    augmentations: Option<usize>,
    live: bool,
}

impl DioKey {
    pub(crate) fn new(
        identity: &str,
        augmentations: Option<&Arc<Vec<Augmentation>>>,
        live: bool,
    ) -> Self {
        Self {
            identity: identity.to_string(),
            augmentations: augmentations.map(|augs| Arc::as_ptr(augs) as usize),
            live,
        }
    }
}

/// Every Dio the Lens built that some holder still keeps.
///
/// An entry is live exactly while `Weak::upgrade` succeeds. The upgrade is
/// atomic with the last strong drop, and a Dio's teardown runs only after
/// that drop, so a lookup gets a Dio no part of which has shut down, or
/// nothing.
#[derive(Default)]
pub(crate) struct DioRegistry {
    dios: Mutex<HashMap<DioKey, Weak<DioInner>>>,
}

impl DioRegistry {
    pub(crate) fn live(&self, key: &DioKey) -> Option<Dio> {
        let dios = self.dios.lock().unwrap_or_else(|e| e.into_inner());
        dios.get(key).and_then(upgrade)
    }

    /// Register `dio` under `key` and return it, unless a concurrent build of
    /// the same key registered first: then that one is returned, and the
    /// caller's `dio` stops when its last handle drops. Dead entries are
    /// pruned here.
    pub(crate) fn insert(&self, key: DioKey, dio: &Dio) -> Dio {
        let mut dios = self.dios.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(winner) = dios.get(&key).and_then(upgrade) {
            return winner;
        }
        dios.retain(|_, weak| weak.strong_count() > 0);
        dios.insert(key, Arc::downgrade(&dio.inner));
        dio.clone()
    }
}

fn upgrade(weak: &Weak<DioInner>) -> Option<Dio> {
    weak.upgrade().map(|inner| Dio { inner })
}

#[cfg(test)]
mod tests;
