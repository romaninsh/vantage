//! SelectableDataSource implementation for MongoDB.
//!
//! Wires `MongoSelect` into the Vantage query pipeline so `table.select()` works.
//! `execute_select` runs the built query against MongoDB using the driver
//! directly: a `find`, or an `aggregate` over the data source's view.

use vantage_expressions::Expression;
use vantage_expressions::traits::datasource::SelectableDataSource;

use crate::condition::MongoCondition;
use crate::mongodb::MongoDB;
use crate::select::MongoSelect;
use crate::types::AnyMongoType;

impl SelectableDataSource<AnyMongoType, MongoCondition> for MongoDB {
    type Select = MongoSelect;

    fn select(&self) -> Self::Select {
        MongoSelect {
            pre_stages: self.view_stages(),
            ..MongoSelect::new()
        }
    }

    fn add_select_column(
        &self,
        select: &mut Self::Select,
        _expression: Expression<AnyMongoType>,
        alias: Option<&str>,
    ) {
        // MongoDB projections use field names, not expressions.
        // If an alias is given, use it as the projected field name.
        if let Some(alias) = alias {
            select.fields.push(alias.to_string());
        }
    }

    async fn execute_select(
        &self,
        select: &Self::Select,
    ) -> vantage_core::Result<Vec<AnyMongoType>> {
        let docs = self.read_docs(select).await?;

        docs.into_iter()
            .map(|doc| {
                AnyMongoType::from_bson(&bson::Bson::Document(doc)).ok_or_else(|| {
                    vantage_core::error!("Failed to convert Document to AnyMongoType")
                })
            })
            .collect()
    }
}
