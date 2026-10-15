//! Source-carrying slot types. A YAML struct field holds one of these instead
//! of `Option<String>`; the kind says how the host compiles it.

use serde::{Deserialize, Serialize};

macro_rules! slot {
    ($(#[$doc:meta])* $name:ident, $desc:literal) => {
        $(#[$doc])*
        #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }

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

        // A slot reads as its source text wherever a `&str` is wanted;
        // `Host::compile` still takes the typed slot, never a bare string.
        impl std::ops::Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
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
