//! DynamoDB condition DSL.
//!
//! `DynamoCondition` carries the pieces DynamoDB's `FilterExpression`
//! consumes — an expression string referencing `#name` / `:value`
//! placeholders, plus the maps that bind them. `In` is deferred so
//! relationship traversal can run a source query at execution time.
//!
//! Multiple conditions on a `Table` get combined via `resolve`, which
//! mangles placeholders to be globally unique within one Scan request
//! and joins the rendered fragments with ` AND `.

use std::pin::Pin;
use std::sync::Arc;

use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::Record;

use super::types::{AnyDynamoType, AttributeValue};

/// Future returned by a deferred condition fetch (e.g. a relationship
/// traversal that lists the source table to discover the IN values).
pub type ValueListFuture =
    Pin<Box<dyn std::future::Future<Output = Result<Vec<AttributeValue>>> + Send>>;

/// Producer for an IN-clause's value list. Cloned every time the
/// condition is evaluated, so backends are free to memoize externally.
pub type ValueListFn = Arc<dyn Fn() -> ValueListFuture + Send + Sync>;

/// A DynamoDB filter condition.
///
/// - `Expr` is a fully-rendered expression with already-mangled placeholders.
/// - `In` defers resolution until a source query has run (relationship traversal).
/// - `And` combines siblings with implicit `AND`.
#[derive(Clone)]
pub enum DynamoCondition {
    Expr {
        expression: String,
        names: IndexMap<String, String>,
        values: IndexMap<String, AttributeValue>,
    },
    In {
        field: String,
        values: ValueListFn,
    },
    And(Vec<DynamoCondition>),
}

impl std::fmt::Debug for DynamoCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Expr {
                expression,
                names,
                values,
            } => f
                .debug_struct("Expr")
                .field("expression", expression)
                .field("names", names)
                .field("values", values)
                .finish(),
            Self::In { field, .. } => f
                .debug_struct("In")
                .field("field", field)
                .finish_non_exhaustive(),
            Self::And(conds) => f.debug_tuple("And").field(conds).finish(),
        }
    }
}

impl DynamoCondition {
    /// Build a `field = value` condition.
    pub fn eq(field: impl Into<String>, value: impl Into<AttributeValue>) -> Self {
        let field = field.into();
        let mut names = IndexMap::new();
        let mut values = IndexMap::new();
        names.insert("#f".to_string(), field);
        values.insert(":v".to_string(), value.into());
        Self::Expr {
            expression: "#f = :v".to_string(),
            names,
            values,
        }
    }

    /// Build a `begins_with(field, prefix)` condition. Used for sort-key
    /// prefix filtering in single-table designs.
    pub fn begins_with(field: impl Into<String>, prefix: impl Into<String>) -> Self {
        let mut names = IndexMap::new();
        let mut values = IndexMap::new();
        names.insert("#f".to_string(), field.into());
        values.insert(":v".to_string(), AttributeValue::S(prefix.into()));
        Self::Expr {
            expression: "begins_with(#f, :v)".to_string(),
            names,
            values,
        }
    }
}

/// The `(field, value)` of a `#f = :v` condition built by [`DynamoCondition::eq`].
pub fn equality_of(condition: &DynamoCondition) -> Option<(String, AttributeValue)> {
    let DynamoCondition::Expr {
        expression,
        names,
        values,
    } = condition
    else {
        return None;
    };
    if expression != "#f = :v" {
        return None;
    }
    Some((names.get("#f")?.clone(), values.get(":v")?.clone()))
}

/// Whether `item` satisfies every condition, evaluated client-side.
///
/// Understands `#f = :v`, `begins_with(#f, :v)` and `In`; any other
/// expression is `Unsupported`, since a write cannot be confined to a set
/// that cannot be evaluated.
pub fn row_in_set<'a>(
    conditions: &'a [DynamoCondition],
    item: &'a Record<AnyDynamoType>,
) -> Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + 'a>> {
    Box::pin(async move {
        for cond in conditions {
            let holds = match cond {
                DynamoCondition::Expr {
                    expression,
                    names,
                    values,
                } => {
                    let (Some(field), Some(want)) = (names.get("#f"), values.get(":v")) else {
                        return Err(unsupported_expression(expression));
                    };
                    let have = item.get(field).map(|v| v.value());
                    match expression.as_str() {
                        "#f = :v" => have == Some(want),
                        "begins_with(#f, :v)" => match (have, want) {
                            (Some(AttributeValue::S(h)), AttributeValue::S(w)) => {
                                h.starts_with(w.as_str())
                            }
                            _ => false,
                        },
                        _ => return Err(unsupported_expression(expression)),
                    }
                }
                DynamoCondition::In { field, values } => {
                    let allowed = (values)().await?;
                    item.get(field)
                        .is_some_and(|v| allowed.iter().any(|a| a == v.value()))
                }
                DynamoCondition::And(children) => row_in_set(children, item).await?,
            };
            if !holds {
                return Ok(false);
            }
        }
        Ok(true)
    })
}

