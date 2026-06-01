//! Type conversion helpers for Rhai ↔ Rust interop.

use rhai::Dynamic;
use vantage_expressions::Order;

pub fn parse_order(dir: &str) -> Result<Order, Box<rhai::EvalAltResult>> {
    match dir.to_lowercase().as_str() {
        "asc" => Ok(Order::Asc),
        "desc" => Ok(Order::Desc),
        _ => Err(rhai_err(format!(
            "order direction must be 'asc' or 'desc', got '{dir}'"
        ))),
    }
}

pub fn rhai_err(msg: impl Into<String>) -> Box<rhai::EvalAltResult> {
    rhai::EvalAltResult::ErrorRuntime(msg.into().into(), rhai::Position::NONE).into()
}

pub fn friendly_type_name(val: &Dynamic) -> &'static str {
    if val.is::<i64>() {
        "i64"
    } else if val.is::<f64>() {
        "f64"
    } else if val.is::<bool>() {
        "bool"
    } else if val.is::<rhai::ImmutableString>() || val.is::<String>() {
        "string"
    } else {
        val.type_name()
    }
}

// ── Macro ──────────────────────────────────────────────────────────────
//
// Uses type aliases Ex, Id from register_engine! scope.

#[macro_export]
macro_rules! register_convert {
    (value: $V:ty) => {
        fn to_expr(val: rhai::Dynamic) -> ::std::result::Result<Expr, Box<rhai::EvalAltResult>> {
            if let Some(e) = val.clone().try_cast::<Ex>() {
                Ok(e.0)
            } else if let Some(i) = val.clone().try_cast::<Id>() {
                Ok($crate::vantage_expressions::Expressive::<$V>::expr(&i.0))
            } else if let Some(v) = val.clone().try_cast::<i64>() {
                Ok(Expr::new(
                    "{}",
                    vec![$crate::vantage_expressions::ExpressiveEnum::Scalar(
                        <$V>::from(v),
                    )],
                ))
            } else if let Some(v) = val.clone().try_cast::<f64>() {
                Ok(Expr::new(
                    "{}",
                    vec![$crate::vantage_expressions::ExpressiveEnum::Scalar(
                        <$V>::from(v),
                    )],
                ))
            } else if let Some(v) = val.clone().try_cast::<bool>() {
                Ok(Expr::new(
                    "{}",
                    vec![$crate::vantage_expressions::ExpressiveEnum::Scalar(
                        <$V>::from(v),
                    )],
                ))
            } else if let Some(v) = val.clone().try_cast::<rhai::ImmutableString>() {
                Ok(Expr::new(
                    "{}",
                    vec![$crate::vantage_expressions::ExpressiveEnum::Scalar(
                        <$V>::from(v.to_string()),
                    )],
                ))
            } else if let Some(v) = val.clone().try_cast::<String>() {
                Ok(Expr::new(
                    "{}",
                    vec![$crate::vantage_expressions::ExpressiveEnum::Scalar(
                        <$V>::from(v),
                    )],
                ))
            } else {
                ::std::result::Result::Err($crate::rhai_engine::convert::rhai_err(format!(
                    "expected Expr, Ident, or scalar — got '{}'",
                    $crate::rhai_engine::convert::friendly_type_name(&val)
                )))
            }
        }

        fn to_expressive_enum(
            val: rhai::Dynamic,
        ) -> ::std::result::Result<
            $crate::vantage_expressions::ExpressiveEnum<$V>,
            Box<rhai::EvalAltResult>,
        > {
            if let Some(e) = val.clone().try_cast::<Ex>() {
                Ok($crate::vantage_expressions::ExpressiveEnum::Nested(e.0))
            } else if let Some(i) = val.clone().try_cast::<Id>() {
                Ok($crate::vantage_expressions::ExpressiveEnum::Nested(
                    $crate::vantage_expressions::Expressive::<$V>::expr(&i.0),
                ))
            } else if let Some(v) = val.clone().try_cast::<i64>() {
                Ok($crate::vantage_expressions::ExpressiveEnum::Scalar(
                    <$V>::from(v),
                ))
            } else if let Some(v) = val.clone().try_cast::<f64>() {
                Ok($crate::vantage_expressions::ExpressiveEnum::Scalar(
                    <$V>::from(v),
                ))
            } else if let Some(v) = val.clone().try_cast::<bool>() {
                Ok($crate::vantage_expressions::ExpressiveEnum::Scalar(
                    <$V>::from(v),
                ))
            } else if let Some(v) = val.clone().try_cast::<rhai::ImmutableString>() {
                Ok($crate::vantage_expressions::ExpressiveEnum::Scalar(
                    <$V>::from(v.to_string()),
                ))
            } else if let Some(v) = val.clone().try_cast::<String>() {
                Ok($crate::vantage_expressions::ExpressiveEnum::Scalar(
                    <$V>::from(v),
                ))
            } else {
                ::std::result::Result::Err($crate::rhai_engine::convert::rhai_err(format!(
                    "unsupported param type '{}'",
                    $crate::rhai_engine::convert::friendly_type_name(&val)
                )))
            }
        }
    };
}
