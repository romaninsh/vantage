//! A hand-written shell that implements only the required `TableShell`
//! methods, so every defaulted method answers as the trait documents.

use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, ColumnFlag, Condition, FilterOp, Page, Preview, TableShell, Vista,
    VistaCapabilities, VistaChange, VistaMetadata,
};

use super::{rec, t};

type Rec = Record<ciborium::Value>;

#[derive(Clone)]
struct Mini {
    metadata: VistaMetadata,
    caps: VistaCapabilities,
}

fn mini(caps: &[Capability]) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_id_column("id");
    Vista::new(
        "mini",
        Mini {
            metadata,
            caps: caps.iter().copied().collect(),
        },
    )
}

#[async_trait]
impl TableShell for Mini {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.caps
    }
    fn driver_name(&self) -> &'static str {
        "mini"
    }
    fn preview(&self) -> Preview {
        Preview::new().with_query("mini")
    }
    async fn list(&self) -> Result<IndexMap<String, Rec>> {
        Ok(IndexMap::from([("a".to_string(), rec(&[("id", t("a"))]))]))
    }
    async fn get(&self, id: &str) -> Result<Option<Rec>> {
        Ok((id == "a").then(|| rec(&[("id", t("a"))])))
    }
    async fn count(&self) -> Result<i64> {
        Ok(1)
    }
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        Ok(Page::from_listing(self.list().await?, offset, limit))
    }
    fn add_condition(&mut self, _condition: &Condition) -> Result<()> {
        vantage_vista::unsupported(Capability::FilterOps)
    }
    async fn watch(&self) -> Result<vantage_vista::VistaChangeStream> {
        let stream = async_stream::stream! {
            yield Ok(VistaChange::Inserted { id: "a".into(), value: rec(&[("id", t("a"))]) });
            yield Ok(VistaChange::Deleted { id: "a".into() });
            yield Ok(VistaChange::Invalidated);
        };
        Ok(Box::pin(stream))
    }
}

#[tokio::test]
async fn a_watching_shell_streams_its_changes_through_the_vista() {
    let vista = mini(&[Capability::Subscribe]);
    let mut changes = vista.watch().await.unwrap();
    let mut seen = Vec::new();
    while let Some(change) = std::future::poll_fn(|cx| Pin::new(&mut changes).poll_next(cx)).await {
        seen.push(change.unwrap());
    }
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0].id(), Some("a"));
    assert_eq!(seen[1].id(), Some("a"));
    assert_eq!(seen[2].id(), None);
    assert!(matches!(seen[2], VistaChange::Invalidated));
}

#[tokio::test]
async fn a_shell_that_implements_nothing_extra_refuses_each_optional_family() {
    let vista = mini(&[
        Capability::Cursor,
        Capability::Traverse,
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::Import,
        Capability::FilterOps,
        Capability::Order,
        Capability::Search,
        Capability::NativeConditions,
    ]);
    let refused = |err: vantage_core::VantageError| {
        assert!(err.is_unsupported() || err.is_unimplemented(), "{err:?}");
    };
    refused(vista.fetch_cursor(None, 1).await.unwrap_err());
    refused(vista.get_ref("x", None).unwrap_err());
    refused(
        vista
            .clone()
            .with_condition("id", FilterOp::Gt, "a")
            .unwrap_err(),
    );
    refused(vista.clone().with_search("a").unwrap_err());
    refused(
        vista
            .clone()
            .with_order("id", vantage_vista::SortDirection::Ascending)
            .unwrap_err(),
    );
    refused(
        vista
            .clone()
            .with_native_condition(vantage_vista::NativeCondition::new(1_u8, "k"))
            .unwrap_err(),
    );
    let _ = Condition::new("id", FilterOp::Eq, "a");
}
