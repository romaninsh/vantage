use std::marker::PhantomData;
use std::sync::Arc;

use indexmap::IndexMap;
use vantage_expressions::Expression;
use vantage_types::{EmptyEntity, Entity};

use crate::{
    pagination::Pagination, references::Reference, sorting::SortDirection, table::hooks::Hooks,
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

/// Type alias for lazy-expression callbacks stored on Table.
///
/// A lazy expression runs *after* the data source returns a record.
/// Callbacks apply in declaration order: each borrows the record as built
/// so far, and the value it returns is inserted under the expression's
/// name. A later lazy expression therefore sees the columns produced by
/// earlier ones — one expensive fetch (a file's contents) can feed several
/// cheap derived columns. See [`Table::with_lazy_expression`].
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
    /// Columns the `Vista` computes after each read (see
    /// `vantage_vista::Column::with_expression`). The table never selects or
    /// writes them; it carries them so a Vista built from this table — a
    /// traversal target included — knows them. Each carries its position
    /// among all columns, stored and computed, as declared.
    pub(super) computed_columns: IndexMap<String, (usize, vantage_vista::Column)>,
    /// When `Some`, `select()` projects only these column names (plus the id
    /// column, always). `None` keeps the default "project every column"
    /// behavior. The set holds both plain column names and dotted implicit
    /// references (`"country.name"`); set via [`Self::with_active_columns`].
    pub(super) active_columns: Option<indexmap::IndexSet<String>>,
    /// Dotted implicit-reference columns imported by traversal
    /// (`"country.name"`). These are read-only, expression-backed projections;
    /// they are tracked here so write paths never persist them (a SCHEMALESS
    /// store would otherwise create a literal `country.name` field).
    pub(super) imported_columns: indexmap::IndexSet<String>,
    pub(super) pagination: Option<Pagination>,
    pub(super) title_field: Option<String>,
    pub(super) title_fields: Vec<String>,
    pub(super) id_field: Option<String>,
    /// When true, the id column is a text/string key and backends must NOT
    /// numerically coerce it (e.g. the Postgres backend otherwise binds an
    /// all-digit id like `"121"` as `bigint`, which breaks against a `TEXT` id
    /// column). Set via [`Self::with_text_id`]. Defaults to false to preserve
    /// the integer-id convention used by other models.
    pub(super) id_text: bool,
    /// When true, the backend makes this table's ids (auto-increment,
    /// generated record ids). Set via [`Self::with_auto_id`]; Vista metadata
    /// flags the id column `auto`.
    pub(super) id_auto: bool,
    /// Column values every row in this set must hold, because they are part of
    /// the set's definition (e.g. a has-many child carries the parent's foreign
    /// key). Registered wherever the table is narrowed by a literal
    /// `column = value` (see [`Self::with_id`], `Reference::resolve_from_row`);
    /// never from an expression scope (and every literal `column = value`
    /// passed to `add_condition`). Enforced on write: insert and replace fill a
    /// null/absent column and reject a conflicting one; patch never fills, and
    /// rejects a present value that differs (or is null).
    pub(super) invariants: IndexMap<String, T::Value>,
    /// Lifecycle hooks (see [`Hook`](super::Hook)). Registered via [`Self::with_hook`].
    pub(super) hooks: Hooks<T>,
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

    /// Apply lazy expressions to one returned record, in declaration order.
    /// Each callback borrows the record as built so far; the value it
    /// returns is inserted under the expression's name before the next
    /// callback runs. See [`Self::with_lazy_expression`].
    /// Public so driver shells that bypass the `list_values` read path
    /// (e.g. a REST shell's windowed fetch) can still apply lazy columns.
    pub async fn apply_lazy_expressions(
        &self,
        record: &mut vantage_types::Record<T::Value>,
    ) -> vantage_core::Result<()> {
        for (name, f) in &self.lazy_expressions {
            let value = f(record).await?;
            record.insert(name.clone(), value);
        }
        Ok(())
    }

    /// Drop imported implicit-reference columns (`"country.name"`) from a write
    /// payload. They are read-only, expression-backed projections that no
    /// backend can honestly store; a round-trip (read → modify → save) would
    /// otherwise carry them back, and a SCHEMALESS store would create a literal
    /// `country.name` field. Called before invariants on the full-record write
    /// paths (insert, insert-returning-id, replace); `patch_value` instead
    /// rejects imported keys outright, since a partial payload is explicit
    /// intent per key.
    pub(super) fn strip_imported_columns(&self, record: &mut vantage_types::Record<T::Value>) {
        for name in &self.imported_columns {
            record.shift_remove(name);
        }
    }

    /// Whether `name` is an imported implicit-reference column
    /// (`"country.name"`) — an expression-backed traversal projection that is
    /// read-only and must never be persisted, ordered, or searched as if it
    /// were a physical field.
    pub fn is_imported_column(&self, name: &str) -> bool {
        self.imported_columns.contains(name)
    }

    /// Whether `name` is computed rather than stored: an imported
    /// implicit-reference column, a server-side expression column, or a lazy
    /// (post-fetch) computed column. Driver factories flag such columns
    /// `calculated` in vista metadata so consumers render them read-only.
    pub fn is_calculated_column(&self, name: &str) -> bool {
        self.imported_columns.contains(name)
            || self.expressions.contains_key(name)
            || self.lazy_expressions.contains_key(name)
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

    /// Register a column the `Vista` computes after each read, positioned
    /// after every column added so far. Driver factories fold these into
    /// `VistaMetadata`. A stored column of the same name (one inherited by a
    /// derived table) is replaced, so the column is never both read and
    /// computed.
    pub fn add_computed_column(&mut self, column: vantage_vista::Column) {
        if let Some((stored_index, _, _)) = self.columns.shift_remove_full(&column.name) {
            let removed = self.position_of_stored(stored_index);
            for (at, _) in self.computed_columns.values_mut() {
                if *at > removed {
                    *at -= 1;
                }
            }
        }
        let at = self.columns.len() + self.computed_columns.len();
        self.computed_columns
            .insert(column.name.clone(), (at, column));
    }

    /// Register `spec`'s column as computed when it declares a `lazy:` script
    /// (see [`vantage_vista::ColumnSpec::lazy_column`]). Returns `false` for a
    /// stored column, leaving the caller to add it.
    pub fn add_lazy_spec_column<C>(
        &mut self,
        spec: &vantage_vista::ColumnSpec<C>,
        name: &str,
    ) -> vantage_core::Result<bool> {
        let Some(column) = spec.lazy_column(name)? else {
            return Ok(false);
        };
        self.add_computed_column(column);
        Ok(true)
    }

    /// Position among all columns of the stored column at `stored_index`:
    /// computed columns occupy their recorded positions, stored columns fill
    /// the remaining slots in order.
    fn position_of_stored(&self, stored_index: usize) -> usize {
        let mut computed: Vec<usize> = self.computed_columns.values().map(|(at, _)| *at).collect();
        computed.sort_unstable();
        let mut at = stored_index;
        for position in computed {
            if position <= at {
                at += 1;
            }
        }
        at
    }

    /// Columns registered via [`Self::add_computed_column`], in order, each
    /// with its position among all columns.
    pub fn computed_columns(&self) -> impl Iterator<Item = (usize, &vantage_vista::Column)> {
        self.computed_columns.values().map(|(at, c)| (*at, c))
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

    /// Refuse a by-id write through a conditioned table whose source can't
    /// keep it inside the set (see [`TableSource::can_confine_writes`]). An
    /// unconditioned table writes anywhere.
    pub(crate) fn require_confined_writes(&self, verb: &'static str) -> vantage_core::Result<()> {
        if self.conditions.is_empty() || self.data_source.can_confine_writes() {
            return Ok(());
        }
        Err(vantage_core::error!(
            "this data source can't keep a write inside a narrowed table",
            verb = verb,
            table = self.table_name()
        )
        .mark_unsupported())
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

    /// Name of the [`id_field`](Self::id_field) column, or `"id"` when there
    /// is none.
    pub fn id_field_name(&self) -> String {
        use crate::traits::column_like::ColumnLike;
        self.id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string())
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
    /// field): insert and replace fill a null/absent column and reject a
    /// conflicting one; patch never fills, and rejects a present value that
    /// differs (or is null).
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::mock_table_source::MockTableSource;

    #[test]
    fn computed_column_replacing_a_stored_one_keeps_positions() {
        let mut table = Table::<MockTableSource, EmptyEntity>::new("t", MockTableSource::new())
            .with_column_of::<String>("a")
            .with_column_of::<String>("b");
        table.add_computed_column(vantage_vista::Column::new("x", "string"));
        table.add_column_of::<String>("c");
        assert_eq!(
            table
                .computed_columns()
                .map(|(at, c)| (at, c.name.as_str()))
                .collect::<Vec<_>>(),
            vec![(2, "x")]
        );

        table.add_computed_column(vantage_vista::Column::new("a", "string"));

        let stored: Vec<&str> = table.columns().keys().map(String::as_str).collect();
        assert_eq!(stored, vec!["b", "c"]);
        assert_eq!(
            table
                .computed_columns()
                .map(|(at, c)| (at, c.name.as_str()))
                .collect::<Vec<_>>(),
            vec![(1, "x"), (3, "a")]
        );
    }
}
