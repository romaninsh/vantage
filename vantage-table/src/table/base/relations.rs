//! The table's relations and contained relations as Vista references.

use vantage_types::Entity;

use super::Table;
use crate::{references::ContainedRelation, traits::table_source::TableSource};

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// Snapshot the table's relations as Vista references (name, target,
    /// cardinality, join key). Driver factories fold this into
    /// `VistaMetadata` so the erased `Vista` carries enough to drive nested
    /// insert and relation traversal.
    ///
    /// A reference's target is the table name of its built target, the name
    /// a catalog resolves. Building a target is offline: no I/O runs until a
    /// query does.
    ///
    /// Each relation gets one join key. A `has_many` key is fully known
    /// here: this table's id column into the target's foreign key. A
    /// `has_one` key constrains the target's id column, so it is
    /// [`JoinKey::to_target_id`](vantage_vista::JoinKey::to_target_id),
    /// resolved at traversal from the target the traversal builds.
    pub fn vista_references(&self) -> Vec<vantage_vista::Reference> {
        self.vista_references_keyed(&self.id_field_name())
    }

    /// [`vista_references`](Self::vista_references) with `source_id` as the
    /// column a `has_many` reads from this table's rows. A contained record
    /// passes its relation's id column, which may differ from the table's id
    /// field.
    pub(crate) fn vista_references_keyed(&self, source_id: &str) -> Vec<vantage_vista::Reference> {
        let Some(refs) = &self.refs else {
            return Vec::new();
        };
        refs.iter()
            .map(|(name, r)| {
                r.vista_reference(name, r.target_table_name(&self.data_source), source_id)
            })
            .collect()
    }

    /// The contained relations declared on this table as Vista references of
    /// a contained kind, for driver factories to fold into `VistaMetadata`.
    /// The records' schema is derived at traversal from each relation's
    /// `build_target` closure.
    pub fn vista_contained(&self) -> Vec<vantage_vista::Reference> {
        self.contained.iter().map(|c| c.reference()).collect()
    }

    /// Look up a contained relation by name (for the driver's traversal).
    pub fn contained_relation(&self, name: &str) -> Option<&ContainedRelation<T>> {
        self.contained.iter().find(|c| c.name() == name)
    }
}
