//! Fire a table's lifecycle hooks around a write.
//!
//! Before-write hooks run on the record about to be written, in phase order,
//! just ahead of set-invariant enforcement; an error cancels the operation.
//! `before_delete` hooks may instead return [`HookReturn::Handled`] to take over
//! (soft-delete). After-commit hooks run for their side-effects. Hooks receive
//! the entity-erased table for relation/datasource access.

use vantage_core::Result;
use vantage_types::{EmptyEntity, Record};

use crate::table::{AfterFn, BeforeDeleteFn, BeforeFn, HookReturn, Phase, Table};
use crate::traits::table_source::TableSource;

pub(crate) async fn run_before<T: TableSource>(
    hooks: &[(Phase, BeforeFn<T>)],
    record: &mut Record<T::Value>,
    table: &Table<T, EmptyEntity>,
) -> Result<()> {
    for (_phase, hook) in hooks {
        hook(record, table).await?;
    }
    Ok(())
}

pub(crate) async fn run_after<T: TableSource>(
    hooks: &[AfterFn<T>],
    id: &T::Id,
    record: &Record<T::Value>,
    table: &Table<T, EmptyEntity>,
) -> Result<()> {
    for hook in hooks {
        hook(id, record, table).await?;
    }
    Ok(())
}

/// Run before-delete hooks; the first `Handled` short-circuits and signals the
/// caller to skip the underlying delete.
pub(crate) async fn run_before_delete<T: TableSource>(
    hooks: &[BeforeDeleteFn<T>],
    id: &T::Id,
    record: &Record<T::Value>,
    table: &Table<T, EmptyEntity>,
) -> Result<HookReturn> {
    for hook in hooks {
        if let HookReturn::Handled = hook(id, record, table).await? {
            return Ok(HookReturn::Handled);
        }
    }
    Ok(HookReturn::Proceed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::mock_table_source::MockTableSource;
    use crate::table::{AfterFn, BeforeFn, Hook};
    use serde_json::{Value, json};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use vantage_dataset::prelude::{InsertableValueSet, ReadableValueSet, WritableValueSet};

    type MockTable = Table<MockTableSource, EmptyEntity>;

    // Non-capturing hooks are free fns returning a boxed future with an elided
    // lifetime tied to the args; `Arc::new(f)` coerces to the hook type.
    fn stamp<'a>(
        rec: &'a mut Record<Value>,
        _t: &'a MockTable,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            rec.insert("created".into(), json!("yes"));
            Ok(())
        })
    }

    // `&String` (not `&str`) is dictated by the hook signature — `T::Id` is `String`.
    #[allow(clippy::ptr_arg)]
    fn veto<'a>(
        _id: &'a String,
        _rec: &'a Record<Value>,
        _t: &'a MockTable,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<HookReturn>> + Send + 'a>> {
        Box::pin(async move { Err(vantage_core::error!("deletion not allowed")) })
    }

    #[allow(clippy::ptr_arg)]
    fn soft_delete<'a>(
        id: &'a String,
        _rec: &'a Record<Value>,
        table: &'a MockTable,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<HookReturn>> + Send + 'a>> {
        Box::pin(async move {
            let mut patch = Record::new();
            patch.insert("deleted".into(), json!("2026-01-01"));
            table.patch_value(id.clone(), &patch).await?;
            Ok(HookReturn::Handled)
        })
    }

    #[tokio::test]
    async fn before_insert_mutates_record() {
        let src = MockTableSource::new().with_data("t", vec![]).await;
        let table = MockTable::new("t", src)
            .with_hook(Hook::BeforeInsert(Phase::Populate, Arc::new(stamp)));
        let id = table
            .insert_return_id_value(&Record::from(json!({"name": "a"})))
            .await
            .unwrap();
        assert_eq!(
            table.get_value(id).await.unwrap().unwrap()["created"],
            json!("yes")
        );
    }

    #[tokio::test]
    async fn before_delete_vetoes() {
        let src = MockTableSource::new()
            .with_data("t", vec![json!({"id": "1", "name": "a"})])
            .await;
        let table = MockTable::new("t", src).with_hook(Hook::BeforeDelete(Arc::new(veto)));
        assert!(table.delete("1").await.is_err());
        assert!(table.get_value("1".to_string()).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn before_delete_handled_soft_deletes() {
        let src = MockTableSource::new()
            .with_data("t", vec![json!({"id": "1", "name": "a"})])
            .await;
        let table = MockTable::new("t", src).with_hook(Hook::BeforeDelete(Arc::new(soft_delete)));
        table.delete("1").await.unwrap();
        // Real delete skipped; the row is still there, now marked.
        let row = table.get_value("1".to_string()).await.unwrap().unwrap();
        assert_eq!(row["deleted"], json!("2026-01-01"));
    }

    #[tokio::test]
    async fn after_delete_runs_side_effect() {
        let hits = Arc::new(AtomicUsize::new(0));
        let seen = hits.clone();
        let hook: AfterFn<MockTableSource> = Arc::new(
            move |_id: &String,
                  _rec: &Record<Value>,
                  _t: &MockTable|
                  -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<()>> + Send + '_>,
            > {
                let seen = seen.clone();
                Box::pin(async move {
                    seen.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            },
        );
        let src = MockTableSource::new()
            .with_data("t", vec![json!({"id": "1", "name": "a"})])
            .await;
        let table = MockTable::new("t", src).with_hook(Hook::AfterDelete(hook));
        table.delete("1").await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn before_hooks_run_in_phase_order() {
        let order = Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));
        let recorder = |label: &'static str,
                        log: Arc<std::sync::Mutex<Vec<&'static str>>>|
         -> BeforeFn<MockTableSource> {
            Arc::new(
                move |_r: &mut Record<Value>,
                      _t: &MockTable|
                      -> std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<()>> + Send + '_>,
                > {
                    let log = log.clone();
                    Box::pin(async move {
                        log.lock().unwrap().push(label);
                        Ok(())
                    })
                },
            )
        };
        let src = MockTableSource::new().with_data("t", vec![]).await;
        // Registered Validate-first, but Normalize must run first.
        let table = MockTable::new("t", src)
            .with_hook(Hook::BeforeInsert(
                Phase::Validate,
                recorder("validate", order.clone()),
            ))
            .with_hook(Hook::BeforeInsert(
                Phase::Normalize,
                recorder("normalize", order.clone()),
            ));
        table
            .insert_return_id_value(&Record::from(json!({"name": "a"})))
            .await
            .unwrap();
        assert_eq!(*order.lock().unwrap(), vec!["normalize", "validate"]);
    }
}
