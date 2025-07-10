use async_trait::async_trait;
use vantage_expressions::{Expressive, OwnedExpression, expr, protocol::DataSource};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Thing {
    table: String,
    id: String,
}

impl Thing {
    pub fn new(table: String, id: String) -> Self {
        Self { table, id }
    }

    pub fn from_str(thing_str: &str) -> Option<Self> {
        if let Some((table, id)) = thing_str.split_once(':') {
            Some(Self {
                table: table.to_string(),
                id: id.to_string(),
            })
        } else {
            None
        }
    }
}

#[async_trait]
impl Expressive for Thing {
    async fn prepare(&self, _data_source: &dyn DataSource) -> OwnedExpression {
        expr!(format!("{}:{}", self.table, self.id))
    }
}
