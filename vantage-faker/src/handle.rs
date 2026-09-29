//! [`FakerHandle`]: the live half of a split [`FakerTable`](crate::FakerTable).

use std::sync::Arc;

use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use vantage_diorama::ChangeEvent;
use vantage_vista::Vista;
use vantage_vista::source::TableShell;

use crate::FakerCtx;

/// The live half of a [`FakerTable`](crate::FakerTable) once its
/// [`Vista`](crate::FakerTable::split) has been handed to a Dio: the delta
/// [`Sender`](broadcast::Sender) and the abort-on-drop mutation-loop guard.
pub struct FakerHandle {
    /// Subscribe to receive [`ChangeEvent`]s and forward them into a Dio.
    pub events: broadcast::Sender<ChangeEvent>,
    pub(crate) ctx: Arc<FakerCtx>,
    /// Name of the master Vista.
    pub(crate) name: String,
    /// Unfiltered clone of the master Vista's shell, cloned again for each
    /// [`vista`](Self::vista).
    pub(crate) shell: Box<dyn TableShell>,
    /// Held only for its abort-on-drop guard — dropping the handle stops the loop.
    pub(crate) _task: Option<AbortOnDrop>,
}

impl FakerHandle {
    /// The table's store handle — see [`FakerTable::ctx`](crate::FakerTable::ctx).
    pub fn ctx(&self) -> &Arc<FakerCtx> {
        &self.ctx
    }

    /// A new master [`Vista`] with the name, metadata and backend shape of the
    /// one [`split`](crate::FakerTable::split) returned, reading the same
    /// store. Use it to feed a fresh Dio after the first one is dropped; the
    /// new Vista starts without the old one's conditions or ordering.
    pub fn vista(&self) -> Vista {
        let shell = self.shell.clone_shell().expect("faker shells always clone");
        Vista::new(self.name.clone(), shell)
    }
}

/// Aborts its task when dropped, so a dropped [`FakerTable`](crate::FakerTable)
/// stops mutating.
pub(crate) struct AbortOnDrop(pub(crate) JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
