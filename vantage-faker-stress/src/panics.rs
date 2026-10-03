//! Counts panics anywhere in the process, so a chaos run can tell a sim
//! that errored from one that took a thread down.

use std::sync::Once;
use std::sync::atomic::{AtomicU64, Ordering};

static PANICS: AtomicU64 = AtomicU64::new(0);
static INSTALL: Once = Once::new();

/// Chain a counting hook in front of the current panic hook. Idempotent.
pub fn install() {
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            PANICS.fetch_add(1, Ordering::Relaxed);
            previous(info);
        }));
    });
}

pub fn count() -> u64 {
    PANICS.load(Ordering::Relaxed)
}
