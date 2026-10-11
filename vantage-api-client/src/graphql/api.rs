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

use std::sync::Arc;

use indexmap::IndexMap;
use serde::Serialize;
use serde_json::Value;
use vantage_api_pool::resilient::{ResilientClient, TransportEvent, TransportObserver};
use vantage_core::{Priority, Result, error};

use crate::graphql::condition::FilterDialect;
use crate::transport::AuthHeader;

/// GraphQL HTTP data source. Cheap to clone — the inner `ResilientClient`
/// is `Arc`-wrapped.
///
/// `dialect` and `filter_arg_name` drive how the `TableSource` impl
/// renders filter arguments — Hasura's `where:` vs SpaceX-style `find:`,
/// etc. Both default to the dialect's natural choice (Generic + `find`).
#[derive(Clone, Debug)]
pub struct GraphqlApi {
    endpoint: String,
    client: ResilientClient,
    auth_header: AuthHeader,
    pub(crate) dialect: FilterDialect,
    pub(crate) filter_arg_name: Option<String>,
    pub(crate) root_args: Option<Value>,
    pub(crate) response_path: Vec<String>,
    pub(crate) supports: Supports,
    pub(crate) count_field: Option<String>,
    /// Column name → server field name, from a spec column's
    /// `graphql: { field: … }`. Set per table, like the overrides above.
    pub(crate) field_aliases: IndexMap<String, String>,
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
///
/// Offset paging is advertised only when both `paginate` and `filter` are
/// on: with filtering done locally, a server page would be cut down after
/// the fact and come back short.
///
/// Build one from [`Supports::default`] with the `with_*` setters:
///
/// ```
/// use vantage_api_client::{CountMode, Supports};
///
/// let supports = Supports::default()
///     .with_filter(false)
///     .with_count(CountMode::Aggregate);
/// assert_eq!(supports.filter, Some(false));
/// ```
#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct Supports {
    /// Conditions may be pushed into the filter argument. Default: on.
    pub filter: Option<bool>,
    /// `order_by:` may be rendered. Default: on for Hasura only.
    pub order: Option<bool>,
    /// A quicksearch may render as an OR of `_ilike`s. Default: on for
    /// Hasura only, and never without `filter`.
    pub search: Option<bool>,
    /// `limit:`/`offset:` may be rendered. Default: off.
    pub paginate: Option<bool>,
    /// How a count is taken; see [`CountMode`].
    pub count: CountMode,
}

impl Supports {
    /// Set [`filter`](Self::filter).
    pub fn with_filter(mut self, on: bool) -> Self {
        self.filter = Some(on);
        self
    }

    /// Set [`order`](Self::order).
    pub fn with_order(mut self, on: bool) -> Self {
        self.order = Some(on);
        self
    }

    /// Set [`search`](Self::search).
    pub fn with_search(mut self, on: bool) -> Self {
        self.search = Some(on);
        self
    }

    /// Set [`paginate`](Self::paginate).
    pub fn with_paginate(mut self, on: bool) -> Self {
        self.paginate = Some(on);
        self
    }

    /// Set [`count`](Self::count).
    pub fn with_count(mut self, count: CountMode) -> Self {
        self.count = count;
        self
    }
}

/// How `get_count` is answered. YAML: `count: list | aggregate`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum CountMode {
    /// List the rows and count them. Works on every schema and for every
    /// role.
    #[default]
    List,
    /// Hasura only: one `<count_field> { aggregate { count } }` query.
    /// Needs a role with `allow_aggregations`; a server that refuses it
    /// fails the count rather than falling back to listing. On the Generic
    /// dialect, or with `filter: false`, the count still lists.
    Aggregate,
}

impl GraphqlApi {
    /// Whether conditions may be pushed into the query. Defaults to true —
    /// most list fields take some filter argument.
    pub fn can_filter(&self) -> bool {
        self.supports.filter.unwrap_or(true)
    }

    /// Whether `order_by:` may be rendered. Only the Hasura dialect renders
    /// a sort, so Generic defaults to sorting client-side.
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

