//! ExprDataSource implementation for CSV
//!
//! Enables executing expressions against CSV by resolving DeferredFn closures.
//! This is used by `column_values_expression` and other deferred-based expressions.

use vantage_expressions::Expression;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::traits::expressive::{DeferredFn, ExpressiveEnum};

use crate::condition::resolve_param;
use crate::type_system::AnyCsvType;
use crate::Csv;

impl ExprDataSource<AnyCsvType> for Csv {
    async fn execute(&self, expr: &Expression<AnyCsvType>) -> vantage_core::Result<AnyCsvType> {
        // For CSV, execution means resolving deferred params.
        // An expression with a single param resolves to that param's value.
        // Multi-param expressions resolve each param and join as comma-separated.
        if expr.parameters.len() == 1 {
            resolve_param(&expr.parameters[0]).await
        } else if expr.parameters.is_empty() {
            Ok(AnyCsvType::new(expr.template.clone()))
        } else {
            // Resolve all params, join with commas
            let mut results = Vec::new();
            for param in &expr.parameters {
                let resolved = resolve_param(param).await?;
                results.push(resolved.value().clone());
            }
            Ok(AnyCsvType::new(results.join(",")))
        }
    }

    fn defer(&self, expr: Expression<AnyCsvType>) -> DeferredFn<AnyCsvType>
    where
        AnyCsvType: Clone + Send + Sync + 'static,
    {
        let csv = self.clone();
        DeferredFn::new(move || {
            let csv = csv.clone();
            let expr = expr.clone();
            Box::pin(async move {
                let result = csv.execute(&expr).await?;
                Ok(ExpressiveEnum::Scalar(result))
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_table::table::Table;
    use vantage_table::traits::table_source::TableSource;
    use vantage_types::EmptyEntity;

    fn test_csv() -> Csv {
        Csv::new(format!("{}/data", env!("CARGO_MANIFEST_DIR")))
    }

    #[tokio::test]
    async fn test_execute_column_values_expression() {
        let csv = test_csv();
        let table = Table::<Csv, EmptyEntity>::new("client", csv.clone())
            .with_column_of::<String>("name")
            .with_column_of::<bool>("is_paying_client");

        let name_col = csv.create_column::<String>("name");
        let associated = csv.column_values_expression(&table, &name_col);
        let result = csv.execute(associated.expression()).await.unwrap();

        // Result is a List of AnyCsvType values
        let names = result.try_get::<Vec<AnyCsvType>>().unwrap();
        let name_strings: Vec<&str> = names.iter().map(|v| v.value().as_str()).collect();
        assert!(name_strings.contains(&"Marty McFly"));
        assert!(name_strings.contains(&"Doc Brown"));
        assert!(name_strings.contains(&"Biff Tannen"));
    }

    #[tokio::test]
    async fn test_execute_column_values_with_condition() {
        use vantage_table::operation::Operation;

        let csv = test_csv();
        let mut table = Table::<Csv, EmptyEntity>::new("client", csv.clone())
            .with_column_of::<String>("name")
            .with_column_of::<bool>("is_paying_client");

        table.add_condition(table["is_paying_client"].eq(AnyCsvType::new(true)));

        let name_col = csv.create_column::<String>("name");
        let associated = csv.column_values_expression(&table, &name_col);
        let result = csv.execute(associated.expression()).await.unwrap();

        let names = result.try_get::<Vec<AnyCsvType>>().unwrap();
        let name_strings: Vec<&str> = names.iter().map(|v| v.value().as_str()).collect();
        assert!(name_strings.contains(&"Marty McFly"));
        assert!(name_strings.contains(&"Doc Brown"));
        assert!(!name_strings.contains(&"Biff Tannen"));
    }
}
