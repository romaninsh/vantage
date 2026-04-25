/// Trait for rendering a value as a string for terminal output.
///
/// Returns plain text — styling (colors, bold, etc.) is applied by
/// the rendering layer (e.g. `vantage-cli-util`) based on type information.
pub trait TerminalRender {
    /// The plain text representation of this value.
    fn render(&self) -> String;

    /// Optional hint for terminal styling. Return a color name like
    /// "green", "red", "dim", or None for default.
    fn color_hint(&self) -> Option<&'static str> {
        None
    }
}

// Standard type implementations

impl TerminalRender for String {
    fn render(&self) -> String {
        self.clone()
    }
}

impl TerminalRender for i64 {
    fn render(&self) -> String {
        self.to_string()
    }
}

impl TerminalRender for i32 {
    fn render(&self) -> String {
        self.to_string()
    }
}

impl TerminalRender for f64 {
    fn render(&self) -> String {
        self.to_string()
    }
}

impl TerminalRender for bool {
    fn render(&self) -> String {
        self.to_string()
    }

    fn color_hint(&self) -> Option<&'static str> {
        if *self { Some("green") } else { Some("red") }
    }
}

impl<T: TerminalRender> TerminalRender for Option<T> {
    fn render(&self) -> String {
        match self {
            Some(v) => v.render(),
            None => "-".to_string(),
        }
    }

    fn color_hint(&self) -> Option<&'static str> {
        match self {
            Some(v) => v.color_hint(),
            None => Some("dim"),
        }
    }
}

#[cfg(feature = "serde")]
impl TerminalRender for serde_json::Value {
    fn render(&self) -> String {
        match self {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Null => "-".to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            other => other.to_string(),
        }
    }

    fn color_hint(&self) -> Option<&'static str> {
        match self {
            serde_json::Value::Bool(true) => Some("green"),
            serde_json::Value::Bool(false) => Some("red"),
            serde_json::Value::Null => Some("dim"),
            _ => None,
        }
    }
}

impl TerminalRender for ciborium::Value {
    fn render(&self) -> String {
        match self {
            ciborium::Value::Text(s) => s.clone(),
            ciborium::Value::Null => "-".to_string(),
            ciborium::Value::Bool(b) => b.to_string(),
            ciborium::Value::Integer(i) => i128::from(*i).to_string(),
            ciborium::Value::Float(f) => f.to_string(),
            ciborium::Value::Bytes(b) => format!("[{} bytes]", b.len()),
            ciborium::Value::Tag(_, inner) => inner.render(),
            other => format!("{:?}", other),
        }
    }

    fn color_hint(&self) -> Option<&'static str> {
        match self {
            ciborium::Value::Bool(true) => Some("green"),
            ciborium::Value::Bool(false) => Some("red"),
            ciborium::Value::Null => Some("dim"),
            _ => None,
        }
    }
}
