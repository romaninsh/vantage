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
    /// Case-insensitive substring over the text or number cells `columns`
    /// names, or every cell when `columns` is empty.
    Search {
        text: String,
        columns: Vec<String>,
    },
    And(Vec<MemoryCondition>),
    Or(Vec<MemoryCondition>),
    Not(Box<MemoryCondition>),
    /// A bare column reference, valid only as an order key.
    Column(String),
    /// Resolved before evaluation into `Cmp { op: InSet }`. The payload is
    /// a CBOR array `[Text(path), Array(values)]`.
    Deferred(DeferredFn<AnyMemoryType>),
    /// A [`Deferred`](Self::Deferred) known to constrain `path` (traversal's
    /// `fk IN (parent ids)`), so the column is readable before resolution.
    /// Resolves exactly as `Deferred` does.
    DeferredIn {
        path: String,
        deferred: DeferredFn<AnyMemoryType>,
    },
}

impl MemoryCondition {
    pub fn cmp(path: impl Into<String>, op: FilterOp, value: impl Into<CborValue>) -> Self {
        Self::Cmp {
            path: path.into(),
            op,
            value: value.into(),
        }
    }

    /// A search for `text` over every cell.
    pub fn search(text: impl Into<String>) -> Self {
        Self::Search {
            text: text.into(),
            columns: Vec::new(),
        }
    }

    /// The condition as text for people: a shell's `preview` and the
    /// table source's `describe_condition`.
    pub(crate) fn describe(&self) -> String {
        let join = |list: &[MemoryCondition], sep: &str| {
            let parts: Vec<String> = list.iter().map(MemoryCondition::describe).collect();
            format!("({})", parts.join(sep))
        };
        match self {
            MemoryCondition::Cmp { path, op, value } => {
                format!("{path} {} {}", op.key_symbol(), render_value(value))
            }
            MemoryCondition::Search { text, .. } => format!("search {text:?}"),
            MemoryCondition::And(all) => join(all, " AND "),
            MemoryCondition::Or(any) => join(any, " OR "),
            MemoryCondition::Not(inner) => format!("NOT {}", inner.describe()),
            MemoryCondition::Column(name) => name.clone(),
            MemoryCondition::Deferred(_) => "<deferred>".to_string(),
            MemoryCondition::DeferredIn { path, .. } => format!("{path} IN <deferred>"),
        }
    }

    pub fn matches(&self, record: &Record<CborValue>) -> Result<bool> {
        Ok(match self {
            Self::Cmp { path, op, value } => cmp_matches(lookup(record, path), *op, value),
            Self::Search { text, columns } => search_matches(record, text, columns),
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
            Self::Deferred(_) | Self::DeferredIn { .. } => {
                return Err(error!("Deferred condition used before resolve()"));
            }
        })
    }

    /// Resolve every `Deferred` in the tree.
    pub fn resolve(
        self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self>> + Send>> {
        Box::pin(async move {
            Ok(match self {
                Self::Deferred(d) | Self::DeferredIn { deferred: d, .. } => {
                    deferred_to_in_set(d).await?
                }
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

/// A condition operand as a literal: text quoted, numbers and booleans bare,
/// arrays bracketed.
fn render_value(value: &CborValue) -> String {
    match value {
        CborValue::Text(s) => format!("{s:?}"),
        CborValue::Integer(i) => i128::from(*i).to_string(),
        CborValue::Float(f) => f.to_string(),
        CborValue::Bool(b) => b.to_string(),
        CborValue::Null => "null".to_string(),
        CborValue::Array(items) => {
            let items: Vec<String> = items.iter().map(render_value).collect();
            format!("[{}]", items.join(", "))
        }
        other => format!("{other:?}"),
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

/// Whether the text or number of a cell `columns` names (every cell when
/// `columns` is empty) holds `text`, case-insensitively.
pub(super) fn search_matches(record: &Record<CborValue>, text: &str, columns: &[String]) -> bool {
    let needle = text.to_lowercase();
    let found = |v: &CborValue| text_of(v).is_some_and(|t| t.to_lowercase().contains(&needle));
    if columns.is_empty() {
        record.values().any(found)
    } else {
        columns
            .iter()
            .filter_map(|column| lookup(record, column))
            .any(found)
    }
}
