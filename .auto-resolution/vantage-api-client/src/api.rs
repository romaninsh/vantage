use indexmap::IndexMap;
use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_types::Record;

/// REST API backend for Vantage — reads data from HTTP JSON endpoints.
///
/// Each table maps to an API endpoint: `{base_url}/{table_name}`.
/// The API returns paginated JSON: `{"data": [...], "pagination": {...}}`.
/// Uses `serde_json::Value` as the native value type — no custom type system needed.
///
/// Currently read-only — write operations return errors.
#[derive(Clone, Debug)]
pub struct RestApi {
    base_url: String,
    client: reqwest::Client,
    pub(crate) auth_header: Option<String>,
}

impl RestApi {
    /// Create a new REST API data source pointing at `base_url`.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
            auth_header: None,
        }
    }

    /// Set the Authorization header value (e.g. "Bearer <token>").
    pub fn with_auth(mut self, auth: impl Into<String>) -> Self {
        self.auth_header = Some(auth.into());
        self
    }

    /// Build the endpoint URL for a given table name.
    fn endpoint_url(&self, table_name: &str) -> String {
        format!("{}/{}", self.base_url, table_name)
    }

    /// Fetch data from the API endpoint and return parsed records.
    ///
    /// The `id_field` parameter determines which JSON field to use as the record ID.
    /// If None, row indices are used.
    pub(crate) async fn fetch_records(
        &self,
        table_name: &str,
        id_field: Option<&str>,
    ) -> Result<IndexMap<String, Record<serde_json::Value>>> {
        let url = self.endpoint_url(table_name);

        let mut request = self.client.get(&url);
        if let Some(ref auth) = self.auth_header {
            request = request.header("Authorization", auth);
        }

        let response = request
            .send()
            .await
            .map_err(|e| error!("API request failed", url = url, detail = e))?;

        if !response.status().is_success() {
            return Err(error!(
                "API returned error status",
                url = url,
                status = response.status().as_u16()
            ));
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| error!("Failed to parse API response as JSON", detail = e))?;

        let data = body["data"]
            .as_array()
            .ok_or_else(|| error!("API response missing 'data' array", url = url))?;

        let mut records = IndexMap::new();

        for (row_idx, item) in data.iter().enumerate() {
            let obj = item
                .as_object()
                .ok_or_else(|| error!("API data item is not an object", index = row_idx))?;

            // Extract ID from the configured id_field, or use row index
            let id = id_field
                .and_then(|field| obj.get(field))
                .and_then(|v| match v {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .unwrap_or_else(|| row_idx.to_string());

            let record: Record<serde_json::Value> = obj
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            records.insert(id, record);
        }

        Ok(records)
    }
}
