use std::str::FromStr;

use vantage_expressions::{Expression, Expressive};

use crate::{
    AnySurrealType,
    types::{SurrealType, SurrealTypeThingMarker},
};

/// A SurrealDB record id, `table:id`, encoded as CBOR `Tag(8)` so stored
/// links can be followed in queries.
///
/// # Examples
///
/// ```ignore
/// use vantage_surrealdb::{thing::Thing, surreal_expr};
///
/// // Create a Thing reference
/// let latvia = Thing::new("country", "lv");
///
/// // Use in queries for relationships
/// db.execute(&surreal_expr!("CREATE country:lv SET name = {}", "Latvia")).await?;
/// db.execute(&surreal_expr!(
///     "CREATE user:test_user SET name = {}, country = {}",
///     "Test User",
///     latvia
/// )).await?;
///
/// // Query with relationship navigation
/// let result = db.execute(&surreal_expr!(
///     "SELECT VALUE [name, country.name] FROM ONLY user:test_user"
/// )).await?;
///
/// let names: Vec<String> = result.try_get()?;
/// assert_eq!(names[0], "Test User");
/// assert_eq!(names[1], "Latvia");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Thing {
    table: String,
    id: String,
}

impl Thing {
    pub fn new(table: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            id: id.into(),
        }
    }

    pub fn table(&self) -> &str {
        &self.table
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

impl std::fmt::Display for Thing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.table, self.id)
    }
}

/// Converts a string to a `Thing`. If the string is in `table:id` format it is
/// parsed normally. Otherwise the bare string is used as the id with a placeholder
/// table `"_"` — callers that need a fully-qualified record id (e.g. for
/// `AnyTable` write operations) should pass `"table:id"` explicitly.
impl From<String> for Thing {
    fn from(s: String) -> Self {
        s.parse().unwrap_or_else(|_| Thing::new("_", s))
    }
}

impl FromStr for Thing {
    type Err = String;

    fn from_str(thing_str: &str) -> Result<Self, Self::Err> {
        if let Some((table, id)) = thing_str.split_once(':') {
            Ok(Self {
                table: table.to_string(),
                id: id.to_string(),
            })
        } else {
            Err(format!("Invalid thing format: {}", thing_str))
        }
    }
}

impl SurrealType for Thing {
    type Target = SurrealTypeThingMarker;

    /// An id of only digits addresses the integer key (`todo:7`), not the
    /// string key `todo:⟨7⟩`.
    fn to_cbor(&self) -> ciborium::Value {
        let key = match integer_key(&self.id) {
            Some(n) => ciborium::Value::Integer(n.into()),
            None => ciborium::Value::Text(self.id.clone()),
        };
        ciborium::Value::Tag(
            8,
            Box::new(ciborium::Value::Array(vec![
                ciborium::Value::Text(self.table.clone()),
                key,
            ])),
        )
    }

    fn from_cbor(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Tag(8, boxed_value) => {
                if let ciborium::Value::Array(arr) = *boxed_value
                    && arr.len() == 2
                    && let ciborium::Value::Text(table) = &arr[0]
                {
                    return match &arr[1] {
                        ciborium::Value::Text(id) => Some(Thing::new(table.clone(), id.clone())),
                        ciborium::Value::Integer(n) => {
                            Some(Thing::new(table.clone(), i128::from(*n).to_string()))
                        }
                        _ => None,
                    };
                }
                None
            }
            ciborium::Value::Text(text) => text.parse().ok(), // `table:id` held as text
            _ => None,
        }
    }
}

impl Expressive<AnySurrealType> for Thing {
    fn expr(&self) -> Expression<AnySurrealType> {
        // `surreal-client` owns escaping. An id like `75KE-F3HG` must render
        // ⟨quoted⟩: bare, the dash parses as subtraction.
        Expression::new(
            format!(
                "{}:{}",
                surreal_client::escape_identifier(&self.table),
                record_key(&self.id)
            ),
            vec![],
        )
    }
}

/// The integer an id names: only its canonical form (`7`, `-1`), the form an
/// integer key reads back as. `007` and `+7` are text keys, so a row listed
/// as `todo:⟨007⟩` is written back to itself, not to `todo:7`.
pub(crate) fn integer_key(id: &str) -> Option<i64> {
    id.parse::<i64>().ok().filter(|n| n.to_string() == id)
}

/// `id` as a record key in SurrealQL: an integer key bare, anything else
/// escaped (`⟨007⟩`).
pub(crate) fn record_key(id: &str) -> String {
    match integer_key(id) {
        Some(n) => n.to_string(),
        None => surreal_client::escape_identifier(id),
    }
}
