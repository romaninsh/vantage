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

use crate::operation;
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
        operation::OP_EQ => {
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
        operation::OP_IN => {
            // param[1] resolves to a comma-separated list or similar;
            // for now we resolve the deferred/nested and collect values
            let resolved = resolve_param(&params[1]).await?;
            let match_values: Vec<&str> = resolved.value().split(',').collect();
            Ok(records
                .into_iter()
                .filter(|(_id, record)| {
                    record
                        .get(&field_name)
                        .map(|v| match_values.contains(&v.value().as_str()))
                        .unwrap_or(false)
                })
                .collect())
        }
        _ => Ok(records), // Unknown operation, skip
    }
}

/// Resolve an expression parameter to a concrete `AnyCsvType` value.
/// Scalars pass through, Deferred closures are called, Nested expressions
/// are recursively resolved.
fn resolve_param(
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
    use crate::{Csv, CsvOperation};
    use vantage_dataset::prelude::ReadableValueSet;
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
        table.add_condition(table["is_paying_client"].eq(true));

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

        table.add_condition(table["name"].eq("Doc Brown".to_string()));

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

        table.add_condition(table["calories"].eq(300_i64));

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
        table.add_condition(table["is_deleted"].eq(false));
        table.add_condition(table["calories"].eq(300_i64));

        let values = table.list_values().await.unwrap();
        assert_eq!(values.len(), 1);
        assert!(values.contains_key("flux_cupcake"));
    }
}
