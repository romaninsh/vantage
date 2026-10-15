//! `TableSource` for MongoDB, with `MongoCondition` as the condition type.
//! Reads build a `MongoSelect` from the table; writes go straight to the
//! driver. Over a data source carrying a `MongoView`, reads run after the
//! view's stages and writes are refused.

use async_trait::async_trait;
use bson::{Bson, doc};
use futures_util::TryStreamExt;
use indexmap::IndexMap;

use vantage_core::{Result, error};
use vantage_expressions::{
    AssociatedExpression, DeferredFn, ExprDataSource, Expression, ExpressiveEnum, expr_any,
};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::condition::MongoCondition;
use crate::id::{MongoId, eq_filter, id_variants};
use crate::mongodb::MongoDB;
use crate::mongodb::impls::joins::{imported_columns, join_alias_of, table_joins};
use crate::mongodb::view::undeclared_view;
use crate::select::join::{alias_index, filter_uses_joins};
use crate::select::{JoinColumn, MongoSelect};
use crate::types::{AnyMongoType, MongoType};

/// Convert a `bson::Document` into a `Record<AnyMongoType>`, optionally
/// extracting the `_id`. A joined column's alias is renamed back to the
/// column's dotted name.
fn doc_to_record(
    doc: bson::Document,
    joins: &[JoinColumn],
) -> (Option<MongoId>, Record<AnyMongoType>) {
    let mut fields = IndexMap::new();
    let mut id: Option<MongoId> = None;

    for (k, v) in doc {
        if k == "_id" {
            id = MongoId::from_bson(&v);
        }
        let key = match alias_index(&k).and_then(|i| joins.get(i)) {
            Some(join) => join.name.clone(),
            None => k,
        };
        fields.insert(key, AnyMongoType::untyped(v));
    }

    (id, Record::from_indexmap(fields))
}

/// A `MongoSelect` for reading `table`: its filter, ordering and paging plus
/// the join chains of its imported columns.
fn select_from_table<E: Entity<AnyMongoType>>(table: &Table<MongoDB, E>) -> Result<MongoSelect> {
    let mut select = select_base(table);
    select.joins = table_joins(table)?;
    Ok(select)
}

/// The table's filter for confining a write. Writes match stored documents
/// directly, where joined columns do not exist, so a condition on one is
/// refused rather than matching nothing. Capabilities are fixed when the
/// vista is built, so a vista narrowed this way still advertises insert,
/// update and delete; the refusal comes at write time.
async fn write_filter<E: Entity<AnyMongoType>>(
    table: &Table<MongoDB, E>,
) -> Result<bson::Document> {
    let filter = select_base(table).build_filter().await?;
    if filter_uses_joins(&filter, imported_columns(table).len()) {
        return Err(error!(
            "MongoDB cannot confine a write by a condition on a joined column",
            table = table.table_name()
        )
        .mark_unsupported());
    }
    Ok(filter)
}

/// Refuse an aggregate over a joined column: the accumulator reads stored
/// fields, so it would total the dotted path instead of the joined value.
fn ensure_stored_column<E: Entity<AnyMongoType>>(
    table: &Table<MongoDB, E>,
    column: &str,
) -> Result<()> {
    if table.is_imported_column(column) {
        return Err(error!(
            "MongoDB cannot aggregate a joined column",
            table = table.table_name(),
            column = column
        )
        .mark_unsupported());
    }
    Ok(())
}

/// Build a `MongoSelect` from a `Table`'s current state (the data source's
/// view stages, conditions, ordering, pagination), without joins.
fn select_base<E: Entity<AnyMongoType>>(table: &Table<MongoDB, E>) -> MongoSelect {
    let mut select = MongoSelect::new();
    select.collection = Some(table.table_name().to_string());
    select.pre_stages = table.data_source().view_stages();

    for condition in table.conditions() {
        select.conditions.push(condition.clone());
    }

    for (cond, direction) in table.orders() {
        // An order is a document whose first key names the field; the
        // direction comes from the entry, not the document's value.
        if let MongoCondition::Doc(doc) = cond
            && let Some((key, _)) = doc.iter().next()
        {
            let dir = match direction {
                vantage_table::sorting::SortDirection::Ascending => 1,
                vantage_table::sorting::SortDirection::Descending => -1,
            };
            select.sort.push((key.to_string(), dir));
        }
    }

    if let Some(pagination) = table.pagination() {
        select.limit = Some(pagination.limit());
        select.skip = Some(pagination.skip());
    }

    select
}

