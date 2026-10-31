//! [`ResolverCell`]: the run's resolver, which `atomic` re-points for the
//! length of a block.
//!
//! `DataVocab` and every handle made by `table()` hold the cell, not the
//! resolver behind it, so pointing the cell reaches handles made before the
//! block as well as inside it.

use std::sync::{Arc, PoisonError, RwLock};

use vantage_core::Result;

use crate::{VistaResolver, vista::Vista};

/// One per run; never shared across runs.
#[derive(Clone)]
pub struct ResolverCell(Arc<CellInner>);

struct CellInner {
    base: Arc<dyn VistaResolver>,
    over: RwLock<Option<Arc<dyn VistaResolver>>>,
}

/// Points its cell back where it was when dropped.
#[must_use = "the cell points back when the guard drops"]
pub struct CellGuard {
    cell: ResolverCell,
    previous: Option<Arc<dyn VistaResolver>>,
}

impl ResolverCell {
    pub fn new(base: Arc<dyn VistaResolver>) -> Self {
        Self(Arc::new(CellInner {
            base,
            over: RwLock::new(None),
        }))
    }

    pub fn base(&self) -> Arc<dyn VistaResolver> {
        self.0.base.clone()
    }

    /// Every resolve goes through `over` until the guard drops.
    pub fn point(&self, over: Arc<dyn VistaResolver>) -> CellGuard {
        let previous = self.swap(Some(over));
        CellGuard {
            cell: self.clone(),
            previous,
        }
    }

    pub fn is_pointed(&self) -> bool {
        self.0
            .over
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// The resolver to use now. The lock is released before the caller
    /// resolves, so a resolve may point or read the cell itself.
    fn current(&self) -> Arc<dyn VistaResolver> {
        let over = self.0.over.read().unwrap_or_else(PoisonError::into_inner);
        over.clone().unwrap_or_else(|| self.0.base.clone())
    }

    fn swap(&self, over: Option<Arc<dyn VistaResolver>>) -> Option<Arc<dyn VistaResolver>> {
        let mut slot = self.0.over.write().unwrap_or_else(PoisonError::into_inner);
        std::mem::replace(&mut *slot, over)
    }
}

impl VistaResolver for ResolverCell {
    fn vista(&self, name: &str) -> Result<Vista> {
        self.current().vista(name)
    }

    fn adopt(&self, vista: Vista) -> Vista {
        self.current().adopt(vista)
    }
}

impl Drop for CellGuard {
    fn drop(&mut self) {
        self.cell.swap(self.previous.take());
    }
}
