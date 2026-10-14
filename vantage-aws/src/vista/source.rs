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
//! materialise the listing and count or filter it in memory. The leading
//! window is cut from the listing; any other offset is refused.
//! [`crate::AwsAccount::with_max_pages`] caps the walk.
//! S3 object listings page by key (see [`cursor`]).

pub(crate) mod cursor;
#[cfg(test)]
mod tests;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Capability, Condition, FilterOp, Page, Preview, TableShell, Vista, VistaCapabilities,
    VistaMetadata, condition_column_error, unknown_relation, unsupported,
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
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "aws"
    }

    /// The RPC a read would dispatch, decoded out of the table name, with the
    /// conditions that fill its path placeholders and request parameters,
    /// and how many pages it walks.
    fn preview(&self) -> Preview {
        let table_name = self.table.table_name();
        let preview = Preview::new();
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
            "every page"
        } else {
            "first page"
        };
        let preview = preview.with_query(format!(
            "{:?} {} {} [{}] ({paging})",
            op.protocol,
            op.service,
            op.target,
            conditions.join(", ")
        ));
        if conditions.is_empty() {
            preview
        } else {
            preview.with_note("conditions on row fields are also applied to the fetched rows")
        }
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

    /// AWS pages by continuation token, never by offset: the leading window
    /// is cut from the listing.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        Ok(Page::from_listing(
            self.table.list_values().await?,
            offset,
            limit,
        ))
    }

    async fn fetch_cursor(
        &self,
        token: Option<CborValue>,
        limit: usize,
    ) -> Result<(Page, Option<CborValue>)> {
        self.fetch_after(token, limit).await
    }

    /// Equality on a declared column or a path placeholder. The value is
    /// sent as-is; the wire builders convert it at execute time. A dotted
    /// path into a declared column is refused, for the caller to evaluate.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        if !self.knows_field(field) {
            return Err(condition_column_error(&self.metadata, field));
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
            .ok_or_else(|| unknown_relation(relation))?;
        self.table.traverse_erased(reference, row, |target| {
            AwsVistaFactory::new(self.table.data_source().clone()).from_table(target)
        })
    }
}
