//! UI-agnostic value rendering.
//!
//! [`TerminalRender`] returns a [`RichText`] — an ordered list of
//! [`Span`]s, each carrying text plus a semantic [`Style`]. UI layers
//! (CLI, GPUI, web) translate styles into their native presentation:
//! `vantage-cli-util` emits ANSI; a GPUI adapter would emit styled
//! `div`s; a web layer could emit CSS classes. The trait itself never
//! references any specific output medium.

/// One styled run of text inside a [`RichText`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

impl Span {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    pub fn plain(text: impl Into<String>) -> Self {
        Self::new(text, Style::Default)
    }
}

/// Semantic styles. UI layers map these to colors / weights / etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    /// Default foreground — no special styling.
    #[default]
    Default,
    /// Subdued. Use for separators and secondary information.
    Dim,
    /// Very subdued. Use for placeholders (null / missing values).
    Muted,
    /// Emphasized. Use for primary identifiers in mixed content.
    Strong,
    /// Positive state — true booleans, healthy status, etc.
    Success,
    /// Negative state — false booleans, errors, deletions, etc.
    Error,
    /// Cautionary state — warnings, deprecated, pending, etc.
    Warning,
    /// Informational highlight — service names, links, etc.
    Info,
}

/// A sequence of styled text spans. The natural unit a rendering
/// implementation produces and a UI layer consumes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RichText {
    pub spans: Vec<Span>,
}

impl RichText {
    pub fn new() -> Self {
        Self { spans: Vec::new() }
    }

    /// Single span with [`Style::Default`].
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            spans: vec![Span::plain(text)],
        }
    }

    /// Single span with the given style.
    pub fn styled(text: impl Into<String>, style: Style) -> Self {
        Self {
            spans: vec![Span::new(text, style)],
        }
    }

    /// Builder-style append.
    pub fn push(mut self, text: impl Into<String>, style: Style) -> Self {
        self.spans.push(Span::new(text, style));
        self
    }

    /// Concatenate all span texts, dropping styles.
    pub fn to_plain(&self) -> String {
        let mut out = String::new();
        for s in &self.spans {
            out.push_str(&s.text);
        }
        out
    }
}

impl std::fmt::Display for RichText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for span in &self.spans {
            f.write_str(&span.text)?;
        }
        Ok(())
    }
}

impl From<&str> for RichText {
    fn from(s: &str) -> Self {
        RichText::plain(s)
    }
}

impl From<String> for RichText {
    fn from(s: String) -> Self {
        RichText::plain(s)
    }
}

/// Render a value as styled text. UI-agnostic — the returned
/// [`RichText`] is translated into ANSI / GPUI / etc. by the consuming
/// layer.
pub trait TerminalRender {
    fn render(&self) -> RichText;
}

// Standard type implementations

impl TerminalRender for String {
    fn render(&self) -> RichText {
        RichText::plain(self.clone())
    }
}

impl TerminalRender for &str {
    fn render(&self) -> RichText {
        RichText::plain(self.to_string())
    }
}

impl TerminalRender for i64 {
    fn render(&self) -> RichText {
        RichText::plain(self.to_string())
    }
}

impl TerminalRender for i32 {
    fn render(&self) -> RichText {
        RichText::plain(self.to_string())
    }
}

impl TerminalRender for f64 {
    fn render(&self) -> RichText {
        RichText::plain(self.to_string())
    }
}

impl TerminalRender for bool {
    fn render(&self) -> RichText {
        if *self {
            RichText::styled("true", Style::Success)
        } else {
            RichText::styled("false", Style::Error)
        }
    }
}

impl<T: TerminalRender> TerminalRender for Option<T> {
    fn render(&self) -> RichText {
        match self {
            Some(v) => v.render(),
            None => RichText::styled("—", Style::Muted),
        }
    }
}

#[cfg(feature = "serde")]
impl TerminalRender for serde_json::Value {
    fn render(&self) -> RichText {
        match self {
            serde_json::Value::String(s) => RichText::plain(s.clone()),
            serde_json::Value::Null => RichText::styled("—", Style::Muted),
            serde_json::Value::Bool(b) => b.render(),
            other => RichText::plain(other.to_string()),
        }
    }
}

impl TerminalRender for ciborium::Value {
    fn render(&self) -> RichText {
        match self {
            ciborium::Value::Text(s) => RichText::plain(s.clone()),
            ciborium::Value::Null => RichText::styled("—", Style::Muted),
            ciborium::Value::Bool(b) => b.render(),
            ciborium::Value::Integer(i) => RichText::plain(i128::from(*i).to_string()),
            ciborium::Value::Float(f) => RichText::plain(f.to_string()),
            ciborium::Value::Bytes(b) => {
                RichText::styled(format!("[{} bytes]", b.len()), Style::Muted)
            }
            ciborium::Value::Tag(_, inner) => inner.render(),
            other => RichText::plain(format!("{:?}", other)),
        }
    }
}
