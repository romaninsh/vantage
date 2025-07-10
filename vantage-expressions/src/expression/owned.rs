//! Owned expressions will greedily own all the parameters.
//! Owned expressions implement Expression trait
//! Owned expressions can be converted into Lazy expression.

use async_trait::async_trait;
use serde_json::Value;

use crate::protocol::{DataSource, Expressive};

#[derive(Debug, Clone)]
pub enum OwnedParameter {
    /// Owned scalar value
    Value(Value),
    OwnedExpression(OwnedExpression),
}

// Direct implementations
impl From<Value> for OwnedParameter {
    fn from(value: Value) -> Self {
        OwnedParameter::Value(value)
    }
}

// Specific implementations for basic types that should convert to Value
impl From<String> for OwnedParameter {
    fn from(value: String) -> Self {
        OwnedParameter::Value(Value::String(value))
    }
}

impl From<&str> for OwnedParameter {
    fn from(value: &str) -> Self {
        OwnedParameter::Value(Value::String(value.to_string()))
    }
}

impl From<i32> for OwnedParameter {
    fn from(value: i32) -> Self {
        OwnedParameter::Value(Value::Number(value.into()))
    }
}

impl From<i64> for OwnedParameter {
    fn from(value: i64) -> Self {
        OwnedParameter::Value(Value::Number(value.into()))
    }
}

impl From<f64> for OwnedParameter {
    fn from(value: f64) -> Self {
        OwnedParameter::Value(Value::Number(
            serde_json::Number::from_f64(value).unwrap_or_else(|| 0.into()),
        ))
    }
}

impl From<bool> for OwnedParameter {
    fn from(value: bool) -> Self {
        OwnedParameter::Value(Value::Bool(value))
    }
}

impl<T: Into<OwnedParameter>> From<Vec<T>> for OwnedParameter {
    fn from(vec: Vec<T>) -> Self {
        let values: Vec<Value> = vec
            .into_iter()
            .map(|item| match item.into() {
                OwnedParameter::Value(v) => v,
                OwnedParameter::OwnedExpression(expr) => Value::String(expr.preview()),
            })
            .collect();
        OwnedParameter::Value(Value::Array(values))
    }
}

impl<T: Into<OwnedParameter> + Clone, const N: usize> From<[T; N]> for OwnedParameter {
    fn from(arr: [T; N]) -> Self {
        arr.to_vec().into()
    }
}

// For types that implement Into<OwnedExpression>
impl<T: Into<OwnedExpression>> From<T> for OwnedParameter {
    fn from(expr: T) -> Self {
        OwnedParameter::OwnedExpression(expr.into())
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
                    $param.into()
                ),*
            ]
        )
    };
}

#[async_trait]
impl Expressive for OwnedExpression {
    async fn prepare(&self, _data_source: &dyn DataSource) -> OwnedExpression {
        self.clone()
    }
}

impl OwnedExpression {
    /// Create a new owned expression with template and parameters
    pub fn new(template: String, parameters: Vec<OwnedParameter>) -> Self {
        Self {
            template,
            parameters,
        }
    }

    /// Create expression with parameters that implement IntoOwnedParameter
    pub fn from_params<T: Into<OwnedParameter>>(template: String, parameters: Vec<T>) -> Self {
        let converted_params = parameters.into_iter().map(|p| p.into()).collect();

        Self {
            template,
            parameters: converted_params,
        }
    }

    /// Create expression from vector of expressions and a delimeter
    pub fn from_vec(vec: Vec<OwnedExpression>, delimeter: &str) -> Self {
        let template = vec
            .iter()
            .map(|_| "{}")
            .collect::<Vec<&str>>()
            .join(delimeter);

        let parameters = vec
            .into_iter()
            .map(|expr| OwnedParameter::OwnedExpression(expr))
            .collect();

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
                OwnedParameter::OwnedExpression(expr) => expr.preview(),
            };
            preview = preview.replacen("{}", &param_str, 1);
        }
        preview
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[derive(Debug)]
    struct Identifier {
        identifier: String,
    }
    impl Identifier {
        pub fn new(identifier: impl Into<String>) -> Self {
            Self {
                identifier: identifier.into(),
            }
        }
    }

    impl Into<OwnedExpression> for Identifier {
        fn into(self) -> OwnedExpression {
            expr!(format!("`{}`", self.identifier))
        }
    }

    use super::*;

    #[test]
    fn test_basic() {
        let expr = OwnedExpression::new(
            "SELECT * FROM {} WHERE name={} AND age>{} AND {} AND gender in {}".to_string(),
            vec![
                OwnedParameter::OwnedExpression(Identifier::new("users").into()),
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
            Identifier::new("users"),
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
