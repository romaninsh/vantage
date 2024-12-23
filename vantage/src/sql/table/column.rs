use std::sync::Arc;

use crate::expr;
use crate::sql::chunk::Chunk;
use crate::sql::Condition;
use crate::sql::Expression;
use crate::sql::Operations;
use crate::sql::WrapArc;
use crate::traits::column::SqlField;

mod chrono;
mod sqlcolumn;

pub use sqlcolumn::SqlColumn;

#[derive(Debug, Clone)]
pub struct Column {
    name: String,
    table_alias: Option<String>,
    column_alias: Option<String>,
}

impl Column {
    pub fn new(name: String, table_alias: Option<String>) -> Column {
        Column {
            name,
            table_alias,
            column_alias: None,
        }
    }
}

impl SqlColumn for Column {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn name_with_table(&self) -> String {
        match &self.table_alias {
            Some(table_alias) => format!("{}.{}", table_alias, self.name),
            None => self.name.clone(),
        }
    }
    fn set_name(&mut self, name: String) {
        self.name = name;
    }
    fn set_table_alias(&mut self, alias: String) {
        self.table_alias = Some(alias);
    }
    fn set_column_alias(&mut self, alias: String) {
        self.column_alias = Some(alias);
    }

    fn get_column_alias(&self) -> Option<String> {
        self.column_alias.clone()
    }
}

impl Chunk for Column {
    fn render_chunk(&self) -> Expression {
        Arc::new(self.clone()).render_chunk()
    }
}
impl Operations for Column {}

impl Operations for Arc<Column> {
    fn eq(&self, other: &impl Chunk) -> Condition {
        Condition::from_field(self.clone(), "=", WrapArc::wrap_arc(other.render_chunk()))
    }

    // fn add(&self, other: impl SqlChunk) -> Expression {
    //     let chunk = other.render_chunk();
    //     expr_arc!(format!("{} + {{}}", &self.name), chunk).render_chunk()
    // }
}

impl Chunk for Arc<Column> {
    fn render_chunk(&self) -> Expression {
        Expression::new(format!("{}", self.name_with_table()), vec![])
    }
}

impl SqlField for Arc<Column> {
    fn render_column(&self, mut alias: Option<&str>) -> Expression {
        // If the alias is the same as the field name, we don't need to render it
        if alias.is_some() && alias.unwrap() == self.name {
            alias = None;
        }

        let alias = alias.or(self.column_alias.as_deref());

        if let Some(alias) = alias {
            expr!(format!("{} AS {}", self.name_with_table(), alias))
        } else {
            expr!(format!("{}", self.name_with_table()))
        }
        .render_chunk()
    }
    fn calculated(&self) -> bool {
        false
    }
}

impl From<String> for Column {
    fn from(name: String) -> Self {
        Column {
            name,
            table_alias: None,
            column_alias: None,
        }
    }
}

impl From<&str> for Column {
    fn from(name: &str) -> Self {
        name.to_string().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field() {
        let field = Arc::new(Column::new("id".to_string(), None));
        let (sql, params) = field.render_chunk().split();

        assert_eq!(sql, "id");
        assert_eq!(params.len(), 0);

        let (sql, params) = field.render_column(Some("id")).render_chunk().split();
        assert_eq!(sql, "id");
        assert_eq!(params.len(), 0);

        let (sql, params) = &field.render_column(Some("id_alias")).render_chunk().split();
        assert_eq!(sql, "id AS id_alias");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_eq() {
        let field = Arc::new(Column::new("id".to_string(), None));
        let (sql, params) = field.eq(&1).render_chunk().split();

        assert_eq!(sql, "(id = {})");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], 1);

        let f_age = Arc::new(Column::new("age".to_string(), Some("u".to_string())));
        let (sql, params) = f_age.add(5).eq(&18).render_chunk().split();

        assert_eq!(sql, "((u.age) + ({}) = {})");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], 5);
        assert_eq!(params[1], 18);
    }
}
