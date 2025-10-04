use std::marker::PhantomData;
use vantage_core::Entity;
use vantage_expressions::protocol::{expressive::IntoExpressive, selectable::Selectable};

use crate::expression::RedbExpression;

pub struct RedbSelect<E: Entity> {
    _phantom: PhantomData<E>,
    table: Option<String>,
    key: Option<RedbExpression>,
    order_column: Option<String>,
    order_ascending: bool,
    limit: Option<i64>,
    skip: Option<i64>,
}

impl<E: Entity> RedbSelect<E> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
            table: None,
            key: None,
            order_column: None,
            order_ascending: true,
            limit: None,
            skip: None,
        }
    }

    pub fn entity_type_name() -> &'static str {
        std::any::type_name::<E>()
    }

    pub fn table(&self) -> Option<&String> {
        self.table.as_ref()
    }

    pub fn key(&self) -> Option<&RedbExpression> {
        self.key.as_ref()
    }

    pub fn limit(&self) -> Option<i64> {
        self.limit
    }

    pub fn skip(&self) -> Option<i64> {
        self.skip
    }

    pub fn order_column(&self) -> Option<&String> {
        self.order_column.as_ref()
    }

    pub fn order_ascending(&self) -> bool {
        self.order_ascending
    }

    pub fn with_condition(mut self, column: impl Into<String>, value: serde_json::Value) -> Self {
        if self.key.is_some() {
            panic!("RedbSelect only supports one condition. Key condition already set");
        }
        self.key = Some(RedbExpression::eq(column.into(), value));
        self
    }

    pub fn with_order(mut self, column: impl Into<String>, ascending: bool) -> Self {
        if self.order_column.is_some() {
            panic!(
                "RedbSelect only supports one order. Order already set for column: {:?}",
                self.order_column
            );
        }
        self.order_column = Some(column.into());
        self.order_ascending = ascending;
        self
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }
}

impl<E: Entity> Default for RedbSelect<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Entity> Selectable<RedbExpression> for RedbSelect<E> {
    fn set_source(
        &mut self,
        source: impl Into<IntoExpressive<RedbExpression>>,
        _alias: Option<String>,
    ) {
        // For ReDB, source is the table name
        if let Some(scalar) = source.into().as_scalar()
            && let Some(table_name) = scalar.as_str()
        {
            self.table = Some(table_name.to_string());
        }
    }

    fn add_field(&mut self, _field: impl Into<String>) {
        // ReDB is key-value, fields don't apply
    }

    fn add_expression(&mut self, _expression: RedbExpression, _alias: Option<String>) {
        // ReDB doesn't support complex expressions
    }

    fn add_where_condition(&mut self, condition: RedbExpression) {
        if self.key.is_some() {
            panic!("RedbSelect only supports one where condition. Key condition already set");
        }
        self.key = Some(condition);
    }

    fn set_distinct(&mut self, _distinct: bool) {
        // Not applicable for key-value store
    }

    fn add_order_by(
        &mut self,
        _field_or_expr: impl Into<IntoExpressive<RedbExpression>>,
        _ascending: bool,
    ) {
    }

    fn add_group_by(&mut self, _expression: RedbExpression) {}

    fn set_limit(&mut self, limit: Option<i64>, skip: Option<i64>) {
        self.limit = limit;
        self.skip = skip;
    }

    fn clear_fields(&mut self) {
        // No-op for ReDB
    }

    fn clear_where_conditions(&mut self) {
        self.key = None;
    }

    fn clear_order_by(&mut self) {
        // No-op for ReDB
    }

    fn clear_group_by(&mut self) {
        // No-op for ReDB
    }

    fn has_fields(&self) -> bool {
        false
    }

    fn has_where_conditions(&self) -> bool {
        self.key.is_some()
    }

    fn has_order_by(&self) -> bool {
        self.order_column.is_some()
    }

    fn has_group_by(&self) -> bool {
        false
    }

    fn is_distinct(&self) -> bool {
        false
    }

    fn get_limit(&self) -> Option<i64> {
        self.limit
    }

    fn get_skip(&self) -> Option<i64> {
        self.skip
    }
}

impl<E: Entity> From<RedbSelect<E>> for RedbExpression {
    fn from(select: RedbSelect<E>) -> Self {
        let table = select.table.unwrap_or_else(|| "unknown".to_string());
        let mut query = serde_json::json!({"table": table});

        if let Some(key) = select.key {
            if let Some((column, value)) = key.as_eq() {
                query["condition"] = serde_json::json!({"column": column, "value": value});
            } else {
                query["key"] = key.into_value().unwrap_or(serde_json::Value::Null);
            }
        } else {
            query["scan"] = serde_json::Value::Bool(true);
        }

        if let Some(order_col) = select.order_column {
            query["order"] =
                serde_json::json!({"column": order_col, "ascending": select.order_ascending});
        }

        RedbExpression::new(query)
    }
}

impl<E: Entity> std::fmt::Debug for RedbSelect<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedbSelect")
            .field("table", &self.table)
            .field("key", &self.key)
            .field("order_column", &self.order_column)
            .field("order_ascending", &self.order_ascending)
            .field("limit", &self.limit)
            .field("skip", &self.skip)
            .finish()
    }
}