fn unsupported_expression(expression: &str) -> vantage_core::VantageError {
    error!(
        "DynamoDB condition cannot be evaluated client-side; write refused",
        expression = expression.to_string()
    )
    .mark_unsupported()
}

/// Resolved condition pieces ready to fold into a Scan/Query request.
#[derive(Debug, Clone, Default)]
pub struct ResolvedFilter {
    pub expression: String,
    pub names: IndexMap<String, String>,
    pub values: IndexMap<String, AttributeValue>,
}

impl ResolvedFilter {
    pub fn is_empty(&self) -> bool {
        self.expression.is_empty()
    }
}

/// Walk a list of conditions, mangle placeholders to be globally unique,
/// and produce a single `FilterExpression` plus combined attribute maps.
pub async fn resolve_conditions<'a, I>(conditions: I) -> Result<ResolvedFilter>
where
    I: IntoIterator<Item = &'a DynamoCondition>,
{
    let mut state = MangleState::default();
    let mut fragments = Vec::new();
    for cond in conditions {
        if let Some(frag) = resolve_one(cond, &mut state).await? {
            fragments.push(frag);
        }
    }
    let expression = match fragments.len() {
        0 => String::new(),
        1 => fragments.into_iter().next().unwrap(),
        _ => fragments
            .into_iter()
            .map(|f| format!("({})", f))
            .collect::<Vec<_>>()
            .join(" AND "),
    };
    Ok(ResolvedFilter {
        expression,
        names: state.names,
        values: state.values,
    })
}

/// Resolve a single condition, returning the rendered expression
/// fragment with mangled placeholders. Returns `None` for empty `And`s.
fn resolve_one<'a>(
    cond: &'a DynamoCondition,
    state: &'a mut MangleState,
) -> Pin<Box<dyn std::future::Future<Output = Result<Option<String>>> + Send + 'a>> {
    Box::pin(async move {
        match cond {
            DynamoCondition::Expr {
                expression,
                names,
                values,
            } => {
                let mut rendered = expression.clone();
                for (placeholder, name) in names {
                    let new_ph = state.fresh_name(name.clone());
                    rendered = replace_placeholder(&rendered, placeholder, &new_ph);
                }
                for (placeholder, value) in values {
                    let new_ph = state.fresh_value(value.clone());
                    rendered = replace_placeholder(&rendered, placeholder, &new_ph);
                }
                Ok(Some(rendered))
            }
            DynamoCondition::In { field, values } => {
                let resolved = (values)().await?;
                if resolved.is_empty() {
                    // `field IN ()` is invalid in DynamoDB. An empty
                    // source set means "match nothing" — emit a
                    // tautologically-false fragment.
                    let name_ph = state.fresh_name(field.clone());
                    return Ok(Some(format!(
                        "attribute_not_exists({}) AND attribute_exists({})",
                        name_ph, name_ph
                    )));
                }
                let name_ph = state.fresh_name(field.clone());
                let value_phs: Vec<String> =
                    resolved.into_iter().map(|v| state.fresh_value(v)).collect();
                Ok(Some(format!("{} IN ({})", name_ph, value_phs.join(", "))))
            }
            DynamoCondition::And(children) => {
                let mut parts = Vec::new();
                for child in children {
                    if let Some(p) = resolve_one(child, state).await? {
                        parts.push(p);
                    }
                }
                Ok(if parts.is_empty() {
                    None
                } else {
                    Some(parts.join(" AND "))
                })
            }
        }
    })
}

/// Replace every occurrence of `from` in `s` with `to`. Used to swap
/// each condition's local placeholder names for globally-unique ones.
fn replace_placeholder(s: &str, from: &str, to: &str) -> String {
    s.replace(from, to)
}

#[derive(Default)]
struct MangleState {
    names: IndexMap<String, String>,
    values: IndexMap<String, AttributeValue>,
    name_seq: usize,
    value_seq: usize,
}

impl MangleState {
    fn fresh_name(&mut self, attr: String) -> String {
        let ph = format!("#n{}", self.name_seq);
        self.name_seq += 1;
        self.names.insert(ph.clone(), attr);
        ph
    }

