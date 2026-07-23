//! `AwsTableShell` — owns a `Table<AwsAccount, EmptyEntity>` and
//! exposes it through the `TableShell` boundary.
//!
//! AWS already speaks `ciborium::Value` natively (the wire protocols
//! parse into CBOR), so this is a passthrough on reads. Conditions
//! translate `(field, CborValue)` into an `AwsCondition::Eq` and push
//! it onto the wrapped table; the dispatch layer folds AwsConditions
//! into the request body at fetch time. AWS is read-only in v0 — only
//! `can_count` is advertised.
//!
//! AWS list APIs expose no HEAD / COUNT / point-get operation, so
//! `get_vista_count` and `get_vista_value` both materialise the listing
//! and then count or filter in memory. [`crate::AwsAccount::with_max_pages`]
//! caps the walk; without it, both methods will keep paginating until the
//! response stream is exhausted. Callers that need an unbounded count on
//! an unbounded list should not call these.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Column as VistaColumn, Reference as VistaReference, ReferenceKind, TableShell, Vista,
    VistaCapabilities, VistaMetadata,
};

use crate::AwsAccount;
use crate::condition::AwsCondition;

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

    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.table.list_values().await
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        // AWS list endpoints don't expose a point-get; fall back to
        // narrowing the listed map by id — same shape as the REST shell.
        let mut data = self.table.list_values().await?;
        Ok(data.shift_remove(id))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let data = self.table.list_values().await?;
        Ok(data.into_iter().next())
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        Ok(self.table.list_values().await?.len() as i64)
    }

    /// One page per call, S3-style. The token is the **last key of the
    /// previous page**, sent as `start-after` — S3 lists keys in
    /// lexicographic order and accepts any key as a starting point, so
    /// unlike an opaque continuation token this cursor survives process
    /// restarts and can be reconstructed from already-fetched data.
    /// `IsTruncated` on the response decides whether a next page exists.
    async fn fetch_next(
        &self,
        _vista: &Vista,
        token: Option<CborValue>,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<CborValue>)> {
        if !self.capabilities.can_fetch_next {
            return Err(self.default_error("fetch_next", "can_fetch_next"));
        }
        let account = self.table.data_source();
        let name = self.table.table_name();
        let mut conditions: Vec<AwsCondition> = self.table.conditions().cloned().collect();
        if let Some(after) = token {
            conditions.push(AwsCondition::eq("start-after".to_string(), after));
        }
        let resp = account.execute_rpc_page(name, &conditions).await?;
        let truncated = match resp.get("IsTruncated") {
            Some(serde_json::Value::String(s)) => s == "true",
            Some(serde_json::Value::Bool(b)) => *b,
            _ => false,
        };
        let rows = account.parse_records(name, resp, self.metadata.id_column.as_deref())?;
        let next = truncated
            .then(|| rows.keys().last().map(|k| CborValue::Text(k.clone())))
            .flatten();
        Ok((rows.into_iter().collect(), next))
    }

    /// Cheap: the wrapped table's query state is small and the account is
    /// `Arc`-shared. Lets consumers narrow a private copy per use — e.g. an
    /// augmentation's `Detail::Fixed` rebuilding its detail vista per row.
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        Some(Box::new(AwsTableShell {
            table: self.table.clone(),
            capabilities: self.capabilities.clone(),
            metadata: self.metadata.clone(),
        }))
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        // `AwsCondition::Eq` carries the value as `CborValue` directly;
        // the wire-format builders (`build_json1_body`, `build_query_form`)
        // do the JSON / string conversion at execute time.
        self.table
            .add_condition(AwsCondition::eq(field.to_string(), value.clone()));
        Ok(())
    }

    fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        let target = self.table.get_ref_from_row::<EmptyEntity>(relation, row)?;
        let factory = crate::vista::factory::AwsVistaFactory::new(self.table.data_source().clone());
        factory.from_table(target)
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        let target = self.table.get_ref_target::<EmptyEntity>(relation)?;
        let factory = crate::vista::factory::AwsVistaFactory::new(self.table.data_source().clone());
        factory.from_table(target)
    }

    fn get_ref_kinds(&self) -> Vec<(String, ReferenceKind)> {
        self.table.ref_kinds()
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "aws"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AwsAccount;
    use crate::models::iam;
    use crate::vista::factory::metadata_from_table;
    use vantage_types::EmptyEntity;

    fn shell_for_iam_users() -> AwsTableShell {
        let aws = AwsAccount::new("AKIATEST", "secret", "eu-west-2");
        let table = iam::users_table(aws).into_entity::<EmptyEntity>();
        let metadata = metadata_from_table(&table);
        let capabilities = VistaCapabilities {
            can_count: true,
            ..VistaCapabilities::default()
        };
        AwsTableShell::new(table, capabilities, metadata)
    }

    #[test]
    fn add_eq_condition_pushes_aws_condition_eq_onto_wrapped_table() {
        // `dispatch` reads `table.conditions()` when assembling the
        // request body; a missing or mistranslated condition is invisible
        // without this introspection.
        let mut shell = shell_for_iam_users();
        shell
            .add_eq_condition("PathPrefix", &CborValue::Text("/admin/".into()))
            .expect("add_eq_condition");

        let conditions: Vec<&AwsCondition> = shell.table.conditions().collect();
        assert_eq!(conditions.len(), 1);
        match conditions[0] {
            AwsCondition::Eq { field, value } => {
                assert_eq!(field, "PathPrefix");
                assert_eq!(value, &CborValue::Text("/admin/".into()));
            }
            other => panic!("expected AwsCondition::Eq, got {other:?}"),
        }
    }
}