/// The `ObjectId` of a two-element `$in` whose elements are the two spellings
/// of one id; `None` for any other array.
fn id_pair_object_id(pair: &[Bson]) -> Option<Bson> {
    let [a, b] = pair else {
        return None;
    };
    if id_variants(a).as_ref() != Some(b) {
        return None;
    }
    [a, b]
        .into_iter()
        .find(|v| matches!(v, Bson::ObjectId(_)))
        .cloned()
}

/// Largest total document size sent in one set-probe command, well under the
/// server's 16 MB command limit.
const PROBE_CHUNK_BYTES: usize = 8 * 1024 * 1024;

/// Split items of the given byte `sizes` into consecutive ranges whose totals
/// stay within `limit`; an item larger than `limit` gets a range of its own.
fn chunk_ranges(sizes: &[usize], limit: usize) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let (mut start, mut total) = (0, 0);
    for (i, size) in sizes.iter().enumerate() {
        if i > start && total + size > limit {
            ranges.push(start..i);
            (start, total) = (i, 0);
        }
        total += size;
    }
    if start < sizes.len() {
        ranges.push(start..sizes.len());
    }
    ranges
}

/// `{ _id: id }`, and the table's filter when it has one.
fn confined(id: &MongoId, filter: bson::Document) -> bson::Document {
    if filter.is_empty() {
        doc! { "_id": id }
    } else {
        doc! { "$and": [ { "_id": id }, filter ] }
    }
}

fn record_to_doc(record: &Record<AnyMongoType>) -> bson::Document {
    let mut d = bson::Document::new();
    for (k, v) in record.iter() {
        d.insert(k, v.value().clone());
    }
    d
}

fn is_duplicate_key(e: &mongodb::error::Error) -> bool {
    use mongodb::error::{ErrorKind, WriteFailure};
    matches!(
        &*e.kind,
        ErrorKind::Write(WriteFailure::WriteError(we)) if we.code == 11000
    )
}

/// A failed write to `collection`: an undeclared server-side view gets its
/// own unsupported error, anything else `message` with the driver's details.
fn write_failed(
    e: &mongodb::error::Error,
    collection: &str,
    message: &'static str,
) -> vantage_core::VantageError {
    undeclared_view(e, collection).unwrap_or_else(|| error!(message, details = e.to_string()))
}

impl MongoDB {
    /// The table's filter document for confining writes; empty when the
    /// table is not narrowed. Refused when it reads a joined column.
    pub(crate) async fn set_filter<E: Entity<AnyMongoType>>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<bson::Document> {
        write_filter(table).await
    }

    /// The `_id`s (as `{_id: ..}` documents) of the members of `docs` that
    /// satisfy `filter`, evaluated by the server over `$documents` stages
    /// (MongoDB 5.1+). `docs` are probed in chunks so no command nears the
    /// server's command size limit.
    pub(crate) async fn docs_matching(
        &self,
        filter: &bson::Document,
        docs: &[bson::Document],
    ) -> Result<Vec<bson::Document>> {
        let sizes = docs
            .iter()
            .map(|d| bson::to_vec(d).map(|v| v.len()))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| error!("MongoDB set probe failed", details = e.to_string()))?;
        let mut matching = Vec::new();
        for range in chunk_ranges(&sizes, PROBE_CHUNK_BYTES) {
            let pipeline = vec![
                doc! { "$documents": &docs[range] },
                doc! { "$match": filter },
                doc! { "$project": { "_id": 1 } },
            ];
            let found: Vec<bson::Document> = self
                .database()
                .aggregate(pipeline)
                .await
                .map_err(|e| error!("MongoDB set probe failed", details = e.to_string()))?
                .try_collect()
                .await
                .map_err(|e| error!("MongoDB set probe cursor failed", details = e.to_string()))?;
            matching.extend(found);
        }
        Ok(matching)
    }

    /// Whether `doc` satisfies the table's filter.
    pub(crate) async fn row_in_set<E: Entity<AnyMongoType>>(
        &self,
        table: &Table<Self, E>,
        doc: &bson::Document,
    ) -> Result<bool> {
        let filter = self.set_filter(table).await?;
        if filter.is_empty() {
            return Ok(true);
        }
        Ok(!self
            .docs_matching(&filter, std::slice::from_ref(doc))
            .await?
            .is_empty())
    }
}

