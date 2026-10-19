//! The table a rule script narrows: [`TableRuleTarget`] implements
//! vantage-vista's [`RuleTarget`] over a [`Table`], so the verbs of a
//! [`RuleHandle`](vantage_vista::rhai::RuleHandle) become conditions, hooks
//! and removed columns on it.

use std::any::Any;
use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_types::{EmptyEntity, Record};
use vantage_vista::FilterOp;
use vantage_vista::rhai::{HookOn, RuleHookFn, RuleHookInput, RuleTarget};

use crate::table::{
    DeleteCx, Hook, HookReturn, Phase, Table, before_delete_hook, before_hook_sync_cx,
};
use crate::traits::table_source::TableSource;

/// How a rule target moves the source's values to and from the CBOR a
/// script sees.
pub struct RuleCodec<T: TableSource> {
    pub to_cbor: fn(T::Value) -> CborValue,
    pub from_cbor: fn(CborValue) -> T::Value,
    pub id_cbor: fn(&T::Id) -> CborValue,
}

impl<T: TableSource> Clone for RuleCodec<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: TableSource> Copy for RuleCodec<T> {}

/// A [`RuleTarget`] over a table. Clones share the table, so a caller keeps
/// one clone, hands another to the `RuleHandle`, and takes the narrowed
/// table back with [`Self::into_table`] once the script has run.
///
/// `where` takes `eq` and `in`, through
/// [`TableSource::eq_value_condition`] and
/// [`TableSource::in_values_condition`]; any other operator is an error
/// naming it. Hooks run in [`Phase::Validate`], so they see the record as
/// it will be written. On a patch the script's `record` is the full row,
/// the stored row with the patched fields on top; on an insert or a replace
/// it is exactly the record written. A delete hands the id and the stored
/// row ([`RuleHookInput::Delete`]).
pub struct TableRuleTarget<T: TableSource> {
    table: Arc<Mutex<Table<T, EmptyEntity>>>,
    codec: RuleCodec<T>,
}

impl<T: TableSource> Clone for TableRuleTarget<T> {
    fn clone(&self) -> Self {
        Self {
            table: self.table.clone(),
            codec: self.codec,
        }
    }
}

impl<T: TableSource> TableRuleTarget<T> {
    pub fn new(table: Table<T, EmptyEntity>, codec: RuleCodec<T>) -> Self {
        Self {
            table: Arc::new(Mutex::new(table)),
            codec,
        }
    }

    /// The table as the rule left it.
    pub fn into_table(self) -> Table<T, EmptyEntity> {
        self.table.lock().unwrap().clone()
    }

    fn table(&self) -> std::sync::MutexGuard<'_, Table<T, EmptyEntity>> {
        self.table.lock().unwrap()
    }

    /// Add `condition` and describe it: the source's description, or
    /// `preview` when the source has none.
    fn narrow(&self, condition: T::Condition, preview: impl FnOnce() -> String) -> String {
        let mut table = self.table();
        let described = table.data_source().describe_condition(&condition);
        table.add_condition(condition);
        if described == "<condition>" {
            preview()
        } else {
            described
        }
    }
}

fn record_to_cbor<T: TableSource>(
    to_cbor: fn(T::Value) -> CborValue,
    record: &Record<T::Value>,
) -> CborValue {
    CborValue::Map(
        record
            .iter()
            .map(|(k, v)| (CborValue::Text(k.clone()), to_cbor(v.clone())))
            .collect(),
    )
}

impl<T> RuleTarget for TableRuleTarget<T>
where
    T: TableSource + 'static,
    T::Condition: 'static,
{
    fn where_value(&mut self, column: &str, op: FilterOp, value: CborValue) -> Result<String> {
        let from_cbor = self.codec.from_cbor;
        let condition = {
            let table = self.table();
            let source = table.data_source();
            match op {
                FilterOp::Eq => source.eq_value_condition(column, from_cbor(value))?,
                FilterOp::InSet => {
                    let CborValue::Array(items) = value else {
                        return Err(error!(
                            "where(column, \"in\", values) needs an array of values",
                            column = column
                        ));
                    };
                    source
                        .in_values_condition(column, items.into_iter().map(from_cbor).collect())?
                }
                other => {
                    return Err(error!(
                        format!(
                            "operator `{}` isn't supported in a rule's where",
                            other.key_symbol()
                        ),
                        column = column
                    )
                    .mark_unsupported());
                }
            }
        };
        Ok(self.narrow(condition, || format!("{column} {}", op.key_symbol())))
    }

    fn native_condition(
        &mut self,
        condition: Box<dyn Any + Send>,
        preview: String,
    ) -> Result<String> {
        let condition = condition.downcast::<T::Condition>().map_err(|_| {
            error!(
                "native condition isn't this table's condition type",
                expected = std::any::type_name::<T::Condition>(),
                preview = preview.clone()
            )
            .mark_incorrect_usage()
        })?;
        Ok(self.narrow(*condition, || preview))
    }

    fn hook(&mut self, on: HookOn, run: RuleHookFn) -> Result<()> {
        let codec = self.codec;
        let refuse = |message: String| error!(message);
        let hook = match on {
            HookOn::BeforeDelete => {
                Hook::BeforeDelete(before_delete_hook::<T, _>(move |cx: DeleteCx<'_, T>| {
                    let outcome = run(RuleHookInput::Delete {
                        id: (codec.id_cbor)(cx.id),
                        row: record_to_cbor::<T>(codec.to_cbor, cx.record),
                    })
                    .map(|()| HookReturn::Proceed)
                    .map_err(refuse);
                    Box::pin(std::future::ready(outcome))
                }))
            }
            on => {
                // The script judges the row as it will be: a patch is the
                // stored row with the patched fields on top; an insert or a
                // replace is exactly the record written.
                let before = before_hook_sync_cx::<T>(move |cx| {
                    let row = match cx.stored {
                        Some(stored) if cx.patch => {
                            let mut row = stored.clone();
                            for (k, v) in cx.record.iter() {
                                row.insert(k.clone(), v.clone());
                            }
                            row
                        }
                        _ => cx.record.clone(),
                    };
                    run(RuleHookInput::Record(record_to_cbor::<T>(
                        codec.to_cbor,
                        &row,
                    )))
                    .map_err(refuse)
                });
                match on {
                    HookOn::BeforeInsert => Hook::BeforeInsert(Phase::Validate, before),
                    HookOn::BeforeUpdate => Hook::BeforeUpdate(Phase::Validate, before),
                    _ => Hook::BeforeSave(Phase::Validate, before),
                }
            }
        };
        self.table().add_hook(hook);
        Ok(())
    }

    fn remove_column(&mut self, column: &str) -> Result<()> {
        self.table().remove_column(column)
    }
}
