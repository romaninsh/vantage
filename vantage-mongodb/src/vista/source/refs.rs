//! Reference traversal: a join's target collection narrowed to a parent row
//! (or built by the reference's Rhai script), or a contained relation's
//! records out of the row's host field.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::Vista;

use super::MongoTableShell;
use crate::vista::factory::MongoVistaFactory;

impl MongoTableShell {
    pub(super) fn ref_target(
        &self,
        relation: &str,
        row: Option<&Record<CborValue>>,
    ) -> Result<Vista> {
        let reference = self.metadata.references.get(relation).ok_or_else(|| {
            error!("Reference not found on vista", relation = relation).mark_not_found()
        })?;
        if reference.kind.is_contained() {
            let row = row.ok_or_else(|| {
                error!(
                    "contained relation is traversed from a parent row",
                    relation = relation
                )
                .mark_incorrect_usage()
            })?;
            return self.contained_target(relation, row);
        }
        // A reference carrying a Rhai `build_script` builds its related rows
        // through the script engine instead of the join keys. The bare target
        // (no row) is the same collection either way.
        #[cfg(feature = "rhai")]
        if let (Some(script), Some(row)) = (reference.build_script.as_deref(), row) {
            return self.get_ref_via_script(script, row);
        }
        let target = self.table.get_ref_target_erased(relation)?;
        let target = self.factory().from_table(target)?;
        let Some(row) = row else {
            return Ok(target);
        };
        // CBOR carries an ObjectId as its hex text, so the key value passes
        // through unchanged: the target's equality filter matches both
        // spellings, and the set invariant it derives (the value a child
        // insert stores as its foreign key) is the ObjectId.
        reference.narrow(target, row, CborValue::clone)
    }

    /// A factory over this table's data source, carrying the spec resolver
    /// and the live-update override.
    pub(super) fn factory(&self) -> MongoVistaFactory {
        let mut factory = MongoVistaFactory::new(self.table.data_source().clone());
        factory.change_streams = self.change_streams;
        match &self.resolver {
            Some(resolver) => factory.with_resolver(resolver.clone()),
            None => factory,
        }
    }

    /// MongoDB stores nested objects and arrays natively, so the host value
    /// passes through unchanged and writes `$set` it back. The shared
    /// `Table::get_contained_ref` does the rest; this supplies the parent's
    /// `MongoId` and the factory wrap.
    fn contained_target(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        let id_field = self.metadata.id_column.as_deref().unwrap_or("_id");
        let parent_id = match row.get(id_field) {
            Some(CborValue::Text(s)) => s.clone(),
            Some(CborValue::Integer(i)) => i128::from(*i).to_string(),
            _ => {
                return Err(error!(
                    "contained traversal requires the parent document's id",
                    relation = relation
                ));
            }
        };
        let mongo_id = self.parse_id(&parent_id);
        let factory = self.factory();
        self.table.get_contained_ref(
            relation,
            row,
            mongo_id,
            move |t| factory.from_table(t),
            |v| Some(v.clone()),
            |c| c,
        )
    }
}
