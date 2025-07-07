//! Owned expressions will greedily own all the parameters.
//! Owned expressions implement Expression trait
//! Owned expressions can be converted into Lazy expression.

use async_trait::async_trait;
use serde_json::Value;

use crate::{protocol::Expression, value::IntoValue};

#[derive(Debug, Clone)]
pub enum OwnedParameter {
    /// Owned scalar value
    Value(Value),
    Identifier(String),
}

/// Trait for types that can be passed as parameters
pub trait IntoOwnedParameter {
    fn into_owned_parameter(self) -> OwnedParameter;
}

/// Implement for all variants of IntoValue
impl<T: IntoValue + 'static> IntoOwnedParameter for T {
    fn into_owned_parameter(self) -> OwnedParameter {
        OwnedParameter::Value(Box::new(self).into_value())
    }
}

impl IntoOwnedParameter for OwnedParameter {
    fn into_owned_parameter(self) -> OwnedParameter {
        self
    }
}

/// Owned expression contains template and Vec of parameters
#[derive(Debug, Clone)]
pub struct OwnedExpression {
    pub template: String,
    pub parameters: Vec<OwnedParameter>,
}

/// Macro to create expressions with template and parameters
#[macro_export]
macro_rules! expr {
    // Simple template without parameters: expr!("age")
    ($template:expr) => {
        $crate::expression::owned::OwnedExpression::new($template.to_string(), vec![])
    };

    // Template with parameters: expr!("{} > {}", param1, param2)
    ($template:expr, $($param:expr),*) => {
        $crate::expression::owned::OwnedExpression::new(
            $template.to_string(),
            vec![
                $(
                    $crate::expression::owned::IntoOwnedParameter::into_owned_parameter($param)
                ),*
            ]
        )
    };
}

#[async_trait]
impl Expression for OwnedExpression {}

impl OwnedExpression {
    /// Create a new owned expression with template and parameters
    pub fn new(template: String, parameters: Vec<OwnedParameter>) -> Self {
        Self {
            template,
            parameters,
        }
    }

    /// Create expression with parameters that implement IntoOwnedParameter
    pub fn from_params<T: IntoOwnedParameter>(template: String, parameters: Vec<T>) -> Self {
        let converted_params = parameters
            .into_iter()
            .map(|p| p.into_owned_parameter())
            .collect();

        Self {
            template,
            parameters: converted_params,
        }
    }

    /// Create expression from vector of values and a delimeter
    pub fn from_vec(vec: Vec<Value>, delimeter: &str) -> Self {
        let template = vec
            .iter()
            .map(|_| "{}")
            .collect::<Vec<&str>>()
            .join(delimeter);

        let parameters = vec.into_iter().map(|v| OwnedParameter::Value(v)).collect();

        Self {
            template,
            parameters,
        }
    }

    pub fn preview(&self) -> String {
        let mut preview = self.template.clone();
        for param in &self.parameters {
            let param_str = match param {
                OwnedParameter::Value(param) => match param {
                    Value::String(s) => format!("{:?}", s),
                    other => format!("{}", other),
                },
                OwnedParameter::Identifier(id) => format!("`{}`", id),
            };
            preview = preview.replacen("{}", &param_str, 1);
        }
        preview
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_basic() {
        let expr = OwnedExpression::new(
            "SELECT * FROM {} WHERE name={} AND age>{} AND {} AND gender in {}".to_string(),
            vec![
                OwnedParameter::Identifier("users".to_string()),
                OwnedParameter::Value(json!("sue")),
                OwnedParameter::Value(json!(18)),
                OwnedParameter::Value(json!(true)),
                OwnedParameter::Value(json!(["female", "male", "other"])),
            ],
        );

        let preview = expr.preview();
        assert_eq!(
            preview,
            "SELECT * FROM `users` WHERE name=\"sue\" AND age>18 AND true AND gender in [\"female\",\"male\",\"other\"]"
        );
    }

    #[test]
    fn test_expr() {
        let expr = expr!(
            "SELECT * FROM {} WHERE name={} AND age>{} AND {} AND gender in {}",
            OwnedParameter::Identifier("users".to_string()),
            "sue",
            18,
            true,
            ["female", "male", "other"]
        );

        let preview = expr.preview();
        assert_eq!(
            preview,
            "SELECT * FROM `users` WHERE name=\"sue\" AND age>18 AND true AND gender in [\"female\",\"male\",\"other\"]"
        );
    }
}
