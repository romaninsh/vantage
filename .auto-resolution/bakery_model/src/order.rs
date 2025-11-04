#![allow(async_fn_in_trait)]
use anyhow::Result;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use vantage::prelude::*;

use crate::{
    lineitem::{LineItem, LineItemTable},
    postgres, Client,
};

#[cfg(test)]
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct Order {
    pub id: i64,
    pub client_id: i64,
    pub client_name: String,
    pub total: i64,
}
impl Entity for Order {}

impl Order {
    pub fn static_table() -> &'static Table<Postgres, Order> {
        static TABLE: OnceLock<Table<Postgres, Order>> = OnceLock::new();

        TABLE.get_or_init(|| {
            Table::new_with_entity("ord", postgres())
                .with_id_column("id")
                .with_column("client_id")
                .with_extension(SoftDelete::new("is_deleted"))
                .with_one("client", "client_id", || Box::new(Client::table()))
                .with_many("line_items", "order_id", || Box::new(LineItem::table()))
                .with_expression("total", |t| {
                    let item = t.sub_line_items();
                    item.sum(item.total()).render_chunk()
                })
                .with_imported_fields("client", &["name"])
        })
    }
    pub fn table() -> Table<Postgres, Order> {
        Order::static_table().clone()
    }
    #[cfg(test)]
    fn mock_table(data: &Value) -> Table<MockDataSource, Order> {
        let data_source = MockDataSource::new(&data);
        Table::new_with_entity("ord", data_source)
            .with_column("id")
            .with_column("client_id")
            .with_column("client_name")
            .with_column("total")
    }
}

pub trait OrderTable: SqlTable {
    fn client_id(&self) -> Arc<PgValueColumn> {
        Order::table().get_column("client_id").unwrap()
    }
    fn product_id(&self) -> Arc<PgValueColumn> {
        Order::table().get_column("product_id").unwrap()
    }

    fn ref_client(&self) -> Table<Postgres, Client>;
    fn ref_line_items(&self) -> Table<Postgres, LineItem>;
    fn sub_line_items(&self) -> Table<Postgres, LineItem>;
}

impl OrderTable for Table<Postgres, Order> {
    fn ref_client(&self) -> Table<Postgres, Client> {
        self.get_ref_as("client").unwrap()
    }
    fn ref_line_items(&self) -> Table<Postgres, LineItem> {
        self.get_ref_as("line_items").unwrap()
    }
    fn sub_line_items(&self) -> Table<Postgres, LineItem> {
        self.get_subquery_as("line_items").unwrap()
    }
}

pub trait OrderTableReports {
    async fn generate_report(&self) -> Result<String>;
}
impl<D: DataSource> OrderTableReports for Table<D, Order> {
    async fn generate_report(&self) -> Result<String> {
        let mut report = String::new();
        for order in self.get().await? {
            report.push_str(&format!(
                " | Ord #{} for client {} (id: {}) total: ${:.2}\n",
                order.id,
                order.client_name,
                order.client_id,
                order.total as f64 / 100.0
            ));
        }
        if report.is_empty() {
            Err(anyhow::anyhow!("No orders found"))
        } else {
            report = format!(" +----------------------------------------------------\n{} +----------------------------------------------------", report);
            Ok(report)
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn test_order_report() {
        let data = json!([{ "id": 1, "client_id": 1, "client_name": "Joe", "total": 100}, { "id": 2, "client_id": 2, "client_name": "Jane", "total": 200}]);

        let orders = Order::mock_table(&data);
        let report = orders.generate_report().await.unwrap();
        assert_eq!(
            report,
            r#" +----------------------------------------------------
 | Ord #1 for client Joe (id: 1) total: $1.00
 | Ord #2 for client Jane (id: 2) total: $2.00
 +----------------------------------------------------"#
        );
    }
}
