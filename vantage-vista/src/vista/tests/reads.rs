//! Read gating, the leading window behind `get_some_value`, the preview and
//! the `get_ref` default.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;

use super::shell;
use crate::{
    Capability, Condition, Page, Preview, TableShell, Vista, VistaCapabilities, VistaChangeStream,
    VistaMetadata, forward_table_shell, unsupported,
};

type Rec = Record<CborValue>;

/// Fails every gated read with a plain error, so a refusal proves the
/// `Vista` answered before the shell was called.
#[derive(Clone)]
struct Tripwire {
    inner: Box<dyn TableShell>,
}

fn reached() -> vantage_core::VantageError {
    error!("the shell was called")
}

forward_table_shell!(Tripwire, inner, {
    async fn fetch_window(&self, _offset: usize, _limit: usize) -> Result<Page> {
        Err(reached())
    }
    async fn fetch_cursor(
        &self,
        _token: Option<CborValue>,
        _limit: usize,
    ) -> Result<(Page, Option<CborValue>)> {
        Err(reached())
    }
    async fn watch(&self) -> Result<VistaChangeStream> {
        Err(reached())
    }
});

fn tripwire(caps: VistaCapabilities) -> Vista {
    Vista::new("t", shell().with_capabilities(caps)).map_shell(|inner| Tripwire { inner })
}

/// Whether the refusal's `capability` context names `capability`. Context
/// values are stored `Debug`-rendered.
fn names(err: &vantage_core::VantageError, capability: Capability) -> bool {
    err.context.get("capability") == Some(&format!("{:?}", capability.as_str()))
}

fn is_reached(result: Result<impl Sized>) -> bool {
    matches!(result, Err(e) if e.to_string().contains("the shell was called"))
}

#[tokio::test]
async fn gated_reads_are_refused_before_the_shell() {
    let vista = tripwire(VistaCapabilities::new());
    let refusals = [
        (Capability::Window, vista.fetch_window(1, 1).await.map(drop)),
        (
            Capability::Cursor,
            vista.fetch_cursor(None, 1).await.map(drop),
        ),
        (Capability::Subscribe, vista.watch().await.map(drop)),
    ];
    for (capability, result) in refusals {
        let err = result.expect_err(capability.as_str());
        assert!(err.is_unsupported(), "{capability}: {err}");
        assert!(names(&err, capability), "{err}");
    }
}

#[tokio::test]
async fn the_leading_window_and_advertised_reads_reach_the_shell() {
    let vista = tripwire(VistaCapabilities::new());
    assert!(is_reached(vista.fetch_window(0, 1).await));

    let caps = [
        Capability::Window,
        Capability::Cursor,
        Capability::Subscribe,
    ]
    .into_iter()
    .collect();
    let vista = tripwire(caps);
    assert!(is_reached(vista.fetch_window(1, 1).await));
    assert!(is_reached(vista.fetch_cursor(None, 1).await));
    assert!(is_reached(vista.watch().await));
}

/// Refuses every window, while listing works.
#[derive(Clone)]
struct NoWindow {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(NoWindow, inner, {
    async fn fetch_window(&self, _offset: usize, _limit: usize) -> Result<Page> {
        unsupported(Capability::Window)
    }
});

#[tokio::test]
async fn get_some_value_reads_the_leading_window_and_never_lists() {
    let caps = VistaCapabilities::new();
    let vista = Vista::new("t", shell().with_capabilities(caps));
    let (id, _) = vista.get_some_value().await.unwrap().unwrap();
    assert_eq!(id, "r1");

    let refusing = vista.map_shell(|inner| NoWindow { inner });
    assert_eq!(refusing.list_values().await.unwrap().len(), 3);
    let err = refusing.get_some_value().await.unwrap_err();
    assert!(err.is_unsupported(), "{err}");
}

#[test]
fn preview_is_the_shells_own() {
    let vista = Vista::new("t", shell())
        .with_condition("name", crate::FilterOp::Eq, "a")
        .unwrap();
    let preview = vista.preview();
    assert!(preview.query.as_deref().unwrap().contains("name"));
    assert_eq!(preview, TableShell::preview(vista.shell()));
    assert_eq!(vista.driver_name(), "mock");
}

/// Implements only what a shell must: no `get_ref`.
#[derive(Clone, Default)]
struct Bare {
    metadata: VistaMetadata,
    capabilities: VistaCapabilities,
}

#[async_trait]
impl TableShell for Bare {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
    fn driver_name(&self) -> &'static str {
        "bare"
    }
    fn preview(&self) -> Preview {
        Preview::new()
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
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        Ok(Page::from_listing(IndexMap::new(), offset, limit))
    }
    fn add_condition(&mut self, _condition: &Condition) -> Result<()> {
        Ok(())
    }
}

#[test]
fn get_ref_defaults_to_a_traverse_refusal() {
    let vista = Vista::new("t", Bare::default());
    for row in [None, Some(&Rec::new())] {
        let err = vista.get_ref("anything", row).unwrap_err();
        assert!(err.is_unsupported(), "{err}");
        assert!(names(&err, Capability::Traverse), "{err}");
    }
}
