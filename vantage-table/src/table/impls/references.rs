//! Table relationship methods for defining and traversing references.

use std::sync::Arc;

use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::Entity;

use crate::{
    references::{HasMany, HasOne, Reference},
    table::Table,
    traits::table_source::TableSource,
};

mod contained;
mod traversal;

impl<T: TableSource + 'static, E: Entity<T::Value> + 'static> Table<T, E> {
    /// Define a one-to-one relationship.
    ///
    /// ```rust,ignore
    /// .with_one("bakery", "bakery_id", Bakery::postgres_table)
    /// ```
    pub fn with_one<E2: Entity<T::Value> + 'static>(
        mut self,
        relation: &str,
        foreign_key: &str,
        build_target: impl Fn(T) -> Table<T, E2> + Send + Sync + 'static,
    ) -> Self
    where
        T::Value: Into<ciborium::Value> + From<ciborium::Value>,
        T::Id: std::fmt::Display + From<String>,
    {
        let reference = HasOne::<T, E, E2>::new(foreign_key, build_target);
        self.add_ref(relation, Box::new(reference));
        self
    }

    /// Define a one-to-many relationship.
    ///
    /// ```rust,ignore
    /// .with_many("orders", "client_id", Order::postgres_table)
    /// ```
    pub fn with_many<E2: Entity<T::Value> + 'static>(
        mut self,
        relation: &str,
        foreign_key: &str,
        build_target: impl Fn(T) -> Table<T, E2> + Send + Sync + 'static,
    ) -> Self
    where
        T::Value: Into<ciborium::Value> + From<ciborium::Value>,
        T::Id: std::fmt::Display + From<String>,
    {
        let reference = HasMany::<T, E, E2>::new(foreign_key, build_target);
        self.add_ref(relation, Box::new(reference));
        self
    }

    pub(crate) fn add_ref(&mut self, relation: &str, reference: Box<dyn Reference>) {
        self.add_ref_arc(relation, Arc::from(reference));
    }

    /// Insert an already-shared reference. Used to inherit relations when
    /// deriving a table from another (the same `Arc` is shared, not rebuilt).
    pub(crate) fn add_ref_arc(&mut self, relation: &str, reference: Arc<dyn Reference>) {
        self.refs
            .get_or_insert_with(IndexMap::new)
            .insert(relation.to_string(), reference);
    }

    /// Copy relations from another table, sharing the underlying `Arc`s. With
    /// `names = None`, copies all relations; otherwise only the listed ones.
    /// An inherited relation keeps working as long as the derived table still
    /// projects the column its foreign key references.
    pub fn copy_relations_from<E2: Entity<T::Value> + 'static>(
        &mut self,
        other: &Table<T, E2>,
        names: Option<&[&str]>,
    ) {
        let Some(refs) = &other.refs else {
            return;
        };
        for (name, reference) in refs {
            if names.is_some_and(|ns| !ns.contains(&name.as_str())) {
                continue;
            }
            self.add_ref_arc(name, reference.clone());
        }
    }

    pub fn references(&self) -> Vec<String> {
        self.refs
            .as_ref()
            .map(|refs| refs.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn lookup_ref(&self, relation: &str) -> Result<&dyn Reference> {
        let table_name = self.table_name();
        let refs = self
            .refs
            .as_ref()
            .ok_or_else(|| error!("No references defined on table", table = table_name))?;

        let reference = refs.get(relation).ok_or_else(|| {
            error!(
                "Reference not found on table",
                relation = relation,
                table = table_name
            )
        })?;

        Ok(reference.as_ref())
    }

    /// A registered relation as Vista metadata. Kind and key questions are
    /// read off this, so they can't disagree with what `vista_references`
    /// publishes. The target name stays empty: nothing here reads it, and
    /// naming it would build the target.
    fn ref_shape(&self, relation: &str) -> Result<vantage_vista::Reference> {
        let reference = self.lookup_ref(relation)?;
        Ok(reference.vista_reference(relation, String::new(), &self.id_field_name()))
    }

    /// Look up cardinality for a registered relation.
    pub fn ref_cardinality(&self, relation: &str) -> Result<vantage_vista::ReferenceKind> {
        Ok(self.ref_shape(relation)?.kind)
    }

    /// Look up the foreign-key/link field for a registered relation. For a
    /// `has_one` this is the source-side column carrying the link — the field
    /// a backend-native traversal (e.g. a SurrealDB idiom path) descends
    /// through, which need not match the relation's registry name. For a
    /// `has_many` it is the target-side column carrying the parent's id.
    pub fn ref_foreign_key(&self, relation: &str) -> Result<String> {
        let shape = self.ref_shape(relation)?;
        let key = shape
            .keys
            .first()
            .ok_or_else(|| error!("Reference has no join key", relation = relation))?;
        Ok(match shape.kind {
            vantage_vista::ReferenceKind::HasMany => key.to.clone().unwrap_or_default(),
            _ => key.from.clone(),
        })
    }

    /// List all registered relations with their cardinality.
    pub fn ref_kinds(&self) -> Vec<(String, vantage_vista::ReferenceKind)> {
        let Some(refs) = &self.refs else {
            return Vec::new();
        };
        let source_id = self.id_field_name();
        refs.iter()
            .map(|(name, r)| {
                let kind = r.vista_reference(name, String::new(), &source_id).kind;
                (name.clone(), kind)
            })
            .collect()
    }
}
