//! Contained relations: records embedded in a host column of the parent row.

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::WritableValueSet;
use vantage_types::{EmptyEntity, Entity, Record};

use crate::{
    references::ContainedRelation,
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

impl<T: TableSource + 'static, E: Entity<T::Value> + 'static> Table<T, E> {
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
}