    fn fresh_value(&mut self, value: AttributeValue) -> String {
        let ph = format!(":v{}", self.value_seq);
        self.value_seq += 1;
        self.values.insert(ph.clone(), value);
        ph
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn row_in_set_evaluates_eq_and_refuses_unknown_expressions() {
        let item: Record<AnyDynamoType> = [(
            "parent".to_string(),
            AnyDynamoType::untyped(AttributeValue::S("p1".into())),
        )]
        .into_iter()
        .collect();
        assert!(
            row_in_set(
                &[DynamoCondition::eq(
                    "parent",
                    AttributeValue::S("p1".into())
                )],
                &item
            )
            .await
            .unwrap()
        );
        assert!(
            !row_in_set(
                &[DynamoCondition::eq(
                    "parent",
                    AttributeValue::S("p2".into())
                )],
                &item
            )
            .await
            .unwrap()
        );
        let odd = DynamoCondition::Expr {
            expression: "size(#f) > :v".into(),
            names: [("#f".to_string(), "parent".to_string())]
                .into_iter()
                .collect(),
            values: [(":v".to_string(), AttributeValue::N("1".into()))]
                .into_iter()
                .collect(),
        };
        assert!(
            row_in_set(&[odd], &item)
                .await
                .unwrap_err()
                .is_unsupported()
        );
    }

    #[tokio::test]
    async fn row_in_set_evaluates_begins_with_and_in() {
        let item: Record<AnyDynamoType> = [(
            "sk".to_string(),
            AnyDynamoType::untyped(AttributeValue::S("ABC".into())),
        )]
        .into_iter()
        .collect();
        assert!(
            row_in_set(&[DynamoCondition::begins_with("sk", "A")], &item)
                .await
                .unwrap()
        );
        assert!(
            !row_in_set(&[DynamoCondition::begins_with("sk", "B")], &item)
                .await
                .unwrap()
        );
        let values = Arc::new(|| -> ValueListFuture {
            Box::pin(async move { Ok(vec![AttributeValue::S("ABC".into())]) })
        });
        let hit = DynamoCondition::In {
            field: "sk".into(),
            values,
        };
        assert!(row_in_set(&[hit], &item).await.unwrap());
    }

    #[test]
    fn equality_of_reads_eq_conditions() {
        assert_eq!(
            equality_of(&DynamoCondition::eq(
                "parent",
                AttributeValue::S("p1".into())
            )),
            Some(("parent".into(), AttributeValue::S("p1".into())))
        );
        assert_eq!(equality_of(&DynamoCondition::begins_with("sk", "A")), None);
    }

    #[tokio::test]
    async fn empty_input_yields_empty_filter() {
        let r = resolve_conditions(std::iter::empty()).await.unwrap();
        assert!(r.is_empty());
    }

    #[tokio::test]
    async fn single_eq_renders_with_mangled_placeholders() {
        let cond = DynamoCondition::eq("name", AttributeValue::S("Alice".into()));
        let r = resolve_conditions(std::iter::once(&cond)).await.unwrap();
        assert_eq!(r.expression, "#n0 = :v0");
        assert_eq!(r.names.get("#n0").unwrap(), "name");
        assert_eq!(
            r.values.get(":v0").unwrap(),
            &AttributeValue::S("Alice".into())
        );
    }

    #[tokio::test]
    async fn two_eqs_get_unique_placeholders() {
        let a = DynamoCondition::eq("name", AttributeValue::S("Alice".into()));
        let b = DynamoCondition::eq("city", AttributeValue::S("Riga".into()));
        let r = resolve_conditions([&a, &b]).await.unwrap();
        assert_eq!(r.expression, "(#n0 = :v0) AND (#n1 = :v1)");
        assert_eq!(r.names.get("#n0").unwrap(), "name");
        assert_eq!(r.names.get("#n1").unwrap(), "city");
    }

    #[tokio::test]
    async fn deferred_in_resolves_to_in_expression() {
        let values = Arc::new(|| -> ValueListFuture {
            Box::pin(async move {
                Ok(vec![
                    AttributeValue::S("a".into()),
                    AttributeValue::S("b".into()),
                ])
            })
        });
        let cond = DynamoCondition::In {
            field: "bakery_id".to_string(),
            values,
        };
        let r = resolve_conditions(std::iter::once(&cond)).await.unwrap();
        assert_eq!(r.expression, "#n0 IN (:v0, :v1)");
        assert_eq!(r.names.get("#n0").unwrap(), "bakery_id");
        assert_eq!(r.values.len(), 2);
    }
}
