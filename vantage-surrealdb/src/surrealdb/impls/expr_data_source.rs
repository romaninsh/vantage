use ciborium::Value as CborValue;
use surreal_client::{SurrealClient, SurrealError};
use vantage_core::{Context, Result};
use vantage_expressions::{DeferredFn, ExprDataSource, Expression};

use crate::{AnySurrealType, SurrealType, surrealdb::SurrealDB};

impl ExprDataSource<AnySurrealType> for SurrealDB {
    /// On the shared connection, or in this clone's transaction, where a
    /// failed statement fails the commit.
    async fn execute(&self, expr: &Expression<AnySurrealType>) -> Result<AnySurrealType> {
        let resolved = expr.resolve_deferred().await?;
        let (query_str, params) = self.prepare_query(&resolved);
        let params_cbor = params.to_cbor();
        match self.tx() {
            None => {
                let client = self.inner.lock().await;
                run_query(&client, None, &query_str, params_cbor).await
            }
            Some(tx) => {
                let open = tx.open().await?;
                tx.watch(run_query(
                    &open.client,
                    Some(&open.txn),
                    &query_str,
                    params_cbor,
                ))
                .await
            }
        }
    }

    fn defer(&self, expr: Expression<AnySurrealType>) -> DeferredFn<AnySurrealType> {
        let client = self.clone();
        DeferredFn::from_fn(move || {
            let client = client.clone();
            let expr = expr.clone();
            Box::pin(async move { client.execute(&expr).await })
        })
    }
}

/// Sends one statement, inside the transaction `txn` when given, and unwraps
/// its one response: the result, or the statement's error.
async fn run_query(
    client: &SurrealClient,
    txn: Option<&CborValue>,
    query_str: &str,
    params_cbor: CborValue,
) -> Result<AnySurrealType> {
    if let Some(tag) = unknown_tag(&params_cbor) {
        return Err(vantage_core::error!(
            "SurrealDB can't decode a parameter carrying this CBOR tag",
            tag = tag,
            expression = query_str.to_string()
        )
        .mark_invalid_value());
    }
    let sent = match txn {
        None => client.query_cbor(query_str, params_cbor).await,
        Some(txn) => client.query_cbor_in(txn, query_str, params_cbor).await,
    };
    let unavailable = sent.as_ref().is_err_and(is_transport_failure);
    let result = sent
        .map_err(|e| match e {
            SurrealError::ServerError { code, message } => {
                vantage_core::error!(
                    "SurrealDB server error",
                    error_code = code,
                    server_message = message
                )
            }
            _ => {
                vantage_core::error!("SurrealDB connection error", error_details = e.to_string())
            }
        })
        .context(vantage_core::error!(
            "Failed to execute expression",
            expression = query_str.to_string()
        ))
        .map_err(|err| {
            if unavailable {
                err.mark_unavailable()
            } else {
                err
            }
        })?;

    // One statement is sent, so exactly one response comes back.
    let responses = result
        .as_array()
        .ok_or_else(|| vantage_core::error!("Expected array response from SurrealDB"))?;

    if responses.len() != 1 {
        return Err(vantage_core::error!(
            "Expected single response from SurrealDB",
            response_count = responses.len()
        ));
    }

    let response = responses.first().unwrap();
    let response_map = response
        .as_map()
        .ok_or_else(|| vantage_core::error!("Expected map response from SurrealDB"))?;

    let is_success = response_map
        .iter()
        .find(|(k, _)| matches!(k, ciborium::Value::Text(key) if key == "status"))
        .and_then(|(_, v)| match v {
            ciborium::Value::Text(status) => Some(status == "OK"),
            _ => None,
        })
        .unwrap_or(false);

    if is_success {
        let extracted_result = response_map
            .iter()
            .find(|(k, _)| matches!(k, ciborium::Value::Text(key) if key == "result"))
            .map(|(_, v)| v)
            .ok_or_else(|| {
                vantage_core::error!(
                    "Successful SurrealDB response missing result field",
                    response_payload = format!("{:?}", response)
                )
            })?;

        AnySurrealType::from_cbor(extracted_result).ok_or_else(|| {
            vantage_core::error!("Failed to convert SurrealDB result to AnySurrealType")
        })
    } else {
        // A failed response carries its message under "result".
        let error_message = response_map
            .iter()
            .find(|(k, _)| matches!(k, ciborium::Value::Text(key) if key == "result"))
            .and_then(|(_, v)| match v {
                ciborium::Value::Text(msg) => Some(msg.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "Unknown SurrealDB error".to_string());

        Err(vantage_core::error!(
            "SurrealDB query failed",
            query_error = error_message,
            error_type = "query_execution"
        ))
    }
}

/// The server was not reached or the socket went away: a dropped or refused
/// connection, a reply that never came, a reconnect still backing off. A
/// statement the server answered with an error is a refusal, never this.
pub(crate) fn is_transport_failure(err: &SurrealError) -> bool {
    match err {
        SurrealError::Connection(_)
        | SurrealError::Timeout(_)
        | SurrealError::WebSocket(_)
        | SurrealError::Io(_) => true,
        SurrealError::Network(e) => {
            e.is_connect() || e.is_timeout() || e.status().is_some_and(|s| s.is_server_error())
        }
        SurrealError::Auth(_)
        | SurrealError::Serialization(_)
        | SurrealError::ServerError { .. }
        | SurrealError::Protocol(_)
        | SurrealError::Json(_)
        | SurrealError::UrlParse(_) => false,
    }
}

/// The CBOR tags SurrealDB's RPC decoder takes: NONE, table, record id,
/// uuid, decimal, datetime, duration, future, range and its bounds,
/// geometries.
const SURREAL_TAGS: &[u64] = &[
    6, 7, 8, 9, 10, 12, 13, 14, 15, 37, 49, 50, 51, 88, 89, 90, 91, 92, 93, 94,
];

/// The first tag in `value` SurrealDB can't decode. The server answers such
/// a request with an error that carries no request id, so the request would
/// wait out its deadline instead of failing.
fn unknown_tag(value: &CborValue) -> Option<u64> {
    match value {
        CborValue::Tag(tag, _) if !SURREAL_TAGS.contains(tag) => Some(*tag),
        CborValue::Tag(_, inner) => unknown_tag(inner),
        CborValue::Array(items) => items.iter().find_map(unknown_tag),
        CborValue::Map(entries) => entries
            .iter()
            .find_map(|(k, v)| unknown_tag(k).or_else(|| unknown_tag(v))),
        _ => None,
    }
}
