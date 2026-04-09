//! TableSource implementation for MongoDB.
//!
//! Uses `MongoCondition` as the condition type. Read/aggregate operations
//! build a `MongoSelect` from table state and use its helper methods
//! (`build_filter`, `build_find_options`, `as_aggregate_pipeline`).
//! Write operations use the `mongodb` driver directly.

use async_trait::async_trait;
use bson::{Bson, doc, oid::ObjectId};
use futures_util::TryStreamExt;
use indexmap::IndexMap;

use vantage_core::{Result, error};
use vantage_expressions::{AssociatedExpression, ExprDataSource, Expression, ExpressiveEnum};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::condition::MongoCondition;
use crate::mongodb::MongoDB;
use crate::select::MongoSelect;
use crate::types::AnyMongoType;

/// Convert a bson::Document into a Record<AnyMongoType>, optionally extracting the _id.
fn doc_to_record(doc: bson::Document) -> (Option<ObjectId>, Record<AnyMongoType>) {
    let mut fields = IndexMap::new();
    let mut oid: Option<ObjectId> = None;

    for (k, v) in doc {
        if k == "_id"
            && let Bson::ObjectId(id) = &v
        {
            oid = Some(*id);
        }
        fields.insert(k, AnyMongoType::untyped(v));
    }

    (oid, Record::from_indexmap(fields))
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

#[async_trait]
impl TableSource for MongoDB {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = AnyMongoType;
    type Value = AnyMongoType;
    type Id = ObjectId;
    type Condition = MongoCondition;

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

    fn search_table_expr<E>(
        &self,
        _table: &Table<Self, E>,
        search_value: &str,
    ) -> Expression<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        // Preview-only expression
        Expression::new(format!("{{\"$regex\": \"{}\"}}", search_value), vec![])
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
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        let coll = self.doc_collection(table.table_name());

        let d = coll
            .find_one(doc! { "_id": id })
            .await
            .map_err(|e| error!("MongoDB find_one failed", details = e.to_string()))?
            .ok_or_else(|| error!("Document not found", id = id.to_hex()))?;

        let (_oid, record) = doc_to_record(d);
        Ok(record)
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
        doc.insert("_id", Bson::ObjectId(*id));
        for (k, v) in record.iter() {
            doc.insert(k, v.value().clone());
        }

        coll.insert_one(doc)
            .await
            .map_err(|e| error!("MongoDB insert_one failed", details = e.to_string()))?;

        self.get_table_value(table, id).await
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
        let filter = doc! { "_id": id };
        let mut replacement = bson::Document::new();
        for (k, v) in record.iter() {
            replacement.insert(k, v.value().clone());
        }

        coll.replace_one(filter, replacement)
            .upsert(true)
            .await
            .map_err(|e| error!("MongoDB replace_one failed", details = e.to_string()))?;

        self.get_table_value(table, id).await
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
        let coll = self.doc_collection(table.table_name());
        let filter = doc! { "_id": id };
        let mut set_doc = bson::Document::new();
        for (k, v) in partial.iter() {
            set_doc.insert(k, v.value().clone());
        }
        let update = doc! { "$set": set_doc };

        coll.update_one(filter, update)
            .await
            .map_err(|e| error!("MongoDB update_one failed", details = e.to_string()))?;

        self.get_table_value(table, id).await
    }

    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let coll = self.doc_collection(table.table_name());
        coll.delete_one(doc! { "_id": *id })
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

        let result = coll
            .insert_one(doc)
            .await
            .map_err(|e| error!("MongoDB insert_one failed", details = e.to_string()))?;

        result
            .inserted_id
            .as_object_id()
            .ok_or_else(|| error!("MongoDB insert did not return ObjectId"))
    }

    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Type>,
    ) -> AssociatedExpression<'a, Self, Self::Value, Vec<Type>>
    where
        E: Entity<Self::Value> + 'static,
        Self: ExprDataSource<Self::Value> + Sized,
    {
        todo!("column_table_values_expr not yet implemented for MongoDB")
    }
}