#[async_trait]
impl TableSource for MongoDB {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = AnyMongoType;
    type Value = AnyMongoType;
    type Id = MongoId;
    type Condition = MongoCondition;
    type Source = String;

    fn eq_value_condition(&self, field: &str, value: Self::Value) -> Result<Self::Condition> {
        Ok(MongoCondition::Doc(eq_filter(field, &value.to_bson())))
    }

    fn condition_equality(&self, condition: &Self::Condition) -> Option<(String, Self::Value)> {
        let MongoCondition::Doc(d) = condition else {
            return None;
        };
        if d.len() != 1 {
            return None;
        }
        let (k, v) = d.iter().next()?;
        if k.starts_with('$') {
            return None;
        }
        let value = match v {
            Bson::Document(inner) if inner.len() == 1 => match inner.iter().next()? {
                (op, operand) if op == "$eq" => operand.clone(),
                (op, Bson::Array(pair)) if op == "$in" => id_pair_object_id(pair)?,
                _ => return None,
            },
            Bson::Document(_) | Bson::Array(_) => return None,
            scalar => scalar.clone(),
        };
        Some((k.clone(), AnyMongoType::untyped(value)))
    }

    /// A 24-char hex string join value is written as the `ObjectId` it spells.
    /// Text surfaces (CBOR, scripts) carry ids as hex strings, so without this
    /// a child inserted into a traversed set would store its foreign key as a
    /// string and no longer match the parent's `ObjectId` `_id`. Lookups still
    /// match both spellings (see `eq_value_condition`).
    fn coerce_reference_value(&self, value: Self::Value) -> Self::Value {
        match value.to_bson() {
            hex @ Bson::String(_) => match id_variants(&hex) {
                Some(oid) => AnyMongoType::untyped(oid),
                None => value,
            },
            _ => value,
        }
    }

    fn can_confine_writes(&self) -> bool {
        self.is_stored()
    }

    /// Dotted columns read through `$lookup` (see the `joins` module), except
    /// over a pipeline view. The lookup matches `localField` against
    /// `foreignField`. Correlated subqueries are not implemented:
    /// `Table::get_subquery_*` panics here (`related_correlated_condition`
    /// keeps the trait's `unimplemented!` default).
    fn supports_traversal(&self) -> bool {
        self.view().is_none()
    }

    /// Names the path only: reads resolve an imported column's join chain
    /// from the table's references and never evaluate this expression.
    fn traversal_path_expr(&self, hops: &[&str], column: &str) -> Option<Expression<Self::Value>> {
        Some(Expression::new(
            format!("{}.{}", hops.join("."), column),
            vec![],
        ))
    }

    fn create_column<Type: ColumnType>(&self, name: &str) -> Self::Column<Type> {
        Column::new(name)
    }

    fn to_any_column<Type: ColumnType>(
        &self,
        column: Self::Column<Type>,
    ) -> Self::Column<Self::AnyType> {
        Column::from_column(column)
    }

