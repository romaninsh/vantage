//! `TableSource` impl — trait method bodies, delegating to read/write helpers
//! in the sibling `query.rs`, `indexes.rs`, and `helpers.rs` modules.

use async_trait::async_trait;
use indexmap::IndexMap;

use redb::ReadableTable;
use vantage_core::{Result, error};
use vantage_expressions::{
    AssociatedExpression, DeferredFn, ExprDataSource, Expression, ExpressiveEnum, expr_any,
};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::condition::RedbCondition;
use crate::redb::helpers::{collect_indexed_pairs, indexed_columns};
use crate::redb::indexes::{delete_indexes, write_indexes};
use crate::redb::query::load_filtered;
use crate::redb::{Redb, index_table_def, index_table_name, main_table_def};
use crate::types::{AnyRedbType, decode_record, encode_record};

#[async_trait]
impl TableSource for Redb {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = AnyRedbType;
    type Value = AnyRedbType;
    type Id = String;
    type Condition = RedbCondition;

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
        _table: &Table<Self, E>,
        _search_value: &str,
    ) -> Self::Condition
    where
        E: Entity<Self::Value>,
    {
        panic!("vantage-redb: full-table search is not supported — use indexed eq() instead")
    }

    // ── Read ─────────────────────────────────────────────────────────

    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
    {
        load_filtered(self, table).await
    }

    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Option<Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
    {
        let txn = self.begin_read()?;
        let main = match txn.open_table(main_table_def(table.table_name())) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => {
                return Err(error!(
                    "Failed to open table for get",
                    details = e.to_string()
                ));
            }
        };
        let bytes = main
            .get(id.as_str())
            .map_err(|e| error!("redb get failed", details = e.to_string()))?;
        match bytes {
            Some(b) => Ok(Some(decode_record(b.value())?)),
            None => Ok(None),
        }
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
    {
        Ok(load_filtered(self, table).await?.into_iter().next())
    }

    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
    {
        if table.conditions().count() == 0 {
            // Cheap path: ask redb for its row count directly.
            use redb::ReadableTableMetadata;
            let txn = self.begin_read()?;
            let main = match txn.open_table(main_table_def(table.table_name())) {
                Ok(t) => t,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(0),
                Err(e) => {
                    return Err(error!(
                        "Failed to open table for count",
                        details = e.to_string()
                    ));
                }
            };
            let n = ReadableTableMetadata::len(&main).map_err(|e: redb::StorageError| {
                error!("redb len failed", details = e.to_string())
            })?;
            Ok(n as i64)
        } else {
            Ok(load_filtered(self, table).await?.len() as i64)
        }
    }

    async fn get_table_sum<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        Err(error!("redb does not support sum aggregation"))
    }

    async fn get_table_max<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        Err(error!("redb does not support max aggregation"))
    }

    async fn get_table_min<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        Err(error!("redb does not support min aggregation"))
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
        let table_name = table.table_name();
        let bytes = encode_record(record)?;
        let indexed_cols = indexed_columns(table);
        let pairs = collect_indexed_pairs(record, &indexed_cols);

        let txn = self.begin_write()?;
        {
            let mut main = txn
                .open_table(main_table_def(table_name))
                .map_err(|e| error!("Failed to open main for insert", details = e.to_string()))?;
            main.insert(id.as_str(), bytes.as_slice())
                .map_err(|e| error!("Main insert failed", details = e.to_string()))?;
            write_indexes(&txn, table_name, &pairs, id)?;
        }
        txn.commit()
            .map_err(|e| error!("Insert commit failed", details = e.to_string()))?;

        Ok(record.clone())
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
        let table_name = table.table_name();
        let new_bytes = encode_record(record)?;
        let indexed_cols = indexed_columns(table);
        let new_pairs = collect_indexed_pairs(record, &indexed_cols);

        let txn = self.begin_write()?;

        // Remove old index entries (read previous row first, in its own scope).
        let old_record = {
            let main = txn
                .open_table(main_table_def(table_name))
                .map_err(|e| error!("Failed to open main for replace", details = e.to_string()))?;
            main.get(id.as_str())
                .map_err(|e| error!("redb get failed", details = e.to_string()))?
                .map(|b| decode_record(b.value()))
                .transpose()?
        };
        if let Some(old) = &old_record {
            let old_pairs = collect_indexed_pairs(old, &indexed_cols);
            delete_indexes(&txn, table_name, &old_pairs, id)?;
        }

        // Write new main row + new index entries.
        {
            let mut main = txn.open_table(main_table_def(table_name)).map_err(|e| {
                error!("Failed to reopen main for replace", details = e.to_string())
            })?;
            main.insert(id.as_str(), new_bytes.as_slice())
                .map_err(|e| error!("Replace insert failed", details = e.to_string()))?;
        }
        write_indexes(&txn, table_name, &new_pairs, id)?;

        txn.commit()
            .map_err(|e| error!("Replace commit failed", details = e.to_string()))?;
        Ok(record.clone())
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
        let table_name = table.table_name();
        let indexed_cols = indexed_columns(table);
        let txn = self.begin_write()?;

        // Phase 1 — read the existing row in its own scope so the read
        // borrow is released before we re-open for write.
        let mut record = {
            let main = txn
                .open_table(main_table_def(table_name))
                .map_err(|e| error!("Failed to open main for patch", details = e.to_string()))?;
            let old_bytes = main
                .get(id.as_str())
                .map_err(|e| error!("Patch read failed", details = e.to_string()))?
                .ok_or_else(|| error!("Cannot patch missing row", id = id.as_str()))?;
            decode_record(old_bytes.value())?
        };

        // Snapshot old indexed pairs before mutating the record (owned copies
        // so the borrow on `record` doesn't outlive the patch loop below).
        let old_indexed: Vec<(String, AnyRedbType)> = record
            .iter()
            .filter(|(name, _)| indexed_cols.contains(name.as_str()))
            .map(|(n, v)| (n.clone(), v.clone()))
            .collect();

        for (k, v) in partial.iter() {
            record.insert(k.clone(), v.clone());
        }

        // Phase 2 — write new bytes + maintain indexes.
        {
            let new_bytes = encode_record(&record)?;
            let mut main = txn
                .open_table(main_table_def(table_name))
                .map_err(|e| error!("Failed to reopen main for patch", details = e.to_string()))?;
            main.insert(id.as_str(), new_bytes.as_slice())
                .map_err(|e| error!("Patch write failed", details = e.to_string()))?;
        }

        let old_pairs: Vec<(&str, &AnyRedbType)> =
            old_indexed.iter().map(|(n, v)| (n.as_str(), v)).collect();
        delete_indexes(&txn, table_name, &old_pairs, id)?;

        let new_pairs = collect_indexed_pairs(&record, &indexed_cols);
        write_indexes(&txn, table_name, &new_pairs, id)?;

        txn.commit()
            .map_err(|e| error!("Patch commit failed", details = e.to_string()))?;
        Ok(record)
    }

    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let table_name = table.table_name();
        let indexed_cols = indexed_columns(table);
        let txn = self.begin_write()?;

        // Read row to know what to delete from indexes, then remove.
        let row = {
            let mut main = txn
                .open_table(main_table_def(table_name))
                .map_err(|e| error!("Failed to open main for delete", details = e.to_string()))?;
            let row = main
                .get(id.as_str())
                .map_err(|e| error!("Delete read failed", details = e.to_string()))?
                .map(|b| decode_record(b.value()))
                .transpose()?;
            main.remove(id.as_str())
                .map_err(|e| error!("Delete remove failed", details = e.to_string()))?;
            row
        };

        if let Some(record) = row {
            let pairs = collect_indexed_pairs(&record, &indexed_cols);
            delete_indexes(&txn, table_name, &pairs, id)?;
        }

        txn.commit()
            .map_err(|e| error!("Delete commit failed", details = e.to_string()))?;
        Ok(())
    }

    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        // Conditional truncate — delete row-by-row so indexes stay consistent.
        if table.conditions().count() > 0 {
            for id in load_filtered(self, table).await?.into_keys() {
                self.delete_table_value(table, &id).await?;
            }
            return Ok(());
        }

        // Wholesale truncate — drop main table and all index tables.
        let table_name = table.table_name();
        let txn = self.begin_write()?;
        let _ = txn
            .delete_table(main_table_def(table_name))
            .map_err(|e| error!("delete_table failed", details = e.to_string()))?;
        for (col_name, col) in table.columns() {
            if col.flags().contains(&ColumnFlag::Indexed) {
                let idx_name = index_table_name(table_name, col_name);
                let _ = txn.delete_table(index_table_def(&idx_name));
            }
        }
        txn.commit()
            .map_err(|e| error!("Truncate commit failed", details = e.to_string()))?;
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
        let id = uuid::Uuid::new_v4().to_string();
        self.insert_table_value(table, &id, record).await?;
        Ok(id)
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
        let target = target_field.to_string();
        // If the source FK column is the source's id column, we have to
        // pull from the IndexMap keys — id values aren't duplicated inside
        // row bodies in redb.
        let source_id_col = source_table
            .id_field()
            .map(|c| ColumnLike::name(c).to_string())
            .unwrap_or_else(|| "id".to_string());

        RedbCondition::Deferred(DeferredFn::new(move || {
            let db = db.clone();
            let source = source.clone();
            let col = col.clone();
            let target = target.clone();
            let source_id_col = source_id_col.clone();
            Box::pin(async move {
                let rows = load_filtered(&db, &source).await?;
                let values: Vec<ciborium::Value> = if col == source_id_col {
                    rows.keys()
                        .map(|id| ciborium::Value::Text(id.clone()))
                        .collect()
                } else {
                    rows.values()
                        .filter_map(|r| r.get(col.as_str()).map(|v| v.value().clone()))
                        .collect()
                };
                // Encode as [target, [values]] so RedbCondition::resolve can
                // decode it into a typed In condition.
                let payload = ciborium::Value::Array(vec![
                    ciborium::Value::Text(target),
                    ciborium::Value::Array(values),
                ]);
                Ok(ExpressiveEnum::Scalar(AnyRedbType::untyped(payload)))
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
        let col = ColumnLike::name(column).to_string();
        let db = self.clone();

        let inner = expr_any!("{}", {
            DeferredFn::new(move || {
                let db = db.clone();
                let table = table_clone.clone();
                let col = col.clone();
                Box::pin(async move {
                    let rows = load_filtered(&db, &table).await?;
                    let values: Vec<AnyRedbType> = rows
                        .values()
                        .filter_map(|r| r.get(col.as_str()).cloned())
                        .collect();
                    Ok(ExpressiveEnum::Scalar(AnyRedbType::untyped(
                        ciborium::Value::Array(
                            values.into_iter().map(|v| v.into_value()).collect(),
                        ),
                    )))
                })
            })
        });

        let expr = expr_any!("{}", { self.defer(inner) });
        AssociatedExpression::new(expr, self)
    }
}
