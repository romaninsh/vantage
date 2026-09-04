//! Source-carrying slot types. A YAML struct field holds one of these instead
//! of `Option<String>`; the kind says how the host compiles it.

use serde::{Deserialize, Serialize};

macro_rules! slot {
    ($(#[$doc:meta])* $name:ident, $desc:literal) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn src(&self) -> &str {
                &self.0
            }
            pub const DESCRIPTION: &'static str = $desc;
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        #[cfg(feature = "schema")]
        impl schemars::JsonSchema for $name {
            fn schema_name() -> String {
                stringify!($name).to_string()
            }
            // `gen` is a reserved keyword in edition 2024, hence the raw identifier.
            fn json_schema(_: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
                use schemars::schema::{InstanceType, Metadata, SchemaObject};
                let mut obj = SchemaObject {
                    instance_type: Some(InstanceType::String.into()),
                    metadata: Some(Box::new(Metadata {
                        description: Some($desc.to_string()),
                        ..Default::default()
                    })),
                    ..Default::default()
                };
                obj.extensions.insert(
                    "x-language".to_string(),
                    serde_json::Value::String("rhai".to_string()),
                );
                obj.into()
            }
        }
    };
}

slot!(
    /// Whole string is one Rhai expression yielding one value. Exactly one
    /// `${ … }` wrapper around the whole string is tolerated and stripped.
    Expr,
    "Rhai expression"
);
slot!(
    /// Literal text with `${ expr }` Rhai holes.
    Template,
    "Text with `${…}` Rhai holes"
);
slot!(
    /// Rhai statements; the result is ignored unless read via `eval`.
    Block,
    "Rhai statements"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Doc {
        when: Expr,
        text: Template,
        action: Block,
    }

    #[test]
    fn slots_deserialize_from_plain_strings() {
        let doc: Doc = serde_json::from_str(
            r#"{"when":"row.x > 1","text":"Hi ${row.name}","action":"state.done = true;"}"#,
        )
        .unwrap();
        assert_eq!(doc.when.src(), "row.x > 1");
        assert_eq!(doc.text.src(), "Hi ${row.name}");
        assert_eq!(doc.action.src(), "state.done = true;");
    }

    #[test]
    fn slots_serialize_transparently() {
        let s = serde_json::to_string(&Expr::from("a + b")).unwrap();
        assert_eq!(s, r#""a + b""#);
    }

    #[cfg(feature = "schema")]
    #[test]
    fn schema_carries_language_marker() {
        let schema = schemars::schema_for!(Expr);
        let json = serde_json::to_value(&schema).unwrap();
        assert_eq!(json["type"], "string");
        assert_eq!(json["x-language"], "rhai");
        assert_eq!(json["description"], Expr::DESCRIPTION);
    }
}
