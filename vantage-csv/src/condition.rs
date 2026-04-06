//! CSV condition evaluation
//!
//! Evaluates `Expression<AnyCsvType>` conditions against in-memory records.
//! Peels expression parameters to extract field names and comparison values,
//! then filters records accordingly. Supports `eq` and `in` operations.

use indexmap::IndexMap;
use vantage_core::Result;
use vantage_expressions::Expression;
use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_types::Record;

use crate::operation::{OP_EQ, OP_IN};
use crate::type_system::AnyCsvType;

/// Evaluate a single condition expression against a set of records,
/// returning only the records that match.
///
/// Peels the expression parameters to extract field name and comparison value.
/// For `Deferred` params, resolves them first by calling the closure.
pub(crate) async fn apply_condition(
    records: IndexMap<String, Record<AnyCsvType>>,
    condition: &Expression<AnyCsvType>,
) -> Result<IndexMap<String, Record<AnyCsvType>>> {
    let params = &condition.parameters;
    if params.len() < 2 {
        return Ok(records); // Unknown condition shape, skip
    }

    // param[0] is Nested(field_expr) — extract field name from its template
    let field_name = match &params[0] {
        ExpressiveEnum::Nested(expr) => expr.template.clone(),
        _ => return Ok(records),
    };

    match condition.template.as_str() {
        OP_EQ => {
            // param[1] is Scalar(value) or Deferred
            let expected = resolve_param(&params[1]).await?;
            Ok(records
                .into_iter()
                .filter(|(_id, record)| {
                    record
                        .get(&field_name)
                        .map(|v| v.value() == expected.value())
                        .unwrap_or(false)
                })
                .collect())
        }
        OP_IN => {
            let resolved = resolve_param(&params[1]).await?;
            // Try to extract as Vec<AnyCsvType> (List variant)
            let match_values: Vec<AnyCsvType> =
                resolved.try_get::<Vec<AnyCsvType>>().unwrap_or_else(|| {
                    // Fallback: treat as single value
                    vec![resolved.clone()]
                });
            Ok(records
                .into_iter()
                .filter(|(_id, record)| {
                    record
                        .get(&field_name)
                        .map(|v| match_values.iter().any(|m| m.value() == v.value()))
                        .unwrap_or(false)
                })
                .collect())
        }
        other => Err(vantage_core::error!(
            "Unsupported CSV condition operator",
            template = other.to_string()
        )),
    }
}

/// Resolve an expression parameter to a concrete `AnyCsvType` value.
/// Scalars pass through, Deferred closures are called, Nested expressions
/// are recursively resolved.
pub(crate) fn resolve_param(
    param: &ExpressiveEnum<AnyCsvType>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<AnyCsvType>> + Send + '_>> {
    Box::pin(async move {
        match param {
            ExpressiveEnum::Scalar(v) => Ok(v.clone()),
            ExpressiveEnum::Deferred(deferred) => {
                let result = deferred.call().await?;
                match result {
                    ExpressiveEnum::Scalar(v) => Ok(v),
                    other => resolve_param(&other).await,
                }
            }
            ExpressiveEnum::Nested(expr) => {
                // A nested expression with no params is just a value reference
                if expr.parameters.is_empty() {
                    Ok(AnyCsvType::new(expr.template.clone()))
                } else if expr.parameters.len() == 1 {
                    resolve_param(&expr.parameters[0]).await
                } else {
                    Ok(AnyCsvType::new(expr.template.clone()))
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use crate::{AnyCsvType, Csv};
    use vantage_dataset::prelude::ReadableValueSet;
    use vantage_table::operation::Operation;
    use vantage_table::table::Table;
    use vantage_types::EmptyEntity;

    fn test_csv() -> Csv {
        Csv::new(format!("{}/data", env!("CARGO_MANIFEST_DIR")))
    }

    #[tokio::test]
    async fn test_eq_condition_bool() {
        let csv = test_csv();
        let mut table = Table::<Csv, EmptyEntity>::new("client", csv)
            .with_column_of::<String>("name")
            .with_column_of::<bool>("is_paying_client");

        // Filter to paying clients only
        table.add_condition(table["is_paying_client"].eq(AnyCsvType::new(true)));

        let values = table.list_values().await.unwrap();
        // Marty and Doc are paying, Biff is not
        assert_eq!(values.len(), 2);
        assert!(values.contains_key("marty"));
        assert!(values.contains_key("doc"));
        assert!(!values.contains_key("biff"));
    }

    #[tokio::test]
    async fn test_eq_condition_string() {
        let csv = test_csv();
        let mut table = Table::<Csv, EmptyEntity>::new("client", csv)
            .with_column_of::<String>("name")
            .with_column_of::<String>("email");

        table.add_condition(table["name"].eq(AnyCsvType::new("Doc Brown".to_string())));

        let values = table.list_values().await.unwrap();
        assert_eq!(values.len(), 1);
        assert!(values.contains_key("doc"));
    }

    #[tokio::test]
    async fn test_eq_condition_int() {
        let csv = test_csv();
        let mut table = Table::<Csv, EmptyEntity>::new("product", csv)
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories");

        table.add_condition(table["calories"].eq(AnyCsvType::new(300_i64)));

        let values = table.list_values().await.unwrap();
        assert_eq!(values.len(), 1);
        assert!(values.contains_key("flux_cupcake"));
    }

    #[tokio::test]
    async fn test_multiple_conditions() {
        let csv = test_csv();
        let mut table = Table::<Csv, EmptyEntity>::new("product", csv)
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<bool>("is_deleted");

        // Chain: not deleted AND calories = 300
        table.add_condition(table["is_deleted"].eq(AnyCsvType::new(false)));
        table.add_condition(table["calories"].eq(AnyCsvType::new(300_i64)));

        let values = table.list_values().await.unwrap();
        assert_eq!(values.len(), 1);
        assert!(values.contains_key("flux_cupcake"));
    }

    #[tokio::test]
    async fn test_in_condition_with_column_values() {
        use vantage_expressions::Expressive;
        use vantage_table::traits::table_source::TableSource;

        let csv = test_csv();

        // Build a source table of paying clients
        let mut clients = Table::<Csv, EmptyEntity>::new("client", csv.clone())
            .with_column_of::<String>("name")
            .with_column_of::<bool>("is_paying_client");
        clients.add_condition(clients["is_paying_client"].eq(AnyCsvType::new(true)));

        // Get paying client names as AssociatedExpression
        let name_col = csv.create_column::<String>("name");
        let paying_names = csv.column_table_values_expr(&clients, &name_col);

        // Use IN condition — AssociatedExpression implements Expressive,
        // so we can nest it via (paying_names) syntax
        let mut all_clients = Table::<Csv, EmptyEntity>::new("client", csv.clone())
            .with_column_of::<String>("name")
            .with_column_of::<bool>("is_paying_client");

        all_clients.add_condition(all_clients["name"].in_(paying_names.expr()));

        let values = all_clients.list_values().await.unwrap();
        assert_eq!(values.len(), 2);
        assert!(values.contains_key("marty"));
        assert!(values.contains_key("doc"));
    }
}
