//! The HTTP half of the protocol: schema, SQL, and reducer calls.
//!
//! SpacetimeDB's WebSocket API is the optimised path and carries subscriptions;
//! these endpoints cover everything else and are what a dynamic client needs,
//! since none of them require generated bindings.

use vantage_core::{Result, error};

use super::Inner;
use crate::schema::SchemaVersion;

/// `GET /v1/database/:db/schema?version=N`, returned as raw JSON.
///
/// Left undecoded so the caller picks the ABI struct — the two versions have
/// incompatible shapes, and which one we asked for is what decides.
pub(crate) async fn schema_json(inner: &Inner, version: SchemaVersion) -> Result<String> {
    let url = format!(
        "{}/v1/database/{}/schema?version={}",
        inner.base_url,
        inner.database,
        version.query_value()
    );
    get_text(inner, &url, "schema").await
}

/// `POST /v1/database/:db/sql`, returned as raw JSON.
///
/// Accepts several statements separated by semicolons and answers with one
/// result per statement, each carrying its own `schema` (a `ProductType`) and
/// `rows`.
pub(crate) async fn sql_json(inner: &Inner, statement: &str) -> Result<String> {
    let url = format!("{}/v1/database/{}/sql", inner.base_url, inner.database);
    let mut request = inner
        .http
        .post(&url)
        .header(reqwest::header::CONTENT_TYPE, "text/plain")
        .body(statement.to_string());
    if let Some(token) = &inner.token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|e| {
        error!(
            "SpacetimeDB SQL request failed",
            url = url.clone(),
            error = e.to_string()
        )
    })?;
    read_body(response, "sql", &url, Some(statement)).await
}

/// `POST /v1/database/:db/call/:reducer` with a JSON array of arguments.
///
/// The idiomatic write path: a reducer enforces the module's own rules, which
/// raw DML bypasses.
pub(crate) async fn call_reducer(
    inner: &Inner,
    reducer: &str,
    args: &serde_json::Value,
) -> Result<String> {
    if !args.is_array() {
        return Err(error!(
            "SpacetimeDB reducer arguments must be a JSON array, positionally matching the \
             reducer's parameters",
            reducer = reducer
        ));
    }
    let url = format!(
        "{}/v1/database/{}/call/{}",
        inner.base_url, inner.database, reducer
    );
    let mut request = inner.http.post(&url).json(args);
    if let Some(token) = &inner.token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|e| {
        error!(
            "SpacetimeDB reducer call failed",
            reducer = reducer.to_string(),
            error = e.to_string()
        )
    })?;
    read_body(response, "call", &url, None).await
}

async fn get_text(inner: &Inner, url: &str, what: &str) -> Result<String> {
    let mut request = inner.http.get(url);
    if let Some(token) = &inner.token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|e| {
        error!(
            "SpacetimeDB request failed",
            what = what.to_string(),
            url = url.to_string(),
            error = e.to_string()
        )
    })?;
    read_body(response, what, url, None).await
}

/// Turn a response into a body, or into an error that names the status *and*
/// quotes the host's own message.
///
/// Worth the care: SpacetimeDB answers a bad SQL statement with a 400 whose body
/// holds the actual parse error, and discarding it in favour of "400 Bad
/// Request" would throw away the only useful part.
async fn read_body(
    response: reqwest::Response,
    what: &str,
    url: &str,
    statement: Option<&str>,
) -> Result<String> {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if status.is_success() {
        return Ok(body);
    }

    let hint = match status {
        reqwest::StatusCode::NOT_FOUND => {
            " — check the database name, and that the module is published to this host"
        }
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            " — anonymous callers may only read public tables; supply a token for writes and \
             private data"
        }
        _ => "",
    };

    Err(error!(
        format!(
            "SpacetimeDB {} request returned {}{}: {}",
            what,
            status,
            hint,
            body.trim()
        ),
        what = what.to_string(),
        url = url.to_string(),
        status = status.as_u16() as i64,
        statement = statement.unwrap_or_default().to_string()
    ))
}
