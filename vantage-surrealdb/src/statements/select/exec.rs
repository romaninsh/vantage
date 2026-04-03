use crate::{AnySurrealType, surrealdb::SurrealDB};
use vantage_core::Result;
use vantage_expressions::{ExprDataSource, Expressive, result};

use super::SurrealSelect;

impl SurrealSelect<result::Single> {
    pub async fn get(&self, db: &SurrealDB) -> Result<AnySurrealType> {
        db.execute(&self.expr()).await
    }
}

impl SurrealSelect<result::List> {
    pub async fn get(&self, db: &SurrealDB) -> Result<Vec<AnySurrealType>> {
        db.execute(&self.expr())
            .await?
            .try_get()
            .ok_or_else(|| vantage_core::error!("Expected array from database query"))
    }
}

impl SurrealSelect<result::Rows> {
    pub async fn get(
        &self,
        db: &SurrealDB,
    ) -> Result<Vec<indexmap::IndexMap<String, AnySurrealType>>> {
        db.execute(&self.expr())
            .await?
            .try_get()
            .ok_or_else(|| vantage_core::error!("Expected array of objects from database query"))
    }
}

impl SurrealSelect<result::SingleRow> {
    pub async fn get(&self, db: &SurrealDB) -> Result<indexmap::IndexMap<String, AnySurrealType>> {
        db.execute(&self.expr())
            .await?
            .try_get()
            .ok_or_else(|| vantage_core::error!("Expected object from database query"))
    }
}
