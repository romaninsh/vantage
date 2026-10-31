//! [`direct`]: the shell of a datasource without transactions, which writes
//! as it always does inside one.

use vantage_core::Result;

use super::{Transaction, joining};
use crate::{TableShell, forward_table_shell};

#[derive(Clone)]
struct DirectShell {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(DirectShell, inner, {
    fn rebind(&self, _tx: &Transaction) -> Result<Box<dyn TableShell>> {
        Ok(direct(self.inner.clone()))
    }
});

/// `shell`, taking part in a transaction by writing outside it: a write
/// through the wrapped Vista lands at once, stays after a rollback, and is
/// reported in [`Committed::unprotected`](super::Committed::unprotected).
///
/// A shell returns it from its [`rebind`](TableShell::rebind):
/// `Ok(direct(Box::new(self.clone())))`. Made there, it marks the rebind,
/// so the [`Transaction`] knows the Vista's writes are direct.
pub fn direct(shell: Box<dyn TableShell>) -> Box<dyn TableShell> {
    joining::note_direct();
    Box::new(DirectShell { inner: shell })
}
