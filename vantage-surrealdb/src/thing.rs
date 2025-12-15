use std::str::FromStr;

use vantage_expressions::{Expression, Expressive};

use crate::{
    AnySurrealType, surreal_expr,
    types::{SurrealType, SurrealTypeThingMarker},
};

/// SurrealDB Thing (record ID) representation
///
/// Thing types enable relational queries between tables in SurrealDB.
/// They use proper CBOR Tag(8) encoding for seamless relationship navigation.
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
    /// Creates a new Thing with table and ID
    ///
    /// # Arguments
    ///
    /// * `table` - Table name
    /// * `id` - Record identifier
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
    use crate::{identifier::Identifier, surrealdb::SurrealDB};
    use indexmap::IndexMap;
    use surreal_client::{SurrealClient, SurrealConnection};
    use vantage_expressions::ExprDataSource;
    use vantage_types::{Record, TryFromRecord, entity};

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

    /// Setup test tables with country and user data
    async fn setup_test_data(db: &SurrealDB) -> (Identifier, Identifier, String) {
        use std::time::{SystemTime, UNIX_EPOCH};

        // Generate unique table and record names
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let country_table = Identifier::new(format!("country_{}", timestamp));
        let user_table = Identifier::new(format!("user_{}", timestamp));
        let country_id = format!("{}:lv", country_table.expr().template);
        let user_id = format!("{}:test_user", user_table.expr().template);

        // Clean up any existing test data
        let cleanup_country = surreal_expr!("DELETE {}", (country_table));
        let _ = db.execute(&cleanup_country).await;

        let cleanup_user = surreal_expr!("DELETE {}", (user_table));
        let _ = db.execute(&cleanup_user).await;

        // 1. Insert country with id "lv" and name "Latvia"
        let create_country =
            surreal_expr!(&format!("CREATE {} SET name = {{}}", country_id), "Latvia");
        db.execute(&create_country)
            .await
            .expect("Failed to create country");

        // 2. Insert user with country = thing(country:lv)
        let country_thing = Thing::new(country_table.expr().template, "lv");
        let create_user = surreal_expr!(
            &format!("CREATE {} SET name = {{}}, country = {{}}", user_id),
            "Test User",
            country_thing
        );
        db.execute(&create_user)
            .await
            .expect("Failed to create user");

        (country_table, user_table, user_id)
    }

    #[derive(Debug, PartialEq)]
    #[entity(SurrealType)]
    struct User {
        name: String,
        country: Thing,
        country_name: String,
    }

    #[tokio::test]
    async fn test_thing_database_integration() {
        let db = get_surrealdb().await;
        let (_country_table, _user_table, user_id) = setup_test_data(&db).await;

        // 3. Perform query to get user name and country name as a single array value
        let join_query = surreal_expr!(&format!(
            "SELECT VALUE [name, country.name] FROM ONLY {}",
            user_id
        ));

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

    #[tokio::test]
    async fn test_thing_record_conversion() {
        let db = get_surrealdb().await;
        let (country_table, _user_table, user_id) = setup_test_data(&db).await;

        // Query with flattened country fields using alias to avoid name collision
        let query = surreal_expr!(&format!(
            "SELECT *, country.name as country_name FROM ONLY {}",
            user_id
        ));

        let result = db.execute(&query).await.expect("Failed to execute query");

        // Convert AnySurrealType result to IndexMap first, then to Record
        let index_map: IndexMap<String, AnySurrealType> = result
            .try_get()
            .expect("Failed to convert result to IndexMap");

        let record: Record<AnySurrealType> = Record::from_indexmap(index_map);

        // Convert Record<AnySurrealType> to User struct using entity macro
        let user = User::from_record(record).expect("Failed to convert record to User");

        assert_eq!(user.name, "Test User", "Expected user name");
        assert_eq!(
            user.country.table,
            country_table.expr().template,
            "Expected country table"
        );
        assert_eq!(user.country.id, "lv", "Expected country id");
        assert_eq!(user.country_name, "Latvia", "Expected country name");

        println!("✅ Thing record conversion test passed");
    }
}
