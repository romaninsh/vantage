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

use crate::graphql::condition::FilterDialect;

/// GraphQL HTTP data source. Cheap to clone — the inner `reqwest::Client`
/// is `Arc`-wrapped.
///
/// `dialect` and `filter_arg_name` drive how the `TableSource` impl
/// renders filter arguments — Hasura's `where:` vs SpaceX-style `find:`,
/// etc. Both default to the dialect's natural choice (Generic + `find`).
#[derive(Clone, Debug)]
pub struct GraphqlApi {
    endpoint: String,
    client: reqwest::Client,
    auth_header: Option<String>,
    pub(crate) dialect: FilterDialect,
    pub(crate) filter_arg_name: Option<String>,
    pub(crate) root_args: Option<Value>,
    pub(crate) response_path: Vec<String>,
    pub(crate) supports: Supports,
}

/// Per-table overrides for what the server will actually accept, each
/// `None` meaning "use the dialect's default".
///
/// These exist because a GraphQL endpoint is not uniform: on one schema
/// the list field takes `where`/`order_by`/`limit`, on the next it takes
/// no arguments at all. Spacelift's `stacks` is the second kind, and
/// pushing a filter at it renders a query the server rejects outright —
/// so this is what stops equality push-down, which Vista otherwise
/// assumes every driver supports.
#[derive(Clone, Copy, Debug, Default)]
pub struct Supports {
    pub filter: Option<bool>,
    pub order: Option<bool>,
    pub search: Option<bool>,
    pub paginate: Option<bool>,
}

impl GraphqlApi {
    /// Whether conditions may be pushed into the query. Defaults to true —
    /// most list fields take some filter argument.
    pub fn can_filter(&self) -> bool {
        self.supports.filter.unwrap_or(true)
    }

    /// Whether `order_by:` may be rendered. Only Hasura has a spelling for
    /// it today, so Generic defaults to sorting client-side.
    pub fn can_order(&self) -> bool {
        self.supports
            .order
            .unwrap_or(matches!(self.dialect, FilterDialect::Hasura))
    }

    /// Whether a quicksearch renders as an OR of `_ilike`s. Same story as
    /// ordering: Hasura only. A search *is* a condition, so it also needs
    /// filter push-down to be on.
    pub fn can_search(&self) -> bool {
        self.can_filter()
            && self
                .supports
                .search
                .unwrap_or(matches!(self.dialect, FilterDialect::Hasura))
    }

    /// Whether operators richer than equality can be rendered. Generic
    /// rejects them at render time, so only Hasura qualifies.
    pub fn can_filter_operators(&self) -> bool {
        self.can_filter() && matches!(self.dialect, FilterDialect::Hasura)
    }

    /// Whether `limit:`/`offset:` may be rendered. Off by default: a
    /// schema that doesn't take them turns every query into an error, and
    /// the cost of not paging is one extra round of rows.
    pub fn can_paginate(&self) -> bool {
        self.supports.paginate.unwrap_or(false)
    }

    /// Path walked into the root field's value before rows are read —
    /// `["edges", "node"]` for a Relay-style connection. Empty means the
    /// root field's value is the row array itself.
    pub fn response_path(&self) -> &[String] {
        &self.response_path
    }

    /// Literal arguments always passed to the root field.
    pub fn root_args(&self) -> Option<&Value> {
        self.root_args.as_ref()
    }
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

    /// Filter dialect — controls how `where:` / `find:` arguments are
    /// rendered. Defaults to [`FilterDialect::Generic`].
    pub fn dialect(&self) -> FilterDialect {
        self.dialect
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

        let response = req.send().await.map_err(|e| {
            error!(
                "GraphQL request failed",
                endpoint = self.endpoint.clone(),
                detail = e.to_string()
            )
        })?;

        if !response.status().is_success() {
            return Err(error!(
                "GraphQL endpoint returned error status",
                endpoint = self.endpoint.clone(),
                status = response.status().as_u16()
            ));
        }

        let mut envelope: Value = response.json().await.map_err(|e| {
            error!(
                "Failed to parse GraphQL response as JSON",
                detail = e.to_string()
            )
        })?;

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
    dialect: FilterDialect,
    filter_arg_name: Option<String>,
    root_args: Option<Value>,
    response_path: Vec<String>,
    supports: Supports,
}

impl GraphqlApiBuilder {
    pub(crate) fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            client: None,
            auth_header: None,
            dialect: FilterDialect::Generic,
            filter_arg_name: None,
            root_args: None,
            response_path: Vec::new(),
            supports: Supports::default(),
        }
    }

    /// Literal arguments always passed to the root field, e.g.
    /// `json!({ "input": {} })` for a mandatory non-null input object.
    pub fn root_args(mut self, args: Value) -> Self {
        self.root_args = Some(args);
        self
    }

    /// Dotted path walked into the root field's value before rows are
    /// read — `"edges.node"` unwraps a Relay-style connection.
    pub fn response_path(mut self, path: impl AsRef<str>) -> Self {
        self.response_path = split_response_path(path.as_ref());
        self
    }

    /// Override what the server accepts; see [`Supports`].
    pub fn supports(mut self, supports: Supports) -> Self {
        self.supports = supports;
        self
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

    /// Pick the filter dialect used to render conditions on tables.
    /// Defaults to [`FilterDialect::Generic`] — flat-arg schemas like SpaceX.
    pub fn dialect(mut self, dialect: FilterDialect) -> Self {
        self.dialect = dialect;
        self
    }

    /// Override the filter argument name. Defaults match the dialect:
    /// `"where"` for Hasura, `"find"` for Generic.
    pub fn filter_arg_name(mut self, name: impl Into<String>) -> Self {
        self.filter_arg_name = Some(name.into());
        self
    }

    pub fn build(self) -> GraphqlApi {
        GraphqlApi {
            endpoint: self.endpoint,
            client: self.client.unwrap_or_default(),
            auth_header: self.auth_header,
            dialect: self.dialect,
            filter_arg_name: self.filter_arg_name,
            root_args: self.root_args,
            response_path: self.response_path,
            supports: self.supports,
        }
    }
}

/// Split a dotted response path, dropping empty segments so a stray
/// leading or trailing dot doesn't produce a lookup for `""`.
pub(crate) fn split_response_path(path: &str) -> Vec<String> {
    path.split('.')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
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
