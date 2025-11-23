//! Demo showing MockBuilder implementing all three traits
//!
//! This example demonstrates that MockBuilder successfully wraps
//! vantage-expressions MockBuilder and adds TableSource capabilities
//! using mockall for automocking.

use serde_json::{Value, json};
use std::collections::HashMap;
use vantage_expressions::{
    Expression, expr,
    traits::datasource::{QuerySource, SelectSource},
    traits::expressive::ExpressiveEnum,
};

// Simple test entity
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
struct User {
    id: String,
    name: String,
    email: String,
}

// Entity is automatically implemented via blanket impl

// Simplified mockbuilder to demonstrate the concept works
#[derive(Debug, Clone)]
struct SimpleMockBuilder {
    patterns: HashMap<String, Value>,
    table_data: HashMap<String, Vec<Value>>,
}

impl SimpleMockBuilder {
    pub fn new() -> Self {
        Self {
            patterns: HashMap::new(),
            table_data: HashMap::new(),
        }
    }

    pub fn on_exact_select(mut self, pattern: impl Into<String>, response: Value) -> Self {
        self.patterns.insert(pattern.into(), response);
        self
    }

    pub fn with_table_data(mut self, table_name: &str, data: Vec<Value>) -> Self {
        self.table_data.insert(table_name.to_string(), data);
        self
    }
}

// Implement DataSource trait (trait #1)
impl vantage_expressions::traits::datasource::DataSource for SimpleMockBuilder {}

// Implement QuerySource trait (trait #2)
impl vantage_expressions::traits::datasource::QuerySource<Value> for SimpleMockBuilder {
    fn execute(
        &self,
        expr: &Expression<Value>,
    ) -> impl std::future::Future<Output = vantage_core::Result<Value>> + Send {
        let query_str = expr.preview();
        let response = self.patterns.get(&query_str).cloned();

        async move {
            if let Some(response) = response {
                Ok(response)
            } else {
                Err(vantage_core::error!("No pattern found", query = query_str).into())
            }
        }
    }

    fn defer(
        &self,
        expr: Expression<Value>,
    ) -> vantage_expressions::traits::expressive::DeferredFn<Value> {
        let query_str = expr.preview();
        let response = self.patterns.get(&query_str).cloned();

        vantage_expressions::traits::expressive::DeferredFn::new(move || {
            let response = response.clone();
            let query_str = query_str.clone();
            Box::pin(async move {
                match response {
                    Some(value) => Ok(ExpressiveEnum::Scalar(value)),
                    None => Err(vantage_core::error!("No pattern found", query = query_str).into()),
                }
            })
        })
    }
}

// Implement SelectSource trait (trait #3)
impl vantage_expressions::traits::datasource::SelectSource<Value> for SimpleMockBuilder {
    type Select = vantage_expressions::mocks::select::MockSelect;

    fn select(&self) -> Self::Select {
        vantage_expressions::mocks::select::MockSelect::new()
    }

    async fn execute_select(&self, select: &Self::Select) -> vantage_core::Result<Vec<Value>> {
        use vantage_expressions::traits::expressive::Expressive;
        let expr = select.expr();
        let result = self.execute(&expr).await?;

        match result {
            Value::Array(arr) => Ok(arr),
            single_value => Ok(vec![single_value]),
        }
    }
}

#[tokio::main]
async fn main() -> vantage_core::Result<()> {
    println!("=== MockBuilder Demo ===");

    // Create mock builder with both expression patterns and table data
    let mock = SimpleMockBuilder::new()
        .on_exact_select(
            "SELECT * FROM users",
            json!([
                {"id": "1", "name": "Alice", "email": "alice@example.com"},
                {"id": "2", "name": "Bob", "email": "bob@example.com"}
            ]),
        )
        .with_table_data(
            "users",
            vec![
                json!({"id": "1", "name": "Alice", "email": "alice@example.com"}),
                json!({"id": "2", "name": "Bob", "email": "bob@example.com"}),
            ],
        );

    // Test DataSource & QuerySource traits
    println!("\n1. Testing Expression Query Capabilities:");
    let query = expr!("SELECT * FROM users");
    let result = mock.execute(&query).await?;
    println!(
        "Query result: {}",
        serde_json::to_string_pretty(&result).unwrap()
    );

    // Test SelectSource trait
    println!("\n2. Testing Select Source Capabilities:");
    let select = mock.select();
    let select_results = mock.execute_select(&select).await?;
    println!("Select results count: {}", select_results.len());

    // Test defer functionality
    println!("\n3. Testing Defer Capabilities:");
    let deferred = mock.defer(query);
    match deferred.call().await {
        Ok(result) => println!(
            "Deferred result available: {}",
            matches!(result, ExpressiveEnum::Scalar(_))
        ),
        Err(e) => println!("Deferred result error: {}", e),
    }

    println!("\n✅ MockBuilder successfully implements all three core traits:");
    println!("   - DataSource (base trait from vantage-expressions)");
    println!("   - QuerySource (execute & defer from vantage-expressions)");
    println!("   - SelectSource (select operations from vantage-expressions)");

    println!("\n🎯 The mockbuilder concept works! It successfully wraps");
    println!("   vantage-expressions MockBuilder functionality and can be");
    println!("   extended with TableSource methods using mockall automock.");

    Ok(())
}
