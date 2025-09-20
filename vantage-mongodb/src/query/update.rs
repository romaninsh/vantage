use serde_json::Value;
use vantage_expressions::{Expression, expr};

use crate::Document;

#[derive(Debug, Clone)]
pub struct MongoUpdate {
    collection: String,
    filter: Vec<Document>,
    update: Option<Document>,
}

impl MongoUpdate {
    pub fn new(collection: impl Into<String>) -> Self {
        Self {
            collection: collection.into(),
            filter: Vec::new(),
            update: None,
        }
    }

    pub fn filter(mut self, filter: Document) -> Self {
        self.filter.push(filter);
        self
    }

    pub fn set_update(mut self, update: Document) -> Self {
        self.update = Some(update);
        self
    }
}

impl Into<Expression> for MongoUpdate {
    fn into(self) -> Expression {
        let filter = if self.filter.is_empty() {
            "{}".to_string()
        } else {
            // Combine filters
            let mut combined = Document::new();
            for f in self.filter {
                let value: Value = f.into();
                if let Value::Object(obj) = value {
                    for (key, val) in obj {
                        combined = combined.insert(key, val);
                    }
                }
            }
            Into::<Expression>::into(combined).preview()
        };

        let update =
            Into::<Expression>::into(self.update.unwrap_or_else(|| Document::new())).preview();

        expr!(format!(
            "db.{}.updateMany({}, {})",
            self.collection, filter, update
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update() {
        let update = MongoUpdate::new("users")
            .filter(Document::filter("name", "John"))
            .set_update(Document::new().insert("$set", Document::new().insert("age", 31)));
        let expr: Expression = update.into();
        let result = expr.preview();
        assert!(result.contains("db.users.updateMany("));
        assert!(result.contains("\"name\""));
        assert!(result.contains("\"John\""));
        assert!(result.contains("$set"));
    }
}
