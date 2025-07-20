use vantage_expressions::{IntoExpressive, OwnedExpression, expr};

#[derive(Debug, Clone)]
pub struct Identifier {
    identifier: String,
}

impl Identifier {
    pub fn new(identifier: impl Into<String>) -> Self {
        Self {
            identifier: identifier.into(),
        }
    }

    fn needs_escaping(&self) -> bool {
        let reserved_keywords = [
            "DEFINE", "CREATE", "SELECT", "UPDATE", "DELETE", "FROM", "WHERE", "SET", "ONLY",
            "TABLE",
        ];

        let upper_identifier = self.identifier.to_uppercase();

        // Check if it contains spaces or is a reserved keyword
        self.identifier.contains(' ') || reserved_keywords.contains(&upper_identifier.as_str())
    }
}

impl Into<OwnedExpression> for Identifier {
    fn into(self) -> OwnedExpression {
        if self.needs_escaping() {
            expr!(format!("⟨{}⟩", self.identifier))
        } else {
            expr!(self.identifier)
        }
    }
}

impl From<Identifier> for IntoExpressive<OwnedExpression> {
    fn from(id: Identifier) -> Self {
        IntoExpressive::nested(id.into())
    }
}
