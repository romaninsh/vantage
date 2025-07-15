pub mod field;
pub mod protocol;
pub mod query;

use serde_json::Value;
use vantage_expressions::{OwnedExpression, expr, protocol::selectable::Selectable};

pub use field::Field;
pub use query::{MongoCount, MongoDelete, MongoInsert, MongoSelect, MongoUpdate};

// Convenience constructors
pub fn select(collection: impl Into<String>) -> MongoSelect {
    let mut select = MongoSelect::new();
    select.set_source(expr!(collection.into()), None);
    select
}

pub fn insert(collection: impl Into<String>) -> MongoInsert {
    MongoInsert::new(collection)
}

pub fn update(collection: impl Into<String>) -> MongoUpdate {
    MongoUpdate::new(collection)
}

pub fn delete(collection: impl Into<String>) -> MongoDelete {
    MongoDelete::new(collection)
}

pub fn count(collection: impl Into<String>) -> MongoCount {
    MongoCount::new(collection)
}

#[derive(Debug, Clone)]
pub struct Document {
    fields: indexmap::IndexMap<String, Value>,
}

impl Document {
    pub fn new() -> Self {
        Self {
            fields: indexmap::IndexMap::new(),
        }
    }

    pub fn insert(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    pub fn filter(key: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::new().insert(key, value)
    }

    pub fn and(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    pub fn or(conditions: Vec<Document>) -> Self {
        let or_array: Vec<Value> = conditions.into_iter().map(|doc| doc.into()).collect();
        Self::new().insert("$or", Value::Array(or_array))
    }

    pub fn gt(key: impl Into<String>, value: impl Into<Value>) -> Self {
        let mut gt_doc = serde_json::Map::new();
        gt_doc.insert("$gt".to_string(), value.into());
        Self::new().insert(key, Value::Object(gt_doc))
    }

    pub fn lt(key: impl Into<String>, value: impl Into<Value>) -> Self {
        let mut lt_doc = serde_json::Map::new();
        lt_doc.insert("$lt".to_string(), value.into());
        Self::new().insert(key, Value::Object(lt_doc))
    }

    pub fn gte(key: impl Into<String>, value: impl Into<Value>) -> Self {
        let mut gte_doc = serde_json::Map::new();
        gte_doc.insert("$gte".to_string(), value.into());
        Self::new().insert(key, Value::Object(gte_doc))
    }

    pub fn lte(key: impl Into<String>, value: impl Into<Value>) -> Self {
        let mut lte_doc = serde_json::Map::new();
        lte_doc.insert("$lte".to_string(), value.into());
        Self::new().insert(key, Value::Object(lte_doc))
    }

    pub fn ne(key: impl Into<String>, value: impl Into<Value>) -> Self {
        let mut ne_doc = serde_json::Map::new();
        ne_doc.insert("$ne".to_string(), value.into());
        Self::new().insert(key, Value::Object(ne_doc))
    }

    pub fn in_array(key: impl Into<String>, values: Vec<Value>) -> Self {
        let mut in_doc = serde_json::Map::new();
        in_doc.insert("$in".to_string(), Value::Array(values));
        Self::new().insert(key, Value::Object(in_doc))
    }

    pub fn regex(key: impl Into<String>, pattern: impl Into<String>) -> Self {
        let mut regex_doc = serde_json::Map::new();
        regex_doc.insert("$regex".to_string(), Value::String(pattern.into()));
        Self::new().insert(key, Value::Object(regex_doc))
    }

    pub fn exists(key: impl Into<String>, exists: bool) -> Self {
        let mut exists_doc = serde_json::Map::new();
        exists_doc.insert("$exists".to_string(), Value::Bool(exists));
        Self::new().insert(key, Value::Object(exists_doc))
    }
}

impl Into<Value> for Document {
    fn into(self) -> Value {
        let mut map = serde_json::Map::new();
        for (key, value) in self.fields {
            map.insert(key, value);
        }
        Value::Object(map)
    }
}

impl Into<OwnedExpression> for Document {
    fn into(self) -> OwnedExpression {
        let value: Value = self.into();
        expr!(serde_json::to_string_pretty(&value).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_document() {
        let doc = Document::new().insert("name", "John").insert("age", 30);

        let expr: OwnedExpression = doc.into();
        let result = expr.preview();

        // Parse back to verify structure
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["name"], "John");
        assert_eq!(parsed["age"], 30);
    }

    #[test]
    fn test_document_filter() {
        let doc = Document::filter("status", "active");
        let expr: OwnedExpression = doc.into();
        let result = expr.preview();

        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "active");
    }

    #[test]
    fn test_document_gt() {
        let doc = Document::gt("age", 18);
        let expr: OwnedExpression = doc.into();
        let result = expr.preview();

        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["age"]["$gt"], 18);
    }

    #[test]
    fn test_document_or() {
        let doc = Document::or(vec![
            Document::filter("status", "active"),
            Document::filter("priority", "high"),
        ]);
        let expr: OwnedExpression = doc.into();
        let result = expr.preview();

        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["$or"].is_array());
        let or_array = parsed["$or"].as_array().unwrap();
        assert_eq!(or_array.len(), 2);
        assert_eq!(or_array[0]["status"], "active");
        assert_eq!(or_array[1]["priority"], "high");
    }

