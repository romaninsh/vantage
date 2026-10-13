//! Positional windows and keyset cursors over the shell's set. Both run on a
//! clone of the table, so the shell's own query is left as it was.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::pagination::Pagination;
use vantage_vista::Page;

use super::{MongoTableShell, keyset};

impl MongoTableShell {
    /// `skip(offset).limit(limit)` in the set's order with an `_id`
    /// tiebreak. Windows are separate queries: without a total order,
    /// MongoDB orders tied rows arbitrarily per query, so consecutive windows
    /// would repeat some and skip others at their boundaries.
    pub(super) async fn window(&self, offset: usize, limit: usize) -> Result<Page> {
        let mut window = self.table.clone();
        keyset::add_id_tiebreak(&mut window);
        window.set_pagination(Some(Pagination::window(offset as i64, limit as i64)));
        Ok(Page::new(self.to_rows(window.list_values().await?)))
    }

    /// The next `limit` rows strictly after the row `token` names, in the
    /// set's order with an `_id` tiebreak. The token carries that row's
    /// sort-key values and belongs to the sort it was issued under; see the
    /// `keyset` module. A full page hands out a token for the next call, even
    /// when no row follows: that call returns an empty page and no token.
    pub(super) async fn seek(
        &self,
        token: Option<CborValue>,
        limit: usize,
    ) -> Result<(Page, Option<CborValue>)> {
        if limit == 0 {
            return Err(error!("fetch_cursor limit must be > 0"));
        }
        let mut page_table = self.table.clone();
        keyset::add_id_tiebreak(&mut page_table);
        let sort = keyset::effective_sort(&page_table);
        if let Some(token) = token {
            let last = keyset::decode_token(&token)?;
            if last.len() != sort.len() {
                return Err(error!("invalid fetch_cursor token for mongodb driver"));
            }
            page_table.add_condition(keyset::seek_filter(&sort, &last));
        }
        page_table.set_pagination(Some(Pagination::new(1, limit as i64)));
        let raw = page_table.list_values().await?;

        let next_token = match raw.last() {
            Some((_, last)) if raw.len() == limit => Some(keyset::encode_token(
                &keyset::sort_values(&page_table, last, &sort),
            )?),
            _ => None,
        };
        Ok((Page::new(self.to_rows(raw)), next_token))
    }
}
