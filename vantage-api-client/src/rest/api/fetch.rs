//! The read entry points and the one HTTP GET they all issue.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Priority, error};
use vantage_dataset::traits::Result;
use vantage_expressions::Expression;
use vantage_table::pagination::Pagination;
use vantage_types::Record;

use super::request::join_query;
use super::{Order, RestApi};

impl RestApi {
    /// Fetch data from the API endpoint and return parsed records.
    ///
    /// `id_field` selects which JSON field is treated as the record ID;
    /// a row without one is keyed by its position (see `insert_row`). The
    /// page-based `pagination` is lowered to a `[offset, offset+limit)`
    /// window; `conditions` are
    /// pushed into the URL query string — eq-conditions become
    /// `?field=value`. Conditions that can't be peeled into a simple
    /// eq are silently skipped (caller-side filtering still applies if
    /// needed). `row_fields` names the fields a row must carry to pass
    /// a client-side filter on them (see `fetch_page`). `order` is sent as
    /// the ordering param when the API declares one.
    pub(crate) async fn fetch_records<'a>(
        &self,
        table_name: &str,
        id_field: Option<&str>,
        row_fields: &[String],
        pagination: Option<&Pagination>,
        order: Order<'_>,
        conditions: impl IntoIterator<Item = &'a Expression<CborValue>>,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        let window = pagination.map(|p| (p.skip(), p.limit()));
        self.fetch_windowed(table_name, id_field, row_fields, window, order, conditions)
            .await
            .map(|(records, _total)| records)
    }

    /// Fetch one half-open row window `[offset, offset+limit)` (an absolute
    /// row index, not a page number), plus the envelope's `total_key` when
    /// the response carries one.
    ///
    /// The total comes out of the **same response as the rows**. Every paged
    /// endpoint reports it on every reply, so a caller wanting both a window
    /// and a grand total takes them together here rather than pairing a
    /// window fetch with [`Self::fetch_total`] — that pairing costs a second
    /// round trip for a number already in hand.
    pub(crate) async fn fetch_window_records_counted<'a>(
        &self,
        table_name: &str,
        id_field: Option<&str>,
        row_fields: &[String],
        (offset, limit): (i64, i64),
        order: Order<'_>,
        conditions: impl IntoIterator<Item = &'a Expression<CborValue>>,
    ) -> Result<(IndexMap<String, Record<CborValue>>, Option<i64>)> {
        self.fetch_windowed(
            table_name,
            id_field,
            row_fields,
            Some((offset, limit)),
            order,
            conditions,
        )
        .await
    }

    /// Read the grand total of matching rows from the response envelope's
    /// configured `total_key` (e.g. `count`). Returns `None` when no
    /// `total_key` is set — the caller then falls back to counting fetched
    /// rows. Issues a cheap `limit=1` request so the body carries the count
    /// without paying for the rows.
    pub(crate) async fn fetch_total<'a>(
        &self,
        table_name: &str,
        conditions: impl IntoIterator<Item = &'a Expression<CborValue>>,
    ) -> Result<Option<i64>> {
        let Some(total_key) = self.total_key.clone() else {
            return Ok(None);
        };
        let (body, _client_filters) = self
            .fetch_raw_body(table_name, Some((0, 1)), None, conditions)
            .await?;
        let total = body
            .get(total_key.as_str())
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                error!(
                    "total_key missing or not an integer in API response",
                    total_key = total_key.as_str()
                )
            })?;
        if self.debug {
            tracing::debug!(target: "vantage_api_client::rest", total, "REST count");
        }
        Ok(Some(total))
    }

    /// Resolve conditions, build the windowed request URL, GET it (with the
    /// auth header if configured), and return the parsed JSON body together
    /// with any client-side filters that still need applying (under
    /// [`FilterStrategy::Client`](super::FilterStrategy::Client)).
    pub(super) async fn fetch_raw_body<'a>(
        &self,
        table_name: &str,
        window: Option<(i64, i64)>,
        order: Order<'_>,
        conditions: impl IntoIterator<Item = &'a Expression<CborValue>>,
    ) -> Result<(serde_json::Value, Vec<(String, String)>)> {
        // Conditions may carry `DeferredFn` values — typically from
        // `related_in_condition` for `with_one`-style traversals where the FK
        // lives in a parent record we haven't fetched yet. Resolve them once,
        // up front, so the rest of the pipeline sees only sync scalars.
        let raw: Vec<&Expression<CborValue>> = conditions.into_iter().collect();
        let mut resolved: Vec<Expression<CborValue>> = Vec::with_capacity(raw.len());
        for cond in raw {
            resolved.push(cond.resolve_deferred().await?);
        }
        let conds: Vec<&Expression<CborValue>> = resolved.iter().collect();
        let (endpoint, consumed) = self.endpoint_url(table_name, &conds)?;

        let (query_consumed, client_filters) = self.split_filters(&conds, consumed);
        let query = self.build_query_string(window, &conds, &query_consumed, order);
        let url = join_query(&endpoint, &query);

        // The `(0, 1)` window is the count probe (reads only the envelope's
        // total); log it at debug so it doesn't drown the real data fetches.
        let probe = window == Some((0, 1));
        if self.debug {
            if probe {
                tracing::debug!(target: "vantage_api_client::rest", table = table_name, url = %url, "REST GET (count probe)");
            } else {
                tracing::info!(target: "vantage_api_client::rest", table = table_name, url = %url, "REST GET");
            }
        }

        // Time every round trip, unconditionally — a remote API is the one part
        // of a read the process cannot bound, and a slow page is far more often
        // one slow GET than anything local. Reported regardless of `debug` so
        // the cost is attributable from a default log; a request over a second
        // is worth an operator's attention, hence `info` at that point.
        let started = std::time::Instant::now();
        let policy = crate::transport::policy_for(Priority::current());
        let response = self
            .client
            .execute_with(&policy, |http| self.auth_header.apply(http.get(&url)))
            .await
            .map_err(|e| {
                let ms = started.elapsed().as_millis() as u64;
                // A transient failure (5xx, network, open breaker) is visible
                // in the datasource's health and will be tried again by a
                // later call; only an answer no retry can change is worth a
                // warning.
                if e.is_final() {
                    tracing::warn!(
                        target: "vantage_api_client::rest",
                        table = table_name,
                        url = %url,
                        ms,
                        attempts = e.attempts,
                        "REST GET failed",
                    );
                } else {
                    tracing::debug!(
                        target: "vantage_api_client::rest",
                        table = table_name,
                        url = %url,
                        ms,
                        attempts = e.attempts,
                        kind = e.kind_name(),
                        "REST GET gave up on a transient failure",
                    );
                }
                crate::transport::client_error(e, "API request failed", &url)
            })?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| error!("Failed to parse API response as JSON", detail = e))?;

        let ms = started.elapsed().as_millis() as u64;
        if ms >= 1000 {
            tracing::info!(
                target: "vantage_api_client::rest",
                table = table_name,
                url = %url,
                ms,
                count_probe = probe,
                "slow REST GET",
            );
        } else {
            tracing::debug!(
                target: "vantage_api_client::rest",
                table = table_name,
                url = %url,
                ms,
                count_probe = probe,
                "REST GET done",
            );
        }

        Ok((body, client_filters))
    }
}
