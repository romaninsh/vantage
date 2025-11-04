use std::sync::Arc;
use std::sync::RwLock;
use std::sync::Weak;

use crate::expr;
use crate::expr_arc;
use crate::prelude::column::SqlColumn;
use crate::prelude::Column;
use crate::prelude::SqlTable;
use crate::sql::chunk::Chunk;
use crate::sql::Condition;
use crate::sql::Operations;
use crate::sql::WrapArc;
use crate::sql::{Expression, ExpressionArc};
use crate::traits::column::SqlField;

#[derive(Debug, Clone)]
pub struct PgUuidColumn {
    name: String,
    table_alias: Option<Weak<RwLock<Option<String>>>>,
    column_alias: Option<String>,
}

impl PgUuidColumn {
    pub fn new(name: &str) -> PgUuidColumn {
        PgUuidColumn {
            name: name.to_string(),
            table_alias: None,
            column_alias: None,
        }
    }
    pub fn with_alias(mut self, alias: &str) -> Self {
        self.set_alias(alias.to_string());
        self
    }
}

impl SqlColumn for PgUuidColumn {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn name_with_table(&self) -> String {
        match self.get_table_alias() {
            Some(table_alias) => format!("{}.{}", table_alias, self.name),
            None => format!("{}", self.name),
        }
    }
    fn set_table_alias(&mut self, table_alias: Weak<RwLock<Option<String>>>) {
        self.table_alias = Some(table_alias);
    }
    fn get_table_alias(&self) -> Option<String> {
        let weak_ref = self.table_alias.as_ref()?;
        let arc_ref = weak_ref.upgrade()?;
        let guard = arc_ref.read().ok()?;
        guard.clone()
    }
    fn set_name(&mut self, name: String) {
        self.name = name;
    }
    fn set_alias(&mut self, alias: String) {
        self.column_alias = Some(alias);
    }

    fn get_alias(&self) -> Option<String> {
        self.column_alias.clone()
    }
}

impl Chunk for PgUuidColumn {
    fn render_chunk(&self) -> Expression {
        Arc::new(self.clone()).render_chunk()
    }
}
impl Operations for PgUuidColumn {}

impl Operations for Arc<PgUuidColumn> {
    fn eq(&self, other: &impl Chunk) -> Condition {
        let column: Arc<Column> = Arc::new(Box::new((**self).clone()) as Box<dyn SqlColumn>);

        Condition::from_field(column, "=", WrapArc::wrap_arc(other.render_chunk()))
    }

    // fn add(&self, other: impl SqlChunk) -> Expression {
    //     let chunk = other.render_chunk();
    //     expr_arc!(format!("{} + {{}}", &self.name), chunk).render_chunk()
    // }
}

impl Chunk for Arc<PgUuidColumn> {
    fn render_chunk(&self) -> Expression {
        expr!(self.name_with_table())
    }
}

impl SqlField for Arc<PgUuidColumn> {
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

impl From<String> for PgUuidColumn {
    fn from(name: String) -> Self {
        PgUuidColumn {
            name,
            table_alias: None,
            column_alias: None,
        }
    }
}

impl From<&str> for PgUuidColumn {
    fn from(name: &str) -> Self {
        name.to_string().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field() {
        let field = Arc::new(PgUuidColumn::new("id"));
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
        let field = Arc::new(PgUuidColumn::new("id"));
        let (sql, params) = field.eq(&1).render_chunk().split();

        assert_eq!(sql, "(id = {})");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], 1);

        let f_age = Arc::new(PgUuidColumn::new("age").with_alias("u"));
        let (sql, params) = f_age.add(5).eq(&18).render_chunk().split();

        assert_eq!(sql, "((u.age) + ({}) = {})");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], 5);
        assert_eq!(params[1], 18);
    }
}
