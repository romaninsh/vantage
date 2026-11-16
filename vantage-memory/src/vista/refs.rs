//! Traversal from a `MemoryTableShell` to the tables its references join.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{Capability, Vista, unknown_relation, unsupported};

use super::MemoryTableShell;
use super::factory::{spec_vista, table_vista};

impl MemoryTableShell {
    /// The target of `relation`: bare for `None`, else narrowed to the rows
    /// related to `row` by the reference's join keys. The target is the
    /// typed relation's table, built from its spec by the factory that built
    /// this one (with its injector), on this shell's datasource, so it joins
    /// an open transaction. Its metadata is its spec's, so its column types
    /// match a root build. Equality conditions are conformed to a column's
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
        // The relation already built and injected the target, so wrapping it
        // needs no injector.
        let specs = self.specs.clone();
        self.table.traverse_erased(reference, row, |target| {
            match specs.resolve(&reference.target) {
                Some(spec) => spec_vista(target, &spec, specs.clone()),
                None => table_vista(target, specs.clone()),
            }
        })
    }
}