    /// Whether offset paging can be offered to a consumer: `limit:` and
    /// `offset:` render, and so do the conditions, so a page is not cut
    /// down by local filtering after the server has sized it.
    pub fn can_page(&self) -> bool {
        self.can_paginate() && self.can_filter()
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

    /// Whether a count runs as a server-side aggregate instead of listing
    /// every row: `count: aggregate` on Hasura, and only when conditions
    /// reach the server — otherwise the aggregate would count rows the
    /// local filter drops.
    pub(crate) fn can_count_server_side(&self) -> bool {
        self.supports.count == CountMode::Aggregate
            && matches!(self.dialect, FilterDialect::Hasura)
            && self.can_filter()
    }

    /// Aggregate field queried for a server-side count of `root_field`:
    /// the configured override, else Hasura's `<root_field>_aggregate`.
    pub(crate) fn count_field(&self, root_field: &str) -> String {
        self.count_field
            .clone()
            .unwrap_or_else(|| format!("{root_field}_aggregate"))
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

    /// What the circuit breaker is doing right now. `None` when the
    /// underlying client has no breaker configured.
    pub fn breaker_state(&self) -> Option<crate::BreakerState> {
        self.client.breaker_state()
    }

    /// The resilient client backing this API — the pool, breaker and
    /// observer a caller outside the read path (e.g. an outbox replaying a
    /// queued write) should share rather than build its own.
    pub fn client(&self) -> &ResilientClient {
        &self.client
    }

    /// Report rows decoded from a response to the observer.
    pub(crate) fn report_rows(&self, n: usize) {
        self.client.report(TransportEvent::rows_pulled(n));
    }

    /// Send a query document with variables. Returns the `data` payload
    /// from the GraphQL response, or an error if the request failed or
    /// the response carried a top-level `errors` array.
    pub async fn post_graphql(
        &self,
        query: &str,
        variables: &serde_json::Map<String, Value>,
    ) -> Result<Value> {
        match self.post_graphql_reply(query, variables).await? {
            GraphqlReply::Data(data) => Ok(data),
            GraphqlReply::Errors(summary) => {
                Err(error!("GraphQL response carried errors", errors = summary))
            }
        }
    }

    /// [`post_graphql`](Self::post_graphql), with a top-level `errors`
    /// array kept apart from transport and decode failures, so a caller
    /// can say what a rejected query means.
    pub(crate) async fn post_graphql_reply(
        &self,
        query: &str,
        variables: &serde_json::Map<String, Value>,
    ) -> Result<GraphqlReply> {
        #[derive(Serialize)]
        struct Body<'a> {
            query: &'a str,
            variables: &'a serde_json::Map<String, Value>,
        }

        let body = Body { query, variables };

        let policy = crate::transport::policy_for(Priority::current());
        let response = self
            .client
            .execute_with(&policy, |http| {
                let req = http.post(&self.endpoint).json(&body);
                match self.auth_header.value() {
                    Some(auth) => req.header(reqwest::header::AUTHORIZATION, auth),
                    None => req,
                }
            })
            .await
            .map_err(|e| {
                crate::transport::client_error(e, "GraphQL request failed", &self.endpoint)
            })?;

        let mut envelope: Value = response.json().await.map_err(|e| {
            error!(
                "Failed to parse GraphQL response as JSON",
                detail = e.to_string()
            )
        })?;

        // GraphQL servers return `{ "data": …, "errors": [...] }`.
        if let Some(errors) = envelope.get("errors")
            && let Some(arr) = errors.as_array()
            && !arr.is_empty()
        {
            let summary = arr
                .iter()
                .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                .collect::<Vec<_>>()
                .join("; ");
            return Ok(GraphqlReply::Errors(summary));
        }

        Ok(GraphqlReply::Data(
            envelope
                .get_mut("data")
                .map(std::mem::take)
                .unwrap_or(Value::Null),
        ))
    }
}

/// A GraphQL response that arrived and decoded: its `data`, or the
/// messages of its top-level `errors` joined with `; `.
pub(crate) enum GraphqlReply {
    Data(Value),
    Errors(String),
}

/// Builder for [`GraphqlApi`]. Use [`GraphqlApi::builder`] to start.
#[derive(Clone, Debug)]
pub struct GraphqlApiBuilder {
    endpoint: String,
    transport: crate::transport::ClientConfig,
    auth_header: AuthHeader,
    dialect: FilterDialect,
    filter_arg_name: Option<String>,
    root_args: Option<Value>,
    response_path: Vec<String>,
    supports: Supports,
    count_field: Option<String>,
}

impl GraphqlApiBuilder {
    pub(crate) fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            transport: crate::transport::ClientConfig::default(),
            auth_header: AuthHeader::default(),
            dialect: FilterDialect::Generic,
            filter_arg_name: None,
            root_args: None,
            response_path: Vec::new(),
            supports: Supports::default(),
            count_field: None,
        }
    }

    /// Concurrent requests to this API at most (default 4).
    pub fn max_parallel(mut self, n: usize) -> Self {
        self.transport.max_parallel = n.max(1);
        self
    }

    /// Requests per second to this API at most.
    pub fn rate_limit(mut self, per_second: f64) -> Self {
        self.transport.rate_limit = Some(per_second);
        self
    }

    /// Give up on one request after `timeout` (default 30 s). Ignored when a
    /// client is supplied with [`http_client`](Self::http_client).
    pub fn request_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.transport.request_timeout = Some(Some(timeout));
        self
    }

    /// Never time a request out. Ignored when a client is supplied.
    pub fn no_request_timeout(mut self) -> Self {
        self.transport.request_timeout = Some(None);
        self
    }

    /// Report every attempt, retry and breaker transition under `key`
    /// (the datasource name).
    pub fn observer(
        mut self,
        key: impl Into<Arc<str>>,
        observer: Arc<dyn TransportObserver>,
    ) -> Self {
        self.transport.observer = Some((key.into(), observer));
        self
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

    /// Override the Hasura aggregate field used by `count: aggregate`
    /// (default `<root_field>_aggregate`).
    pub fn count_field(mut self, field: impl Into<String>) -> Self {
        self.count_field = Some(field.into());
        self
    }

    /// Set the `Authorization` header value (e.g. `"Bearer <token>"`).
    pub fn auth(mut self, auth: impl Into<String>) -> Self {
        self.auth_header = AuthHeader::new(auth);
        self.transport.auth_refresher = None;
        self
    }

    /// Get the bearer token from `refresher` instead of a fixed header: it
    /// is asked on the first request, and again when the API answers `401`.
    /// A request waits for it, so a refresher may run an interactive
    /// sign-in. Replaces [`auth`](Self::auth).
    pub fn auth_refresher(mut self, refresher: crate::AuthRefresher) -> Self {
        self.auth_header = AuthHeader::default();
        self.transport.auth_refresher = Some(refresher);
        self
    }

    /// A pre-configured `reqwest::Client` (timeouts, proxies, TLS).
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.transport.http = Some(client);
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

    /// Finish the configuration. Builds no HTTP client yet: unless one was
    /// given with [`http_client`](Self::http_client), the default client is
    /// created on the first request.
    pub fn build(self) -> GraphqlApi {
        GraphqlApi {
            endpoint: self.endpoint,
            client: crate::transport::build_client(self.transport),
            auth_header: self.auth_header,
            dialect: self.dialect,
            filter_arg_name: self.filter_arg_name,
            root_args: self.root_args,
            response_path: self.response_path,
            supports: self.supports,
            count_field: self.count_field,
            field_aliases: IndexMap::new(),
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

    #[test]
    fn debug_masks_auth_header() {
        let api = GraphqlApi::builder("https://example.test/graphql")
            .auth("Bearer secret-token")
            .build();
        let text = format!("{api:?}");
        assert!(!text.contains("secret-token"), "{text}");
        assert!(text.contains("<set>"), "{text}");
    }
}
