//! Sending a table's inserts: the `api.insert` block lowered to one request
//! shape against a [`RestApi`].

mod template;

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use flate2::Compression;
use flate2::write::GzEncoder;
use indexmap::IndexMap;
use reqwest::Method;
use serde_json::Value;
use uuid::Uuid;
use vantage_api_pool::resilient::{CallPolicy, ErrorKind};
use vantage_core::{Result, VantageError, error};
use vantage_types::{PlainDialect, Record, record_to_json};

use self::template::BodyTemplate;
use super::RestApi;
use super::api::join_base_path;
use super::vista::spec::{ApiInsertBlock, Encoding};
use crate::redact::{MASK, Redaction};
use crate::transport::client_error;

/// An insert target: where a table's records are POSTed and which statuses
/// count as success. Built by [`RestApi::insert_endpoint`].
#[derive(Clone)]
pub struct RestInsertEndpoint {
    api: RestApi,
    path: String,
    query: Vec<(String, String)>,
    accept: Vec<u16>,
    batch: usize,
    group_by: Option<String>,
    body: Option<BodyTemplate>,
    encoding: Option<Encoding>,
    idempotency_key: Option<String>,
    idempotent: bool,
    secrets: IndexMap<String, String>,
}

impl std::fmt::Debug for RestInsertEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let query: Vec<String> = self
            .query
            .iter()
            .map(|(name, _)| format!("{name}={MASK}"))
            .collect();
        f.debug_struct("RestInsertEndpoint")
            .field("api", &self.api)
            .field("path", &self.path)
            .field("query", &query)
            .field("accept", &self.accept)
            .field("batch", &self.batch)
            .field("group_by", &self.group_by)
            .field("body", &self.body)
            .field("encoding", &self.encoding)
            .field("idempotency_key", &self.idempotency_key)
            .field("idempotent", &self.idempotent)
            .field("secrets", &self.secrets.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// A request ready to send: the exact bytes and the headers that go with
/// them.
#[derive(Debug)]
struct RenderedRequest {
    body: Vec<u8>,
    headers: Vec<(String, String)>,
}

/// A failed attempt and what it says about trying again.
struct Failure {
    error: VantageError,
    retryable: bool,
    retry_after: Option<Duration>,
}

/// Attempts per request, the first included.
const MAX_ATTEMPTS: usize = 5;
/// The wait before the first retry; it doubles for each one after.
const BASE_BACKOFF: Duration = Duration::from_secs(1);
/// The longest a server's `Retry-After` is obeyed.
const RETRY_AFTER_CAP: Duration = Duration::from_secs(30);

fn is_retryable(status: u16) -> bool {
    status == 429 || (500..600).contains(&status)
}

/// The `api.insert` target a table's own copy of the data source carries,
/// built once with the table.
#[derive(Clone, Debug)]
pub(crate) struct TableInsert {
    pub(crate) endpoint: RestInsertEndpoint,
}

impl RestApi {
    /// A copy of this data source for one table: it carries that table's
    /// insert settings, and refuses every read when `read` is false.
    pub(crate) fn for_table(&self, insert: Option<TableInsert>, read: bool) -> Self {
        let mut api = self.clone();
        api.insert = insert.map(Arc::new);
        api.readable = read;
        api
    }

    pub(crate) fn has_insert(&self) -> bool {
        self.insert.is_some()
    }

    pub(crate) fn is_readable(&self) -> bool {
        self.readable
    }

    /// The configured insert target, or an unsupported error when the table
    /// has no `api.insert`.
    pub(crate) fn table_insert(&self) -> Result<RestInsertEndpoint> {
        match &self.insert {
            Some(t) => Ok(t.endpoint.clone()),
            None => Err(error!("REST table has no api.insert block").mark_unsupported()),
        }
    }

    /// The insert target for `block` over the table at `endpoint`. The block's
    /// `path` replaces `endpoint` when set. Fails unless the method is `POST`.
    pub fn insert_endpoint(
        &self,
        block: &ApiInsertBlock,
        endpoint: &str,
    ) -> Result<RestInsertEndpoint> {
        if !block.method.eq_ignore_ascii_case("POST") {
            return Err(error!(
                "api.insert.method must be POST",
                method = block.method.as_str()
            ));
        }
        if block.batch == 0 {
            return Err(error!("api.insert.batch must be at least 1"));
        }
        let body = block
            .body
            .as_deref()
            .map(BodyTemplate::compile)
            .transpose()?;
        Ok(RestInsertEndpoint {
            api: self.clone(),
            path: block.path.clone().unwrap_or_else(|| endpoint.to_string()),
            query: block
                .query
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            accept: block.accept.clone(),
            batch: block.batch,
            group_by: block.group_by.clone(),
            body,
            encoding: block.encoding,
            idempotency_key: block.idempotency_key.clone(),
            idempotent: block.idempotent,
            secrets: IndexMap::new(),
        })
    }
}

impl RestInsertEndpoint {
    /// The secrets the body template reads as `secret.<name>`. They are
    /// bound as data, never substituted into the template source.
    pub fn with_secrets(mut self, secrets: IndexMap<String, String>) -> Self {
        self.secrets = secrets;
        self
    }

    /// Every credential this target can send: the data source's query
    /// auth, each `api.insert.query` value (any of them may be a key) and
    /// the body secrets.
    fn redaction(&self) -> Redaction {
        let redaction = self
            .query
            .iter()
            .fold(self.api.redaction(), |r, (name, value)| {
                r.param(name.clone()).value(value.clone())
            });
        self.secrets
            .values()
            .fold(redaction, |r, secret| r.value(secret.clone()))
    }

    /// [`Self::send_with_ids`] with a fresh UUIDv7 per record.
    pub async fn send(&self, records: &[Record<CborValue>]) -> Result<()> {
        let ids: Vec<Uuid> = records.iter().map(|_| Uuid::now_v7()).collect();
        self.send_with_ids(records, &ids).await
    }

    /// POST `records` in requests of at most `batch` records, never mixing
    /// two `group_by` values. Stops at the first failure.
    ///
    /// `ids` holds one id per record, in order. With `idempotency_key` set,
    /// each request carries its first record's id in that header. A request
    /// is rendered once and its bytes are reused on every retry; retries
    /// happen only when `idempotency_key` or `idempotent` says a repeat is
    /// safe.
    pub async fn send_with_ids(&self, records: &[Record<CborValue>], ids: &[Uuid]) -> Result<()> {
        if records.len() != ids.len() {
            return Err(error!(
                "Insert needs one id per record",
                records = records.len(),
                ids = ids.len()
            ));
        }
        let records: Vec<(Value, Uuid)> = records
            .iter()
            .zip(ids)
            .map(|(r, id)| (record_to_json(&PlainDialect, r.clone()), *id))
            .collect();
        let redaction = self.redaction();
        for chunk in self.chunks(&records) {
            let rendered = self.render(&chunk, &redaction)?;
            self.deliver(&rendered, &redaction).await?;
        }
        Ok(())
    }

    /// Requests in order of each group's first appearance; records keep
    /// their input order within a group.
    fn chunks(&self, records: &[(Value, Uuid)]) -> Vec<Vec<(Value, Uuid)>> {
        let mut groups: IndexMap<String, Vec<(Value, Uuid)>> = IndexMap::new();
        for entry in records {
            let key = match &self.group_by {
                Some(field) => entry.0.get(field).unwrap_or(&Value::Null).to_string(),
                None => String::new(),
            };
            groups.entry(key).or_default().push(entry.clone());
        }
        groups
            .into_values()
            .flat_map(|group| {
                group
                    .chunks(self.batch)
                    .map(<[(Value, Uuid)]>::to_vec)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn render(&self, chunk: &[(Value, Uuid)], redaction: &Redaction) -> Result<RenderedRequest> {
        let records: Vec<Value> = chunk.iter().map(|(record, _)| record.clone()).collect();
        let records = records.as_slice();
        let body = match &self.body {
            Some(template) => template.render(records, &self.secrets, redaction)?,
            None if self.batch == 1 => records[0].clone(),
            None => Value::Array(records.to_vec()),
        };
        let bytes = serde_json::to_vec(&body)
            .map_err(|e| error!("Insert body is not valid JSON", reason = e.to_string()))?;
        let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
        if let (Some(name), Some((_, first))) = (&self.idempotency_key, chunk.first()) {
            headers.push((name.clone(), first.to_string()));
        }
        let bytes = match self.encoding {
            Some(Encoding::Gzip) => {
                headers.push(("content-encoding".to_string(), "gzip".to_string()));
                let mut gz = GzEncoder::new(Vec::new(), Compression::default());
                gz.write_all(&bytes)
                    .and_then(|()| gz.finish())
                    .map_err(|e| error!("Insert body gzip failed", reason = e.to_string()))?
            }
            None => bytes,
        };
        Ok(RenderedRequest {
            body: bytes,
            headers,
        })
    }

    /// Whether a failed request may be sent again: only when the table
    /// declared the repeat safe.
    fn repeatable(&self) -> bool {
        self.idempotent || self.idempotency_key.is_some()
    }

    /// POST one rendered request, resending the same bytes on a retryable
    /// failure until [`MAX_ATTEMPTS`] are spent. The pool makes a single
    /// attempt per call, so the attempt cap, the back-off and the
    /// `Retry-After` ceiling are decided here.
    async fn deliver(&self, request: &RenderedRequest, redaction: &Redaction) -> Result<()> {
        let path = self.request_path();
        let headers: Vec<(&str, &str)> = request
            .headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let policy = CallPolicy::background();
        let mut attempt = 1;
        loop {
            let failure = self
                .attempt(&policy, &path, &headers, request.body.clone(), redaction)
                .await?;
            let Some(failure) = failure else {
                return Ok(());
            };
            if !failure.retryable || !self.repeatable() || attempt >= MAX_ATTEMPTS {
                return Err(failure.error);
            }
            let delay = match failure.retry_after {
                Some(asked) => asked.min(RETRY_AFTER_CAP),
                None => BASE_BACKOFF * 2u32.pow(attempt as u32 - 1),
            };
            tokio::time::sleep(delay).await;
            attempt += 1;
        }
    }

    /// One request. `None` is success; a failure says whether a repeat may
    /// help and what delay the server asked for.
    async fn attempt(
        &self,
        policy: &CallPolicy,
        path: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
        redaction: &Redaction,
    ) -> Result<Option<Failure>> {
        let answer = self
            .api
            .http_request_with_policy(policy, Method::POST, path, headers, Some(body), redaction)
            .await?;
        let shown = self
            .api
            .shown_url(&join_base_path(self.api.base_url(), path));
        let response = match answer {
            Ok(response) => response,
            Err(e) => {
                let retryable = match e.kind {
                    ErrorKind::Status(status) => is_retryable(status),
                    ErrorKind::Transport(_) => true,
                    _ => false,
                };
                let retry_after = e.retry_after;
                return Ok(Some(Failure {
                    error: client_error(e, "REST insert failed", &shown, redaction),
                    retryable,
                    retry_after,
                }));
            }
        };
        let status = response.status().as_u16();
        if self.accept.is_empty() || self.accept.contains(&status) {
            return Ok(None);
        }
        Ok(Some(Failure {
            error: error!(
                "REST insert answered a status the table does not accept",
                path = redaction.url(&self.api.shown_url(path)),
                status = status
            ),
            retryable: is_retryable(status),
            retry_after: None,
        }))
    }

    fn request_path(&self) -> String {
        if self.query.is_empty() {
            return self.path.clone();
        }
        let query = self
            .query
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let joiner = if self.path.contains('?') { '&' } else { '?' };
        format!("{}{joiner}{query}", self.path)
    }
}
