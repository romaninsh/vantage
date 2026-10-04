//! Lowering a YAML [`VistaSpec`] onto a table:
//! the steps every driver factory shares. Type mapping and driver blocks stay
//! with the driver, passed in as `build` functions.

use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_expressions::{Expressive, SelectableDataSource};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{ColumnSpec, ReferenceKind, ReferenceSpec, SpecResolver, VistaSpec, flags};

use crate::{source::SelectSource, table::Table, traits::table_source::TableSource};

/// Rebuilds a referenced table from its spec, a data source and the resolver.
type SpecTableBuilder<S, T> = fn(&S, T, Option<SpecResolver<S>>) -> Result<Table<T, EmptyEntity>>;

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// Add one spec column. A `lazy:` column becomes a computed column;
    /// any other is built by `build` (unless the table already has it, as a
    /// derived table may) and registered as a title when flagged `title`.
    /// Returns whether the column is stored.
    pub fn add_spec_column<C>(
        &mut self,
        name: &str,
        spec: &ColumnSpec<C>,
        build: impl FnOnce(&str, &ColumnSpec<C>) -> Result<T::Column<T::AnyType>>,
    ) -> Result<bool> {
        if self.add_lazy_spec_column(spec, name)? {
            return Ok(false);
        }
        if !self.columns.contains_key(name) {
            self.add_column(build(name, spec)?);
        }
        if spec.has_flag(flags::TITLE) {
            self.add_title_field(name);
        }
        Ok(true)
    }

    /// [`add_spec_column`](Self::add_spec_column) for each column, in order.
    pub fn add_spec_columns<C>(
        &mut self,
        columns: &IndexMap<String, ColumnSpec<C>>,
        build: impl Fn(&str, &ColumnSpec<C>) -> Result<T::Column<T::AnyType>>,
    ) -> Result<()> {
        for (name, spec) in columns {
            self.add_spec_column(name, spec, &build)?;
        }
        Ok(())
    }

    /// Mark the spec's id column as the id field. Errors when the spec
    /// doesn't declare that column.
    pub fn set_spec_id_field(&mut self, id: &str) -> Result<()> {
        if !self.columns.contains_key(id) {
            return Err(error!("id column not present in spec.columns", id = id));
        }
        self.set_id_field(id);
        Ok(())
    }
}

impl<T: TableSource + 'static, E: Entity<T::Value> + 'static> Table<T, E>
where
    T::Value: Into<ciborium::Value> + From<ciborium::Value>,
    T::Id: std::fmt::Display + From<String>,
{
    /// Register each spec `references:` entry as `with_one` / `with_many`.
    ///
    /// At traversal time the reference asks `resolver` for the target's
    /// current spec and rebuilds it with `build`. Without a resolver, or on a
    /// miss or a build error, the target is a column-less `Table::new(target)`
    /// and the next query against it fails.
    pub fn with_spec_references<R, S: 'static>(
        mut self,
        references: &IndexMap<String, ReferenceSpec<R>>,
        resolver: Option<SpecResolver<S>>,
        build: SpecTableBuilder<S, T>,
    ) -> Self {
        for (relation, reference) in references {
            let target = reference.table.clone();
            let foreign_key = reference.foreign_key_or(relation);
            let resolver = resolver.clone();
            let build_target = move |source: T| -> Table<T, EmptyEntity> {
                if let Some(r) = &resolver
                    && let Some(spec) = r(&target)
                    && let Ok(table) = build(&spec, source.clone(), Some(r.clone()))
                {
                    return table;
                }
                Table::new(target.clone(), source)
            };
            self = match reference.kind {
                ReferenceKind::HasOne => self.with_one(relation, &foreign_key, build_target),
                ReferenceKind::HasMany => self.with_many(relation, &foreign_key, build_target),
            };
        }
        self
    }
}

// Same bounds as `Table::derive_from`, which this builds on.
impl<T, E, V, Cond, S> Table<T, E>
where
    T: SelectableDataSource<V, Cond, Select = S>
        + TableSource<Value = V, Condition = Cond, Source = SelectSource<S>>,
    V: Clone + Send + Sync + 'static + From<String>,
    Cond: Clone + Send + Sync + 'static,
    S: Expressive<V> + Clone,
    E: Entity<V>,
{
    /// Lower a `base:` spec: a table over `select` (the base's select, possibly
    /// transformed) that inherits the listed base columns and relations, adds
    /// the spec's own columns (aggregate outputs, say) and contained
    /// relations, and keeps the base's id unless the spec names one.
    pub fn derive_from_spec<E2: Entity<V> + 'static, X, C, R>(
        base: &Table<T, E2>,
        spec: &VistaSpec<X, C, R>,
        select: S,
        inherit_columns: &[String],
        inherit_relations: &[String],
        build_column: impl Fn(&str, &ColumnSpec<C>) -> Result<T::Column<T::AnyType>>,
    ) -> Result<Self>
    where
        T: 'static,
        E: 'static,
    {
        let columns: Vec<&str> = inherit_columns.iter().map(String::as_str).collect();
        let relations: Vec<&str> = inherit_relations.iter().map(String::as_str).collect();
        let mut table = Self::derive_from(
            base,
            spec.name.clone(),
            move |_| select,
            &columns,
            &relations,
        );
        table.add_spec_columns(&spec.columns, &build_column)?;
        if let Some(id) = &spec.id_column {
            table.set_id_field(id);
        }
        table.with_contained_specs(&spec.contained, build_column)
    }
}
