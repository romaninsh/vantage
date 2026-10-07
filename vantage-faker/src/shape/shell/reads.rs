//! Shaped windowed, paged, cursor and count reads.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::Vista;
use vantage_vista::source::TableShell;

use super::super::{OpClass, ShapedShell};

impl ShapedShell {
    pub(super) async fn shaped_fetch_window(
        &self,
        vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        self.gate(
            self.shape.capabilities.can_fetch_window,
            "fetch_window",
            "can_fetch_window",
        )?;
        tracing::debug!(target: "vantage_faker::shape", op = "window", offset, limit, "request");
        self.toll(OpClass::Window).await?;
        let offset = self.skewed_offset(offset);
        self.inner.fetch_window(vista, offset, limit).await
    }

    /// The total rides the same response envelope as the rows (no second
    /// toll) — and only exists where the shape can count.
    pub(super) async fn shaped_fetch_window_counted(
        &self,
        vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<i64>)> {
        let rows = TableShell::fetch_window(self, vista, offset, limit).await?;
        let total = if self.shape.capabilities.can_count {
            Some(self.lied_total(vista).await?)
        } else {
            None
        };
        Ok((rows, total))
    }

    pub(super) async fn shaped_fetch_page(
        &self,
        vista: &Vista,
        page: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        self.gate(
            self.shape.capabilities.can_fetch_page,
            "fetch_page",
            "can_fetch_page",
        )?;
        self.toll(OpClass::Window).await?;
        let offset = self.skewed_offset(page.saturating_sub(1) * self.page_size);
        self.inner.fetch_window(vista, offset, self.page_size).await
    }

    pub(super) async fn shaped_fetch_next(
        &self,
        vista: &Vista,
        token: Option<CborValue>,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<CborValue>)> {
        self.gate(
            self.shape.capabilities.can_fetch_next,
            "fetch_next",
            "can_fetch_next",
        )?;
        self.toll(OpClass::Window).await?;
        let offset = match &token {
            None => 0,
            Some(t) => self.decode_cursor(t)?,
        };
        let offset = self.skewed_offset(offset);
        let rows = self
            .inner
            .fetch_window(vista, offset, self.page_size)
            .await?;
        let next = (rows.len() == self.page_size).then(|| self.cursor_token(offset + rows.len()));
        Ok((rows, next))
    }

    pub(super) async fn shaped_count(&self, vista: &Vista) -> Result<i64> {
        self.gate(
            self.shape.capabilities.can_count,
            "get_vista_count",
            "can_count",
        )?;
        tracing::debug!(target: "vantage_faker::shape", op = "count", "request");
        self.toll(OpClass::Count).await?;
        self.lied_total(vista).await
    }
}
