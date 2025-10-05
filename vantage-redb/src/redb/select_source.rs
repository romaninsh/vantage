//! SelectSource implementation for ReDB

use vantage_core::Context;
use vantage_expressions::protocol::datasource::SelectSource;

use super::core::Redb;

// Note: SelectSource now supports generic Entity type
impl SelectSource<crate::expression::RedbExpression> for Redb {
    type Select<E>
        = crate::RedbSelect<E>
    where
        E: vantage_core::Entity;

    fn select<E>(&self) -> Self::Select<E>
    where
        E: vantage_core::Entity,
    {
        crate::RedbSelect::new()
    }

    async fn execute_select<E>(&self, select: &Self::Select<E>) -> vantage_core::Result<Vec<E>>
    where
        E: vantage_core::Entity,
    {
        self.redb_execute_select(select)
            .await
            .context("Failed to execute ReDB select")
    }
}
