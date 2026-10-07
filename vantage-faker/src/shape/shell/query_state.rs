//! Shaped query-state setters and the clone a consumer narrows.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_vista::FilterOp;
use vantage_vista::source::TableShell;

use super::super::ShapedShell;

impl ShapedShell {
    pub(super) fn shaped_add_op_condition(
        &mut self,
        field: &str,
        op: FilterOp,
        value: &CborValue,
    ) -> Result<()> {
        if op == FilterOp::Eq {
            return self.inner.add_eq_condition(field, value);
        }
        self.gate(
            self.shape.capabilities.can_filter_operators,
            "add_op_condition",
            "can_filter_operators",
        )?;
        self.inner.add_op_condition(field, op, value)
    }

    pub(super) fn shaped_add_search(&mut self, text: &str) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_search,
            "add_search",
            "can_search",
        )?;
        self.inner.add_search(text)?;
        self.searching.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub(super) fn shaped_clear_search(&mut self) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_search,
            "clear_search",
            "can_search",
        )?;
        self.inner.clear_search()?;
        self.searching.store(false, Ordering::Relaxed);
        Ok(())
    }

    pub(super) fn shaped_add_order(
        &mut self,
        field: &str,
        dir: vantage_vista::sort::SortDirection,
    ) -> Result<()> {
        self.gate(self.shape.capabilities.can_order, "add_order", "can_order")?;
        self.inner.add_order(field, dir)
    }

    pub(super) fn shaped_clear_orders(&mut self) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_order,
            "clear_orders",
            "can_order",
        )?;
        self.inner.clear_orders()
    }

    pub(super) fn shaped_set_page_size(&mut self, size: usize) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_set_page_size,
            "set_page_size",
            "can_set_page_size",
        )?;
        self.page_size = size.max(1);
        Ok(())
    }

    /// Same store and fault/jitter stream, fresh query state — the clone a
    /// consumer narrows (`add_order` + `fetch_window`) without disturbing
    /// this handle. Each clone tracks its own `searching`.
    pub(super) fn shaped_clone(&self) -> Option<Box<dyn TableShell>> {
        let inner = self.inner.clone_shell()?;
        Some(Box::new(Self {
            inner,
            shape: self.shape.clone(),
            capabilities: self.capabilities.clone(),
            rng: self.rng.clone(),
            epoch: self.epoch,
            page_size: self.page_size,
            searching: Arc::new(AtomicBool::new(false)),
        }))
    }
}
