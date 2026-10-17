use std::sync::Arc;

use indexmap::IndexMap;
use surreal_client::{LiveStream, SurrealClient};
use vantage_core::{Context, Result, error};
use vantage_expressions::{Expression, ExpressionFlattener, Flatten};

use crate::{AnySurrealType, surrealdb::SurrealDB};

impl SurrealDB {
    /// Wrap a connected client as a datasource.
    pub fn new(client: SurrealClient) -> Self {
        Self {
            inner: Arc::new(tokio::sync::Mutex::new(client)),
        }
    }

    /// Start a SurrealDB `LIVE SELECT` on `resource` (a table name) and return
    /// the change-notification stream. Backs the Vista `watch` path. The client
    /// is cloned out so the live RPC doesn't hold the datasource lock while it
    /// waits.
    pub async fn live(&self, resource: &str) -> Result<LiveStream> {
        let client = self.inner.lock().await.clone();
        client
            .live(resource)
            .await
            .context(error!("surrealdb live query failed", resource = resource))
    }

    /// Replace each `{}` scalar with a `$_argN` placeholder and collect the
    /// values, so every scalar reaches SurrealDB as a bound parameter.
    pub(super) fn prepare_query(
        &self,
        expr: &Expression<AnySurrealType>,
    ) -> (String, IndexMap<String, AnySurrealType>) {
        let flattener = ExpressionFlattener::new();
        let flattened = flattener.flatten(expr);

        let mut query = String::new();
        let mut params = IndexMap::new();
        let template_parts: Vec<&str> = flattened.template.split("{}").collect();
        let mut param_counter = 0;

        query.push_str(template_parts[0]);

        for (i, param) in flattened.parameters.iter().enumerate() {
            match param {
                vantage_expressions::ExpressiveEnum::Scalar(s) => {
                    param_counter += 1;
                    let param_name = format!("_arg{}", param_counter);
                    query.push_str(&format!("${}", param_name));
                    params.insert(param_name, s.clone());
                }
                vantage_expressions::ExpressiveEnum::Deferred(_) => {
                    unreachable!("Deferred params should be resolved before prepare_query");
                }
                vantage_expressions::ExpressiveEnum::Nested(_) => {
                    unreachable!("Nested params should be flattened before prepare_query");
                }
            }

            if i + 1 < template_parts.len() {
                query.push_str(template_parts[i + 1]);
            }
        }

        (query, params)
    }
}
