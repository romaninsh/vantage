//! TableSource implementation for MongoDB.
//!
//! Uses `MongoCondition` as the condition type. Read/aggregate operations
//! build a `MongoSelect` from table state and use its helper methods
//! (`build_filter`, `build_find_options`, `as_aggregate_pipeline`).
//! Write operations use the `mongodb` driver directly.

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
use crate::id::MongoId;
use crate::mongodb::MongoDB;
use crate::select::MongoSelect;
use crate::types::{AnyMongoType, MongoType};

/// Convert a `bson::Document` into a `Record<AnyMongoType>`, optionally extracting the `_id`.
fn doc_to_record(doc: bson::Document) -> (Option<MongoId>, Record<AnyMongoType>) {
    let mut fields = IndexMap::new();
    let mut id: Option<MongoId> = None;

    for (k, v) in doc {
        if k == "_id" {
            id = MongoId::from_bson(&v);
        }
        fields.insert(k, AnyMongoType::untyped(v));
    }

    (id, Record::from_indexmap(fields))
}

/// Build a `MongoSelect` from a `Table`'s current state (conditions, ordering, pagination).
fn select_from_table<E: Entity<AnyMongoType>>(table: &Table<MongoDB, E>) -> MongoSelect {
    let mut select = MongoSelect::new();
    select.collection = Some(table.table_name().to_string());

    for condition in table.conditions() {
        select.conditions.push(condition.clone());
    }

    for (cond, direction) in table.orders() {
        // Order entries are MongoCondition — extract field name, apply direction
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

impl MongoDB {
    /// Whether `doc` satisfies the table's filter, evaluated by the server over a
    /// one-document `$documents` stage (MongoDB 5.1+).
    pub(crate) async fn row_in_set<E: Entity<AnyMongoType>>(
        &self,
        table: &Table<Self, E>,
        doc: &bson::Document,
    ) -> Result<bool> {
        let filter = select_from_table(table).build_filter().await?;
        if filter.is_empty() {
            return Ok(true);
        }
        let pipeline = vec![
            doc! { "$documents": [doc.clone()] },
            doc! { "$match": filter },
        ];
        let mut cursor = self
            .database()
            .aggregate(pipeline)
            .await
            .map_err(|e| error!("MongoDB set probe failed", details = e.to_string()))?;
        Ok(cursor
            .try_next()
            .await
            .map_err(|e| error!("MongoDB set probe cursor failed", details = e.to_string()))?
            .is_some())
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
        Ok(MongoCondition::Doc(doc! { field: value.to_bson() }))
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
            Bson::Document(inner) if inner.len() == 1 => inner.get("$eq")?.clone(),
            Bson::Document(_) | Bson::Array(_) => return None,
            scalar => scalar.clone(),
        };
        Some((k.clone(), AnyMongoType::untyped(value)))
    }

    fn can_confine_writes(&self) -> bool {
        true
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
        // Escape regex metacharacters so the search is a literal substring match
        let escaped = regex_escape(search_value);

        // Build $or across all string-like columns with case-insensitive regex
        let conditions: Vec<bson::Bson> = table
            .columns()
            .values()
            .map(|col| {
                bson::Bson::Document(doc! { col.name(): { "$regex": &escaped, "$options": "i" } })
            })
            .collect();

        if conditions.is_empty() {
            // No columns — return always-false filter (consistent with SQL backends)
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

    // ── Read ─────────────────────────────────────────────────────────

    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
    {
        let select = select_from_table(table);
        let filter = select.build_filter().await?;
        let options = select.build_find_options();
        let coll = self.doc_collection(table.table_name());

        let cursor = coll
            .find(filter)
            .with_options(options)
            .await
            .map_err(|e| error!("MongoDB find failed", details = e.to_string()))?;

        let docs: Vec<bson::Document> = cursor
            .try_collect()
            .await
            .map_err(|e| error!("MongoDB cursor failed", details = e.to_string()))?;

        let mut records = IndexMap::new();
        for d in docs {
            let (oid, record) = doc_to_record(d);
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
        let coll = self.doc_collection(table.table_name());
        let filter = select_from_table(table).build_filter().await?;

        let Some(d) = coll
            .find_one(confined(id, filter))
            .await
            .map_err(|e| error!("MongoDB find_one failed", details = e.to_string()))?
        else {
            return Ok(None);
        };

        let (_oid, record) = doc_to_record(d);
        Ok(Some(record))
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
    {
        let select = select_from_table(table);
        let filter = select.build_filter().await?;
        let coll = self.doc_collection(table.table_name());

        let d = coll
            .find_one(filter)
            .await
            .map_err(|e| error!("MongoDB find_one failed", details = e.to_string()))?;

        match d {
            Some(d) => {
                let (oid, record) = doc_to_record(d);
                Ok(oid.map(|id| (id, record)))
            }
            None => Ok(None),
        }
    }

    // ── Aggregates ───────────────────────────────────────────────────

    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
    {
        let select = select_from_table(table);
        let filter = select.build_filter().await?;
        let coll = self.doc_collection(table.table_name());

        let count = coll
            .count_documents(filter)
            .await
            .map_err(|e| error!("MongoDB count_documents failed", details = e.to_string()))?;

        Ok(count as i64)
    }

    async fn get_table_sum<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        let select = select_from_table(table);
        let pipeline = select.as_aggregate_pipeline("$sum", column.name()).await?;
        let coll = self.doc_collection(table.table_name());

        let mut cursor = coll
            .aggregate(pipeline)
            .await
            .map_err(|e| error!("MongoDB aggregate (sum) failed", details = e.to_string()))?;

        if let Some(result) = cursor
            .try_next()
            .await
            .map_err(|e| error!("MongoDB aggregate cursor failed", details = e.to_string()))?
        {
            let val = result.get("val").cloned().unwrap_or(Bson::Int64(0));
            Ok(AnyMongoType::untyped(val))
        } else {
            Ok(AnyMongoType::untyped(Bson::Int64(0)))
        }
    }

    async fn get_table_max<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        let select = select_from_table(table);
        let pipeline = select.as_aggregate_pipeline("$max", column.name()).await?;
        let coll = self.doc_collection(table.table_name());

        let mut cursor = coll
            .aggregate(pipeline)
            .await
            .map_err(|e| error!("MongoDB aggregate (max) failed", details = e.to_string()))?;

        if let Some(result) = cursor
            .try_next()
            .await
            .map_err(|e| error!("MongoDB aggregate cursor failed", details = e.to_string()))?
        {
            let val = result.get("val").cloned().unwrap_or(Bson::Null);
            Ok(AnyMongoType::untyped(val))
        } else {
            Ok(AnyMongoType::untyped(Bson::Null))
        }
    }

    async fn get_table_min<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        let select = select_from_table(table);
        let pipeline = select.as_aggregate_pipeline("$min", column.name()).await?;
        let coll = self.doc_collection(table.table_name());

        let mut cursor = coll
            .aggregate(pipeline)
            .await
            .map_err(|e| error!("MongoDB aggregate (min) failed", details = e.to_string()))?;

        if let Some(result) = cursor
            .try_next()
            .await
            .map_err(|e| error!("MongoDB aggregate cursor failed", details = e.to_string()))?
        {
            let val = result.get("val").cloned().unwrap_or(Bson::Null);
            Ok(AnyMongoType::untyped(val))
        } else {
            Ok(AnyMongoType::untyped(Bson::Null))
        }
    }

    // ── Write ────────────────────────────────────────────────────────

    async fn insert_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
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
                return Err(error!("MongoDB insert_one failed", details = e.to_string()));
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
        let coll = self.doc_collection(table.table_name());
        let table_filter = select_from_table(table).build_filter().await?;
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
                .map_err(|e| error!("MongoDB replace_one failed", details = e.to_string()))?;
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
                .map_err(|e| error!("MongoDB insert_one failed", details = e.to_string()))?;
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
        let not_found = || error!("Record not found", id = id.to_string()).mark_not_found();
        let coll = self.doc_collection(table.table_name());
        let table_filter = select_from_table(table).build_filter().await?;
        let filter = confined(id, table_filter);

        let existing = self
            .get_table_value(table, id)
            .await?
            .ok_or_else(not_found)?;
        let mut merged = record_to_doc(&existing);
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
            .map_err(|e| error!("MongoDB update_one failed", details = e.to_string()))?;
        if result.matched_count == 0 {
            return Err(not_found());
        }

        self.get_table_value(table, id).await?.ok_or_else(not_found)
    }

    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let filter = select_from_table(table).build_filter().await?;
        let coll = self.doc_collection(table.table_name());
        coll.delete_one(confined(id, filter))
            .await
            .map_err(|e| error!("MongoDB delete_one failed", details = e.to_string()))?;
        Ok(())
    }

    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let select = select_from_table(table);
        let filter = select.build_filter().await?;
        let coll = self.doc_collection(table.table_name());
        coll.delete_many(filter)
            .await
            .map_err(|e| error!("MongoDB delete_many failed", details = e.to_string()))?;
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
            .map_err(|e| error!("MongoDB insert_one failed", details = e.to_string()))?;

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
        let db = self.clone();
        let source = source_table.clone();
        let col = source_column.to_string();
        let field = target_field.to_string();

        MongoCondition::Deferred(DeferredFn::new(move || {
            let db = db.clone();
            let source = source.clone();
            let col = col.clone();
            let field = field.clone();
            Box::pin(async move {
                // Build a projected query that only fetches the source column
                let select = select_from_table(&source);
                let filter = select.build_filter().await?;
                let collection_name = select
                    .collection
                    .as_deref()
                    .ok_or_else(|| error!("No collection name for related_in_condition"))?;

                let mut projection = doc! { &col: 1 };
                if col != "_id" {
                    projection.insert("_id", 0);
                }
                let opts = mongodb::options::FindOptions::builder()
                    .projection(projection)
                    .build();

                let coll = db.collection::<bson::Document>(collection_name);
                let mut cursor = coll
                    .find(filter)
                    .with_options(opts)
                    .await
                    .map_err(|e| error!("MongoDB find failed", details = e.to_string()))?;

                let mut values = Vec::new();
                while cursor
                    .advance()
                    .await
                    .map_err(|e| error!("MongoDB cursor error", details = e.to_string()))?
                {
                    let doc = cursor.deserialize_current().map_err(|e| {
                        error!("MongoDB deserialize error", details = e.to_string())
                    })?;
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
fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if ".*+?^${}()|[]\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
