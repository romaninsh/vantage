use vantage_expressions::{Expression, expr};

#[derive(Debug, Clone)]
pub struct QueryConditions {
    conditions: Vec<Expression>,
}

impl QueryConditions {
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
        }
    }

    pub fn add_condition(&mut self, condition: Expression) {
        self.conditions.push(condition);
    }

    pub fn clear(&mut self) {
        self.conditions.clear();
    }

    pub fn has_conditions(&self) -> bool {
        !self.conditions.is_empty()
    }

    pub fn render(&self) -> Expression {
        if self.conditions.is_empty() {
            expr!("")
        } else if self.conditions.len() == 1 {
            expr!(" WHERE {}", self.conditions[0].clone())
        } else {
            // Combine multiple conditions with AND
            let conditions: Vec<Expression> = self
                .conditions
                .iter()
                .map(|c| expr!("({})", c.clone()))
                .collect();
            let combined = Expression::from_vec(conditions, " AND ");
            expr!(" WHERE {}", combined)
        }
    }

    pub fn render_conditions(&self) -> Expression {
        if self.conditions.is_empty() {
            expr!("")
        } else if self.conditions.len() == 1 {
            self.conditions[0].clone()
        } else {
            // Combine multiple conditions with AND
            let conditions: Vec<Expression> = self
                .conditions
                .iter()
                .map(|c| expr!("({})", c.clone()))
                .collect();
            Expression::from_vec(conditions, " AND ")
        }
    }
}

impl Default for QueryConditions {
    fn default() -> Self {
        Self::new()
    }
}
