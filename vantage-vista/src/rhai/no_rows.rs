//! The target of a multi-row `ref` step over no parent rows: the target
//! table's schema and writes, with a set that holds nothing.
//!
//! An empty key list can't be sent as an `in` condition (SQL rejects
//! `col IN ()`), so the empty set is answered here, the same on every backend.
//! Narrowing (so a bad column fails as on any set), refs and writes by id
//! go to the target table; cursor paging and watching are refused, as the
//! trait defaults do.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;

use crate::{
    AggregateSpec, TableShell, Vista, VistaChangeStream, VistaRowStream, forward_table_shell,
    stream_from_list,
};

type Rec = Record<CborValue>;

pub(crate) struct NoRowsShell {
    inner: Box<dyn TableShell>,
}

impl NoRowsShell {
    /// `target` as a set with no rows.
    pub(crate) fn wrap(target: Vista) -> Vista {
        let name = target.name().to_string();
        Vista::new(
            name,
            Box::new(NoRowsShell {
                inner: target.source,
            }),
        )
    }
}

forward_table_shell!(NoRowsShell, inner, {
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        let inner = self.inner.clone_shell()?;
        Some(Box::new(NoRowsShell { inner }))
    }
    fn preview_query(&self, _vista: &Vista) -> serde_json::Value {
        serde_json::json!({
            "driver": self.inner.driver_name(),
            "query": null,
            "note": "`ref` over no rows: nothing is queried",
        })
    }

    async fn list_vista_values(&self, _vista: &Vista) -> Result<IndexMap<String, Rec>> {
        Ok(IndexMap::new())
    }
    async fn get_vista_value(&self, _vista: &Vista, _id: &String) -> Result<Option<Rec>> {
        Ok(None)
    }
    async fn get_vista_value_with_row(
        &self,
        _vista: &Vista,
        _id: &String,
        _row: &Rec,
    ) -> Result<Option<Rec>> {
        Ok(None)
    }
    async fn get_vista_some_value(&self, _vista: &Vista) -> Result<Option<(String, Rec)>> {
        Ok(None)
    }
    fn stream_vista_values<'a>(&'a self, _vista: &'a Vista) -> VistaRowStream<'a> {
        stream_from_list(async { Ok(IndexMap::new()) })
    }
    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        Ok(0)
    }
    fn aggregate_vista(&self, _vista: &Vista, _spec: &AggregateSpec) -> Result<Option<Vista>> {
        Ok(None)
    }
    async fn fetch_page(&self, _vista: &Vista, _page: usize) -> Result<Vec<(String, Rec)>> {
        Ok(Vec::new())
    }
    async fn fetch_next(
        &self,
        _vista: &Vista,
        _token: Option<CborValue>,
    ) -> Result<(Vec<(String, Rec)>, Option<CborValue>)> {
        Err(self.default_error("fetch_next", "can_fetch_next"))
    }
    async fn watch_vista(&self, _vista: &Vista) -> Result<VistaChangeStream> {
        Err(self.default_error("watch_vista", "can_subscribe"))
    }
    async fn fetch_window(
        &self,
        _vista: &Vista,
        _offset: usize,
        _limit: usize,
    ) -> Result<Vec<(String, Rec)>> {
        Ok(Vec::new())
    }
    async fn fetch_window_counted(
        &self,
        _vista: &Vista,
        _offset: usize,
        _limit: usize,
    ) -> Result<(Vec<(String, Rec)>, Option<i64>)> {
        Ok((Vec::new(), Some(0)))
    }

    /// The set is empty, so there is nothing to delete.
    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        Ok(())
    }
});
