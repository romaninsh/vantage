//! The target of a multi-row `ref` step over no parent rows: the target
//! table's schema and writes, with a set that holds nothing.
//!
//! An empty key list can't be sent as an `in` condition (SQL rejects
//! `col IN ()`), so the empty set is answered here, the same on every backend.
//! Narrowing (so a bad column fails as on any set) and refs go to the target
//! table. Writes follow the write contract for a set that holds nothing:
//! inserts are `Conflict`, a patch is `NotFound`, a delete has nothing to
//! remove. Cursor paging and watching are refused, as the trait defaults do.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, VantageError, error};
use vantage_types::Record;

use crate::{
    AggregateSpec, TableShell, Vista, VistaCapabilities, VistaChangeStream, VistaRowStream,
    forward_table_shell, stream_from_list,
};

type Rec = Record<CborValue>;

pub(crate) struct NoRowsShell {
    inner: Box<dyn TableShell>,
    /// The target's, with `can_confine_writes`: every write stays in the
    /// empty set.
    capabilities: VistaCapabilities,
}

impl NoRowsShell {
    /// `target` as a set with no rows.
    pub(crate) fn wrap(target: Vista) -> Vista {
        let name = target.name().to_string();
        let capabilities = VistaCapabilities {
            can_confine_writes: true,
            ..target.capabilities().clone()
        };
        Vista::new(
            name,
            Box::new(NoRowsShell {
                inner: target.source,
                capabilities,
            }),
        )
    }
}

fn nothing_to_write_into() -> VantageError {
    error!("the set holds no rows; nothing can be written into it").mark_conflict()
}

forward_table_shell!(NoRowsShell, inner, {
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        let inner = self.inner.clone_shell()?;
        Some(Box::new(NoRowsShell {
            inner,
            capabilities: self.capabilities.clone(),
        }))
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
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

    async fn insert_vista_value(&self, _vista: &Vista, _id: &String, _record: &Rec) -> Result<Rec> {
        Err(nothing_to_write_into())
    }
    async fn replace_vista_value(
        &self,
        _vista: &Vista,
        _id: &String,
        _record: &Rec,
    ) -> Result<Rec> {
        Err(nothing_to_write_into())
    }
    async fn upsert_vista_value(&self, _vista: &Vista, _id: &String, _record: &Rec) -> Result<Rec> {
        Err(nothing_to_write_into())
    }
    async fn insert_vista_return_id_value(&self, _vista: &Vista, _record: &Rec) -> Result<String> {
        Err(nothing_to_write_into())
    }
    async fn import_vista_values(
        &self,
        _vista: &Vista,
        records: &IndexMap<String, Rec>,
    ) -> Result<usize> {
        if records.is_empty() {
            return Ok(0);
        }
        Err(nothing_to_write_into())
    }
    async fn patch_vista_value(&self, _vista: &Vista, id: &String, _partial: &Rec) -> Result<Rec> {
        Err(error!("row is not in the set", id = id).mark_not_found())
    }
    /// The set is empty, so there is nothing to delete.
    async fn delete_vista_value(&self, _vista: &Vista, _id: &String) -> Result<()> {
        Ok(())
    }
    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        Ok(())
    }
});
