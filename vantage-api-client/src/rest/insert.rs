//! Sending a table's inserts: the `api.insert` block lowered to one request
//! shape against a [`RestApi`].

use std::sync::Arc;

use ciborium::Value as CborValue;
use reqwest::Method;
use vantage_core::{Result, error};
use vantage_types::{PlainDialect, Record, record_to_json};

use super::RestApi;
use super::vista::spec::ApiInsertBlock;

/// An insert target: where a table's records are POSTed and which statuses
/// count as success. Built by [`RestApi::insert_endpoint`].
#[derive(Clone, Debug)]
pub struct RestInsertEndpoint {
    api: RestApi,
    path: String,
    query: Vec<(String, String)>,
    accept: Vec<u16>,
}

/// The `api.insert` settings a table's own copy of the data source carries,
/// so the table source can send inserts without the spec.
#[derive(Clone, Debug)]
pub(crate) struct TableInsert {
    pub(crate) block: ApiInsertBlock,
    pub(crate) endpoint: String,
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
            Some(t) => self.insert_endpoint(&t.block, &t.endpoint),
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
        Ok(RestInsertEndpoint {
            api: self.clone(),
            path: block.path.clone().unwrap_or_else(|| endpoint.to_string()),
            query: block
                .query
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            accept: block.accept.clone(),
        })
    }
}

impl RestInsertEndpoint {
    /// POST `records`, each as its own request with the record as the JSON
    /// body. Stops at the first failure.
    pub async fn send(&self, records: &[Record<CborValue>]) -> Result<()> {
        // Task 2 groups records into batched bodies here.
        for record in records {
            self.post(record_to_json(&PlainDialect, record.clone()))
                .await?;
        }
        Ok(())
    }

    async fn post(&self, body: serde_json::Value) -> Result<()> {
        let path = self.request_path();
        let response = self
            .api
            .http_request(Method::POST, &path, &[], Some(&body))
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
