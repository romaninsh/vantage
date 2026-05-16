//! `GraphqlApi` — the data source struct.
//!
//! Wraps a single HTTP endpoint and a `reqwest` client. Each query goes
//! out as one POST with `{ "query": …, "variables": {…} }` and the JSON
//! `data` payload comes back as `serde_json::Value`. Higher layers
//! (`GraphqlSelect`, `TableSource`) build the request body and parse the
//! response.
//!
//! The query language itself is handled by the query builder in the
//! `select` module — `GraphqlApi` is just transport.

use serde::Serialize;
use serde_json::Value;
use vantage_core::{Result, error};

/// GraphQL HTTP data source. Cheap to clone — the inner `reqwest::Client`
/// is `Arc`-wrapped.
#[derive(Clone, Debug)]
pub struct GraphqlApi {
    endpoint: String,
    client: reqwest::Client,
    auth_header: Option<String>,
}

impl GraphqlApi {
    /// Connect to a GraphQL endpoint at `endpoint` (e.g.
    /// `https://api.spacex.land/graphql/`). Uses the default reqwest
    /// client; for finer control go through [`GraphqlApi::builder`].
    pub fn new(endpoint: impl Into<String>) -> Self {
        GraphqlApi::builder(endpoint).build()
    }

    /// Start configuring a [`GraphqlApi`].
    pub fn builder(endpoint: impl Into<String>) -> GraphqlApiBuilder {
        GraphqlApiBuilder::new(endpoint.into())
    }

    /// Endpoint URL the client posts to.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Send a query document with variables. Returns the `data` payload
    /// from the GraphQL response, or an error if the request failed or
    /// the response carried a top-level `errors` array.
    pub async fn post_graphql(
        &self,
        query: &str,
        variables: &serde_json::Map<String, Value>,
    ) -> Result<Value> {
        #[derive(Serialize)]
        struct Body<'a> {
            query: &'a str,
            variables: &'a serde_json::Map<String, Value>,
        }

        let body = Body { query, variables };

        let mut req = self.client.post(&self.endpoint).json(&body);
        if let Some(ref auth) = self.auth_header {
            req = req.header("Authorization", auth);
        }

        let response = req
            .send()
            .await
            .map_err(|e| error!("GraphQL request failed", endpoint = self.endpoint.clone(), detail = e.to_string()))?;

        if !response.status().is_success() {
            return Err(error!(
                "GraphQL endpoint returned error status",
                endpoint = self.endpoint.clone(),
                status = response.status().as_u16()
            ));
        }

        let mut envelope: Value = response
            .json()
            .await
            .map_err(|e| error!("Failed to parse GraphQL response as JSON", detail = e.to_string()))?;

        // GraphQL servers return `{ "data": …, "errors": [...] }`. Surface
        // any errors as a Vantage error and otherwise hand back `data`.
        if let Some(errors) = envelope.get("errors")
            && let Some(arr) = errors.as_array()
            && !arr.is_empty()
        {
            let summary = arr
                .iter()
                .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(error!("GraphQL response carried errors", errors = summary));
        }

        Ok(envelope
            .get_mut("data")
            .map(std::mem::take)
            .unwrap_or(Value::Null))
    }
}

/// Builder for [`GraphqlApi`]. Use [`GraphqlApi::builder`] to start.
#[derive(Debug, Clone)]
pub struct GraphqlApiBuilder {
    endpoint: String,
    client: Option<reqwest::Client>,
    auth_header: Option<String>,
}

impl GraphqlApiBuilder {
    pub(crate) fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            client: None,
            auth_header: None,
        }
    }

    /// Set the `Authorization` header value (e.g. `"Bearer <token>"`).
    pub fn auth(mut self, auth: impl Into<String>) -> Self {
        self.auth_header = Some(auth.into());
        self
    }

    /// Use a pre-configured `reqwest::Client` (e.g. one with custom
    /// timeouts or a proxy).
    pub fn client(mut self, client: reqwest::Client) -> Self {
        self.client = Some(client);
        self
    }

    pub fn build(self) -> GraphqlApi {
        GraphqlApi {
            endpoint: self.endpoint,
            client: self.client.unwrap_or_default(),
            auth_header: self.auth_header,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_keeps_endpoint() {
        let api = GraphqlApi::new("https://api.spacex.land/graphql/");
        assert_eq!(api.endpoint(), "https://api.spacex.land/graphql/");
    }

    #[test]
    fn builder_sets_auth_without_panicking() {
        // Auth header is private — this just confirms the builder chain
        // compiles end-to-end and produces a usable client.
        let api = GraphqlApi::builder("https://example.test/graphql")
            .auth("Bearer abc")
            .build();
        assert_eq!(api.endpoint(), "https://example.test/graphql");
    }
}
