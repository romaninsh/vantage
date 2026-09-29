//! Filter conditions both layers compile to.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_expressions::traits::expressive::{DeferredFn, ExpressiveEnum};
use vantage_types::Record;
use vantage_vista::{FilterOp, operand_text};

use super::compare::{cmp_values, like, lookup, text_of, values_eq};
use crate::AnyMemoryType;

#[derive(Clone)]
pub enum MemoryCondition {
    /// `path op value`. For `InSet` / `NotInSet`, `value` is an array.
    Cmp {
        path: String,
        op: FilterOp,
        value: CborValue,
    },
    /// Case-insensitive substring over every text or number cell.
    Search(String),
    And(Vec<MemoryCondition>),
    Or(Vec<MemoryCondition>),
    Not(Box<MemoryCondition>),
    /// A bare column reference, valid only as an order key.
    Column(String),
    /// Resolved before evaluation into `Cmp { op: InSet }`. The payload is
    /// a CBOR array `[Text(path), Array(values)]`.
    Deferred(DeferredFn<AnyMemoryType>),
}

impl MemoryCondition {
    pub fn cmp(path: impl Into<String>, op: FilterOp, value: impl Into<CborValue>) -> Self {
        Self::Cmp {
            path: path.into(),
            op,
            value: value.into(),
        }
    }

    pub fn matches(&self, record: &Record<CborValue>) -> Result<bool> {
        Ok(match self {
            Self::Cmp { path, op, value } => cmp_matches(lookup(record, path), *op, value),
            Self::Search(text) => search_matches(record, text),
            Self::And(all) => {
                for c in all {
                    if !c.matches(record)? {
                        return Ok(false);
                    }
                }
                true
            }
            Self::Or(any) => {
                for c in any {
                    if c.matches(record)? {
                        return Ok(true);
                    }
                }
                false
            }
            Self::Not(inner) => !inner.matches(record)?,
            Self::Column(c) => {
                return Err(error!("A column reference is not a filter", column = c));
            }
            Self::Deferred(_) => return Err(error!("Deferred condition used before resolve()")),
        })
    }

    /// Resolve every `Deferred` in the tree.
    pub fn resolve(
        self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self>> + Send>> {
        Box::pin(async move {
            Ok(match self {
                Self::Deferred(d) => deferred_to_in_set(d).await?,
                Self::And(all) => Self::And(resolve_all(all).await?),
                Self::Or(any) => Self::Or(resolve_all(any).await?),
                Self::Not(inner) => Self::Not(Box::new(inner.resolve().await?)),
                other => other,
            })
        })
    }
}

async fn resolve_all(list: Vec<MemoryCondition>) -> Result<Vec<MemoryCondition>> {
    let mut out = Vec::with_capacity(list.len());
    for c in list {
        out.push(c.resolve().await?);
    }
    Ok(out)
}

async fn deferred_to_in_set(d: DeferredFn<AnyMemoryType>) -> Result<MemoryCondition> {
    let ExpressiveEnum::Scalar(any) = d.call().await? else {
        return Err(error!("Deferred condition produced a non-scalar"));
    };
    match any.into_value() {
        CborValue::Array(parts) if parts.len() == 2 => {
            let mut it = parts.into_iter();
            match (it.next(), it.next()) {
                (Some(CborValue::Text(path)), Some(values @ CborValue::Array(_))) => {
                    Ok(MemoryCondition::cmp(path, FilterOp::InSet, values))
                }
                _ => Err(error!("Deferred condition: expected [path, [values]]")),
            }
        }
        _ => Err(error!("Deferred condition: expected [path, [values]]")),
    }
}

fn in_set(cell: &CborValue, set: &CborValue) -> bool {
    match set {
        CborValue::Array(items) => items.iter().any(|v| values_eq(cell, v)),
        single => values_eq(cell, single),
    }
}

fn cmp_matches(cell: Option<&CborValue>, op: FilterOp, value: &CborValue) -> bool {
    let cell = cell.unwrap_or(&CborValue::Null);
    let null = matches!(cell, CborValue::Null);
    match op {
        FilterOp::Eq => values_eq(cell, value),
        FilterOp::Ne => !values_eq(cell, value),
        FilterOp::InSet => in_set(cell, value),
        FilterOp::NotInSet => !in_set(cell, value),
        FilterOp::Like => !null && text_of(cell).is_some_and(|t| like(&operand_text(value), &t)),
        ordered => !null && cmp_values(cell, value).is_some_and(|o| ordered.matches_ordering(o)),
    }
}

fn search_matches(record: &Record<CborValue>, text: &str) -> bool {
    let needle = text.to_lowercase();
    record
        .values()
        .any(|v| text_of(v).is_some_and(|t| t.to_lowercase().contains(&needle)))
}
