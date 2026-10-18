//! The MongoDB filter type, standing where `Expression<T>` does for SQL
//! backends: an immediate document, a deferred one, or a combination.

use bson::{Bson, Document};
use vantage_expressions::{DeferredFn, ExpressiveEnum};

use crate::types::AnyMongoType;

/// A MongoDB filter condition that can contain deferred values.
#[derive(Clone)]
pub enum MongoCondition {
    /// An immediate filter, e.g. `doc! { "price": { "$gt": 100 } }`.
    Doc(Document),
    /// A filter resolved at query time. It must produce an `AnyMongoType`
    /// wrapping a `Bson::Document`; anything else fails to resolve.
    Deferred(DeferredFn<AnyMongoType>),
    /// A [`Deferred`](Self::Deferred) filter known to constrain `column`
    /// (traversal's `{ fk: { $in: parent ids } }`), so the column is readable
    /// before resolution. Resolves exactly as `Deferred` does.
    DeferredIn {
        column: String,
        deferred: DeferredFn<AnyMongoType>,
    },
    /// Conditions combined with `$and`.
    And(Vec<MongoCondition>),
}

impl MongoCondition {
    /// Resolve every deferred part and merge the result into one document.
    pub fn resolve(
        &self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = vantage_core::Result<Document>> + Send + '_>,
    > {
        Box::pin(async move {
            match self {
                MongoCondition::Doc(doc) => Ok(doc.clone()),
                MongoCondition::Deferred(deferred)
                | MongoCondition::DeferredIn { deferred, .. } => {
                    let result = deferred.call().await?;
                    let resolved = match result {
                        ExpressiveEnum::Scalar(val) => val,
                        other => {
                            return Err(vantage_core::error!(
                                "MongoCondition::Deferred resolved to non-scalar",
                                result = format!("{:?}", other)
                            ));
                        }
                    };
                    bson_to_document(resolved.into_value())
                }
                MongoCondition::And(conditions) => {
                    let mut docs = Vec::with_capacity(conditions.len());
                    for c in conditions {
                        docs.push(c.resolve().await?);
                    }
                    merge_documents(docs)
                }
            }
        })
    }
}

impl From<Document> for MongoCondition {
    fn from(doc: Document) -> Self {
        MongoCondition::Doc(doc)
    }
}

impl std::fmt::Debug for MongoCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MongoCondition::Doc(doc) => write!(f, "Doc({doc})"),
            MongoCondition::Deferred(_) => write!(f, "Deferred(...)"),
            MongoCondition::DeferredIn { column, .. } => write!(f, "DeferredIn({column}, ...)"),
            MongoCondition::And(conditions) => f.debug_tuple("And").field(conditions).finish(),
        }
    }
}

fn bson_to_document(value: Bson) -> vantage_core::Result<Document> {
    match value {
        Bson::Document(doc) => Ok(doc),
        other => Err(vantage_core::error!(
            "Expected Bson::Document from deferred condition",
            actual = format!("{:?}", other)
        )),
    }
}

/// Merge multiple documents into a single filter.
///
/// - Empty list → `{}`
/// - Single doc → returned as-is
/// - Multiple docs → `{ "$and": [ doc1, doc2, ... ] }`
pub(crate) fn merge_documents(docs: Vec<Document>) -> vantage_core::Result<Document> {
    Ok(match docs.len() {
        0 => Document::new(),
        1 => docs.into_iter().next().unwrap(),
        _ => {
            let array: Vec<Bson> = docs.into_iter().map(Bson::Document).collect();
            bson::doc! { "$and": array }
        }
    })
}

/// Resolve an iterator of `MongoCondition` references into a single filter document.
pub async fn resolve_conditions<'a>(
    conditions: impl Iterator<Item = &'a MongoCondition>,
) -> vantage_core::Result<Document> {
    let mut docs = Vec::new();
    for c in conditions {
        docs.push(c.resolve().await?);
    }
    merge_documents(docs)
}
