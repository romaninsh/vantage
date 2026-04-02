use vantage_core::{Result, util::error::Context};
use vantage_expressions::SelectSource;

use crate::{SurrealDB, SurrealSelect};

impl SelectSource for SurrealDB {
    type Select<E>
        = SurrealSelect
    where
        E: vantage_core::Entity;

    fn select<E>(&self) -> Self::Select<E>
    where
        E: vantage_core::Entity,
    {
        SurrealSelect::new()
    }

    async fn execute_select<E>(&self, select: &Self::Select<E>) -> Result<Vec<E>>
    where
        E: vantage_core::Entity,
    {
        use vantage_expressions::QuerySource;

        // For SurrealDB, convert select to expression and execute
        let expr = select.clone().into();
        let raw_result = self.execute(&expr).await;

        // Parse JSON response into Vec<E>
        match raw_result {
            serde_json::Value::Array(items) => {
                let entities = items
                    .into_iter()
                    .map(|item| serde_json::from_value::<E>(item))
                    .collect::<std::result::Result<Vec<E>, _>>()
                    .context("Failed to deserialize entities")?;
                Ok(entities)
            }
            other => {
                // Single object, convert to Vec
                let entity =
                    serde_json::from_value::<E>(other).context("Failed to deserialize entity")?;
                Ok(vec![entity])
            }
        }
    }
}
