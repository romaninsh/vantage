//! Reference traversal: a join's target table narrowed to a parent row, or
//! a contained relation's records out of the row's host column.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_types::{Entity, Record};
use vantage_vista::{Reference, Vista, VistaMetadata, unknown_relation};

use super::dialect::{Shell, SqlShellDialect};
use crate::types::{cbor_to_json, parse_json_host};

impl<T, E> Shell<T, E>
where
    T: SqlShellDialect,
    E: Entity<T::Value> + 'static,
{
    pub(super) fn ref_target(
        &self,
        relation: &str,
        row: Option<&Record<CborValue>>,
    ) -> Result<Vista> {
        let reference = reference(&self.metadata, relation)?;
        if reference.kind.is_contained() {
            return self.contained_target(relation, row);
        }
        T::join_target(self, relation, reference, row)
    }

    /// The bare target of `relation`, narrowed to `row` through the strict
    /// join when there is one.
    pub(crate) fn join_target(
        &self,
        relation: &str,
        reference: &Reference,
        row: Option<&Record<CborValue>>,
    ) -> Result<Vista> {
        let target = self.table.get_ref_target_erased(relation)?;
        let target = T::target_factory(self)(target)?;
        match row {
            Some(row) => reference.narrow(target, row),
            None => Ok(target),
        }
    }

    /// SQL has no native nesting, so the host column stores the collection
    /// as JSON: parsed on read, re-serialized on write. The shared
    /// `Table::get_contained_ref` does the rest; without a row it gives the
    /// bare target.
    fn contained_target(&self, relation: &str, row: Option<&Record<CborValue>>) -> Result<Vista> {
        let parent = match row {
            Some(row) => Some((
                row,
                parent_id(relation, self.metadata.id_column.as_deref(), row)?,
            )),
            None => None,
        };
        self.table.get_contained_ref(
            relation,
            parent,
            T::target_factory(self),
            parse_json_host,
            |c| CborValue::Text(cbor_to_json(c).to_string()),
        )
    }
}

/// The reference `relation` declares, joined or contained.
fn reference<'a>(metadata: &'a VistaMetadata, relation: &str) -> Result<&'a Reference> {
    metadata
        .references
        .get(relation)
        .ok_or_else(|| unknown_relation(relation))
}

/// The parent row's id as the string the host-column writeback patches by.
fn parent_id(relation: &str, id_column: Option<&str>, row: &Record<CborValue>) -> Result<String> {
    match row.get(id_column.unwrap_or("id")) {
        Some(CborValue::Text(s)) => Ok(s.clone()),
        Some(CborValue::Integer(i)) => Ok(i128::from(*i).to_string()),
        _ => Err(error!(
            "contained traversal requires the parent row's id",
            relation = relation
        )),
    }
}
