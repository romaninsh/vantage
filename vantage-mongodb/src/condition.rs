//! MongoDB condition type — analogous to `Expression<T>` for SQL backends.
//!
//! `MongoCondition` can hold immediate `bson::Document` filters, async-deferred
//! values (resolved at query time via `DeferredFn`), or nested combinations.

use bson::{Bson, Document};
use vantage_expressions::{DeferredFn, ExpressiveEnum};

use crate::types::AnyMongoType;

/// A MongoDB filter condition that can contain deferred values.
///
/// # Variants
///
/// - `Doc` — an immediate `bson::Document` filter, e.g. `doc! { "price": { "$gt": 100 } }`
/// - `Deferred` — resolves async at query time via `DeferredFn<AnyMongoType>`.
///   The resolved `AnyMongoType` must be a `Bson::Document`.
/// - `And` — combines multiple conditions with `$and` semantics.
#[derive(Clone)]
pub enum MongoCondition {
    /// Immediate filter document.
    Doc(Document),
    /// Async-resolved filter — the `DeferredFn` should produce an `AnyMongoType`
    /// wrapping a `Bson::Document`.
    Deferred(DeferredFn<AnyMongoType>),
    /// Multiple conditions combined with `$and`.
    And(Vec<MongoCondition>),
}

impl MongoCondition {
    /// Resolve all deferred values and merge into a single `bson::Document`.
    pub fn resolve(
        &self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = vantage_core::Result<Document>> + Send + '_>,
    > {
        Box::pin(async move {
            match self {
                MongoCondition::Doc(doc) => Ok(doc.clone()),
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
                        docs.push(c.resolve().await?);
                    }
                    merge_documents(docs)
                }
            }
        })
    }
}

// ── Conversions ──────────────────────────────────────────────────────

impl From<Document> for MongoCondition {
    fn from(doc: Document) -> Self {
        MongoCondition::Doc(doc)
    }
}

/// Required by `ReferenceOne`/`ReferenceMany` trait bounds for `get_linked_table`
/// (JOIN-style queries). MongoDB does not support JOINs — this will never be called
/// via `get_related_table` (which uses `related_in_condition` instead).
impl From<vantage_expressions::Expression<AnyMongoType>> for MongoCondition {
    fn from(_expr: vantage_expressions::Expression<AnyMongoType>) -> Self {
        unimplemented!("MongoDB does not support Expression-based conditions (JOINs)")
    }
}

impl std::fmt::Debug for MongoCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MongoCondition::Doc(doc) => write!(f, "Doc({doc})"),
            MongoCondition::Deferred(_) => write!(f, "Deferred(...)"),
            MongoCondition::And(conditions) => f.debug_tuple("And").field(conditions).finish(),
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Convert a `Bson` value into a `Document`, or error if it's not one.
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
fn merge_documents(docs: Vec<Document>) -> vantage_core::Result<Document> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_document() {
        let doc = bson::doc! { "price": { "$gt": 100 } };
        let cond: MongoCondition = doc.clone().into();
        match cond {
            MongoCondition::Doc(d) => assert_eq!(d, doc),
            _ => panic!("expected Doc variant"),
        }
    }

    #[tokio::test]
    async fn test_resolve_doc() {
        let cond = MongoCondition::Doc(bson::doc! { "active": true });
        let resolved = cond.resolve().await.unwrap();
        assert_eq!(resolved, bson::doc! { "active": true });
    }

    #[tokio::test]
    async fn test_resolve_and() {
        let cond = MongoCondition::And(vec![
            bson::doc! { "a": 1 }.into(),
            bson::doc! { "b": 2 }.into(),
        ]);
        let resolved = cond.resolve().await.unwrap();
        assert_eq!(resolved, bson::doc! { "$and": [{ "a": 1 }, { "b": 2 }] });
    }

    #[tokio::test]
    async fn test_resolve_and_single() {
        let cond = MongoCondition::And(vec![bson::doc! { "x": 1 }.into()]);
        let resolved = cond.resolve().await.unwrap();
        assert_eq!(resolved, bson::doc! { "x": 1 });
    }

    #[tokio::test]
    async fn test_resolve_and_empty() {
        let cond = MongoCondition::And(vec![]);
        let resolved = cond.resolve().await.unwrap();
        assert_eq!(resolved, bson::doc! {});
    }

    #[tokio::test]
    async fn test_resolve_conditions_helper() {
        let conds = [
            MongoCondition::Doc(bson::doc! { "a": 1 }),
            MongoCondition::Doc(bson::doc! { "b": 2 }),
        ];
        let resolved = resolve_conditions(conds.iter()).await.unwrap();
        assert_eq!(resolved, bson::doc! { "$and": [{ "a": 1 }, { "b": 2 }] });
    }

    #[tokio::test]
    async fn test_deferred_resolves_document() {
        let deferred = DeferredFn::new(move || {
            Box::pin(async move {
                let doc = bson::doc! { "status": "active" };
                Ok(ExpressiveEnum::Scalar(AnyMongoType::untyped(
                    Bson::Document(doc),
                )))
            })
        });
        let cond = MongoCondition::Deferred(deferred);
        let resolved = cond.resolve().await.unwrap();
        assert_eq!(resolved, bson::doc! { "status": "active" });
    }

    #[tokio::test]
    async fn test_nested_and_with_deferred() {
        let deferred = DeferredFn::new(move || {
            Box::pin(async move {
                let doc = bson::doc! { "owner_id": { "$in": ["a", "b"] } };
                Ok(ExpressiveEnum::Scalar(AnyMongoType::untyped(
                    Bson::Document(doc),
                )))
            })
        });
        let cond = MongoCondition::And(vec![
            bson::doc! { "active": true }.into(),
            MongoCondition::Deferred(deferred),
        ]);
        let resolved = cond.resolve().await.unwrap();
        assert_eq!(
            resolved,
            bson::doc! { "$and": [
                { "active": true },
                { "owner_id": { "$in": ["a", "b"] } }
            ] }
        );
    }
}
