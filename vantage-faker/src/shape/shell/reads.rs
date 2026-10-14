//! Shaped window, cursor and count reads.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_vista::{Capability, Page};

use super::super::{OpClass, ShapedShell};

impl ShapedShell {
    /// The inner window at a possibly skewed offset. The total rides the
    /// same response envelope as the rows (no second toll), with the
    /// configured lie applied. The leading window (offset 0) is the floor
    /// every readable shell answers; any other offset needs `Window`.
    pub(super) async fn shaped_fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        if offset > 0 {
            self.gate(Capability::Window)?;
        }
        tracing::debug!(target: "vantage_faker::shape", op = "window", offset, limit, "request");
        self.toll(OpClass::Window).await?;
        let offset = self.skewed_offset(offset);
        let mut page = self.inner.fetch_window(offset, limit).await?;
        page.total = page.total.map(|truth| self.lied(truth));
        Ok(page)
    }

    /// A continuation-token API over the inner windows: at most the shape's
    /// `page_size` rows per call, a token carrying the next offset and its
    /// issue time (so it can expire), and no total.
    pub(super) async fn shaped_fetch_cursor(
        &self,
        token: Option<CborValue>,
        limit: usize,
    ) -> Result<(Page, Option<CborValue>)> {
        self.gate(Capability::Cursor)?;
        if limit == 0 {
            return Err(error!("cursor limit must be positive"));
        }
        self.toll(OpClass::Window).await?;
        let offset = match &token {
            None => 0,
            Some(t) => self.decode_cursor(t)?,
        };
        let offset = self.skewed_offset(offset);
        let served = limit.clamp(1, self.page_size());
        let rows = self.inner.fetch_window(offset, served).await?.rows;
        let next = (rows.len() == served).then(|| self.cursor_token(offset + rows.len()));
        Ok((Page::new(rows), next))
    }

    pub(super) async fn shaped_count(&self) -> Result<i64> {
        tracing::debug!(target: "vantage_faker::shape", op = "count", "request");
        self.toll(OpClass::Count).await?;
        Ok(self.lied(self.inner.count().await?))
    }
}
