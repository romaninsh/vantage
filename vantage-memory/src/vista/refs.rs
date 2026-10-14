//! Traversal from a `MemoryTableShell` to the tables its references join.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{Capability, Vista, VistaResolver, unknown_relation, unsupported};

use super::MemoryTableShell;

impl MemoryTableShell {
    /// The target of `relation`: bare for `None`, else narrowed to the rows
    /// related to `row` by the reference's join keys. The target comes from
    /// the shell's [`MemoryCatalog`](super::MemoryCatalog), so it carries its
    /// registered columns; equality conditions are conformed to a column's
    /// stored form, so an integer foreign key (as seed files usually write
    /// them) meets a text id.
    ///
    /// An undeclared relation is not-found. vantage-memory keeps no
    /// contained records, so a contained relation is refused.
    pub(super) fn traverse(
        &self,
        relation: &str,
        row: Option<&Record<CborValue>>,
    ) -> Result<Vista> {
        let reference = self
            .metadata
            .references
            .get(relation)
            .ok_or_else(|| unknown_relation(relation))?;
        if reference.kind.is_contained() {
            return unsupported(Capability::Traverse);
        }
        let target = self.catalog.vista(&reference.target)?;
        match row {
            Some(row) => reference.narrow(target, row),
            None => Ok(target),
        }
    }
}