    fn convert_any_column<Type: ColumnType>(
        &self,
        any_column: Self::Column<Self::AnyType>,
    ) -> Option<Self::Column<Type>> {
        Some(Column::from_column(any_column))
    }

    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<ExpressiveEnum<Self::Value>>,
    ) -> Expression<Self::Value> {
        Expression::new(template, parameters)
    }

    fn search_table_condition<E>(
        &self,
        table: &Table<Self, E>,
        search_value: &str,
    ) -> Self::Condition
    where
        E: Entity<Self::Value>,
    {
        let escaped = regex_escape(search_value);

        // Each column is searched at its stored BSON path (the alias), or at
        // its join alias when it is a joined column.
        let conditions: Vec<bson::Bson> = table
            .search_columns()
            .into_iter()
            .map(|col| {
                let field = join_alias_of(table, col.name())
                    .unwrap_or_else(|| col.alias().unwrap_or(col.name()).to_string());
                bson::Bson::Document(doc! { field: { "$regex": &escaped, "$options": "i" } })
            })
            .collect();

        if conditions.is_empty() {
            // No column to search matches nothing, as on the SQL backends.
            doc! { "_id": { "$exists": false } }.into()
        } else if conditions.len() == 1 {
            match conditions.into_iter().next().unwrap() {
                bson::Bson::Document(d) => d.into(),
                _ => unreachable!(),
            }
        } else {
            doc! { "$or": conditions }.into()
        }
    }

    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
    {
        let select = select_from_table(table)?;
        let docs = self.read_docs(&select).await?;

        let mut records = IndexMap::new();
        for d in docs {
            let (oid, record) = doc_to_record(d, &select.joins);
            let id = oid.ok_or_else(|| error!("Document missing _id field"))?;
            records.insert(id, record);
        }

        Ok(records)
    }

    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Option<Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
    {
        let mut select = select_from_table(table)?;
        if self.reads_pipeline(&select) {
            select.conditions.push(doc! { "_id": id }.into());
            select.sort.clear();
            (select.skip, select.limit) = (None, Some(1));
            let d = self.read_docs(&select).await?.into_iter().next();
            return Ok(d.map(|d| doc_to_record(d, &select.joins).1));
        }

        let coll = self.doc_collection(table.table_name());
        let filter = select.build_filter().await?;

        let Some(d) = coll
            .find_one(confined(id, filter))
            .await
            .map_err(|e| error!("MongoDB find_one failed", details = e.to_string()))?
        else {
            return Ok(None);
        };

        let (_oid, record) = doc_to_record(d, &[]);
        Ok(Some(record))
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
    {
        let mut select = select_from_table(table)?;
        if self.reads_pipeline(&select) {
            (select.skip, select.limit) = (None, Some(1));
            let d = self.read_docs(&select).await?.into_iter().next();
            return Ok(d.and_then(|d| {
                let (oid, record) = doc_to_record(d, &select.joins);
                oid.map(|id| (id, record))
            }));
        }

        let filter = select.build_filter().await?;
        let coll = self.doc_collection(table.table_name());

        let mut find = coll.find_one(filter);
        if let Some(sort) = select.build_sort() {
            find = find.sort(sort);
        }
        let d = find
            .await
            .map_err(|e| error!("MongoDB find_one failed", details = e.to_string()))?;

        match d {
            Some(d) => {
                let (oid, record) = doc_to_record(d, &[]);
                Ok(oid.map(|id| (id, record)))
            }
            None => Ok(None),
        }
    }

    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
    {
        self.count_docs(&select_from_table(table)?).await
    }

    async fn get_table_sum<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        ensure_stored_column(table, column.name())?;
        let select = select_from_table(table)?;
        let val = self
            .accumulate(&select, "$sum", column.name(), Bson::Int64(0))
            .await?;
        Ok(AnyMongoType::untyped(val))
    }

    async fn get_table_max<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        ensure_stored_column(table, column.name())?;
        let select = select_from_table(table)?;
        let val = self
            .accumulate(&select, "$max", column.name(), Bson::Null)
            .await?;
        Ok(AnyMongoType::untyped(val))
    }

    async fn get_table_min<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        ensure_stored_column(table, column.name())?;
        let select = select_from_table(table)?;
        let val = self
            .accumulate(&select, "$min", column.name(), Bson::Null)
            .await?;
        Ok(AnyMongoType::untyped(val))
    }

    async fn insert_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        self.ensure_writable(table.table_name())?;
        let coll = self.doc_collection(table.table_name());
        let mut doc = bson::Document::new();
        doc.insert("_id", id);
        for (k, v) in record.iter() {
            doc.insert(k, v.value().clone());
        }

        if let Some(existing) = self.get_table_value(table, id).await? {
            return Ok(existing);
        }
        let held_outside =
            || error!("Id is held outside the table's set", id = id.to_string()).mark_conflict();
        if coll
            .find_one(doc! { "_id": id })
            .await
            .map_err(|e| error!("MongoDB find_one failed", details = e.to_string()))?
            .is_some()
        {
            return Err(held_outside());
        }
        if !self.row_in_set(table, &doc).await? {
            return Err(error!(
                "Document does not match the table's filter",
                id = id.to_string()
            )
            .mark_conflict());
        }

        if let Err(e) = coll.insert_one(doc).await {
            if !is_duplicate_key(&e) {
                return Err(write_failed(
                    &e,
                    table.table_name(),
                    "MongoDB insert_one failed",
                ));
            }
            // A concurrent insert won the race: return its row if it is ours.
            return self
                .get_table_value(table, id)
                .await?
                .ok_or_else(held_outside);
        }

        self.get_table_value(table, id)
            .await?
            .ok_or_else(|| error!("Inserted row disappeared", id = id.to_string()))
    }

    async fn replace_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        self.ensure_writable(table.table_name())?;
        let coll = self.doc_collection(table.table_name());
        let table_filter = write_filter(table).await?;
        let mut replacement = bson::Document::new();
        for (k, v) in record.iter() {
            replacement.insert(k, v.value().clone());
        }
        let mut full = doc! { "_id": id };
        full.extend(replacement.clone());

        if self.get_table_value(table, id).await?.is_some() {
            if !self.row_in_set(table, &full).await? {
                return Err(error!(
                    "Replacement document leaves the table's set",
                    id = id.to_string()
                )
                .mark_conflict());
            }
            coll.replace_one(confined(id, table_filter), replacement)
                .await
                .map_err(|e| write_failed(&e, table.table_name(), "MongoDB replace_one failed"))?;
        } else {
            if coll
                .find_one(doc! { "_id": id })
                .await
                .map_err(|e| error!("MongoDB find_one failed", details = e.to_string()))?
                .is_some()
            {
                return Err(
                    error!("Id is held outside the table's set", id = id.to_string())
                        .mark_conflict(),
                );
            }
            if !self.row_in_set(table, &full).await? {
                return Err(error!(
                    "Document does not match the table's filter",
                    id = id.to_string()
                )
                .mark_conflict());
            }
            coll.insert_one(full)
                .await
                .map_err(|e| write_failed(&e, table.table_name(), "MongoDB insert_one failed"))?;
        }

        self.get_table_value(table, id)
            .await?
            .ok_or_else(|| error!("Replaced row disappeared", id = id.to_string()))
    }

    async fn patch_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        self.ensure_writable(table.table_name())?;
        let not_found = || error!("Record not found", id = id.to_string()).mark_not_found();
        let coll = self.doc_collection(table.table_name());
        let table_filter = write_filter(table).await?;
        let filter = confined(id, table_filter);

        let existing = self
            .get_table_value(table, id)
            .await?
            .ok_or_else(not_found)?;
        let mut merged = record_to_doc(&existing);
        for name in imported_columns(table) {
            merged.remove(name);
        }
        let mut set_doc = bson::Document::new();
        for (k, v) in partial.iter() {
            set_doc.insert(k, v.value().clone());
            merged.insert(k, v.value().clone());
        }
        if !self.row_in_set(table, &merged).await? {
            return Err(error!(
                "Patched document leaves the table's set",
                id = id.to_string()
            )
            .mark_conflict());
        }

        let result = coll
            .update_one(filter, doc! { "$set": set_doc })
            .await
            .map_err(|e| write_failed(&e, table.table_name(), "MongoDB update_one failed"))?;
        if result.matched_count == 0 {
            return Err(not_found());
        }

        self.get_table_value(table, id).await?.ok_or_else(not_found)
    }

    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        self.ensure_writable(table.table_name())?;
        let filter = write_filter(table).await?;
        let coll = self.doc_collection(table.table_name());
        coll.delete_one(confined(id, filter))
            .await
            .map_err(|e| write_failed(&e, table.table_name(), "MongoDB delete_one failed"))?;
        Ok(())
    }

    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        self.ensure_writable(table.table_name())?;
        let filter = write_filter(table).await?;
        let coll = self.doc_collection(table.table_name());
        coll.delete_many(filter)
            .await
            .map_err(|e| write_failed(&e, table.table_name(), "MongoDB delete_many failed"))?;
        Ok(())
    }

    async fn insert_table_return_id_value<E>(
        &self,
        table: &Table<Self, E>,
        record: &Record<Self::Value>,
    ) -> Result<Self::Id>
    where
        E: Entity<Self::Value>,
    {
        self.ensure_writable(table.table_name())?;
        let coll = self.doc_collection(table.table_name());
        let mut doc = bson::Document::new();
        for (k, v) in record.iter() {
            doc.insert(k, v.value().clone());
        }
        if !self.row_in_set(table, &doc).await? {
            return Err(error!("Document does not match the table's filter").mark_conflict());
        }

        let result = coll
            .insert_one(doc)
            .await
            .map_err(|e| write_failed(&e, table.table_name(), "MongoDB insert_one failed"))?;

        MongoId::from_bson(&result.inserted_id)
            .ok_or_else(|| error!("MongoDB insert did not return a valid id"))
    }

    fn related_in_condition<SourceE: Entity<Self::Value> + 'static>(
        &self,
        target_field: &str,
        source_table: &Table<Self, SourceE>,
        source_column: &str,
    ) -> Self::Condition
    where
        Self: Sized,
    {
        let source = source_table.clone();
        let field = target_field.to_string();
        // A reference key is read under its own field name, as the vista
        // factories require; a renamed, nested or joined key is refused.
        let renamed = source_table
            .columns()
            .get(source_column)
            .and_then(|c| c.alias())
            .is_some_and(|alias| alias != source_column)
            || source_table.is_imported_column(source_column);
        let col = source_column.to_string();

        MongoCondition::Deferred(DeferredFn::new(move || {
            let source = source.clone();
            let col = col.clone();
            let field = field.clone();
            Box::pin(async move {
                if renamed {
                    return Err(error!(
                        "reference key must be stored under its own field name",
                        column = col
                    ));
                }
                // Build a projected query that only fetches the source column
                let mut select = select_from_table(&source)?;
                select.fields = vec![col.clone()];

                let mut values = Vec::new();
                for doc in source.data_source().read_docs(&select).await? {
                    if let Some(v) = doc.get(&col) {
                        // Push the value itself, plus the alternate representation
                        // so `$in` matches regardless of whether the target stores
                        // this id as ObjectId or as the hex string equivalent.
                        values.push(v.clone());
                        match v {
                            Bson::ObjectId(oid) => {
                                values.push(Bson::String(oid.to_hex()));
                            }
                            Bson::String(s) => {
                                if let Ok(oid) = bson::oid::ObjectId::parse_str(s) {
                                    values.push(Bson::ObjectId(oid));
                                }
                            }
                            _ => {}
                        }
                    }
                }

                let doc = doc! { &field: { "$in": values } };
                Ok(ExpressiveEnum::Scalar(AnyMongoType::untyped(
                    Bson::Document(doc),
                )))
            })
        }))
    }

    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        table: &Table<Self, E>,
        column: &Self::Column<Type>,
    ) -> AssociatedExpression<'a, Self, Self::Value, Vec<Type>>
    where
        E: Entity<Self::Value> + 'static,
        Self: ExprDataSource<Self::Value> + Sized,
    {
        let table_clone = table.clone();
        let col = column.name().to_string();
        let db = self.clone();

        let inner = expr_any!("{}", {
            DeferredFn::new(move || {
                let db = db.clone();
                let table = table_clone.clone();
                let col = col.clone();
                Box::pin(async move {
                    let records = db.list_table_values(&table).await?;
                    let values: Vec<AnyMongoType> = records
                        .values()
                        .filter_map(|r| r.get(&col).cloned())
                        .collect();
                    Ok(ExpressiveEnum::Scalar(AnyMongoType::new(values)))
                })
            })
        });

        let expr = expr_any!("{}", { self.defer(inner) });
        AssociatedExpression::new(expr, self)
    }
}

/// Escape regex metacharacters so a user-provided string is treated as a
/// literal substring in a `$regex` filter.
pub(crate) fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if ".*+?^${}()|[]\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
