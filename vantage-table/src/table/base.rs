use std::marker::PhantomData;
use std::sync::Arc;

use indexmap::IndexMap;
use vantage_expressions::Expression;
use vantage_types::{EmptyEntity, Entity};

use crate::{
    pagination::Pagination, references::Reference, sorting::SortDirection,
    traits::table_source::TableSource, traits::table_source_spec::TableSourceSpec,
};

/// Type alias for expression closures stored on Table.
///
/// Stored against the entity-erased `Table<T, EmptyEntity>` rather than the
/// concrete `Table<T, E>` so the closures survive [`Table::into_entity`] — an
/// expression only ever reads entity-agnostic table state (columns and
/// relations by name, conditions, subqueries), never the entity's typed fields.
/// [`Table::with_expression`] adapts the caller's `Fn(&Table<T, E>)` into this
/// shape; see `Table::as_entity_erased` for the soundness of the cast.
pub type ExpressionFn<T> =
    Arc<dyn Fn(&Table<T, EmptyEntity>) -> Expression<<T as TableSource>::Value> + Send + Sync>;

#[derive(Clone)]
pub struct Table<T, E>
where
    T: TableSource,
    E: Entity<T::Value>,
{
    pub(super) data_source: T,
    pub(super) _phantom: PhantomData<E>,
    pub(super) source: T::Source,
    pub(super) columns: IndexMap<String, T::Column<T::AnyType>>,
    pub(super) conditions: IndexMap<i64, T::Condition>,
    pub(super) next_condition_id: i64,
    pub(super) order_by: IndexMap<i64, (T::Condition, SortDirection)>,
    pub(super) next_order_id: i64,
    pub(super) refs: Option<IndexMap<String, Arc<dyn Reference>>>,
    pub(super) contained: Vec<crate::references::ContainedRelation<T>>,
    pub(super) expressions: IndexMap<String, ExpressionFn<T>>,
    pub(super) pagination: Option<Pagination>,
    pub(super) title_field: Option<String>,
    pub(super) title_fields: Vec<String>,
    pub(super) id_field: Option<String>,
    /// Column values every row in this set must hold, because they are part of
    /// the set's definition (e.g. a has-many child carries the parent's foreign
    /// key). Registered wherever the table is narrowed by a literal
    /// `column = value` (see [`Self::with_id`], `Reference::resolve_from_row`);
    /// never from an expression scope. Enforced on write: a column the caller
    /// left null/absent is filled, a matching value is kept, and a conflicting
    /// value is rejected.
    pub(super) invariants: IndexMap<String, T::Value>,
}

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// Create a new Table with the given table name and data source
    pub fn new(table_name: impl Into<String>, data_source: T) -> Self {
        Self {
            data_source,
            _phantom: PhantomData,
            source: T::Source::from_name(table_name.into()),
            columns: IndexMap::new(),
            conditions: IndexMap::new(),
            next_condition_id: 1,
            order_by: IndexMap::new(),
            next_order_id: 1,
            refs: None,
            contained: Vec::new(),
            expressions: IndexMap::new(),
            pagination: None,
            title_field: None,
            title_fields: Vec::new(),
            id_field: None,
            invariants: IndexMap::new(),
        }
    }

    /// Convert this table to use a different entity type.
    ///
    /// Computed expressions are carried over — they're stored entity-erased
    /// (see [`ExpressionFn`]), so aggregates survive reference traversal that
    /// erases the entity to `EmptyEntity` (e.g. `get_ref_from_row`).
    pub fn into_entity<E2: Entity<T::Value>>(self) -> Table<T, E2> {
        Table {
            data_source: self.data_source,
            _phantom: PhantomData,
            source: self.source,
            columns: self.columns,
            conditions: self.conditions,
            next_condition_id: self.next_condition_id,
            order_by: self.order_by,
            next_order_id: self.next_order_id,
            refs: self.refs,
            contained: self.contained,
            expressions: self.expressions,
            pagination: self.pagination,
            title_field: self.title_field,
            title_fields: self.title_fields,
            id_field: self.id_field,
            invariants: self.invariants,
        }
    }

    /// Borrow this table as its entity-erased form `Table<T, EmptyEntity>`.
    ///
    /// `E` appears in `Table` only as `PhantomData<E>` (a zero-sized field), so
    /// `Table<T, E>` and `Table<T, EmptyEntity>` are layout-identical and this
    /// reinterpret is sound. Used to feed `self` to the entity-erased
    /// [`ExpressionFn`] closures at evaluation time.
    pub(crate) fn as_entity_erased(&self) -> &Table<T, EmptyEntity> {
        // SAFETY: identical layout (E is PhantomData only); lifetime is tied to
        // `&self`, and the borrow is shared/read-only.
        unsafe { &*(self as *const Table<T, E> as *const Table<T, EmptyEntity>) }
    }

    /// Snapshot the table's relations as Vista references (name, target type,
    /// cardinality, foreign key). Driver factories fold this into
    /// `VistaMetadata` so the erased `Vista` carries enough to drive nested
    /// insert and relation traversal.
    pub fn vista_references(&self) -> Vec<vantage_vista::Reference> {
        self.refs
            .as_ref()
            .map(|refs| {
                refs.iter()
                    .map(|(name, r)| {
                        vantage_vista::Reference::new(
                            name.clone(),
                            r.target_type_name().to_string(),
                            r.cardinality(),
                            r.foreign_key().to_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Shape-only specs (name, host, kind, id) for the contained relations
    /// declared on this table, for driver factories to fold into
    /// `VistaMetadata`. Columns are derived at traversal from each relation's
    /// `build_target` closure.
    pub fn vista_contained(&self) -> Vec<vantage_vista::ContainedSpec> {
        self.contained.iter().map(|c| c.spec()).collect()
    }

    /// Look up a contained relation by name (for the driver's traversal).
    pub fn contained_relation(
        &self,
        name: &str,
    ) -> Option<&crate::references::ContainedRelation<T>> {
        self.contained.iter().find(|c| c.name() == name)
    }

    /// Use a callback with a builder pattern for configuration
    pub fn with<F>(mut self, func: F) -> Self
    where
        F: FnOnce(&mut Self),
    {
        func(&mut self);
        self
    }

    /// Get the table name.
    ///
    /// For a query-sourced table this is its FROM alias.
    pub fn table_name(&self) -> &str {
        self.source.name()
    }

    /// The table's source (a name, or a query used as a derived source).
    pub fn source(&self) -> &T::Source {
        &self.source
    }

    /// Override the table name. Used by REST API drivers to swap a
    /// canonical resource path for a per-reference URI template at
    /// traversal time.
    ///
    /// This replaces the source with a name-based one, so it must not be
    /// called on a query-sourced (derived) table.
    pub fn set_table_name(&mut self, name: impl Into<String>) {
        self.source = T::Source::from_name(name.into());
    }

    /// Get the underlying data source
    pub fn data_source(&self) -> &T {
        &self.data_source
    }

    /// Get the title field column if set
    pub fn title_field(&self) -> Option<&T::Column<T::AnyType>> {
        self.title_field
            .as_ref()
            .and_then(|name| self.columns.get(name))
    }

    /// Names of columns marked as display titles (set via
    /// [`Self::with_title_column_of`]). These show alongside the id in
    /// list views and on the leading lines of single-record displays.
    pub fn title_fields(&self) -> &[String] {
        &self.title_fields
    }

    /// Get the id field column if set
    pub fn id_field(&self) -> Option<&T::Column<T::AnyType>> {
        self.id_field
            .as_ref()
            .and_then(|name| self.columns.get(name))
    }

    /// Mark an already-added column as the id field.
    ///
    /// Use this when the id column has been added via [`Self::add_column`]
    /// (so its type and aliases were chosen explicitly) and you only need
    /// to flag it. [`Self::with_id_column`] is the typed shortcut that
    /// creates the column for you.
    pub fn set_id_field(&mut self, name: impl Into<String>) {
        self.id_field = Some(name.into());
    }

    /// Mark an already-added column as a display title.
    ///
    /// Companion to [`Self::set_id_field`] for spec-driven construction.
    pub fn add_title_field(&mut self, name: impl Into<String>) {
        let name = name.into();
        if !self.title_fields.contains(&name) {
            self.title_fields.push(name.clone());
        }
        if self.title_field.is_none() {
            self.title_field = Some(name);
        }
    }

    /// Get the current pagination configuration, if set
    pub fn pagination(&self) -> Option<&Pagination> {
        self.pagination.as_ref()
    }

    /// Column values every row in this set must hold (see the `invariants`
    /// field): enforced on write — filled when null/absent, kept when matching,
    /// rejected when conflicting.
    pub fn invariants(&self) -> &IndexMap<String, T::Value> {
        &self.invariants
    }

    /// Register an invariant value for `column` on this set.
    ///
    /// A later call for the same column overwrites the earlier invariant.
    pub fn add_invariant(&mut self, column: impl Into<String>, value: T::Value) {
        self.invariants.insert(column.into(), value);
    }

    /// Builder form of [`Self::add_invariant`].
    pub fn with_invariant(mut self, column: impl Into<String>, value: T::Value) -> Self {
        self.add_invariant(column, value);
        self
    }
}

impl<T: TableSource, E: Entity<T::Value>> std::ops::Index<&str> for Table<T, E> {
    type Output = T::Column<T::AnyType>;

    fn index(&self, index: &str) -> &Self::Output {
        &self.columns[index]
    }
}

impl<T: TableSource, E: Entity<T::Value>> std::fmt::Debug for Table<T, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Table")
            .field("table_name", &self.table_name())
            .field("columns", &self.columns.keys().collect::<Vec<_>>())
            .field("conditions_count", &self.conditions.len())
            .field(
                "refs_count",
                &self.refs.as_ref().map(|r| r.len()).unwrap_or(0),
            )
            .field("expressions_count", &self.expressions.len())
            .finish()
    }
}
