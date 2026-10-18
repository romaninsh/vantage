//! Reference traversal: a join's target table narrowed to a parent row (or
//! built by the reference's Rhai script), or a contained relation's records
//! out of the row's host column.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_types::{Entity, Record};
use vantage_vista::{Vista, unknown_relation};

use super::SurrealTableShell;
use crate::thing::Thing;
use crate::types::{AnySurrealType, SurrealType};
use crate::vista::factory::SurrealVistaFactory;

impl<E> SurrealTableShell<E>
where
    E: Entity<AnySurrealType> + 'static,
{
    pub(super) fn ref_target(
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
            return self.contained_target(relation, row);
        }
        // A reference carrying a Rhai `build_script` builds its related rows
        // through the script engine instead of the join keys. The bare target
        // (no row) is the same table either way.
        #[cfg(feature = "rhai")]
        if let (Some(script), Some(row)) = (reference.build_script.as_deref(), row) {
            return self.get_ref_via_script(script, row);
        }
        let target = self.table.get_ref_target_erased(relation)?;
        let target = self.factory().from_table(target)?;
        // A record id held as text is converted by the target's own
        // `add_condition`, which knows its record-link columns.
        match row {
            Some(row) => reference.narrow(target, row),
            None => Ok(target),
        }
    }

    /// The factory that built this shell, carrying its spec resolver and
    /// build injector.
    pub(super) fn factory(&self) -> SurrealVistaFactory {
        self.factory.clone()
    }

    /// SurrealDB stores the embedded object / array natively, so the host
    /// value passes through unchanged and writes `UPDATE … MERGE` it back.
    /// The shared `Table::get_contained_ref` does the rest; this supplies the
    /// parent's `Thing` id and the factory wrap. Without a row it gives the
    /// bare target.
    fn contained_target(&self, relation: &str, row: Option<&Record<CborValue>>) -> Result<Vista> {
        let parent = match row {
            Some(row) => Some((row, self.parent_thing(relation, row)?)),
            None => None,
        };
        let factory = self.factory();
        self.table.get_contained_ref(
            relation,
            parent,
            move |t| factory.from_table(t),
            |v| Some(v.clone()),
            |c| c,
        )
    }

    /// The parent row's id as the `Thing` the host-column writeback patches.
    fn parent_thing(&self, relation: &str, row: &Record<CborValue>) -> Result<Thing> {
        let id_field = self.metadata.id_column.as_deref().unwrap_or("id");
        let id_cbor = row.get(id_field).ok_or_else(|| {
            error!(
                "contained traversal requires the parent row's id",
                relation = relation
            )
        })?;
        Thing::from_cbor(id_cbor.clone())
            .or_else(|| match id_cbor {
                CborValue::Text(s) => Some(self.parse_id(s)),
                _ => None,
            })
            .ok_or_else(|| {
                error!(
                    "could not resolve the parent id into a Thing",
                    relation = relation
                )
            })
    }
}
