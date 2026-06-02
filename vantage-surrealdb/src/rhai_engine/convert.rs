//! Conversion helpers for Rhai ↔ Rust interop.

use vantage_expressions::Order;

/// Create a Rhai runtime error.
pub fn rhai_err(msg: impl Into<String>) -> Box<rhai::EvalAltResult> {
    rhai::EvalAltResult::ErrorRuntime(msg.into().into(), rhai::Position::NONE).into()
}

/// Parse "asc"/"desc" into Order.
pub fn parse_order(dir: &str) -> Result<Order, Box<rhai::EvalAltResult>> {
    match dir.to_lowercase().as_str() {
        "asc" => Ok(Order::Asc),
        "desc" => Ok(Order::Desc),
        _ => Err(rhai_err(format!(
            "order direction must be 'asc' or 'desc', got '{dir}'"
        ))),
    }
}
