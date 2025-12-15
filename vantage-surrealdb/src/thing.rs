//! # SurrealDB Thing (Record ID)
//!
//! doc wip

use std::str::FromStr;

use vantage_expressions::{Expression, Expressive};

use crate::{
    AnySurrealType, surreal_expr,
    types::{SurrealType, SurrealTypeThingMarker},
};

/// SurrealDB Thing (record ID) representation
///
/// doc wip
///
/// # Examples
///
/// ```rust
/// use vantage_surrealdb::thing::Thing;
///
/// // doc wip
/// let thing = Thing::new("users".to_string(), "john".to_string());
/// let parsed = "users:john".parse::<Thing>();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Thing {
    table: String,
    id: String,
}

impl Thing {
    /// Creates a new Thing with table and ID
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `table` - doc wip
    /// * `id` - doc wip
    pub fn new(table: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            id: id.into(),
        }
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

    fn to_cbor(&self) -> ciborium::Value {
        // Thing is stored as Tag(8, [table, id]) in SurrealDB CBOR format
        ciborium::Value::Tag(
            8,
            Box::new(ciborium::Value::Array(vec![
                ciborium::Value::Text(self.table.clone()),
                ciborium::Value::Text(self.id.clone()),
            ])),
        )
    }

    fn from_cbor(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Tag(8, boxed_value) => {
                if let ciborium::Value::Array(arr) = *boxed_value {
                    if arr.len() == 2 {
                        if let (ciborium::Value::Text(table), ciborium::Value::Text(id)) =
                            (&arr[0], &arr[1])
                        {
                            return Some(Thing::new(table.clone(), id.clone()));
                        }
                    }
                }
                None
            }
            ciborium::Value::Text(text) => text.parse().ok(), // Fallback for string format
            _ => None,
        }
    }
}

impl Expressive<AnySurrealType> for Thing {
    fn expr(&self) -> Expression<AnySurrealType> {
        surreal_expr!(format!("{}:{}", self.table, self.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surrealdb::SurrealDB;
    use surreal_client::{SurrealClient, SurrealConnection};
    use vantage_expressions::ExprDataSource;

    const DB_URL: &str = "cbor://localhost:8000/rpc";
    const ROOT_USER: &str = "root";
    const ROOT_PASS: &str = "root";
    const TEST_NAMESPACE: &str = "bakery";
    const TEST_DATABASE: &str = "thing_test";

    async fn get_client() -> SurrealClient {
        SurrealConnection::new()
            .url(DB_URL)
            .namespace(TEST_NAMESPACE)
            .database(TEST_DATABASE)
            .auth_root(ROOT_USER, ROOT_PASS)
            .with_debug(true)
            .connect()
            .await
            .expect("Failed to connect to SurrealDB")
    }

    async fn get_surrealdb() -> SurrealDB {
        let client = get_client().await;
        SurrealDB::new(client)
    }

    #[tokio::test]
    async fn test_thing_database_integration() {
        let db = get_surrealdb().await;

        // Clean up any existing test data
        let cleanup_country = surreal_expr!("DELETE country");
        let _ = db.execute(&cleanup_country).await;

        let cleanup_user = surreal_expr!("DELETE user");
        let _ = db.execute(&cleanup_user).await;

        // 1. Insert country with id "lv" and name "Latvia"
        let create_country = surreal_expr!("CREATE country:lv SET name = {}", "Latvia");

        let _create_result = db
            .execute(&create_country)
            .await
            .expect("Failed to create country");

        // 2. Insert user with country = thing(country:lv)
        let country_thing = Thing::new("country", "lv");
        let create_user = surreal_expr!(
            "CREATE user:test_user SET name = {}, country = {}",
            "Test User",
            country_thing
        );

        let _user_result = db
            .execute(&create_user)
            .await
            .expect("Failed to create user");

        // 3. Perform query to get user name and country name as a single array value
        let join_query =
            surreal_expr!("SELECT VALUE [name, country.name] FROM ONLY user:test_user");

        let join_result = db
            .execute(&join_query)
            .await
            .expect("Failed to execute join query");

        // Convert result directly to Vec<String> using type system
        let names: Vec<String> = join_result
            .try_get()
            .expect("Failed to convert result to Vec<String>");

        assert_eq!(names.len(), 2, "Expected array with 2 elements");
        assert_eq!(names[0], "Test User", "Expected user name");
        assert_eq!(names[1], "Latvia", "Expected country name");

        println!("✅ Thing database integration test passed");
    }
}
