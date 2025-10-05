//! SelectSource implementation for ReDB

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

    async fn execute_select<E>(
        &self,
        select: &Self::Select<E>,
    ) -> vantage_expressions::util::Result<Vec<E>>
    where
        E: vantage_core::Entity,
    {
        self.redb_execute_select(select)
            .await
            .map_err(|e| vantage_expressions::util::Error::new(format!("{}", e)))
    }
}
