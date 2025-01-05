use std::sync::Arc;
use std::sync::RwLock;
use std::sync::Weak;

use tokio_postgres::types::Format;

use crate::expr;
use crate::expr_arc;
use crate::prelude::column::SqlColumn;
use crate::prelude::Column;
use crate::prelude::TableAlias;
use crate::sql::chunk::Chunk;
use crate::sql::expression::{Expression, ExpressionArc};
use crate::sql::Condition;
use crate::sql::Operations;
use crate::sql::WrapArc;
use crate::traits::column::SqlField;

#[derive(Debug, Clone)]
pub struct PgValueColumn {
    name: String,
    table_alias: Option<TableAlias>,
    use_table_alias: bool,
    use_quotes: bool,
    column_alias: Option<String>,
}

impl PgValueColumn {
    pub fn new(name: &str) -> PgValueColumn {
        PgValueColumn {
            name: name.to_string(),
            table_alias: None,
            use_table_alias: false,
            use_quotes: false,
            column_alias: None,
        }
    }
    pub fn with_alias(mut self, alias: &str) -> Self {
        self.set_alias(alias.to_string());
        self
    }
    pub fn with_quotes(&self) -> Self {
        let mut c = self.clone();
        c.use_quotes = true;
        c
    }
    pub fn with_table_alias(&self) -> Self {
        let mut c = self.clone();
        c.use_table_alias = true;
        c
    }
}

impl SqlColumn for PgValueColumn {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn name_with_table(&self) -> String {
        match &self.table_alias {
            Some(alias) => {
                if self.use_table_alias {
                    if self.use_quotes {
                        format!("\"{}\".\"{}\"", alias.get(), self.name)
                    } else {
                        format!("{}.{}", alias.get(), self.name)
                    }
                } else {
                    match alias.try_get() {
                        Some(table_alias) => {
                            if self.use_quotes {
                                format!("\"{}\".\"{}\"", table_alias, self.name)
                            } else {
                                format!("{}.{}", table_alias, self.name)
                            }
                        }
                        None => {
                            if self.use_quotes {
                                format!("\"{}\"", self.name)
                            } else {
                                format!("{}", self.name)
                            }
                        }
                    }
                }
            }
            None => {
                if self.use_quotes {
                    format!("\"{}\"", self.name)
                } else {
                    format!("{}", self.name)
                }
            }
        }
    }
    fn set_table_alias(&mut self, table_alias: &TableAlias) {
        self.table_alias = Some(table_alias.clone());
    }
    fn set_name(&mut self, name: String) {
        self.name = name;
    }
    fn get_table_alias(&self) -> &Option<TableAlias> {
        &self.table_alias
    }
    // fn set_table_alias(&mut self, alias: String) {
    //     self.table_alias = Some(alias);
    // }
    fn set_alias(&mut self, alias: String) {
        self.column_alias = Some(alias);
    }

    fn get_alias(&self) -> Option<String> {
        self.column_alias.clone()
    }
}

impl Chunk for PgValueColumn {
    fn render_chunk(&self) -> Expression {
        expr!(self.name_with_table())
    }
}
impl Operations for PgValueColumn {}

impl Operations for Arc<PgValueColumn> {
    fn eq(&self, other: &impl Chunk) -> Condition {
        let column: Arc<Column> = Arc::new(Box::new((**self).clone()) as Box<dyn SqlColumn>);

        Condition::from_field(column, "=", WrapArc::wrap_arc(other.render_chunk()))
    }

    // fn add(&self, other: impl SqlChunk) -> Expression {
    //     let chunk = other.render_chunk();
    //     expr_arc!(format!("{} + {{}}", &self.name), chunk).render_chunk()
    // }
}

impl Chunk for Arc<PgValueColumn> {
    fn render_chunk(&self) -> Expression {
        expr!(self.name_with_table())
    }
}

impl SqlField for Arc<PgValueColumn> {
    fn render_column(&self, mut alias: Option<&str>) -> Expression {
        // If the alias is the same as the field name, we don't need to render it
        if alias.is_some() && alias.unwrap() == self.name {
            alias = None;
        }

        let alias = alias.or(self.column_alias.as_deref());

        if let Some(alias) = alias {
            expr!(format!(
                "{} AS {}",
                self.name_with_table(),
                alias.to_string()
            ))
        } else {
            expr!(self.name_with_table())
        }
    }
    fn calculated(&self) -> bool {
        false
    }
}

impl From<String> for PgValueColumn {
    fn from(name: String) -> Self {
        PgValueColumn {
            name,
            use_table_alias: false,
            use_quotes: false,
            table_alias: None,
            column_alias: None,
        }
    }
}

impl From<&str> for PgValueColumn {
    fn from(name: &str) -> Self {
        name.to_string().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field() {
        let field = Arc::new(PgValueColumn::new("id"));
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
        let field = Arc::new(PgValueColumn::new("id"));
        let (sql, params) = field.eq(&1).render_chunk().split();

        assert_eq!(sql, "(id = {})");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], 1);

        let f_age = Arc::new(PgValueColumn::new("age").with_alias("u"));
        let (sql, params) = f_age.add(5).eq(&18).render_chunk().split();

        // dispite the "alias" of "u" the column name is used here, alias is ignored
        assert_eq!(sql, "((age) + ({}) = {})");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], 5);
        assert_eq!(params[1], 18);
    }
}
