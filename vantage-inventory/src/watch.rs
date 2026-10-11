//! A recursive `notify` watcher over a project root that hands every
//! created, modified or removed path to a callback. Feed each path to
//! [`Inventory::reload_path`](crate::Inventory::reload_path) on whichever
//! thread owns the inventory.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

/// A running watcher. Dropping it stops the watch; the callback is not
/// called again.
pub struct Watcher {
    _inner: RecommendedWatcher,
}

impl std::fmt::Debug for Watcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watcher").finish_non_exhaustive()
    }
}

/// Watch `root` recursively and call `on_change` on the watcher's own
/// thread, once per changed path. Paths arrive as the backend reports
/// them, which can be the canonical spelling of `root` (for example
/// `/private/var/...` on macOS); [`Inventory::reload_path`] accepts either.
///
/// An error the backend reports (a hit watch limit, a dropped event
/// queue) arrives as `Err`; the watch keeps running.
///
/// [`Inventory::reload_path`]: crate::Inventory::reload_path
pub fn watch<F>(root: &Path, on_change: F) -> Result<Watcher>
where
    F: Fn(Result<PathBuf>) + Send + 'static,
{
    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| match res {
            Ok(event) if should_reload(&event) => {
                event.paths.into_iter().for_each(|p| on_change(Ok(p)))
            }
            Ok(_) => {}
            Err(e) => on_change(Err(e.into())),
        },
        notify::Config::default().with_poll_interval(Duration::from_millis(500)),
    )
    .context("failed to create file watcher")?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .with_context(|| format!("failed to watch {}", root.display()))?;
    Ok(Watcher { _inner: watcher })
}

fn should_reload(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
    )
}
