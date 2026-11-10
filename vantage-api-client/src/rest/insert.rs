//! Sending a table's inserts: the `api.insert` block lowered to one request
//! shape against a [`RestApi`].

mod template;

use std::io::Write;
use std::sync::Arc;

use ciborium::Value as CborValue;
use flate2::Compression;
use flate2::write::GzEncoder;
use indexmap::IndexMap;
use reqwest::Method;
use serde_json::Value;
use vantage_core::{Result, error};
use vantage_types::{PlainDialect, Record, record_to_json};

use self::template::BodyTemplate;
use super::RestApi;
use super::vista::spec::{ApiInsertBlock, Encoding};

/// An insert target: where a table's records are POSTed and which statuses
/// count as success. Built by [`RestApi::insert_endpoint`].
#[derive(Clone, Debug)]
pub struct RestInsertEndpoint {
    api: RestApi,
    path: String,
    query: Vec<(String, String)>,
    accept: Vec<u16>,
    batch: usize,
    group_by: Option<String>,
    body: Option<BodyTemplate>,
    encoding: Option<Encoding>,
    secrets: IndexMap<String, String>,
}

/// A request ready to send: the exact bytes and the headers that go with
/// them.
#[derive(Debug)]
struct RenderedRequest {
    body: Vec<u8>,
    headers: Vec<(&'static str, &'static str)>,
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

    /// POST `records` in requests of at most `batch` records, never mixing
    /// two `group_by` values. Stops at the first failure.
    pub async fn send(&self, records: &[Record<CborValue>]) -> Result<()> {
        let records: Vec<Value> = records
            .iter()
            .map(|r| record_to_json(&PlainDialect, r.clone()))
            .collect();
        for chunk in self.chunks(&records) {
            let rendered = self.render(&chunk)?;
            self.post(&rendered).await?;
        }
        Ok(())
    }

    /// Requests in order of each group's first appearance; records keep
    /// their input order within a group.
    fn chunks(&self, records: &[Value]) -> Vec<Vec<Value>> {
        let mut groups: IndexMap<String, Vec<Value>> = IndexMap::new();
        for record in records {
            let key = match &self.group_by {
                Some(field) => record.get(field).unwrap_or(&Value::Null).to_string(),
                None => String::new(),
            };
            groups.entry(key).or_default().push(record.clone());
        }
        groups
            .into_values()
            .flat_map(|group| {
                group
                    .chunks(self.batch)
                    .map(<[Value]>::to_vec)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn render(&self, records: &[Value]) -> Result<RenderedRequest> {
        let body = match &self.body {
            Some(template) => template.render(records, &self.secrets)?,
            None if self.batch == 1 => records[0].clone(),
            None => Value::Array(records.to_vec()),
        };
        let bytes = serde_json::to_vec(&body)
            .map_err(|e| error!("Insert body is not valid JSON", reason = e.to_string()))?;
        let mut headers = vec![("content-type", "application/json")];
        let bytes = match self.encoding {
            Some(Encoding::Gzip) => {
                headers.push(("content-encoding", "gzip"));
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

    async fn post(&self, request: &RenderedRequest) -> Result<()> {
        let path = self.request_path();
        let response = self
            .api
            .http_request_bytes(
                Method::POST,
                &path,
                &request.headers,
                Some(request.body.clone()),
            )
            .await?;
        let status = response.status().as_u16();
        if !self.accept.is_empty() && !self.accept.contains(&status) {
            return Err(error!(
                "REST insert answered a status the table does not accept",
                path = path,
                status = status
            ));
        }
        Ok(())
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
