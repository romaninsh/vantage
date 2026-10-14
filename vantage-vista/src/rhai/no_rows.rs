//! The target of a multi-row `ref` step over no parent rows: the target
//! table's schema and writes, with a set that holds nothing.
//!
//! An empty key list can't be sent as an `in` condition (SQL rejects
//! `col IN ()`), so the empty set is answered here, the same on every backend.
//! Narrowing (so a bad column fails as on any set) and refs go to the target
//! table. Writes follow the write contract for a set that holds nothing:
//! inserts are `Conflict`, a patch is `NotFound`, a delete has nothing to
//! remove. Cursor paging and watching are refused.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, VantageError, error};
use vantage_types::Record;

use crate::{
    Capability, Page, Preview, TableShell, Vista, VistaCapabilities, VistaChangeStream,
    forward_table_shell, unsupported,
};

type Rec = Record<CborValue>;

#[derive(Clone)]
pub(crate) struct NoRowsShell {
    inner: Box<dyn TableShell>,
    /// The target's, with `ConfineWrites`: every write stays in the empty set.
    capabilities: VistaCapabilities,
}

impl NoRowsShell {
    /// `target` as a set with no rows.
    pub(crate) fn wrap(target: Vista) -> Vista {
        let capabilities = target
            .capabilities()
            .with(Capability::ConfineWrites)
            .without(Capability::Cursor)
            .without(Capability::Subscribe);
        target.map_shell(|inner| NoRowsShell {
            inner,
            capabilities,
        })
    }
}

fn nothing_to_write_into() -> VantageError {
    error!("the set holds no rows; nothing can be written into it").mark_conflict()
}

forward_table_shell!(NoRowsShell, inner, {
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
    fn preview(&self) -> Preview {
        Preview::new().with_note("`ref` over no rows: nothing is queried")
    }

    async fn list(&self) -> Result<IndexMap<String, Rec>> {
        Ok(IndexMap::new())
    }
    async fn get(&self, _id: &str) -> Result<Option<Rec>> {
        Ok(None)
    }
    async fn count(&self) -> Result<i64> {
        Ok(0)
    }
    async fn fetch_window(&self, _offset: usize, _limit: usize) -> Result<Page> {
        Ok(Page::new(Vec::new()).with_total(0))
    }
    async fn fetch_cursor(
        &self,
        _token: Option<CborValue>,
        _limit: usize,
    ) -> Result<(Page, Option<CborValue>)> {
        unsupported(Capability::Cursor)
    }
    async fn watch(&self) -> Result<VistaChangeStream> {
        unsupported(Capability::Subscribe)
    }

    async fn insert(&self, _id: Option<&str>, _record: &Rec) -> Result<(String, Rec)> {
        Err(nothing_to_write_into())
    }
    async fn replace(&self, _id: &str, _record: &Rec) -> Result<Rec> {
        Err(nothing_to_write_into())
    }
    async fn upsert(&self, _id: &str, _record: &Rec) -> Result<Rec> {
        Err(nothing_to_write_into())
    }
    async fn import(&self, rows: &IndexMap<String, Rec>) -> Result<usize> {
        if rows.is_empty() {
            return Ok(0);
        }
        Err(nothing_to_write_into())
    }
    async fn patch(&self, id: &str, _partial: &Rec) -> Result<Rec> {
        Err(error!("row is not in the set", id = id).mark_not_found())
    }
    /// The set is empty, so there is nothing to delete.
    async fn delete(&self, _id: &str) -> Result<()> {
        Ok(())
    }
    async fn delete_all(&self) -> Result<()> {
        Ok(())
    }
});
