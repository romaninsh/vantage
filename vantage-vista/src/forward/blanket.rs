//! Every `ForwardShell + Clone` is a [`TableShell`]: each method calls the
//! wrapper's `ForwardShell` version, whether overridden or forwarded.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;

use super::ForwardShell;
use crate::{
    Condition, NativeCondition, Page, Preview, SortDirection, TableShell, Vista, VistaCapabilities,
    VistaChangeStream, VistaMetadata,
};

type Rec = Record<CborValue>;

#[async_trait]
impl<T: ForwardShell + Clone> TableShell for T {
    fn metadata(&self) -> &VistaMetadata {
        ForwardShell::metadata(self)
    }
    fn capabilities(&self) -> &VistaCapabilities {
        ForwardShell::capabilities(self)
    }
    fn driver_name(&self) -> &'static str {
        ForwardShell::driver_name(self)
    }
    fn preview(&self) -> Preview {
        ForwardShell::preview(self)
    }
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(&self, engine: &mut vantage_rhai::rhai::Engine) {
        ForwardShell::register_rhai_extensions(self, engine)
    }

    async fn list(&self) -> Result<IndexMap<String, Rec>> {
        ForwardShell::list(self).await
    }
    async fn get(&self, id: &str) -> Result<Option<Rec>> {
        ForwardShell::get(self, id).await
    }
    async fn count(&self) -> Result<i64> {
        ForwardShell::count(self).await
    }
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        ForwardShell::fetch_window(self, offset, limit).await
    }
    async fn fetch_cursor(
        &self,
        token: Option<CborValue>,
        limit: usize,
    ) -> Result<(Page, Option<CborValue>)> {
        ForwardShell::fetch_cursor(self, token, limit).await
    }
    async fn watch(&self) -> Result<VistaChangeStream> {
        ForwardShell::watch(self).await
    }

    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        ForwardShell::add_condition(self, condition)
    }
    fn add_native_condition(&mut self, condition: NativeCondition) -> Result<()> {
        ForwardShell::add_native_condition(self, condition)
    }
    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        ForwardShell::set_search(self, text)
    }
    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        ForwardShell::set_order(self, order)
    }

    async fn insert(&self, id: Option<&str>, record: &Rec) -> Result<(String, Rec)> {
        ForwardShell::insert(self, id, record).await
    }
    async fn replace(&self, id: &str, record: &Rec) -> Result<Rec> {
        ForwardShell::replace(self, id, record).await
    }
    async fn upsert(&self, id: &str, record: &Rec) -> Result<Rec> {
        ForwardShell::upsert(self, id, record).await
    }
    async fn patch(&self, id: &str, partial: &Rec) -> Result<Rec> {
        ForwardShell::patch(self, id, partial).await
    }
    async fn delete(&self, id: &str) -> Result<()> {
        ForwardShell::delete(self, id).await
    }
    async fn delete_all(&self) -> Result<()> {
        ForwardShell::delete_all(self).await
    }
    async fn import(&self, rows: &IndexMap<String, Rec>) -> Result<usize> {
        ForwardShell::import(self, rows).await
    }

    fn get_ref(&self, relation: &str, row: Option<&Rec>) -> Result<Vista> {
        ForwardShell::get_ref(self, relation, row)
    }
}
