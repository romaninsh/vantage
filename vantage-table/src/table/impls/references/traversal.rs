//! Traversal of same-persistence relations into their target tables.

use std::any::Any;

use vantage_core::{Result, error};
use vantage_types::{EmptyEntity, Entity, Record};

use crate::{
    references::Reference,
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

/// Unbox a target built by a relation's stored factory as `X`.
fn downcast_target<X: 'static>(target: Box<dyn Any>, relation: &str) -> Result<X> {
    target
        .downcast::<X>()
        .map(|target| *target)
        .map_err(|_| error!("Failed to downcast related table", relation = relation))
}

impl<T: TableSource + 'static, E: Entity<T::Value> + 'static> Table<T, E> {
    /// Narrow the table to a single row by id.
    ///
    /// Pairs with `get_some_value` for the "I only know an id" workflow.
    /// The actual condition construction goes through
    /// `TableSource::eq_value_condition`, so backends that don't yet
    /// implement that path return an error here.
    pub fn with_id(mut self, id: impl Into<T::Value>) -> Result<Self> {
        let id_name = self
            .id_field()
            .ok_or_else(|| error!("id field not set on table"))?
            .name()
            .to_string();
        let id = id.into();
        let condition = self
            .data_source()
            .eq_value_condition(&id_name, id.clone())?;
        self.add_condition(condition);
        // A row inserted into "this id" should conform to it.
        self.add_invariant(id_name, id);
        Ok(self)
    }

    /// Traverse a same-persistence reference using a known source row as the
    /// join origin.
    ///
    /// Reads the join field value out of `row`, builds the target table via
    /// the reference's stored factory, and applies one eq-condition that
    /// selects the related rows. No subquery, no deferred fetch — `row`
    /// already carries the value.
    ///
    /// `HasOne` reads from its stored foreign-key column; `HasMany` reads
    /// from the source's id field (looked up here and forwarded into the
    /// reference). The returned table preserves columns, refs, and
    /// expressions from the reference's factory; only the entity type
    /// changes if `E2` differs from the factory's output.
    pub fn get_ref_from_row<E2: Entity<T::Value> + 'static>(
        &self,
        relation: &str,
        row: &Record<T::Value>,
    ) -> Result<Table<T, E2>> {
        let reference = self.lookup_ref(relation)?;
        let source_id = self.id_field_name();

        let target_dyn = reference.resolve_from_row(
            self.data_source() as &dyn Any,
            &source_id,
            row as &dyn Any,
        )?;

        let target_empty: Table<T, EmptyEntity> =
            *target_dyn
                .downcast::<Table<T, EmptyEntity>>()
                .map_err(|_| error!("Failed to downcast target table to Table<T, EmptyEntity>"))?;

        Ok(target_empty.into_entity::<E2>())
    }

    /// Traverse a same-backend relation into a typed `Table<T, E2>` with an
    /// `IN (subquery)` filter on the source column.
    ///
    /// Use this when the parent table already carries the narrowing
    /// conditions (e.g. `clients.add_condition(is_paying = true)`) and you
    /// want every related child row matching that filter. For the
    /// "I have a specific row in hand" case, prefer
    /// [`Table::get_ref_from_row`] — it pushes a plain eq-condition
    /// instead of a subquery.
    pub fn get_ref_as<E2: Entity<T::Value> + 'static>(
        &self,
        relation: &str,
    ) -> Result<Table<T, E2>> {
        let reference = self.lookup_ref(relation)?;

        let source_id = self.id_field_name();

        let mut target: Table<T, E2> = downcast_target(
            reference.build_target(self.data_source() as &dyn Any),
            relation,
        )?;

        let target_id = target.id_field_name();

        let (src_col, tgt_col) = reference.columns(&source_id, &target_id);

        let condition = self
            .data_source()
            .related_in_condition(&tgt_col, self, &src_col);
        target.add_condition(condition);

        Ok(target)
    }

    /// Get a correlated related table for use inside SELECT expressions.
    ///
    /// Unlike [`Self::get_ref_as`] (which uses `IN (subquery)`), this produces a
    /// correlated condition like `order.client_id = client.id`, suitable
    /// for embedding as a subquery in a SELECT clause via
    /// [`Self::with_expression`].
    pub fn get_subquery_as<E2: Entity<T::Value> + 'static>(
        &self,
        relation: &str,
    ) -> Result<Table<T, E2>> {
        let reference = self.lookup_ref(relation)?;
        let mut target: Table<T, E2> = downcast_target(
            reference.build_target(self.data_source() as &dyn Any),
            relation,
        )?;
        self.attach_correlated_condition(reference, &mut target);
        Ok(target)
    }

    /// Entity-erased [`get_subquery_as`](Self::get_subquery_as): builds the
    /// correlated target as `Table<T, EmptyEntity>` without the caller naming
    /// the target's concrete entity type. Used by implicit-reference traversal,
    /// which walks relations by name and cannot know each hop's entity type.
    pub fn get_subquery_erased(&self, relation: &str) -> Result<Table<T, EmptyEntity>> {
        let reference = self.lookup_ref(relation)?;
        let mut target: Table<T, EmptyEntity> = downcast_target(
            reference.build_target_erased(self.data_source() as &dyn Any),
            relation,
        )?;
        self.attach_correlated_condition(reference, &mut target);
        Ok(target)
    }

    /// Attach the correlated join condition binding `target` back to `self`
    /// (`target.<id> = self.<foreign_key>`). Shared by the typed and erased
    /// subquery builders, which otherwise differ only in the target entity type
    /// and which `build_target*` closure produced `target`.
    fn attach_correlated_condition<E2: Entity<T::Value> + 'static>(
        &self,
        reference: &dyn Reference,
        target: &mut Table<T, E2>,
    ) {
        let source_id = self.id_field_name();
        let target_id = target.id_field_name();
        let (src_col, tgt_col) = reference.columns(&source_id, &target_id);
        let condition = self.data_source().related_correlated_condition(
            target.table_name(),
            &tgt_col,
            self.table_name(),
            &src_col,
        );
        target.add_condition(condition);
    }

    /// Build the relation's target table with **no condition** applied, as
    /// `Table<T, EmptyEntity>`.
    ///
    /// Unlike [`Self::get_ref_from_row`] / [`Self::get_ref_as`] (which select
    /// the related rows for a known parent), this returns the bare target — the
    /// table you'd insert a new related row into. Used by Vista's nested insert
    /// to obtain the destination for a has-one/has-many child before any join
    /// value exists, and to walk an implicit-reference chain and read the final
    /// target's column definitions.
    ///
    /// The result is entity-erased on purpose: a relation's target is built by
    /// a caller-supplied constructor whose entity type the traversal cannot
    /// know, so naming an entity type here would only give a downcast that
    /// fails whenever the two types disagree.
    pub fn get_ref_target_erased(&self, relation: &str) -> Result<Table<T, EmptyEntity>> {
        let reference = self.lookup_ref(relation)?;
        downcast_target(
            reference.build_target_erased(self.data_source() as &dyn Any),
            relation,
        )
    }
}
