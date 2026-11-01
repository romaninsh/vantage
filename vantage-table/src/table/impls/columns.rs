use indexmap::IndexMap;
use vantage_expressions::{Expression, Expressive, traits::datasource::ExprDataSource};
use vantage_types::Entity;

use crate::{
    column::core::ColumnType, prelude::ColumnLike, table::Table, traits::table_source::TableSource,
};

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// Add a column to the table (accepts any typed column, converts to `Column<AnyType>`)
    pub fn add_column<NewColumnType>(&mut self, column: T::Column<NewColumnType>)
    where
        NewColumnType: ColumnType,
    {
        let name = column.name().to_string();

        if self.columns.contains_key(&name) {
            panic!("Duplicate column: {}", name);
        }

        let any_column = self.data_source.to_any_column(column);
        self.columns.insert(name, any_column);
    }

    /// Add a column using builder pattern
    pub fn with_column<NewColumnType>(mut self, column: T::Column<NewColumnType>) -> Self
    where
        NewColumnType: ColumnType,
    {
        self.add_column(column);
        self
    }

    /// Copy column definitions from another table, skipping any whose name is
    /// already present. With `names = None`, copies all columns; otherwise only
    /// the listed ones. Used to inherit columns when deriving a table from
    /// another (see `Table::derive_from`).
    pub fn copy_columns_from<E2: Entity<T::Value>>(
        &mut self,
        other: &Table<T, E2>,
        names: Option<&[&str]>,
    ) {
        for col in other.columns().values() {
            let name = col.name();
            if names.is_some_and(|ns| !ns.contains(&name)) {
                continue;
            }
            if !self.columns.contains_key(name) {
                self.add_column(col.clone());
            }
        }
    }

    /// Add a typed column to the table (mutable)
    pub fn add_column_of<NewColumnType>(&mut self, name: impl Into<String>)
    where
        NewColumnType: ColumnType,
    {
        let column = self
            .data_source
            .create_column::<NewColumnType>(&name.into());
        self.add_column(column);
    }

    /// Add an ID column — sets both the column and the id_field flag.
    pub fn with_id_column(mut self, name: impl Into<String>) -> Self
    where
        T::Id: ColumnType,
    {
        let name = name.into();
        self.id_field = Some(name.clone());
        let column = self.data_source.create_column::<T::Id>(&name);
        self.add_column(column);
        self
    }

    /// Mark the id column as a text/string key so backends do not numerically
    /// coerce it. Use for models whose id column is `TEXT` even when some ids
    /// look numeric (e.g. ids from an external API mixed with generated UUIDs);
    /// without this the Postgres backend binds an all-digit id like `"121"` as
    /// `bigint`, which fails against a `TEXT` id column.
    pub fn with_text_id(mut self) -> Self {
        self.id_text = true;
        self
    }

    /// Whether the id column is a text key (see [`Self::with_text_id`]).
    pub fn id_is_text(&self) -> bool {
        self.id_text
    }

    /// Mark the id column as backend-made (SQL auto-increment, SurrealDB
    /// generated record ids, MongoDB ObjectIds). Vista metadata then flags it
    /// [`ColumnFlag::Auto`](vantage_vista::ColumnFlag::Auto).
    pub fn with_auto_id(mut self) -> Self {
        self.id_auto = true;
        self
    }

    /// Whether the backend makes this table's ids (see [`Self::with_auto_id`]).
    pub fn has_auto_id(&self) -> bool {
        self.id_auto
    }

    /// Add a typed column AND mark it as a display title.
    ///
    /// Title columns show alongside the id in generic list views and
    /// lead the body of single-record displays. Multiple title columns
    /// are allowed; their order matches the order of these calls.
    pub fn with_title_column_of<NewColumnType>(mut self, name: impl Into<String>) -> Self
    where
        NewColumnType: ColumnType,
    {
        let name = name.into();
        if !self.title_fields.contains(&name) {
            self.title_fields.push(name.clone());
        }
        if self.title_field.is_none() {
            self.title_field = Some(name.clone());
        }
        let column = self.data_source.create_column::<NewColumnType>(&name);
        self.add_column(column);
        self
    }

    /// Add a pre-built column AND mark it as a display title — the
    /// flag-carrying twin of [`Self::with_title_column_of`], for callers
    /// that need flags (searchable, mandatory) on a title column.
    pub fn with_title_column<NewColumnType>(mut self, column: T::Column<NewColumnType>) -> Self
    where
        NewColumnType: ColumnType,
    {
        let name = column.name().to_string();
        if !self.title_fields.contains(&name) {
            self.title_fields.push(name.clone());
        }
        if self.title_field.is_none() {
            self.title_field = Some(name);
        }
        self.add_column(column);
        self
    }

    /// Add a typed column to the table (builder pattern)
    pub fn with_column_of<NewColumnType>(self, name: impl Into<String>) -> Self
    where
        NewColumnType: ColumnType,
    {
        let column = self
            .data_source
            .create_column::<NewColumnType>(&name.into());
        self.with_column(column)
    }

    /// Take a column out of the table: stored, expression, lazy or
    /// vista-computed, so it is neither selected nor shown. Invariants and
    /// conditions naming it stay, so inserts are still stamped with its
    /// value and rows are still narrowed by it; references hold their
    /// foreign key by name and keep traversing. A write that sets it is
    /// refused `Forbidden`, unless the value is the column's invariant.
    pub fn remove_column(&mut self, name: &str) -> vantage_core::Result<()> {
        if name == self.id_field_name() {
            return Err(vantage_core::error!(format!(
                "`{name}` is the id column of `{}`; it can't be removed",
                self.table_name()
            )));
        }
        let mut found = false;
        if let Some((stored_index, _, _)) = self.columns.shift_remove_full(name) {
            found = true;
            let removed = self.position_of_stored(stored_index);
            for (at, _) in self.computed_columns.values_mut() {
                if *at > removed {
                    *at -= 1;
                }
            }
        }
        if let Some((removed, _)) = self.computed_columns.shift_remove(name) {
            found = true;
            for (at, _) in self.computed_columns.values_mut() {
                if *at > removed {
                    *at -= 1;
                }
            }
        }
        found |= self.expressions.shift_remove(name).is_some();
        found |= self.lazy_expressions.shift_remove(name).is_some();
        if !found {
            return Err(vantage_core::error!(format!(
                "no column `{name}` in `{}`",
                self.table_name()
            )));
        }
        self.imported_columns.shift_remove(name);
        self.removed_columns.insert(name.to_string());
        if let Some(active) = &mut self.active_columns {
            active.shift_remove(name);
        }
        self.title_fields.retain(|title| title != name);
        if self.title_field.as_deref() == Some(name) {
            self.title_field = self.title_fields.first().cloned();
        }
        Ok(())
    }

    /// Get all columns as type-erased columns (`Column<AnyType>`)
    pub fn columns(&self) -> &IndexMap<String, T::Column<T::AnyType>> {
        &self.columns
    }

    /// Get a typed column by converting from stored `Column<AnyType>`
    pub fn get_column<Type>(&self, name: &str) -> Option<T::Column<Type>>
    where
        Type: ColumnType,
    {
        let any_column = self.columns.get(name)?;
        self.data_source
            .convert_any_column::<Type>(any_column.clone())
    }

    /// Get an expression for a column or computed expression by name.
    ///
    /// If `name` matches a registered expression (from `with_expression`), evaluates
    /// and returns it. Otherwise returns the column as an expression. Returns `None`
    /// if the name doesn't match either.
    pub fn get_column_expr(&self, name: &str) -> Option<vantage_expressions::Expression<T::Value>>
    where
        T::Column<T::AnyType>: vantage_expressions::Expressive<T::Value>,
    {
        if let Some(expr_fn) = self.expressions.get(name) {
            Some(expr_fn(self.as_entity_erased()))
        } else {
            use vantage_expressions::Expressive;
            self.columns.get(name).map(|c| c.expr())
        }
    }
}

impl<T, E> Table<T, E>
where
    T: TableSource + ExprDataSource<T::Value>,
    E: Entity<T::Value> + 'static,
{
    /// Expression yielding all values of the named column under the
    /// table's current conditions.
    ///
    /// SQL backends materialise this as a `SELECT col FROM tbl WHERE …`
    /// subquery (embeddable directly into IN clauses); non-query
    /// backends wrap a `DeferredFn` that runs `list_table_values` and
    /// projects the column at execute time.
    ///
    /// Panics if `column_name` isn't a column on this table — the
    /// callsite is meant to be a literal column reference, so a typo
    /// is a programmer error, not a runtime failure mode worth
    /// surfacing as `Result`.
    pub fn column_values_expr(&self, column_name: &str) -> Expression<T::Value> {
        let col = self
            .get_column::<T::AnyType>(column_name)
            .unwrap_or_else(|| panic!("column {column_name:?} not found on table"));
        self.data_source.column_table_values_expr(self, &col).expr()
    }
}
