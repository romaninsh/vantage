//! One response turned into keyed rows.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_api_pool::resilient::TransportEvent;
use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_expressions::Expression;
use vantage_table::table::retain_matching;
use vantage_types::Record;

use super::{Order, ResponseShape, RestApi};
use crate::row_keys::insert_row;

/// One response's rows, see [`RestApi::fetch_page`].
pub(super) struct Page {
    pub(super) rows: IndexMap<String, Record<CborValue>>,
    pub(super) total: Option<i64>,
    pub(super) server_rows: usize,
    pub(super) lossy: bool,
}

impl RestApi {
    /// One request: the page's rows keyed by id, the envelope total, and how
    /// many rows the server sent — more than `rows` holds when client filters
    /// dropped some (`lossy`). Two rows sharing an id within this one
    /// response are an error.
    ///
    /// Client-side filters go through vantage-table's
    /// [`retain_matching`]: a row lacking the filtered field fails only when
    /// the field is in `row_fields`.
    pub(super) async fn fetch_page(
        &self,
        table_name: &str,
        id_field: Option<&str>,
        row_fields: &[String],
        window: Option<(i64, i64)>,
        order: Order<'_>,
        conditions: &[&Expression<CborValue>],
    ) -> Result<Page> {
        let (body, client_filters) = self
            .fetch_raw_body(table_name, window, order, conditions.iter().copied())
            .await?;
        let total = self
            .total_key
            .as_deref()
            .and_then(|key| body.get(key))
            .and_then(|v| v.as_i64());
        let data = self.extract_array(&body, table_name)?;

        // Where this page starts in the server's result, so an id-less row
        // is keyed by its absolute position (see `insert_row`).
        let offset = match window {
            Some((offset, _)) if !self.no_pagination => offset.max(0) as usize,
            _ => 0,
        };
        let mut records = IndexMap::new();
        for (row_idx, item) in data.iter().enumerate() {
            let obj = item
                .as_object()
                .ok_or_else(|| error!("API data item is not an object", index = row_idx))?;

            // The configured id field when it holds a string or number;
            // otherwise the row is keyed by its position.
            let id = id_field
                .and_then(|field| obj.get(field))
                .and_then(|v| match v {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Number(n) => Some(n.to_string()),
                    _ => None,
                });

            // The one JSON-to-CBOR boundary: Table and Vista speak CBOR.
            let mut record: Record<CborValue> = Record::new();
            for (k, v) in obj {
                record.insert(k.clone(), vantage_types::json_to_cbor(v.clone()));
            }

            insert_row(&mut records, id, offset + row_idx, record)?;
        }

        if !client_filters.is_empty() {
            let filters: Vec<(&str, &str)> = client_filters
                .iter()
                .map(|(field, want)| (field.as_str(), want.as_str()))
                .collect();
            retain_matching(&mut records, &filters, row_fields, |value, want| {
                crate::cbor_to_query_string(value).as_deref() == Some(want)
            });
        }

        // Counts the rows this call hands back, so a client-side filter shows
        // up as fewer rows pulled than the server sent.
        self.client
            .report(TransportEvent::rows_pulled(records.len()));

        // The envelope counted what the SERVER matched, before any rows were
        // dropped above — reporting that total now would size a grid to rows it
        // will never be given. No total is better than a wrong one.
        let total = if client_filters.is_empty() {
            total
        } else {
            None
        };
        Ok(Page {
            lossy: records.len() < data.len(),
            server_rows: data.len(),
            rows: records,
            total,
        })
    }

    /// Pull the row array out of the response body, according to the
    /// configured `ResponseShape`. A bare-array API answers a single-record
    /// endpoint (`activities/{id}`) with one object: that reads as one row.
    fn extract_array<'a>(
        &self,
        body: &'a serde_json::Value,
        table_name: &str,
    ) -> Result<&'a [serde_json::Value]> {
        match &self.response_shape {
            ResponseShape::BareArray => match body {
                serde_json::Value::Array(rows) => Ok(rows),
                serde_json::Value::Object(_) => Ok(std::slice::from_ref(body)),
                _ => Err(error!(
                    "Expected response body to be a JSON array or object (BareArray shape)"
                )),
            },
            ResponseShape::Wrapped { array_key } => body[array_key]
                .as_array()
                .map(Vec::as_slice)
                .ok_or_else(|| {
                    error!(
                        "Response missing array under wrapper key",
                        array_key = array_key
                    )
                }),
            ResponseShape::WrappedByTableName => body[table_name]
                .as_array()
                .map(Vec::as_slice)
                .ok_or_else(|| {
                    error!(
                        "Response missing array under table-name key",
                        table_name = table_name
                    )
                }),
        }
    }
}