    #[test]
    fn test_document_complex() {
        let doc = Document::new()
            .insert("name", "John")
            .and("age", Document::new().insert("$gt", 18))
            .and("status", "active");

        let expr: OwnedExpression = doc.into();
        let result = expr.preview();

        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["name"], "John");
        assert_eq!(parsed["status"], "active");
        assert_eq!(parsed["age"]["$gt"], 18);
    }

    #[test]
    fn test_document_regex() {
        let doc = Document::regex("name", "^John");
        let expr: OwnedExpression = doc.into();
        let result = expr.preview();

        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["name"]["$regex"], "^John");
    }

    #[test]
    fn test_document_exists() {
        let doc = Document::exists("email", true);
        let expr: OwnedExpression = doc.into();
        let result = expr.preview();

        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["email"]["$exists"], true);
    }

    #[test]
    fn test_document_in_array() {
        let doc = Document::in_array(
            "status",
            vec![
                Value::String("active".to_string()),
                Value::String("pending".to_string()),
            ],
        );
        let expr: OwnedExpression = doc.into();
        let result = expr.preview();

        let parsed: Value = serde_json::from_str(&result).unwrap();
        let in_array = parsed["status"]["$in"].as_array().unwrap();
        assert_eq!(in_array.len(), 2);
        assert_eq!(in_array[0], "active");
        assert_eq!(in_array[1], "pending");
    }

    #[test]
    fn test_convenience_functions() {
        // Test select convenience function
        let select_query = super::select("users");
        let expr: OwnedExpression = select_query.into();
        assert_eq!(expr.preview(), "db.users.find({})");

        // Test insert convenience function
        let insert_query =
            super::insert("users").insert_one(Document::new().insert("name", "John"));
        let expr: OwnedExpression = insert_query.into();
        assert!(expr.preview().contains("db.users.insertOne"));

        // Test update convenience function
        let update_query = super::update("users").filter(Document::filter("id", 1));
        let expr: OwnedExpression = update_query.into();
        assert!(expr.preview().contains("db.users.updateMany"));

        // Test delete convenience function
        let delete_query = super::delete("users").filter(Document::filter("status", "inactive"));
        let expr: OwnedExpression = delete_query.into();
        assert!(expr.preview().contains("db.users.deleteMany"));

        // Test count convenience function
        let count_query = super::count("users").filter(Document::gt("age", 18));
        let expr: OwnedExpression = count_query.into();
        assert!(expr.preview().contains("db.users.countDocuments"));
    }
}
