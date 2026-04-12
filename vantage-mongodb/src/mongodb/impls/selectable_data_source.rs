//! SelectableDataSource implementation for MongoDB.
//!
//! Wires `MongoSelect` into the Vantage query pipeline so `table.select()` works.
//! `execute_select` runs the built query against MongoDB using the driver directly.

use futures_util::TryStreamExt;

use vantage_expressions::Expression;
use vantage_expressions::traits::datasource::SelectableDataSource;

use crate::condition::MongoCondition;
use crate::mongodb::MongoDB;
use crate::select::MongoSelect;
use crate::types::AnyMongoType;

impl SelectableDataSource<AnyMongoType, MongoCondition> for MongoDB {
    type Select = MongoSelect;

    fn select(&self) -> Self::Select {
        MongoSelect::new()
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
        let coll_name = select
            .collection
            .as_deref()
            .ok_or_else(|| vantage_core::error!("MongoSelect has no collection set"))?;

        let filter = select.build_filter().await?;
        let options = select.build_find_options();
        let coll = self.doc_collection(coll_name);

        let cursor = coll.find(filter).with_options(options).await.map_err(|e| {
            vantage_core::error!(
                "MongoDB execute_select find failed",
                details = e.to_string()
            )
        })?;

        let docs: Vec<bson::Document> = cursor.try_collect().await.map_err(|e| {
            vantage_core::error!(
                "MongoDB execute_select cursor failed",
                details = e.to_string()
            )
        })?;

        docs.into_iter()
            .map(|doc| {
                AnyMongoType::from_bson(&bson::Bson::Document(doc)).ok_or_else(|| {
                    vantage_core::error!("Failed to convert Document to AnyMongoType")
                })
            })
            .collect()
    }
}
