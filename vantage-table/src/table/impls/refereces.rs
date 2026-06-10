//! Table relationship methods for defining and traversing references.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_dataset::WritableValueSet;
use vantage_expressions::Expression;
use vantage_types::{EmptyEntity, Entity, Record};

use crate::{
    column::flags::ColumnFlag,
    references::{ContainedRelation, HasMany, HasOne, Reference},
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

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

    /// Declare a `contains_one` relation: a single record embedded in
    /// `host_column` (e.g. a product's `inventory` object), surfaced as a
    /// sub-`Vista`. `build_target` builds the contained record's table —
    /// the same closure shape as [`with_one`](Self::with_one).
    ///
    /// ```rust,ignore
    /// .with_contained_one("inventory", "inventory", |db| {
    ///     Table::new("inventory", db).with_column_of::<i64>("stock")
    /// })
    /// ```
    pub fn with_contained_one(
        mut self,
        relation: &str,
        host_column: &str,
        build_target: impl Fn(T) -> Table<T, EmptyEntity> + Send + Sync + 'static,
    ) -> Self {
        self.contained.push(ContainedRelation::new(
            relation,
            host_column,
            vantage_vista::ContainedKind::ContainsOne,
            None,
            build_target,
        ));
        self
    }

    /// Declare a `contains_many` relation: an array of records embedded in
    /// `host_column` (e.g. an order's `lines`). `build_target` builds the
    /// contained record's table; `id_column` names the field used as each
    /// record's id (`None` → positional index).
    ///
    /// ```rust,ignore
    /// .with_contained_many("lines", "lines", |db| {
    ///     Table::new("lines", db)
    ///         .with_column_of::<Thing>("product")
    ///         .with_column_of::<i64>("quantity")
    /// }, None)
    /// ```
    pub fn with_contained_many(
        mut self,
        relation: &str,
        host_column: &str,
        build_target: impl Fn(T) -> Table<T, EmptyEntity> + Send + Sync + 'static,
        id_column: Option<&str>,
    ) -> Self {
        self.contained.push(ContainedRelation::new(
            relation,
            host_column,
            vantage_vista::ContainedKind::ContainsMany,
            id_column.map(str::to_string),
            build_target,
        ));
        self
    }

    /// Harvest this table's columns as Vista columns (name + declared type,
    /// hidden flag). Used to give a contained sub-Vista its schema.
    pub fn vista_columns(&self) -> Vec<vantage_vista::Column>
    where
        T::Column<T::AnyType>: ColumnLike<T::AnyType>,
    {
        self.columns()
            .iter()
            .map(|(name, col)| {
                let mut vc = vantage_vista::Column::new(name.clone(), col.get_type().to_string());
                if col.flags().contains(&ColumnFlag::Hidden) {
                    vc = vc.hidden();
                }
                vc
            })
            .collect()
    }

    /// Resolve a contained relation embedded in `row` into a sub-`Vista`.
    ///
    /// This is the backend-agnostic skeleton every driver shares: seed the
    /// embedded records, wire the eager writeback (patch the host column on the
    /// parent row), and the traverse-out resolver. The driver supplies only the
    /// three things it alone knows:
    /// - `parent_id` — the row's id in the driver's native id type;
    /// - `wrap` — turn a target `Table` into a `Vista` via the driver's factory
    ///   (used when a contained record traverses out to a real table);
    /// - `decode_host` / `encode_host` — the host-column codec: native
    ///   passthrough, or JSON parse/serialize for backends without nested
    ///   columns.
    #[allow(clippy::too_many_arguments)]
    pub fn get_contained_ref(
        &self,
        relation: &str,
        row: &Record<CborValue>,
        parent_id: T::Id,
        wrap: impl Fn(Table<T, EmptyEntity>) -> Result<vantage_vista::Vista> + Send + Sync + 'static,
        decode_host: impl Fn(&CborValue) -> Option<CborValue>,
        encode_host: impl Fn(CborValue) -> CborValue + Send + Sync + 'static,
    ) -> Result<vantage_vista::Vista>
    where
        T::Value: From<CborValue> + Send + Sync,
        T::Id: Clone + Send + Sync,
        T::Column<T::AnyType>: ColumnLike<T::AnyType>,
    {
        let rel = self
            .contained_relation(relation)
            .ok_or_else(|| error!("unknown contained relation", relation = relation))?;
        let host_value = row.get(rel.host_column()).and_then(decode_host);

        let contained_table = rel.build_target(self.data_source().clone());
        let mut spec = vantage_vista::ContainedSpec::new(rel.name(), rel.host_column(), rel.kind());
        if let Some(id) = rel.id_column() {
            spec = spec.with_id_column(id);
        }
        spec = spec.with_columns(contained_table.vista_columns());

        let host_column = rel.host_column().to_string();
        let parent_table = self.clone();
        let writeback: vantage_vista::ContainedWriteback =
            Arc::new(move |collection: CborValue| {
                let parent_table = parent_table.clone();
                let host_column = host_column.clone();
                let parent_id = parent_id.clone();
                let value = T::Value::from(encode_host(collection));
                Box::pin(async move {
                    let mut patch: Record<T::Value> = Record::new();
                    patch.insert(host_column, value);
                    parent_table.patch_value(parent_id.clone(), &patch).await?;
                    Ok(())
                })
            });

        let ref_resolver: vantage_vista::ContainedRefResolver =
            Arc::new(move |relation: &str, child_row: &Record<CborValue>| {
                let native: Record<T::Value> = child_row
                    .iter()
                    .map(|(k, v)| (k.clone(), T::Value::from(v.clone())))
                    .collect();
                let target = contained_table.get_ref_from_row::<EmptyEntity>(relation, &native)?;
                wrap(target)
            });

        vantage_vista::build_contained_vista(
            &spec,
            host_value.as_ref(),
            writeback,
            Some(ref_resolver),
        )
    }

    /// Lower a YAML `contained:` section into `with_contained_*` registrations,
    /// reusing the driver's `build_col` to construct each contained record's
    /// columns. Column-build errors surface here; the per-relation target
    /// closure stays infallible by cloning the pre-built columns.
    pub fn with_contained_specs<C>(
        mut self,
        specs: &IndexMap<String, vantage_vista::ContainedYaml<C>>,
        build_col: impl Fn(&str, &vantage_vista::ColumnSpec<C>) -> Result<T::Column<T::AnyType>>,
    ) -> Result<Self>
    where
        T::Column<T::AnyType>: Clone,
    {
        for (relation, c) in specs {
            let cols = c
                .columns
                .iter()
                .map(|(n, cs)| build_col(n, cs))
                .collect::<Result<Vec<_>>>()?;
            let rel = relation.clone();
            let host = c.host_column.clone();
            let build = move |db: T| {
                let mut t = Table::<T, EmptyEntity>::new(rel.clone(), db);
                for col in &cols {
                    t.add_column(col.clone());
                }
                t
            };
            self = match c.kind {
                vantage_vista::ContainedKind::ContainsOne => {
                    self.with_contained_one(relation, &host, build)
                }
                vantage_vista::ContainedKind::ContainsMany => {
                    self.with_contained_many(relation, &host, build, c.id_column.as_deref())
                }
            };
        }
        Ok(self)
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

    /// Borrow this table's relations, if any.
    pub(crate) fn refs_ref(&self) -> Option<&IndexMap<String, Arc<dyn Reference>>> {
        self.refs.as_ref()
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
        let Some(refs) = other.refs_ref() else {
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
        let condition = self.data_source().eq_value_condition(&id_name, id.into())?;
        self.add_condition(condition);
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
        let (reference, _) = self.lookup_ref(relation)?;
        let source_id = self
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let target_dyn = reference.resolve_from_row(
            self.data_source() as &dyn std::any::Any,
            &source_id,
            row as &dyn std::any::Any,
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
        let (reference, relation_str) = self.lookup_ref(relation)?;

        let source_id = self
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let mut target: Table<T, E2> = *reference
            .build_target(self.data_source() as &dyn std::any::Any)
            .downcast::<Table<T, E2>>()
            .map_err(|_| {
                error!(
                    "Failed to downcast related table",
                    relation = relation_str.as_str()
                )
            })?;

        let target_id = target
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

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
        let (reference, relation_str) = self.lookup_ref(relation)?;

        let source_id = self
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let mut target: Table<T, E2> = *reference
            .build_target(self.data_source() as &dyn std::any::Any)
            .downcast::<Table<T, E2>>()
            .map_err(|_| {
                error!(
                    "Failed to downcast related table",
                    relation = relation_str.as_str()
                )
            })?;

        let target_id = target
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let (src_col, tgt_col) = reference.columns(&source_id, &target_id);

        let condition = self.data_source().related_correlated_condition(
            target.table_name(),
            &tgt_col,
            self.table_name(),
            &src_col,
        );
        target.add_condition(condition);

        Ok(target)
    }

    /// Build the relation's target table with **no condition** applied.
    ///
    /// Unlike [`Self::get_ref_from_row`] / [`Self::get_ref_as`] (which select
    /// the related rows for a known parent), this returns the bare target — the
    /// table you'd insert a new related row into. Used by Vista's nested insert
    /// to obtain the destination for a has-one/has-many child before any join
    /// value exists.
    pub fn get_ref_target<E2: Entity<T::Value> + 'static>(
        &self,
        relation: &str,
    ) -> Result<Table<T, E2>> {
        let (reference, relation_str) = self.lookup_ref(relation)?;
        let target: Table<T, E2> = *reference
            .build_target(self.data_source() as &dyn std::any::Any)
            .downcast::<Table<T, E2>>()
            .map_err(|_| {
                error!(
                    "Failed to downcast related table",
                    relation = relation_str.as_str()
                )
            })?;
        Ok(target)
    }

    /// Add a computed expression field using builder pattern.
    ///
    /// The closure receives `&Table<T, E>` and returns an `Expression<T::Value>`.
    /// It is evaluated lazily when `select()` builds the query.
    pub fn with_expression(
        mut self,
        name: &str,
        expr_fn: impl Fn(&Table<T, E>) -> Expression<T::Value> + Send + Sync + 'static,
    ) -> Self {
        self.expressions.insert(name.to_string(), Arc::new(expr_fn));
        self
    }

    fn lookup_ref(&self, relation: &str) -> Result<(&dyn Reference, String)> {
        let table_name = self.table_name().to_string();
        let refs = self.refs.as_ref().ok_or_else(|| {
            error!(
                "No references defined on table",
                table = table_name.as_str()
            )
        })?;

        let relation_str = relation.to_string();
        let reference = refs.get(relation).ok_or_else(|| {
            error!(
                "Reference not found on table",
                relation = relation_str.as_str(),
                table = table_name.as_str()
            )
        })?;

        Ok((reference.as_ref(), relation_str))
    }

    /// Look up cardinality for a registered relation.
    pub fn ref_cardinality(&self, relation: &str) -> Result<vantage_vista::ReferenceKind> {
        let (reference, _) = self.lookup_ref(relation)?;
        Ok(reference.cardinality())
    }

    /// List all registered relations with their cardinality.
    pub fn ref_kinds(&self) -> Vec<(String, vantage_vista::ReferenceKind)> {
        self.refs
            .as_ref()
            .map(|refs| {
                refs.iter()
                    .map(|(name, r)| (name.clone(), r.cardinality()))
                    .collect()
            })
            .unwrap_or_default()
    }
}
