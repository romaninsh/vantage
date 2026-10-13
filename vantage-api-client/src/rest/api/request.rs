//! Lowering a table's conditions, window and sort into a request URL.

use ciborium::Value as CborValue;
use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_expressions::Expression;

use super::{FilterStrategy, Order, PaginationMode, RestApi};

impl RestApi {
    /// Build the endpoint path for `table_name`, substituting any
    /// `{placeholder}` segments from matching eq-conditions.
    ///
    /// Returns the absolute URL up to (but excluding) the query string,
    /// alongside the indices of conditions consumed by the substitution
    /// — those are dropped from the query string by `build_query_string`.
    ///
    /// Tables that don't use templates (no `{}` in the name) pass
    /// through unchanged and consume no conditions.
    pub(super) fn endpoint_url(
        &self,
        table_name: &str,
        conditions: &[&Expression<CborValue>],
    ) -> Result<(String, Vec<usize>)> {
        if table_name.matches('{').count() != crate::rest::placeholders(table_name).count() {
            return Err(error!(
                "Unclosed `{` in table name URI template",
                table_name = table_name
            ));
        }
        let mut consumed = Vec::new();
        let mut path = table_name.to_string();
        for placeholder in crate::rest::placeholders(table_name) {
            let (idx, value) = conditions
                .iter()
                .enumerate()
                .find_map(|(i, cond)| {
                    if consumed.contains(&i) {
                        return None;
                    }
                    let (field, value) = crate::condition_to_query_param(cond)?;
                    (field == placeholder).then_some((i, value))
                })
                .ok_or_else(|| {
                    error!(
                        "No eq-condition provided for URI placeholder",
                        placeholder = placeholder,
                        table_name = table_name
                    )
                })?;
            consumed.push(idx);
            // Earlier substitutions are URL-encoded, so they hold no `{`:
            // the first `{placeholder}` left is this one.
            path = path.replacen(&format!("{{{placeholder}}}"), &urlencode(&value), 1);
        }
        Ok((format!("{}/{}", self.base_url, path), consumed))
    }

    /// Decide which conditions go in the query string and which are applied to
    /// the rows after they arrive.
    ///
    /// Under [`FilterStrategy::Client`] non-path eq-conditions are *not* sent —
    /// the API rejects or ignores unknown params — so they come back as
    /// client-side filters and every condition is marked consumed to keep it out
    /// of the URL. Otherwise nothing is filtered locally and only the path
    /// placeholders are consumed.
    ///
    /// Shared by the real fetch and by [`preview_request`](Self::preview_request)
    /// so a previewed URL cannot claim a filter the fetch would have applied in
    /// memory, or vice versa.
    pub(super) fn split_filters(
        &self,
        conds: &[&Expression<CborValue>],
        consumed: Vec<usize>,
    ) -> (Vec<usize>, Vec<(String, String)>) {
        if self.filter_strategy == FilterStrategy::Client {
            let filters = conds
                .iter()
                .enumerate()
                .filter(|(i, _)| !consumed.contains(i))
                .filter_map(|(_, c)| crate::condition_to_query_param(c))
                .collect();
            ((0..conds.len()).collect(), filters)
        } else {
            (consumed, Vec::new())
        }
    }

    /// Build the combined query-string from pagination + conditions.
    /// `consumed` lists condition indices already baked into the URI
    /// path; those don't appear in the query string. Conditions that
    /// don't peel cleanly into eq pairs are skipped — same "best effort"
    /// stance as before.
    pub(super) fn build_query_string(
        &self,
        window: Option<(i64, i64)>,
        conditions: &[&Expression<CborValue>],
        consumed: &[usize],
        order: Order<'_>,
    ) -> String {
        let mut params: Vec<(String, String)> = Vec::new();

        // Pagination first — matches the order users see in the URL bar.
        // When `no_pagination` is set the API doesn't accept page/limit
        // query params (and may treat them as strict filters that
        // return empty), so we leave them off.
        //
        // `window` is a half-open `[offset, offset+limit)` band. Skip-based
        // APIs take the offset verbatim; page-based APIs are addressed by
        // 1-based page, derived from the offset (the loader may hand
        // non-page-aligned windows, so it rounds down to the containing page).
        if !self.no_pagination
            && let Some((offset, limit)) = window
        {
            let offset = offset.max(0);
            let limit = limit.max(1);
            let page_value = match self.pagination.mode {
                PaginationMode::Skip => offset.to_string(),
                PaginationMode::Page => (offset / limit + 1).to_string(),
            };
            params.push((self.pagination.page.clone(), page_value));
            params.push((self.pagination.limit.clone(), limit.to_string()));
        }

        // A sort reaches the query only when the API declared how it takes
        // one; the vista never offers `set_order` otherwise.
        if let (Some(spec), Some((field, dir))) = (&self.ordering, order) {
            params.push((spec.param.clone(), spec.value(field, dir)));
        }

        // Conditions: each `eq` becomes `?field=value`. Multiple
        // conditions AND together (JSON Server semantics).
        for (i, cond) in conditions.iter().enumerate() {
            if consumed.contains(&i) {
                continue;
            }
            if let Some((field, value)) = crate::condition_to_query_param(cond) {
                params.push((field, value));
            }
        }

        if params.is_empty() {
            return String::new();
        }
        let mut s = String::from("?");
        for (i, (k, v)) in params.iter().enumerate() {
            if i > 0 {
                s.push('&');
            }
            // Minimal URL encoding — we encode `&` and `=` and spaces
            // because those break the query format. Anything else
            // passes through; the JSON Server convention is permissive.
            s.push_str(&urlencode(k));
            s.push('=');
            s.push_str(&urlencode(v));
        }
        s
    }
}

fn urlencode(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

/// Append a `build_query_string` result (always opening with `?`, or empty)
/// to an endpoint URL. The table path may itself carry a query string (e.g.
/// `launches/?mode=detailed`), in which case the appended params must join
/// with `&` — otherwise the URL gets two `?` and the API rejects it.
pub(super) fn join_query(endpoint: &str, query: &str) -> String {
    match query.strip_prefix('?') {
        Some(rest) if endpoint.contains('?') => format!("{endpoint}&{rest}"),
        _ => format!("{endpoint}{query}"),
    }
}
