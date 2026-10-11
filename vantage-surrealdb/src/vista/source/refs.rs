//! Reference traversal: a join's target table narrowed to a parent row (or
//! built by the reference's Rhai script), or a contained relation's records
//! out of the row's host column.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};
use vantage_vista::Vista;

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
        // (no row) is the same table either way.
        #[cfg(feature = "rhai")]
        if let (Some(script), Some(row)) = (reference.build_script.as_deref(), row) {
            return self.get_ref_via_script(script, row);
        }
        let target = self.table.get_ref_target_erased(relation)?;
        let target = self.factory().from_table(target)?;
        let Some(row) = row else {
            return Ok(target);
        };
        // A record id in `table:key` string form (one that round-tripped
        // through JSON or a script) is re-tagged into a record id: SurrealDB
        // compares links by type, so the string would match nothing.
        let db = self.table.data_source();
        reference.narrow(target, row, |value| {
            db.coerce_reference_value(AnySurrealType::from(value.clone()))
                .into_value()
        })
    }

    /// A factory over this table's data source, carrying the spec resolver.
    pub(super) fn factory(&self) -> SurrealVistaFactory {
        let factory = SurrealVistaFactory::new(self.table.data_source().clone());
        match &self.resolver {
            Some(resolver) => factory.with_resolver(resolver.clone()),
            None => factory,
        }
    }

    /// SurrealDB stores the embedded object / array natively, so the host
    /// value passes through unchanged and writes `UPDATE … MERGE` it back.
    /// The shared `Table::get_contained_ref` does the rest; this supplies the
    /// parent's `Thing` id and the factory wrap.
    fn contained_target(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        let id_field = self.metadata.id_column.as_deref().unwrap_or("id");
        let id_cbor = row.get(id_field).ok_or_else(|| {
            error!(
                "contained traversal requires the parent row's id",
                relation = relation
            )
        })?;
        let thing = Thing::from_cbor(id_cbor.clone())
            .or_else(|| match id_cbor {
                CborValue::Text(s) => Some(self.parse_id(s)),
                _ => None,
            })
            .ok_or_else(|| {
                error!(
                    "could not resolve the parent id into a Thing",
                    relation = relation
                )
            })?;
        let factory = self.factory();
        self.table.get_contained_ref(
            relation,
            row,
            thing,
            move |t| factory.from_table(t),
            |v| Some(v.clone()),
            |c| c,
        )
    }
}
