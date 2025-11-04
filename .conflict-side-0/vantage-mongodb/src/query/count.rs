use serde_json::Value;
use vantage_expressions::{Expression, expr};

use crate::Document;

#[derive(Debug, Clone)]
pub struct MongoCount {
    collection: String,
    filter: Vec<Document>,
}

impl MongoCount {
    pub fn new(collection: impl Into<String>) -> Self {
        Self {
            collection: collection.into(),
            filter: Vec::new(),
        }
    }

    pub fn filter(mut self, filter: Document) -> Self {
        self.filter.push(filter);
        self
    }
}

impl From<MongoCount> for Expression {
    fn from(val: MongoCount) -> Self {
        let filter = if val.filter.is_empty() {
            "{}".to_string()
        } else {
            // Combine filters
            let mut combined = Document::new();
            for f in val.filter {
                let value: Value = f.into();
                if let Value::Object(obj) = value {
                    for (key, val) in obj {
                        combined = combined.insert(key, val);
                    }
                }
            }
            Into::<Expression>::into(combined).preview()
        };

        expr!(format!("db.{}.countDocuments({})", val.collection, filter))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count() {
        let count = MongoCount::new("users").filter(Document::gt("age", 18));
        let expr: Expression = count.into();
        let result = expr.preview();
        assert!(result.contains("db.users.countDocuments("));
        assert!(result.contains("$gt"));
    }
}
