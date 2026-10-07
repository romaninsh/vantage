//! ExprDataSource implementation for CSV
//!
//! Enables executing expressions against CSV by resolving DeferredFn closures.
//! This is used by `column_values_expression` and other deferred-based expressions.

use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::traits::expressive::DeferredFn;
use vantage_expressions::{Expression, defer_execute, execute_by_resolving};

use crate::Csv;
use crate::condition::resolve_param;
use crate::type_system::AnyCsvType;

impl ExprDataSource<AnyCsvType> for Csv {
    async fn execute(&self, expr: &Expression<AnyCsvType>) -> vantage_core::Result<AnyCsvType> {
        // For CSV, execution means resolving deferred params. Multi-param
        // expressions resolve each param and join the values comma-separated.
        execute_by_resolving(
            expr,
            |template| AnyCsvType::new(template.to_string()),
            |params| async move {
                let mut results = Vec::with_capacity(params.len());
                for param in params {
                    results.push(resolve_param(param).await?.value().clone());
                }
                Ok(AnyCsvType::new(results.join(",")))
            },
        )
        .await
    }

    fn defer(&self, expr: Expression<AnyCsvType>) -> DeferredFn<AnyCsvType>
    where
        AnyCsvType: Clone + Send + Sync + 'static,
    {
        defer_execute(self, expr)
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
    async fn test_execute_column_table_values_expr() {
        let csv = test_csv();
        let table = Table::<Csv, EmptyEntity>::new("client", csv.clone())
            .with_column_of::<String>("name")
            .with_column_of::<bool>("is_paying_client");

        let name_col = csv.create_column::<String>("name");
        let associated = csv.column_table_values_expr(&table, &name_col);
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
        use crate::operation::CsvOperation;

        let csv = test_csv();
        let mut table = Table::<Csv, EmptyEntity>::new("client", csv.clone())
            .with_column_of::<String>("name")
            .with_column_of::<bool>("is_paying_client");

        table.add_condition(table["is_paying_client"].eq(AnyCsvType::new(true)));

        let name_col = csv.create_column::<String>("name");
        let associated = csv.column_table_values_expr(&table, &name_col);
        let result = csv.execute(associated.expression()).await.unwrap();

        let names = result.try_get::<Vec<AnyCsvType>>().unwrap();
        let name_strings: Vec<&str> = names.iter().map(|v| v.value().as_str()).collect();
        assert!(name_strings.contains(&"Marty McFly"));
        assert!(name_strings.contains(&"Doc Brown"));
        assert!(!name_strings.contains(&"Biff Tannen"));
    }
}
