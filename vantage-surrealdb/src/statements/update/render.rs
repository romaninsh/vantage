use crate::Expr;
use crate::identifier::Identifier;
use crate::types::AnySurrealType;
use indexmap::IndexMap;
use vantage_expressions::Expressive;

use super::{SurrealUpdate, UpdateMode};

impl SurrealUpdate {
    /// Render the statement as a string (for debugging — never use in queries).
    pub fn preview(&self) -> String {
        self.expr().preview()
    }

    /// Build a CBOR object value from current fields (for CONTENT/MERGE).
    fn fields_as_object(&self) -> AnySurrealType {
        let map: Vec<(ciborium::Value, ciborium::Value)> = self
            .fields
            .iter()
            .map(|(k, v)| (ciborium::Value::Text(k.clone()), v.value().clone()))
            .collect();
        AnySurrealType::from_cbor(&ciborium::Value::Map(map))
            .unwrap_or_else(|| AnySurrealType::new(IndexMap::<String, AnySurrealType>::new()))
    }

    fn render_where(&self) -> Option<Expr> {
        if self.conditions.is_empty() {
            return None;
        }
        Some(
            self.conditions
                .iter()
                .cloned()
                .reduce(|a, b| crate::surreal_expr!("{} AND {}", (a), (b)))
                .unwrap(),
        )
    }

    fn append_where(&self, base: Expr) -> Expr {
        match self.render_where() {
            Some(cond) => crate::surreal_expr!("{} WHERE {}", (base), (cond)),
            None => base,
        }
    }
}

impl Expressive<AnySurrealType> for SurrealUpdate {
    fn expr(&self) -> Expr {
        let raw = match self.mode {
            UpdateMode::Set => {
                if self.fields.is_empty() {
                    crate::surreal_expr!("UPDATE {}", (self.target))
                } else {
                    let placeholders: Vec<String> = self
                        .fields
                        .keys()
                        .map(|k| format!("{} = {{}}", Identifier::new(k).expr().preview()))
                        .collect();
                    let template = format!("UPDATE {{}} SET {}", placeholders.join(", "));

                    let mut params: Vec<vantage_expressions::ExpressiveEnum<AnySurrealType>> =
                        vec![vantage_expressions::ExpressiveEnum::Nested(
                            self.target.clone(),
                        )];

                    for value in self.fields.values() {
                        params.push(vantage_expressions::ExpressiveEnum::Scalar(value.clone()));
                    }

                    vantage_expressions::Expression::new(template, params)
                }
            }
            UpdateMode::Content => {
                let obj = self.fields_as_object();
                crate::surreal_expr!("UPDATE {} CONTENT {}", (self.target), obj)
            }
            UpdateMode::Merge => {
                let obj = self.fields_as_object();
                crate::surreal_expr!("UPDATE {} MERGE {}", (self.target), obj)
            }
        };
        self.append_where(raw)
    }
}

impl From<SurrealUpdate> for Expr {
    fn from(update: SurrealUpdate) -> Self {
        update.expr()
    }
}
