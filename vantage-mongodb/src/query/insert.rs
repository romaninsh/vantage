use vantage_expressions::{OwnedExpression, expr};

use crate::Document;

#[derive(Debug, Clone)]
pub struct MongoInsert {
    collection: String,
    documents: Vec<Document>,
}

impl MongoInsert {
    pub fn new(collection: impl Into<String>) -> Self {
        Self {
            collection: collection.into(),
            documents: Vec::new(),
        }
    }

    pub fn insert_one(mut self, doc: Document) -> Self {
        self.documents = vec![doc];
        self
    }

    pub fn insert_many(mut self, docs: Vec<Document>) -> Self {
        self.documents = docs;
        self
    }
}

impl Into<OwnedExpression> for MongoInsert {
    fn into(self) -> OwnedExpression {
        if self.documents.len() == 1 {
            expr!(format!(
                "db.{}.insertOne({})",
                self.collection,
                Into::<OwnedExpression>::into(self.documents[0].clone()).preview()
            ))
        } else {
            let docs: Vec<String> = self
                .documents
                .iter()
                .map(|doc| Into::<OwnedExpression>::into(doc.clone()).preview())
                .collect();
            expr!(format!(
                "db.{}.insertMany([{}])",
                self.collection,
                docs.join(", ")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_one() {
        let insert = MongoInsert::new("users")
            .insert_one(Document::new().insert("name", "John").insert("age", 30));
        let expr: OwnedExpression = insert.into();
        let result = expr.preview();
        assert!(result.contains("db.users.insertOne("));
        assert!(result.contains("\"name\""));
        assert!(result.contains("\"John\""));
    }

    #[test]
    fn test_insert_many() {
        let insert = MongoInsert::new("users").insert_many(vec![
            Document::new().insert("name", "John"),
            Document::new().insert("name", "Jane"),
        ]);
        let expr: OwnedExpression = insert.into();
        let result = expr.preview();
        assert!(result.contains("db.users.insertMany(["));
        assert!(result.contains("\"John\""));
        assert!(result.contains("\"Jane\""));
    }
}
