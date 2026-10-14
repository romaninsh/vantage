//! Shaped narrowing setters. The query state is the inner shell's narrowing
//! plus `searching`, both plain values, so a clone narrows on its own.

use vantage_core::Result;
use vantage_vista::{Capability, Condition, FilterOp};

use super::super::ShapedShell;

impl ShapedShell {
    /// Equality push-down is universal, so it is never gated; any other
    /// operator needs `FilterOps`.
    pub(super) fn shaped_add_condition(&mut self, condition: &Condition) -> Result<()> {
        if condition.op != FilterOp::Eq {
            self.gate(Capability::FilterOps)?;
        }
        self.inner.add_condition(condition)
    }

    pub(super) fn shaped_set_search(&mut self, text: Option<&str>) -> Result<()> {
        self.gate(Capability::Search)?;
        self.inner.set_search(text)?;
        self.searching = text.is_some();
        Ok(())
    }
}
