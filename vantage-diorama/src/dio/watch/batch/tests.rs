mod commit;
mod order;
mod tail;

use std::sync::Arc;

use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaChange, VistaChangeStream, VistaMetadata};

use super::*;
use crate::dio::watch::admit::Admit;
use crate::lens::{Cache, Lens};

type Rec = Record<CborValue>;

fn row(note: &str) -> Rec {
    [("note".to_string(), CborValue::Text(note.into()))]
        .into_iter()
        .collect()
}

fn inserted(id: &str, note: &str) -> VistaChange {
    VistaChange::Inserted {
        id: id.into(),
        value: row(note),
    }
}

fn ready(rest: Vec<Result<VistaChange>>) -> VistaChangeStream {
    Box::pin(futures::stream::iter(rest))
}

async fn dio_over(cache: Cache) -> Dio {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_id_column("id");
    Lens::builder()
        .cache(cache)
        .build()
        .expect("lens")
        .dio(Vista::new("row", MockShell::new().with_metadata(metadata)))
        .build()
        .await
        .expect("dio")
}

async fn dio() -> Dio {
    dio_over(Cache::memory()).await
}

fn label(event: DioEvent) -> String {
    match event {
        DioEvent::RecordInserted { id } => format!("inserted {id}"),
        DioEvent::RecordChanged { id } => format!("changed {id}"),
        DioEvent::RecordRemoved { id } => format!("removed {id}"),
        DioEvent::Refreshing => "refreshing".into(),
        DioEvent::DatasetChanged => "dataset changed".into(),
        other => format!("{other:?}"),
    }
}

fn drain(bus: &mut tokio::sync::broadcast::Receiver<DioEvent>) -> Vec<String> {
    std::iter::from_fn(|| bus.try_recv().ok().map(label)).collect()
}

/// Apply `changes` as one ready pass: the first, then the rest as a stream
/// that has them all ready and then ends.
async fn apply_all(dio: &Dio, admit: &Admit, changes: Vec<VistaChange>) -> Result<()> {
    let mut changes = changes.into_iter();
    let first = changes.next().expect("at least one change");
    let mut stream = ready(changes.map(Ok).collect());
    apply_ready(&Arc::downgrade(&dio.inner), first, &mut stream, admit)
        .await
        .0
}

/// One unfiltered pass over `stream`, starting with an insert of `a`.
async fn pass(dio: &Dio, stream: &mut VistaChangeStream) -> (Result<()>, Option<Tail>) {
    let weak = Arc::downgrade(&dio.inner);
    apply_ready(&weak, inserted("a", "1"), stream, &Admit::All).await
}
