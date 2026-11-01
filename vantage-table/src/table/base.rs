use std::marker::PhantomData;
use std::sync::Arc;

use indexmap::IndexMap;
use vantage_expressions::Expression;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::Denials;

use crate::{
    pagination::Pagination, references::Reference, sorting::SortDirection, table::hooks::Hooks,
    traits::table_source::TableSource, traits::table_source_spec::TableSourceSpec,
};

mod accessors;
mod computed;
mod relations;

/// An expression closure stored on a table, against the entity-erased table so
/// it survives [`Table::into_entity`]. See [`Table::with_expression`].
pub type ExpressionFn<T> =
    Arc<dyn Fn(&Table<T, EmptyEntity>) -> Expression<<T as TableSource>::Value> + Send + Sync>;

/// A lazy-expression callback stored on a table: runs on each returned record
/// and sees the columns earlier callbacks produced. See
/// [`Table::with_lazy_expression`].
pub type LazyExpressionFn<T> = Arc<
    dyn Fn(
            &vantage_types::Record<<T as TableSource>::Value>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = vantage_core::Result<<T as TableSource>::Value>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

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
    pub(super) lazy_expressions: IndexMap<String, LazyExpressionFn<T>>,
    /// Vista-computed columns with their position among all columns, stored
    /// and computed. The table never selects or writes them.
    pub(super) computed_columns: IndexMap<String, (usize, vantage_vista::Column)>,
    /// When `Some`, `select()` projects only these names (plus the id column);
    /// dotted names are implicit references. See [`Self::with_active_columns`].
    pub(super) active_columns: Option<indexmap::IndexSet<String>>,
    /// Dotted implicit-reference columns (`"country.name"`): read-only
    /// projections that write paths must never persist (a SCHEMALESS store
    /// would create a literal `country.name` field).
    pub(super) imported_columns: indexmap::IndexSet<String>,
    pub(super) pagination: Option<Pagination>,
    pub(super) title_field: Option<String>,
    pub(super) title_fields: Vec<String>,
    pub(super) id_field: Option<String>,
    /// See [`Self::with_text_id`].
    pub(super) id_text: bool,
    /// See [`Self::with_auto_id`].
    pub(super) id_auto: bool,
    /// See [`Self::invariants`].
    pub(super) invariants: IndexMap<String, T::Value>,
    pub(super) hooks: Hooks<T>,
    /// See [`Self::set_lock`].
    pub(super) lock: bool,
    /// See [`Self::deny`].
    pub(super) denials: Denials,
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
            lazy_expressions: IndexMap::new(),
            computed_columns: IndexMap::new(),
            active_columns: None,
            imported_columns: indexmap::IndexSet::new(),
            pagination: None,
            title_field: None,
            title_fields: Vec::new(),
            id_field: None,
            id_text: false,
            id_auto: false,
            invariants: IndexMap::new(),
            hooks: Hooks::default(),
            lock: false,
            denials: Denials::default(),
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
            lazy_expressions: self.lazy_expressions,
            computed_columns: self.computed_columns,
            active_columns: self.active_columns,
            imported_columns: self.imported_columns,
            pagination: self.pagination,
            title_field: self.title_field,
            title_fields: self.title_fields,
            id_field: self.id_field,
            id_text: self.id_text,
            id_auto: self.id_auto,
            invariants: self.invariants,
            hooks: self.hooks,
            lock: self.lock,
            denials: self.denials,
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
