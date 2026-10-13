//! Table relationship methods for defining and traversing references.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_dataset::WritableValueSet;
use vantage_expressions::Expression;
use vantage_types::{EmptyEntity, Entity, Record};

use crate::{
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
        self.contained
            .push(ContainedRelation::one(relation, host_column, build_target));
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
        self.contained.push(ContainedRelation::many(
            relation,
            host_column,
            id_column.map(str::to_string),
            build_target,
        ));
        self
    }

    /// Resolve a contained relation embedded in a parent row into a
    /// sub-`Vista`.
    ///
    /// This is the backend-agnostic skeleton every driver shares: seed the
    /// embedded records, wire the eager writeback (patch the host column on the
    /// parent row), and the traverse-out resolver. The contained records'
    /// relations are the contained table's own; the sub-Vista narrows each
    /// bare target by the relation's keys. The driver supplies only the
    /// things it alone knows:
    /// - `parent` — the parent row and its id in the driver's native id
    ///   type, or `None` for the relation's bare target: the contained
    ///   schema with no records and no writes;
    /// - `wrap` — turn a relation's bare target `Table` into a `Vista` via the
    ///   driver's factory (used when a contained record traverses out to a
    ///   real table);
    /// - `decode_host` / `encode_host` — the host-column codec: native
    ///   passthrough, or JSON parse/serialize for backends without nested
    ///   columns.
    pub fn get_contained_ref(
        &self,
        relation: &str,
        parent: Option<(&Record<CborValue>, T::Id)>,
        wrap: impl Fn(Table<T, EmptyEntity>) -> Result<vantage_vista::Vista> + Send + Sync + 'static,
        decode_host: impl Fn(&CborValue) -> Option<CborValue>,
        encode_host: impl Fn(CborValue) -> CborValue + Send + Sync + 'static,
    ) -> Result<vantage_vista::Vista>
    where
        T::Value: From<CborValue> + Send + Sync + vantage_types::InvariantValue,
        T::Id: Clone + Send + Sync,
        T::Column<T::AnyType>: ColumnLike<T::AnyType>,
    {
        let rel = self
            .contained_relation(relation)
            .ok_or_else(|| error!("unknown contained relation", relation = relation))?;

        let contained_table = rel.build_target(self.data_source().clone());
        let mut schema = vantage_vista::VistaMetadata::new();
        for column in contained_table.vista_columns() {
            schema = schema.with_column(column);
        }
        // A contained record's own key is its relation's id column.
        let source_id = rel
            .id_column()
            .map_or_else(|| contained_table.id_field_name(), str::to_string);
        // The resolver is keyed by target table name; each target is built
        // through the first relation that names it.
        let mut relation_by_target: IndexMap<String, String> = IndexMap::new();
        for reference in contained_table.vista_references_keyed(&source_id) {
            relation_by_target
                .entry(reference.target.clone())
                .or_insert_with(|| reference.name.clone());
            schema = schema.with_reference(reference);
        }
        if let Some(id) = rel.id_column() {
            schema = schema.with_id_column(id);
        }

        let (host_value, writeback) = match parent {
            Some((row, parent_id)) => (
                row.get(rel.host_column()).and_then(decode_host),
                Some(self.host_writeback(rel.host_column(), parent_id, encode_host)),
            ),
            None => (None, None),
        };

        let ref_resolver: Arc<dyn vantage_vista::VistaResolver> = Arc::new(move |target: &str| {
            let relation = relation_by_target.get(target).ok_or_else(|| {
                error!("unknown relation target", target = target).mark_not_found()
            })?;
            wrap(contained_table.get_ref_target_erased(relation)?)
        });

        vantage_vista::build_contained_vista(
            &rel.reference(),
            schema,
            host_value.as_ref(),
            writeback,
            Some(ref_resolver),
        )
    }

    /// Patches `host_column` on the parent row `parent_id` with each
    /// re-serialized collection, encoded by `encode_host`.
    fn host_writeback(
        &self,
        host_column: &str,
        parent_id: T::Id,
        encode_host: impl Fn(CborValue) -> CborValue + Send + Sync + 'static,
    ) -> vantage_vista::ContainedWriteback
    where
        T::Value: From<CborValue> + Send + Sync + vantage_types::InvariantValue,
        T::Id: Clone + Send + Sync,
    {
        let host_column = host_column.to_string();
        let parent_table = self.clone();
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
        })
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
                vantage_vista::ReferenceKind::ContainsOne => {
                    self.with_contained_one(relation, &host, build)
                }
                vantage_vista::ReferenceKind::ContainsMany => {
                    self.with_contained_many(relation, &host, build, c.id_column.as_deref())
                }
                _ => {
                    return Err(error!(
                        "contained relation kind must be contains_one or contains_many",
                        relation = relation.as_str()
                    ));
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
        let (reference, _) = self.lookup_ref(relation)?;
        let source_id = self.id_field_name();

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

        let source_id = self.id_field_name();

        let mut target: Table<T, E2> = *reference
            .build_target(self.data_source() as &dyn std::any::Any)
            .downcast::<Table<T, E2>>()
            .map_err(|_| {
                error!(
                    "Failed to downcast related table",
                    relation = relation_str.as_str()
                )
            })?;

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
        let (reference, relation_str) = self.lookup_ref(relation)?;
        let mut target: Table<T, E2> = *reference
            .build_target(self.data_source() as &dyn std::any::Any)
            .downcast::<Table<T, E2>>()
            .map_err(|_| {
                error!(
                    "Failed to downcast related table",
                    relation = relation_str.as_str()
                )
            })?;
        self.attach_correlated_condition(reference, &mut target);
        Ok(target)
    }

    /// Entity-erased [`get_subquery_as`](Self::get_subquery_as): builds the
    /// correlated target as `Table<T, EmptyEntity>` without the caller naming
    /// the target's concrete entity type. Used by implicit-reference traversal,
    /// which walks relations by name and cannot know each hop's entity type.
    pub fn get_subquery_erased(&self, relation: &str) -> Result<Table<T, EmptyEntity>> {
        let (reference, relation_str) = self.lookup_ref(relation)?;
        let mut target: Table<T, EmptyEntity> = *reference
            .build_target_erased(self.data_source() as &dyn std::any::Any)
            .downcast::<Table<T, EmptyEntity>>()
            .map_err(|_| {
                error!(
                    "Failed to downcast related table",
                    relation = relation_str.as_str()
                )
            })?;
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
        let (reference, relation_str) = self.lookup_ref(relation)?;
        let target: Table<T, EmptyEntity> = *reference
            .build_target_erased(self.data_source() as &dyn std::any::Any)
            .downcast::<Table<T, EmptyEntity>>()
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
    ) -> Self
    where
        T: 'static,
        E: 'static,
    {
        // Adapt the caller's `Fn(&Table<T, E>)` into the entity-erased
        // `ExpressionFn<T>` we store, so it survives `into_entity`. The closure
        // never touches entity-typed fields, only name-keyed table state.
        let wrapped: crate::table::base::ExpressionFn<T> =
            Arc::new(move |erased: &Table<T, vantage_types::EmptyEntity>| {
                // SAFETY: layout-identical (E is PhantomData only); shared borrow.
                let typed: &Table<T, E> = unsafe {
                    &*(erased as *const Table<T, vantage_types::EmptyEntity> as *const Table<T, E>)
                };
                expr_fn(typed)
            });
        self.expressions.insert(name.to_string(), wrapped);
        self
    }

    /// Add a lazy computed column using builder pattern.
    ///
    /// Unlike [`Self::with_expression`] (which lowers into the data source's
    /// query), a lazy expression runs in Rust on the record the data source
    /// *returned*. Callbacks apply in declaration order: each borrows the
    /// record as built so far, and the value it returns is inserted under
    /// `name` before the next callback runs — so one expensive fetch (a
    /// file's contents) can feed several cheap derived columns declared
    /// after it.
    ///
    /// The name is also registered as a column, so the computed field shows
    /// up in the table's schema (and in Vista metadata derived from it).
    /// Because the callback's future must not borrow the record, clone what
    /// you need from `record` before going async.
    ///
    /// Lazy expressions run on the `list`/`get` read paths (both raw-record
    /// and entity forms). They never participate in queries — no conditions,
    /// ordering, or pushdown — they exist only on returned data.
    pub fn with_lazy_expression<F, Fut>(mut self, name: &str, f: F) -> Self
    where
        F: Fn(&vantage_types::Record<T::Value>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<T::Value>> + Send + 'static,
    {
        if !self.columns.contains_key(name) {
            let column = self.data_source.create_column::<T::AnyType>(name);
            self.add_column(column);
        }
        self.add_lazy_expression(name, Arc::new(move |record| Box::pin(f(record))));
        self
    }

    /// Register a pre-boxed lazy expression callback under `name`, without
    /// touching the column set. Factories lowering a spec (which declares
    /// its columns separately) use this; application code should prefer
    /// [`Self::with_lazy_expression`].
    pub fn add_lazy_expression(&mut self, name: &str, f: crate::table::base::LazyExpressionFn<T>) {
        self.lazy_expressions.insert(name.to_string(), f);
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
        Ok(reference.cardinality().into())
    }

    /// Look up the foreign-key/link field for a registered relation. For a
    /// `has_one` this is the source-side column carrying the link — the field
    /// a backend-native traversal (e.g. a SurrealDB idiom path) descends
    /// through, which need not match the relation's registry name.
    pub fn ref_foreign_key(&self, relation: &str) -> Result<String> {
        let (reference, _) = self.lookup_ref(relation)?;
        Ok(reference.foreign_key().to_string())
    }

    /// List all registered relations with their cardinality.
    pub fn ref_kinds(&self) -> Vec<(String, vantage_vista::ReferenceKind)> {
        self.refs
            .as_ref()
            .map(|refs| {
                refs.iter()
                    .map(|(name, r)| (name.clone(), r.cardinality().into()))
                    .collect()
            })
            .unwrap_or_default()
    }
}
