//! The request a read would send, rendered without sending it.

use ciborium::Value as CborValue;
use vantage_expressions::Expression;
use vantage_vista::Preview;

use super::request::join_query;
use super::{Order, RestApi};

impl RestApi {
    /// Render the request a read would issue, without issuing it.
    ///
    /// Shares [`endpoint_url`](Self::endpoint_url) and
    /// [`build_query_string`](Self::build_query_string) with the real fetch
    /// path, so a previewed URL cannot drift from the one that gets sent.
    ///
    /// One difference is deliberate: `fetch_raw_body` first *awaits* any
    /// deferred condition (a foreign key whose value arrives with the parent
    /// row), and awaiting is what a preview must not do. Those are counted
    /// in the note and left out of the URL instead.
    pub(crate) fn preview_request<'a>(
        &self,
        table_name: &str,
        window: Option<(i64, i64)>,
        order: Order<'_>,
        conditions: impl IntoIterator<Item = &'a Expression<CborValue>>,
    ) -> Preview {
        let conds: Vec<&Expression<CborValue>> = conditions.into_iter().collect();
        let preview = Preview::new();

        // Conditions the query-param lowering cannot peel into an eq pair —
        // deferred foreign keys, and anything else not shaped `field = value`.
        let unresolved = conds
            .iter()
            .filter(|c| crate::condition_to_query_param(c).is_none())
            .count();

        let (endpoint, consumed) = match self.endpoint_url(table_name, &conds) {
            Ok(pair) => pair,
            // A URI template placeholder went unfilled. If some condition is
            // still unresolved, the real fetch would have awaited it *before*
            // building the path, so this is a preview limitation and the
            // template is the honest answer. With nothing outstanding, the
            // fetch would fail here too — report that.
            Err(_) if unresolved > 0 => {
                return preview
                    .with_query(self.request(&format!("{}/{}", self.base_url, table_name)))
                    .with_note(format!(
                        "path placeholders are filled from conditions resolved at fetch \
                         time; the template is shown unfilled ({unresolved} unresolved)"
                    ));
            }
            Err(e) => return preview.with_note(format!("the request can't be built: {e}")),
        };

        let (query_consumed, client_filters) = self.split_filters(&conds, consumed);
        let query = self.build_query_string(window, &conds, &query_consumed, order);

        let mut notes = Vec::new();
        // Under `FilterStrategy::Client` these never reach the server: the
        // rows come back unfiltered and are narrowed in memory.
        if !client_filters.is_empty() {
            let filters: Vec<String> = client_filters
                .into_iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            notes.push(format!("filtered after the fetch: {}", filters.join(", ")));
        }
        if unresolved > 0 {
            notes.push(format!(
                "{unresolved} condition(s) not rendered; deferred ones resolve at fetch time"
            ));
        }
        let preview = preview.with_query(self.request(&join_query(&endpoint, &query)));
        if notes.is_empty() {
            preview
        } else {
            preview.with_note(notes.join("; "))
        }
    }

    /// The GET of `url`, with the `Authorization` header when one is
    /// configured, its value masked.
    fn request(&self, url: &str) -> String {
        match self.auth_header.masked() {
            Some(masked) => format!("GET {url}\nAuthorization: {masked}"),
            None => format!("GET {url}"),
        }
    }
}
