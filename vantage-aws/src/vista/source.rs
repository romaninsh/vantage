//! `AwsTableShell` — owns a `Table<AwsAccount, EmptyEntity>` and
//! exposes it through the `TableShell` boundary.
//!
//! AWS already speaks `ciborium::Value` natively (the wire protocols parse
//! into CBOR), so reads are a passthrough. Equality conditions become
//! `AwsCondition::Eq` on the wrapped table; the dispatch layer folds them
//! into the request (a path placeholder or request parameter) and applies
//! them to rows that carry the field. A clone copies the table's conditions
//! and shares the account. AWS is read-only: no write is advertised.
//!
//! AWS list APIs expose no COUNT, offset or point-get, so `count` and `get`
//! materialise the listing and count or filter it in memory, and offset
//! windows are refused. [`crate::AwsAccount::with_max_pages`] caps the walk.
//! S3 object listings page by key (see [`cursor`]).

pub(crate) mod cursor;
#[cfg(test)]
mod tests;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Capability, Column as VistaColumn, Condition, FilterOp, Page, Preview,
    Reference as VistaReference, TableShell, Vista, VistaCapabilities, VistaMetadata, unsupported,
};

use crate::AwsAccount;
use crate::condition::AwsCondition;
use crate::vista::factory::AwsVistaFactory;

#[derive(Clone)]
pub struct AwsTableShell {
    pub(crate) table: Table<AwsAccount, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
}

impl AwsTableShell {
    pub(crate) fn new(
        table: Table<AwsAccount, EmptyEntity>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
        }
    }

    /// Whether a condition may name `field`: a declared column, or a
    /// `{placeholder}` in the request path, which the table needn't declare.
    fn knows_field(&self, field: &str) -> bool {
        self.metadata.columns.contains_key(field)
            || self.table.table_name().contains(&format!("{{{field}}}"))
    }
}

#[async_trait]
impl TableShell for AwsTableShell {
    fn columns(&self) -> &IndexMap<String, VistaColumn> {
        &self.metadata.columns
    }

    fn references(&self) -> &IndexMap<String, VistaReference> {
        &self.metadata.references
    }

    fn id_column(&self) -> Option<&str> {
        self.metadata.id_column.as_deref()
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "aws"
    }

    /// The RPC a read would dispatch, decoded out of the table name, plus
    /// the conditions that shape it.
    fn preview(&self) -> Preview {
        let table_name = self.table.table_name();
        let preview = Preview::new("aws");
        let op = match crate::dispatch::parse_table_name(table_name) {
            Ok(op) => op,
            Err(e) => return preview.with_note(format!("{table_name}: {e}")),
        };
        let conditions: Vec<String> = self
            .table
            .conditions()
            .map(|c| format!("{} = {:?}", c.field(), c))
            .collect();
        let paging = if op.cursor.is_some() {
            "every page is fetched"
        } else {
            "single page"
        };
        preview
            .with_query(format!(
                "{:?} {} {} [{}]",
                op.protocol,
                op.service,
                op.target,
                conditions.join(", ")
            ))
            .with_note(format!(
                "{paging}; a condition fills a path placeholder or becomes a request \
                 parameter, and is applied to the rows that carry its field"
            ))
    }

    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        self.table.list_values().await
    }

    /// AWS list endpoints have no point-get: the set is listed and the row
    /// picked out.
    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        Ok(self.table.list_values().await?.shift_remove(id))
    }

    async fn count(&self) -> Result<i64> {
        Ok(self.table.list_values().await?.len() as i64)
    }

    /// AWS pages by continuation token, never by offset.
    async fn fetch_window(&self, _offset: usize, _limit: usize) -> Result<Page> {
        unsupported(Capability::Window)
    }

    async fn fetch_cursor(
        &self,
        token: Option<CborValue>,
        limit: usize,
    ) -> Result<(Page, Option<CborValue>)> {
        if !self.capabilities.has(Capability::Cursor) {
            return unsupported(Capability::Cursor);
        }
        self.fetch_after(token, limit).await
    }

    /// Equality on a declared column or a path placeholder. The value is
    /// sent as-is; the wire builders convert it at execute time.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        if !self.knows_field(field) {
            return Err(error!("Unknown column for condition", field = field));
        }
        if condition.op != FilterOp::Eq {
            return unsupported(Capability::FilterOps);
        }
        self.table
            .add_condition(AwsCondition::eq(field, condition.value.clone()));
        Ok(())
    }

    /// The target comes from the typed table's `with_many`/`with_one`; from
    /// a row it is narrowed by the reference's join keys.
    fn get_ref(&self, relation: &str, row: Option<&Record<CborValue>>) -> Result<Vista> {
        let reference = self
            .metadata
            .references
            .get(relation)
            .ok_or_else(|| error!("Unknown relation", relation = relation).mark_not_found())?;
        let target = self.table.get_ref_target_erased(relation)?;
        let target = AwsVistaFactory::new(self.table.data_source().clone()).from_table(target)?;
        match row {
            Some(row) => reference.narrow(target, row, CborValue::clone),
            None => Ok(target),
        }
    }
}
