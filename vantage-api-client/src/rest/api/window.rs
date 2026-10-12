//! Assembling a row window from one or more server pages.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::traits::Result;
use vantage_expressions::Expression;
use vantage_types::Record;

use super::{Order, PaginationMode, RestApi};

impl RestApi {
    /// Also reports the envelope total when `total_key` is configured and the
    /// body carries it. Unlike [`Self::fetch_total`] this never errors on a
    /// missing total: the rows are the point here, and a caller that needs a
    /// definitive count can still ask for one.
    ///
    /// A window comes back short only when the server ran out: callers take a
    /// short window as the end of the set.
    pub(super) async fn fetch_windowed<'a>(
        &self,
        table_name: &str,
        id_field: Option<&str>,
        row_fields: &[String],
        window: Option<(i64, i64)>,
        order: Order<'_>,
        conditions: impl IntoIterator<Item = &'a Expression<CborValue>>,
    ) -> Result<(IndexMap<String, Record<CborValue>>, Option<i64>)> {
        // Non-paginating endpoints return the whole list on the first
        // window; a later window would just re-deliver the same rows and the
        // perpetual grid would never mark itself exhausted. Short-circuit any
        // window past the start to empty so the grid sees the chunk shrink
        // and stops asking for more.
        if self.no_pagination && window.is_some_and(|(offset, _)| offset > 0) {
            return Ok((IndexMap::new(), None));
        }
        let conditions: Vec<&Expression<CborValue>> = conditions.into_iter().collect();
        let fetch =
            |window| self.fetch_page(table_name, id_field, row_fields, window, order, &conditions);
        let Some((offset, limit)) = window.filter(|_| !self.no_pagination) else {
            let page = fetch(window).await?;
            return Ok((page.rows, page.total));
        };
        let (offset, limit) = (offset.max(0), limit.max(1));

        // Page-based APIs are addressed by whole pages: start at the page
        // holding `offset`.
        let start = match self.pagination.mode {
            PaginationMode::Skip => offset,
            PaginationMode::Page => offset - offset % limit,
        };
        let first = fetch(Some((start, limit))).await?;
        if start == offset && !first.lossy {
            return Ok((first.rows, first.total));
        }

        // Assemble the window page by page until it is full or the server
        // runs out. When client filters dropped rows, a row's place in the
        // result is only known by walking from the start.
        let (mut server_offset, mut skip) = if first.lossy {
            (0, offset)
        } else {
            (start, offset - start)
        };
        let mut page = (server_offset == start).then_some(first);
        let mut rows = IndexMap::new();
        let mut total = None;
        let mut lossy = false;
        loop {
            let current = match page.take() {
                Some(p) => p,
                None => fetch(Some((server_offset, limit))).await?,
            };
            total = total.or(current.total);
            lossy |= current.lossy;
            for (id, row) in current.rows {
                if skip > 0 {
                    skip -= 1;
                } else if (rows.len() as i64) < limit {
                    // An id repeated across pages (rows shifting between
                    // fetches) keeps the later row and doesn't end the window.
                    rows.insert(id, row);
                }
            }
            if rows.len() as i64 >= limit || (current.server_rows as i64) < limit {
                break;
            }
            server_offset += limit;
        }
        Ok((rows, if lossy { None } else { total }))
    }
}
