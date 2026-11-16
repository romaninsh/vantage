//! The MongoDB filter type, standing where `Expression<T>` does for SQL
//! backends: an immediate document, a deferred one, or a combination.

use std::sync::Arc;

use bson::{Bson, Document};
use futures_util::future::BoxFuture;
use vantage_core::Result;
use vantage_expressions::{DeferredFn, ExpressiveEnum};

use crate::mongodb::MongoDB;
use crate::operation::negate_doc;
use crate::types::AnyMongoType;

/// Reads the filter a [`MongoCondition::DeferredIn`] resolves to, through the
/// `MongoDB` resolving the condition: inside a transaction, its session.
pub type RelatedRead = Arc<dyn Fn(&MongoDB) -> BoxFuture<'static, Result<Document>> + Send + Sync>;

/// A MongoDB filter condition that can contain deferred values.
#[derive(Clone)]
pub enum MongoCondition {
    /// An immediate filter, e.g. `doc! { "price": { "$gt": 100 } }`.
    Doc(Document),
    /// A filter resolved at query time. It must produce an `AnyMongoType`
    /// wrapping a `Bson::Document`; anything else fails to resolve.
    Deferred(DeferredFn<AnyMongoType>),
    /// A filter over another table's rows known to constrain `column`
    /// (traversal's `{ fk: { $in: parent ids } }`), so the column is readable
    /// before resolution. The rows are read when the condition is resolved,
    /// by the `MongoDB` resolving it.
    DeferredIn { column: String, read: RelatedRead },
    /// Conditions combined with `$and`.
    And(Vec<MongoCondition>),
    /// The negation of a condition with a deferred part, negated once that
    /// part is resolved.
    Not(Box<MongoCondition>),
}

impl MongoCondition {
    /// Resolve every deferred part and merge the result into one document;
    /// `db` does the reads of a `DeferredIn`.
    pub fn resolve<'a>(&'a self, db: &'a MongoDB) -> BoxFuture<'a, Result<Document>> {
        Box::pin(async move {
            match self {
                MongoCondition::Doc(doc) => Ok(doc.clone()),
                MongoCondition::DeferredIn { read, .. } => read(db).await,
                MongoCondition::Not(inner) => Ok(negate_doc(inner.resolve(db).await?)),
                MongoCondition::Deferred(deferred) => {
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
                        docs.push(c.resolve(db).await?);
                    }
                    Ok(merge_documents(docs))
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
            MongoCondition::Not(inner) => f.debug_tuple("Not").field(inner).finish(),
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
pub(crate) fn merge_documents(docs: Vec<Document>) -> Document {
    match docs.len() {
        0 => Document::new(),
        1 => docs.into_iter().next().unwrap(),
        _ => {
            let array: Vec<Bson> = docs.into_iter().map(Bson::Document).collect();
            bson::doc! { "$and": array }
        }
    }
}

/// Resolve an iterator of `MongoCondition` references into a single filter
/// document; `db` does the reads of a `DeferredIn`.
pub async fn resolve_conditions<'a>(
    conditions: impl Iterator<Item = &'a MongoCondition>,
    db: &MongoDB,
) -> vantage_core::Result<Document> {
    let mut docs = Vec::new();
    for c in conditions {
        docs.push(c.resolve(db).await?);
    }
    Ok(merge_documents(docs))
}
